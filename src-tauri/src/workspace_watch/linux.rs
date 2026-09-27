use super::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
};
const MAX_WATCHES: usize = 8192;
const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
const MASK: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MODIFY
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_ONLYDIR
    | libc::IN_EXCL_UNLINK;

pub(super) struct Tracker {
    fd: Option<File>,
    watches: BTreeMap<i32, String>,
    reverse: BTreeMap<String, i32>,
    identity: (u64, u64),
}
#[derive(Default)]
struct Events {
    files: BTreeSet<String>,
    touched: BTreeSet<String>,
    trees: BTreeSet<String>,
    reset: bool,
}
#[derive(Default)]
struct Budget {
    inspected: usize,
    bytes: usize,
}
fn directory(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}
fn identity(meta: &fs::Metadata) -> (u64, u64) {
    (meta.dev(), meta.ino())
}
fn stamp(meta: &fs::Metadata) -> Stamp {
    Stamp {
        digest: None,
        directory: meta.is_dir(),
        regular: meta.is_file(),
        identity: identity(meta),
        version: if meta.is_dir() {
            (0, 0, 0, 0, 0)
        } else {
            (
                meta.len(),
                meta.mtime(),
                meta.mtime_nsec(),
                meta.ctime(),
                meta.ctime_nsec(),
            )
        },
    }
}
fn base(dir: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd()))
}
fn missing(error: io::Error) -> Result<Option<(PathBuf, File)>, WatchError> {
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP)
    ) {
        Ok(None)
    } else {
        Err(WatchError::Unavailable)
    }
}
// Pin every ancestor from the approved root. No cached watch inode is used for reads.
fn parent(root: &Path, path: &str) -> Result<Option<(PathBuf, File)>, WatchError> {
    let mut dir = directory(root).map_err(|_| WatchError::Unavailable)?;
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        let child = base(&dir).join(part);
        if parts.peek().is_none() {
            return Ok(Some((child, dir)));
        }
        dir = match directory(&child) {
            Ok(dir) => dir,
            Err(error) => return missing(error),
        };
    }
    Err(WatchError::InvalidPath)
}
fn inspect(root: &Path, path: &str) -> Result<Option<Stamp>, WatchError> {
    let Some((path, _parent)) = parent(root, path)? else {
        return Ok(None);
    };
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(stamp(&meta))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(WatchError::Unavailable),
    }
}
// Open tabs retain content verification, including ignored build directories and
// filesystems with coarse timestamps. Closed sources no longer need repeated reads.
fn inspect_open(root: &Path, path: &str, budget: &mut u64) -> Result<Option<Stamp>, WatchError> {
    use sha2::{Digest, Sha256};
    let Some((child, _parent)) = parent(root, path)? else {
        return Ok(None);
    };
    let meta = match fs::symlink_metadata(&child) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(WatchError::Unavailable),
    };
    let mut current = stamp(&meta);
    if meta.is_file() && meta.len() <= 2 * 1024 * 1024 {
        if *budget + meta.len() > 32 * 1024 * 1024 {
            return Err(WatchError::TooLarge);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(&child)
            .map_err(|_| WatchError::Unavailable)?;
        let before = file.metadata().map_err(|_| WatchError::Unavailable)?;
        if stamp(&before) != current {
            return Err(WatchError::Unavailable);
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| WatchError::Unavailable)?;
        *budget += bytes.len() as u64;
        if bytes.len() > 2 * 1024 * 1024
            || *budget > 32 * 1024 * 1024
            || stamp(&file.metadata().map_err(|_| WatchError::Unavailable)?) != current
        {
            return Err(WatchError::Unavailable);
        }
        current.digest = Some(Sha256::digest(bytes).into());
    }
    Ok(Some(current))
}
impl Tracker {
    pub(super) fn native(&self) -> bool {
        self.fd.is_some()
    }
    pub(super) fn build(
        root: &Path,
        paths: &[String],
    ) -> Result<(Self, BTreeMap<String, Stamp>, usize), WatchError> {
        let dir = directory(root).map_err(|_| WatchError::Unavailable)?;
        let meta = dir.metadata().map_err(|_| WatchError::Unavailable)?;
        // SAFETY: no pointer arguments; flags make the owned descriptor nonblocking/CLOEXEC.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        let fd = if fd < 0 {
            None
        } else {
            Some(unsafe { File::from_raw_fd(fd) })
        }; // new owned fd
        let mut tracker = Self {
            fd,
            watches: BTreeMap::new(),
            reverse: BTreeMap::new(),
            identity: identity(&meta),
        };
        let mut entries = BTreeMap::new();
        let mut budget = Budget::default();
        tracker.walk(dir, "", 0, &mut entries, &mut budget)?;
        let mut read_bytes = 0;
        for path in paths {
            budget.inspected += 1;
            if let Some(stamp) = inspect_open(root, path, &mut read_bytes)? {
                entries.insert(path.clone(), stamp);
            }
        }
        if entries.len() > MAX_ENTRIES
            || entries.keys().map(String::len).sum::<usize>() > MAX_PATH_BYTES
        {
            return Err(WatchError::TooLarge);
        }
        Ok((tracker, entries, budget.inspected))
    }
    fn arm(&mut self, dir: &File, path: &str) {
        let Some(fd) = &self.fd else { return };
        let name = std::ffi::CString::new(base(dir).to_str().unwrap()).unwrap();
        // /proc/self/fd intentionally resolves the pinned directory, never a workspace symlink.
        // SAFETY: the C string and both owned descriptors remain alive during the call.
        let wd = if self.watches.len() >= MAX_WATCHES {
            -1
        } else {
            unsafe { libc::inotify_add_watch(fd.as_raw_fd(), name.as_ptr(), MASK) }
        };
        if wd < 0 || self.watches.get(&wd).is_some_and(|old| old != path) {
            // Resource limits or aliases cannot leave a partially watched tree reported as native.
            self.fd = None;
            self.watches.clear();
            self.reverse.clear();
            return;
        }
        self.watches.insert(wd, path.into());
        self.reverse.insert(path.into(), wd);
    }
    fn forget(&mut self, path: &str) {
        let prefix = format!("{path}/");
        let paths: Vec<_> = self
            .reverse
            .range(prefix.clone()..)
            .take_while(|(p, _)| p.starts_with(&prefix))
            .map(|(p, _)| p.clone())
            .chain(self.reverse.contains_key(path).then(|| path.to_owned()))
            .collect();
        for path in paths {
            let wd = self.reverse.remove(&path).unwrap();
            self.watches.remove(&wd);
            if let Some(fd) = &self.fd {
                // SAFETY: valid owned inotify fd; an already-removed watch is harmless.
                unsafe { libc::inotify_rm_watch(fd.as_raw_fd(), wd) };
            }
        }
    }
    fn walk(
        &mut self,
        dir: File,
        relative: &str,
        depth: usize,
        entries: &mut BTreeMap<String, Stamp>,
        budget: &mut Budget,
    ) -> Result<(), WatchError> {
        if depth > MAX_DEPTH {
            return Err(WatchError::TooLarge);
        }
        self.arm(&dir, relative); // watch before enumerating, catching concurrent creations
        for entry in fs::read_dir(base(&dir)).map_err(|_| WatchError::Unavailable)? {
            let entry = entry.map_err(|_| WatchError::Unavailable)?;
            budget.inspected += 1;
            if budget.inspected > MAX_ENTRIES {
                return Err(WatchError::TooLarge);
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if matches!(name, ".git" | "target" | "node_modules") || name.contains('\\') {
                continue;
            }
            let path = if relative.is_empty() {
                name.to_owned()
            } else {
                format!("{relative}/{name}")
            };
            budget.bytes += path.len();
            if path.len() > 4096 || budget.bytes > MAX_PATH_BYTES {
                return Err(WatchError::TooLarge);
            }
            let meta = match fs::symlink_metadata(entry.path()) {
                Ok(meta) => meta,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return Err(WatchError::Unavailable),
            };
            if meta.is_dir() {
                let child = directory(&entry.path()).map_err(|_| WatchError::Unavailable)?;
                let pinned = child.metadata().map_err(|_| WatchError::Unavailable)?;
                if identity(&pinned) != identity(&meta) {
                    return Err(WatchError::Unavailable);
                }
                entries.insert(path.clone(), stamp(&pinned));
                self.walk(child, &path, depth + 1, entries, budget)?;
            } else {
                entries.insert(path, stamp(&meta));
            }
        }
        Ok(())
    }
    fn read_events(&mut self) -> Result<Events, WatchError> {
        let mut result = Events::default();
        let mut total = 0;
        let mut buffer = [0u8; 65536];
        loop {
            let Some(fd) = &mut self.fd else {
                result.reset = true;
                break;
            };
            let length = match fd.read(&mut buffer) {
                Ok(0) => return Err(WatchError::Unavailable),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(WatchError::Unavailable),
            };
            total += length;
            if total > MAX_EVENT_BYTES {
                result.reset = true;
                break;
            }
            self.decode(&buffer[..length], &mut result)?;
            if result.reset {
                break;
            }
        }
        Ok(result)
    }
    fn decode(&self, bytes: &[u8], result: &mut Events) -> Result<(), WatchError> {
        let mut offset = 0;
        while offset < bytes.len() {
            let header = bytes
                .get(offset..offset + 16)
                .ok_or(WatchError::Unavailable)?;
            let wd = i32::from_ne_bytes(header[0..4].try_into().unwrap());
            let mask = u32::from_ne_bytes(header[4..8].try_into().unwrap());
            let size = u32::from_ne_bytes(header[12..16].try_into().unwrap()) as usize;
            offset += 16;
            let raw = bytes
                .get(offset..offset + size)
                .ok_or(WatchError::Unavailable)?;
            offset += size;
            if mask & (libc::IN_Q_OVERFLOW | libc::IN_UNMOUNT) != 0 {
                result.reset = true;
                continue;
            }
            let Some(dir) = self.watches.get(&wd) else {
                continue;
            }; // removed watches may still queue IN_IGNORED
            if mask & (libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_IGNORED) != 0 {
                if dir.is_empty() {
                    result.reset = true;
                } else {
                    result.trees.insert(dir.clone());
                }
                continue;
            }
            let name = raw.split(|b| *b == 0).next().unwrap_or_default();
            let Ok(name) = std::str::from_utf8(name) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            if name.contains(['/', '\\']) || matches!(name, "." | "..") {
                return Err(WatchError::InvalidPath);
            }
            let path = if dir.is_empty() {
                name.into()
            } else {
                format!("{dir}/{name}")
            };
            if excluded(&path) {
                continue;
            }
            if mask & libc::IN_ISDIR != 0 {
                result.trees.insert(path);
            } else {
                if mask & libc::IN_MODIFY != 0 {
                    result.touched.insert(path.clone());
                }
                result.files.insert(path);
            }
        }
        Ok(())
    }
}
pub(super) fn advance(old: &mut Observed, paths: &[String]) -> Result<(), WatchError> {
    let root_meta = directory(&old.root)
        .and_then(|d| d.metadata())
        .map_err(|_| WatchError::Unavailable)?;
    let root_changed = old
        .tracker
        .as_ref()
        .is_some_and(|t| t.identity != identity(&root_meta));
    let mut force = old.tracker.is_none() || root_changed;
    let mut rebuild = force || old.audited.elapsed() >= AUDIT_INTERVAL || old.mode() == "polling";
    let events = if rebuild {
        Events::default()
    } else {
        old.tracker.as_mut().unwrap().read_events()?
    };
    if events.reset {
        rebuild = true;
        force = true;
    }
    let mut updates = BTreeMap::new();
    let mut inspected = 0;
    if rebuild {
        old.tracker = None; // release quotas before installing the replacement watches
        let (tracker, entries, count) = Tracker::build(&old.root, paths)?;
        inspected = count;
        for path in old.entries.keys() {
            if !entries.contains_key(path) {
                updates.insert(path.clone(), None);
            }
        }
        updates.extend(entries.into_iter().map(|(p, s)| (p, Some(s))));
        old.apply(updates, force, &BTreeSet::new())?;
        old.tracker = Some(tracker);
        old.audited = Instant::now();
    } else {
        let tracker = old.tracker.as_mut().unwrap();
        let mut trees = BTreeSet::new();
        for path in events.trees {
            let mut prefix = path.as_str();
            let mut covered = false;
            while let Some((parent, _)) = prefix.rsplit_once('/') {
                if trees.contains(parent) {
                    covered = true;
                    break;
                }
                prefix = parent;
            }
            if !covered {
                trees.insert(path);
            }
        }
        // Detach all old prefixes first, so rename A -> B works in either lexical order.
        for path in &trees {
            for child in subtree(&old.entries, path) {
                updates.insert(child, None);
            }
            tracker.forget(path);
        }
        let mut collected = BTreeMap::new();
        let mut budget = Budget::default();
        for path in &trees {
            let Some((child, _parent)) = parent(&old.root, path)? else {
                continue;
            };
            let meta = match fs::symlink_metadata(&child) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return Err(WatchError::Unavailable),
            };
            collected.insert(path.clone(), stamp(&meta));
            if meta.is_dir() {
                let dir = directory(&child).map_err(|_| WatchError::Unavailable)?;
                if dir.metadata().map(|m| identity(&m)).ok() != Some(identity(&meta)) {
                    return Err(WatchError::Unavailable);
                }
                tracker.walk(
                    dir,
                    path,
                    path.split('/').count(),
                    &mut collected,
                    &mut budget,
                )?;
            }
        }
        inspected += budget.inspected;
        updates.extend(collected.into_iter().map(|(p, s)| (p, Some(s))));
        for path in &events.files {
            if path.len() > 4096 {
                return Err(WatchError::TooLarge);
            }
            inspected += 1;
            updates.insert(path.clone(), inspect(&old.root, path)?);
        }
        let mut read_bytes = 0;
        for path in paths {
            inspected += 1;
            updates.insert(
                path.clone(),
                inspect_open(&old.root, path, &mut read_bytes)?,
            );
        }
        for path in old.extras.iter().filter(|p| !paths.contains(p)) {
            updates.insert(path.clone(), None);
        }
        old.apply(updates, false, &events.touched)?;
    }
    old.extras = paths.iter().filter(|p| excluded(p)).cloned().collect();
    #[cfg(test)]
    {
        old.inspected = inspected;
    }
    let _ = inspected;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn overflow_rebuilds_the_cache_and_replays_resync_until_acknowledged() {
        let f = super::super::tests::Fixture::new();
        let service = WorkspaceWatchService::default();
        let first = service.poll(&f.0, None, &[]).unwrap();
        let mut fds = [-1; 2];
        // SAFETY: two-element output array; ownership transferred immediately below.
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) },
            0
        );
        let reader = unsafe { File::from_raw_fd(fds[0]) };
        let mut writer = unsafe { File::from_raw_fd(fds[1]) };
        let mut event = Vec::new();
        event.extend((-1i32).to_ne_bytes());
        event.extend(libc::IN_Q_OVERFLOW.to_ne_bytes());
        event.extend([0u8; 8]);
        writer.write_all(&event).unwrap();
        service
            .0
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .tracker
            .as_mut()
            .unwrap()
            .fd = Some(reader);
        std::fs::write(f.0.join("missed.rs"), "fn recovered() {}\n").unwrap();
        let recovered = service.poll(&f.0, Some(&first.token), &[]).unwrap();
        assert!(recovered.resync && recovered.rust_changed && recovered.tree_changed);
        assert_eq!(service.list(&f.0).unwrap()[0].path, "missed.rs");
        assert!(service.poll(&f.0, Some(&first.token), &[]).unwrap().resync);
        assert!(
            !service
                .poll(&f.0, Some(&recovered.token), &[])
                .unwrap()
                .resync
        );
    }
    #[test]
    fn watch_quota_uses_complete_polling_snapshot_and_stop_releases_all_state() {
        let f = super::super::tests::Fixture::new();
        for n in 0..MAX_WATCHES {
            std::fs::create_dir(f.0.join(format!("d{n}"))).unwrap();
        }
        let service = WorkspaceWatchService::default();
        let first = service.poll(&f.0, None, &[]).unwrap();
        assert_eq!(first.mode, "polling");
        assert_eq!(
            service.0.lock().unwrap().as_ref().unwrap().entries.len(),
            MAX_WATCHES
        );
        std::fs::write(f.0.join("new.txt"), "data").unwrap();
        let next = service.poll(&f.0, Some(&first.token), &[]).unwrap();
        assert_eq!(next.mode, "polling");
        assert_eq!(next.changes, ["new.txt"]);
        let state = service.0.lock().unwrap();
        let tracker = state.as_ref().unwrap().tracker.as_ref().unwrap();
        assert!(tracker.fd.is_none() && tracker.watches.is_empty());
        drop(state);
        service.stop().unwrap();
        assert!(service.0.lock().unwrap().is_none());
    }
}
