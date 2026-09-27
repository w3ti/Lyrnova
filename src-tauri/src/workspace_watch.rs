//! Workspace-bound incremental observation. Only polling drains events; no worker thread.
use crate::workspace::{EntryKind, WorkspaceEntry};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
mod linux;

const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 64;
const MAX_PATH_BYTES: usize = 16 * 1024 * 1024;
const AUDIT_INTERVAL: Duration = Duration::from_secs(60);
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum WatchError {
    Unavailable,
    TooLarge,
    InvalidPath,
    WorkspaceChanged,
    UnsupportedPlatform,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchSnapshot {
    pub token: String,
    pub resync: bool,
    pub changes: Vec<String>,
    pub rust_changed: bool,
    pub tree_changed: bool,
    pub mode: &'static str,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    digest: Option<[u8; 32]>,
    directory: bool,
    regular: bool,
    identity: (u64, u64),
    version: (u64, i64, i64, i64, i64),
}
#[derive(Default)]
pub struct WorkspaceWatchService(Mutex<Option<Observed>>);
struct Observed {
    root: PathBuf,
    token: String,
    previous: Option<String>,
    entries: BTreeMap<String, Stamp>,
    changes: Vec<String>,
    tree_changed: bool,
    force_resync: bool,
    #[cfg(target_os = "linux")]
    tracker: Option<linux::Tracker>,
    audited: Instant,
    extras: BTreeSet<String>,
    path_bytes: usize,
    #[cfg(test)]
    inspected: usize,
}
impl WorkspaceWatchService {
    pub fn stop(&self) -> Result<(), WatchError> {
        *self.0.lock().map_err(|_| WatchError::Unavailable)? = None;
        Ok(())
    }
    pub fn poll(
        &self,
        root: &Path,
        token: Option<&str>,
        paths: &[String],
    ) -> Result<WatchSnapshot, WatchError> {
        validate_paths(paths)?;
        if token.is_some_and(|s| s.len() > 64) {
            return Err(WatchError::TooLarge);
        }
        let mut state = self.0.lock().map_err(|_| WatchError::Unavailable)?;
        let old = update(&mut state, root, paths)?;
        let resync = token.is_none()
            || (token != Some(&old.token) && token != old.previous.as_deref())
            || (token != Some(&old.token) && old.force_resync);
        let changes = if !resync && token != Some(&old.token) {
            old.changes.clone()
        } else {
            vec![]
        };
        Ok(WatchSnapshot {
            token: old.token.clone(),
            rust_changed: resync || changes.iter().any(|path| rust_input(path)),
            tree_changed: resync || (token != Some(&old.token) && old.tree_changed),
            resync,
            changes,
            mode: old.mode(),
        })
    }
    pub fn list(&self, root: &Path) -> Result<Vec<WorkspaceEntry>, WatchError> {
        let mut state = self.0.lock().map_err(|_| WatchError::Unavailable)?;
        let paths: Vec<_> = state
            .as_ref()
            .map(|old| old.extras.iter().cloned().collect())
            .unwrap_or_default();
        // Advancing the cache keeps manual refresh current. The token/replay
        // protocol still delivers these changes to the next frontend poll.
        let old = update(&mut state, root, &paths)?;
        let mut children: BTreeMap<&str, Vec<WorkspaceEntry>> = BTreeMap::new();
        for (path, stamp) in &old.entries {
            if excluded(path) || (!stamp.directory && !stamp.regular) {
                continue;
            }
            let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
            children.entry(parent).or_default().push(WorkspaceEntry {
                path: path.clone(),
                name: name.into(),
                kind: if stamp.directory {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                },
            });
        }
        for entries in children.values_mut() {
            entries.sort_by(|a, b| {
                (
                    a.kind != EntryKind::Directory,
                    a.name.to_lowercase(),
                    &a.name,
                )
                    .cmp(&(
                        b.kind != EntryKind::Directory,
                        b.name.to_lowercase(),
                        &b.name,
                    ))
            });
        }
        fn flatten(
            parent: &str,
            children: &mut BTreeMap<&str, Vec<WorkspaceEntry>>,
            out: &mut Vec<WorkspaceEntry>,
        ) {
            for entry in children.remove(parent).unwrap_or_default() {
                let path = entry.path.clone();
                let directory = entry.kind == EntryKind::Directory;
                out.push(entry);
                if directory {
                    flatten(&path, children, out);
                }
            }
        }
        let mut result = Vec::new();
        flatten("", &mut children, &mut result);
        Ok(result)
    }
}
fn validate_paths(paths: &[String]) -> Result<(), WatchError> {
    if paths.len() > 128 {
        return Err(WatchError::TooLarge);
    }
    for path in paths {
        if path.is_empty()
            || path.len() > 4096
            || path.contains(['\\', '\0'])
            || path
                .split('/')
                .any(|p| matches!(p, "" | "." | ".." | ".git"))
            || path.split('/').count() > MAX_DEPTH
        {
            return Err(WatchError::InvalidPath);
        }
    }
    Ok(())
}
fn excluded(path: &str) -> bool {
    path.split('/')
        .any(|p| matches!(p, ".git" | "target" | "node_modules"))
}
fn subtree(entries: &BTreeMap<String, Stamp>, path: &str) -> Vec<String> {
    let prefix = format!("{path}/");
    entries
        .range(prefix.clone()..)
        .take_while(|(p, _)| p.starts_with(&prefix))
        .map(|(p, _)| p.clone())
        .chain(entries.contains_key(path).then(|| path.to_owned()))
        .collect()
}
impl Observed {
    fn mode(&self) -> &'static str {
        #[cfg(target_os = "linux")]
        if self.tracker.as_ref().is_some_and(|t| t.native()) {
            return "events";
        }
        "polling"
    }
    fn apply(
        &mut self,
        updates: BTreeMap<String, Option<Stamp>>,
        force: bool,
        touched: &BTreeSet<String>,
    ) -> Result<(), WatchError> {
        let mut count = self.entries.len();
        let mut bytes = self.path_bytes;
        let mut changes = Vec::new();
        let mut tree_changed = false;
        for (path, stamp) in &updates {
            let old = self.entries.get(path);
            let unchanged = match (old, stamp.as_ref()) {
                (Some(a), Some(b)) => {
                    a.directory == b.directory
                        && a.regular == b.regular
                        && a.identity == b.identity
                        && a.version == b.version
                        && a.digest.zip(b.digest).is_none_or(|(a, b)| a == b)
                }
                (None, None) => true,
                _ => false,
            };
            if unchanged && !(touched.contains(path) && stamp.as_ref().is_some_and(|s| s.regular)) {
                continue;
            }
            if old.is_none() {
                count += 1;
                bytes += path.len();
            }
            if stamp.is_none() {
                count -= 1;
                bytes -= path.len();
            }
            tree_changed |= old.map(|s| (s.directory, s.regular))
                != stamp.as_ref().map(|s| (s.directory, s.regular));
            changes.push(path.clone());
        }
        if count > MAX_ENTRIES || bytes > MAX_PATH_BYTES {
            return Err(WatchError::TooLarge);
        }
        for (path, stamp) in updates {
            if let Some(stamp) = stamp {
                self.entries.insert(path, stamp);
            } else {
                self.entries.remove(&path);
            }
        }
        self.path_bytes = bytes;
        if !changes.is_empty() || force {
            self.previous = Some(std::mem::replace(
                &mut self.token,
                uuid::Uuid::new_v4().to_string(),
            ));
            self.changes = changes;
            self.tree_changed = tree_changed;
            self.force_resync = force;
        }
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn build(root: &Path, paths: &[String]) -> Result<Observed, WatchError> {
    let (tracker, entries, inspected) = linux::Tracker::build(root, paths)?;
    let path_bytes = entries.keys().map(String::len).sum();
    let _ = inspected;
    Ok(Observed {
        root: root.into(),
        token: uuid::Uuid::new_v4().to_string(),
        previous: None,
        entries,
        changes: vec![],
        tree_changed: false,
        force_resync: false,
        tracker: Some(tracker),
        audited: Instant::now(),
        extras: paths.iter().filter(|p| excluded(p)).cloned().collect(),
        path_bytes,
        #[cfg(test)]
        inspected,
    })
}
#[cfg(not(target_os = "linux"))]
fn build(_: &Path, _: &[String]) -> Result<Observed, WatchError> {
    Err(WatchError::UnsupportedPlatform)
}
fn update<'a>(
    state: &'a mut Option<Observed>,
    root: &Path,
    paths: &[String],
) -> Result<&'a mut Observed, WatchError> {
    if state.as_ref().is_none_or(|old| old.root != root) {
        *state = Some(build(root, paths)?);
    } else {
        #[cfg(target_os = "linux")]
        {
            let old = state.as_mut().unwrap();
            if let Err(error) = linux::advance(old, paths) {
                old.tracker = None;
                return Err(error);
            }
        }
    }
    Ok(state.as_mut().unwrap())
}
fn rust_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.ends_with(".rs")
        || matches!(
            name,
            "Cargo.toml"
                | "Cargo.lock"
                | "rust-analyzer.toml"
                | ".rust-analyzer.toml"
                | "rust-toolchain"
                | "rust-toolchain.toml"
                | "rustfmt.toml"
                | ".rustfmt.toml"
        )
        || path == ".cargo/config"
        || path == ".cargo/config.toml"
        || path.ends_with("/.cargo/config")
        || path.ends_with("/.cargo/config.toml")
}
#[cfg(test)]
mod tests {
    use super::*;
    pub(super) struct Fixture(pub(super) PathBuf);
    impl Fixture {
        pub(super) fn new() -> Self {
            let path = std::env::temp_dir().join(format!("lyrnova-watch-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn open_ignored_file_keeps_content_verification_without_native_events() {
        let f = Fixture::new();
        std::fs::create_dir(f.0.join("target")).unwrap();
        let path = f.0.join("target/open.txt");
        std::fs::write(&path, "one").unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let service = WorkspaceWatchService::default();
        let first = service
            .poll(&f.0, None, &["target/open.txt".into()])
            .unwrap();
        std::fs::write(&path, "two").unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let changed = service
            .poll(&f.0, Some(&first.token), &["target/open.txt".into()])
            .unwrap();
        assert_eq!(changed.changes, ["target/open.txt"]);
        assert!(
            service.0.lock().unwrap().as_ref().unwrap().entries["target/open.txt"]
                .digest
                .is_some()
        );
    }
    #[test]
    fn opening_an_unchanged_source_and_periodic_audit_do_not_restart_analysis() {
        let f = Fixture::new();
        std::fs::write(f.0.join("lib.rs"), "fn f() {}\n").unwrap();
        let service = WorkspaceWatchService::default();
        let first = service.poll(&f.0, None, &[]).unwrap();
        let opened = service
            .poll(&f.0, Some(&first.token), &["lib.rs".into()])
            .unwrap();
        assert!(!opened.rust_changed);
        assert_eq!(opened.token, first.token);
        service.0.lock().unwrap().as_mut().unwrap().audited = Instant::now() - AUDIT_INTERVAL;
        let audit = service
            .poll(&f.0, Some(&opened.token), &["lib.rs".into()])
            .unwrap();
        assert!(!audit.rust_changed);
        assert_eq!(audit.token, first.token);
    }
    #[test]
    fn large_workspace_idle_and_single_file_edit_do_not_rescan_the_tree() {
        let f = Fixture::new();
        for d in 0..12 {
            let dir = f.0.join(format!("dir{d}"));
            std::fs::create_dir(&dir).unwrap();
            for n in 0..1000 {
                std::fs::write(dir.join(format!("file{n}.txt")), "before").unwrap();
            }
        }
        let service = WorkspaceWatchService::default();
        let first = service.poll(&f.0, None, &[]).unwrap();
        assert_eq!(first.mode, "events");
        assert_eq!(service.list(&f.0).unwrap().len(), 12012);
        let idle = service.poll(&f.0, Some(&first.token), &[]).unwrap();
        assert!(idle.changes.is_empty());
        assert_eq!(service.0.lock().unwrap().as_ref().unwrap().inspected, 0);
        std::fs::write(f.0.join("dir7/file845.txt"), "edited").unwrap();
        let edit = service.poll(&f.0, Some(&idle.token), &[]).unwrap();
        assert_eq!(edit.changes, ["dir7/file845.txt"]);
        assert!(!edit.rust_changed);
        assert!(!edit.tree_changed);
        assert_eq!(service.0.lock().unwrap().as_ref().unwrap().inspected, 1);
        let token = edit.token;
        std::fs::create_dir(f.0.join("added")).unwrap();
        std::fs::write(f.0.join("added/lib.rs"), "fn f() {}").unwrap();
        let added = service.poll(&f.0, Some(&token), &[]).unwrap();
        assert_eq!(added.changes, ["added", "added/lib.rs"]);
        assert!(added.rust_changed && added.tree_changed);
        assert!(service.0.lock().unwrap().as_ref().unwrap().inspected <= 2);
    }
    #[test]
    fn directory_moves_remove_old_watches_and_observe_new_descendants() {
        let f = Fixture::new();
        let outside = Fixture::new();
        std::fs::create_dir_all(f.0.join("z/sub")).unwrap();
        std::fs::write(f.0.join("z/sub/lib.rs"), "one").unwrap();
        let service = WorkspaceWatchService::default();
        let first = service.poll(&f.0, None, &[]).unwrap();
        std::fs::rename(f.0.join("z"), f.0.join("a")).unwrap();
        let moved = service.poll(&f.0, Some(&first.token), &[]).unwrap();
        assert_eq!(
            moved.changes,
            ["a", "a/sub", "a/sub/lib.rs", "z", "z/sub", "z/sub/lib.rs"]
        );
        std::fs::write(f.0.join("a/sub/lib.rs"), "two").unwrap();
        let changed = service.poll(&f.0, Some(&moved.token), &[]).unwrap();
        assert_eq!(changed.changes, ["a/sub/lib.rs"]);
        std::fs::rename(f.0.join("a"), outside.0.join("a")).unwrap();
        std::fs::write(outside.0.join("a/sub/private.rs"), "secret").unwrap();
        let removed = service.poll(&f.0, Some(&changed.token), &[]).unwrap();
        assert!(!removed.changes.iter().any(|p| p.contains("private")));
        assert!(service.list(&f.0).unwrap().is_empty());
        std::fs::write(outside.0.join("a/sub/private.rs"), "changed outside").unwrap();
        assert!(
            service
                .poll(&f.0, Some(&removed.token), &[])
                .unwrap()
                .changes
                .is_empty()
        );
        std::fs::rename(outside.0.join("a"), f.0.join("returned")).unwrap();
        let returned = service.poll(&f.0, Some(&removed.token), &[]).unwrap();
        assert!(returned.changes.contains(&"returned/sub/private.rs".into()));
    }
    #[test]
    fn replaced_directory_symlink_never_reads_external_descendants() {
        let f = Fixture::new();
        let outside = Fixture::new();
        std::fs::create_dir(f.0.join("src")).unwrap();
        std::fs::write(f.0.join("src/lib.rs"), "one").unwrap();
        std::fs::write(outside.0.join("private.rs"), "secret").unwrap();
        let service = WorkspaceWatchService::default();
        let first = service.poll(&f.0, None, &[]).unwrap();
        std::fs::remove_dir_all(f.0.join("src")).unwrap();
        std::os::unix::fs::symlink(&outside.0, f.0.join("src")).unwrap();
        let changed = service
            .poll(&f.0, Some(&first.token), &["src/lib.rs".into()])
            .unwrap();
        assert_eq!(changed.changes, ["src", "src/lib.rs"]);
        assert!(service.list(&f.0).unwrap().is_empty());
        assert!(
            !service
                .0
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .entries
                .contains_key("src/private.rs")
        );
    }
    #[test]
    fn audit_and_recovery_do_not_drop_unacknowledged_changes() {
        let f = Fixture::new();
        std::fs::write(f.0.join("lib.rs"), "one").unwrap();
        let service = WorkspaceWatchService::default();
        let first = service.poll(&f.0, None, &[]).unwrap();
        service.0.lock().unwrap().as_mut().unwrap().audited = Instant::now() - AUDIT_INTERVAL;
        assert_eq!(
            service.poll(&f.0, Some(&first.token), &[]).unwrap().token,
            first.token
        );
        std::fs::write(f.0.join("lib.rs"), "two").unwrap();
        service.list(&f.0).unwrap();
        let changed = service.poll(&f.0, Some(&first.token), &[]).unwrap();
        assert_eq!(changed.changes, ["lib.rs"]);
        service.0.lock().unwrap().as_mut().unwrap().tracker = None;
        let reset = service.poll(&f.0, Some(&changed.token), &[]).unwrap();
        assert!(reset.resync && reset.rust_changed);
        assert!(
            service
                .poll(&f.0, Some(&changed.token), &[])
                .unwrap()
                .resync
        );
        assert!(!service.poll(&f.0, Some(&reset.token), &[]).unwrap().resync);
        service.stop().unwrap();
        assert!(service.0.lock().unwrap().is_none());
        assert!(service.poll(&f.0, Some(&reset.token), &[]).unwrap().resync);
    }
    #[test]
    fn observes_create_atomic_replace_rename_delete_and_replays_a_missed_batch() {
        let f = Fixture::new();
        let service = WorkspaceWatchService::default();
        let initial = service.poll(&f.0, None, &[]).unwrap();
        assert!(initial.resync);
        std::fs::write(f.0.join("lib.rs"), "one").unwrap();
        let created = service.poll(&f.0, Some(&initial.token), &[]).unwrap();
        assert_eq!(created.changes, ["lib.rs"]);
        assert!(created.rust_changed);
        assert_eq!(
            service
                .poll(&f.0, Some(&initial.token), &[])
                .unwrap()
                .changes,
            created.changes
        );
        std::fs::write(f.0.join("temp"), "two").unwrap();
        std::fs::rename(f.0.join("temp"), f.0.join("lib.rs")).unwrap();
        let replaced = service.poll(&f.0, Some(&created.token), &[]).unwrap();
        assert_eq!(replaced.changes, ["lib.rs"]);
        assert!(
            service
                .poll(&f.0, Some(&initial.token), &[])
                .unwrap()
                .resync
        );
        std::fs::rename(f.0.join("lib.rs"), f.0.join("other.rs")).unwrap();
        let moved = service.poll(&f.0, Some(&replaced.token), &[]).unwrap();
        assert_eq!(moved.changes, ["lib.rs", "other.rs"]);
        std::fs::remove_file(f.0.join("other.rs")).unwrap();
        assert_eq!(
            service.poll(&f.0, Some(&moved.token), &[]).unwrap().changes,
            ["other.rs"]
        );
    }
    #[test]
    fn same_size_rewrites_are_detected_even_when_mtime_is_restored() {
        let f = Fixture::new();
        let path = f.0.join("lib.rs");
        std::fs::write(&path, "one").unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let service = WorkspaceWatchService::default();
        let initial = service.poll(&f.0, None, &[]).unwrap();
        std::fs::write(&path, "two").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert_eq!(
            service
                .poll(&f.0, Some(&initial.token), &[])
                .unwrap()
                .changes,
            ["lib.rs"]
        );
    }
    #[test]
    fn ignores_build_trees_but_checks_open_files_and_never_follows_symlinks() {
        let f = Fixture::new();
        let outside = Fixture::new();
        std::fs::create_dir(f.0.join("target")).unwrap();
        std::fs::write(f.0.join("target/open.rs"), "one").unwrap();
        std::fs::write(outside.0.join("secret.rs"), "private").unwrap();
        std::os::unix::fs::symlink(&outside.0, f.0.join("linked")).unwrap();
        let service = WorkspaceWatchService::default();
        let initial = service.poll(&f.0, None, &[]).unwrap();
        std::fs::write(f.0.join("target/open.rs"), "two").unwrap();
        std::fs::write(outside.0.join("secret.rs"), "changed").unwrap();
        assert!(
            service
                .poll(&f.0, Some(&initial.token), &[])
                .unwrap()
                .changes
                .is_empty()
        );
        let tracked = service
            .poll(&f.0, Some(&initial.token), &["target/open.rs".into()])
            .unwrap();
        assert_eq!(tracked.changes, ["target/open.rs"]);
        assert!(
            !service
                .0
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .entries
                .contains_key("linked/secret.rs")
        );
        assert!(service.poll(&f.0, None, &["../secret.rs".into()]).is_err());
        assert!(service.poll(&f.0, None, &[".git/config".into()]).is_err());
    }
    #[test]
    fn cursors_are_workspace_bound_and_limits_fail_without_replacing_the_last_snapshot() {
        let f = Fixture::new();
        let other = Fixture::new();
        let service = WorkspaceWatchService::default();
        let initial = service.poll(&f.0, None, &[]).unwrap();
        assert!(
            service
                .poll(&other.0, Some(&initial.token), &[])
                .unwrap()
                .resync
        );
        let initial = service.poll(&f.0, None, &[]).unwrap();
        let mut nested = f.0.clone();
        for _ in 0..=MAX_DEPTH {
            nested.push("deep");
            std::fs::create_dir(&nested).unwrap();
        }
        assert!(matches!(
            service.poll(&f.0, Some(&initial.token), &[]),
            Err(WatchError::TooLarge)
        ));
        std::fs::remove_dir_all(&f.0).unwrap();
        std::fs::create_dir(&f.0).unwrap();
        assert!(
            service
                .poll(&f.0, Some(&initial.token), &[])
                .unwrap()
                .resync
        );
    }
}
