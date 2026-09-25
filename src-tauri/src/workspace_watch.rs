//! Bounded metadata polling. No resident thread or watch descriptors survive a poll.
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_ENTRIES: usize = 5_000;
const MAX_DEPTH: usize = 64;
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
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    directory: bool,
    digest: Option<[u8; 32]>,
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
}
impl WorkspaceWatchService {
    pub fn poll(
        &self,
        root: &Path,
        token: Option<&str>,
        paths: &[String],
    ) -> Result<WatchSnapshot, WatchError> {
        if paths.len() > 128 || token.is_some_and(|s| s.len() > 64) {
            return Err(WatchError::TooLarge);
        }
        let mut state = self.0.lock().map_err(|_| WatchError::Unavailable)?;
        let entries = scan(root, paths)?;
        if state.as_ref().is_none_or(|old| old.root != root) {
            *state = Some(Observed {
                root: root.into(),
                token: uuid::Uuid::new_v4().to_string(),
                previous: None,
                entries,
                changes: vec![],
            });
        } else {
            let old = state.as_mut().unwrap();
            if entries != old.entries {
                old.changes = entries
                    .keys()
                    .chain(old.entries.keys())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .filter(|key| entries.get(*key) != old.entries.get(*key))
                    .cloned()
                    .collect();
                old.previous = Some(std::mem::replace(
                    &mut old.token,
                    uuid::Uuid::new_v4().to_string(),
                ));
                old.entries = entries;
            }
        }
        let old = state.as_ref().unwrap();
        let resync =
            token.is_none() || (token != Some(&old.token) && token != old.previous.as_deref());
        let changes = if !resync && token != Some(&old.token) {
            old.changes.clone()
        } else {
            vec![]
        };
        let rust_changed = resync || changes.iter().any(|path| rust_input(path));
        Ok(WatchSnapshot {
            token: old.token.clone(),
            resync,
            changes,
            rust_changed,
        })
    }
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
#[cfg(target_os = "linux")]
fn scan(root: &Path, paths: &[String]) -> Result<BTreeMap<String, Stamp>, WatchError> {
    use std::{
        fs::{self, File, OpenOptions},
        os::{
            fd::AsRawFd,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };
    fn directory(path: &Path) -> Result<File, WatchError> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| WatchError::Unavailable)
    }
    fn stamp(metadata: &fs::Metadata) -> Stamp {
        Stamp {
            directory: metadata.is_dir(),
            digest: None,
            identity: (metadata.dev(), metadata.ino()),
            version: if metadata.is_dir() {
                (0, 0, 0, 0, 0)
            } else {
                (
                    metadata.len(),
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        }
    }
    fn digest(
        path: &Path,
        meta: &fs::Metadata,
        budget: &mut u64,
    ) -> Result<Option<[u8; 32]>, WatchError> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        if !meta.is_file() {
            return Ok(None);
        }
        if meta.len() > 2 * 1024 * 1024 {
            return Ok(None);
        }
        if *budget + meta.len() > 32 * 1024 * 1024 {
            return Err(WatchError::TooLarge);
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| WatchError::Unavailable)?;
        if !file.metadata().is_ok_and(|m| m.is_file()) {
            return Err(WatchError::Unavailable);
        }
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| WatchError::Unavailable)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(WatchError::Unavailable);
        }
        *budget += bytes.len() as u64;
        if *budget > 32 * 1024 * 1024 {
            return Err(WatchError::TooLarge);
        }
        Ok(Some(Sha256::digest(bytes).into()))
    }
    fn walk(
        dir: File,
        relative: &str,
        depth: usize,
        entries: &mut BTreeMap<String, Stamp>,
        budget: &mut u64,
        visited: &mut usize,
    ) -> Result<(), WatchError> {
        if depth > MAX_DEPTH {
            return Err(WatchError::TooLarge);
        }
        // Every directory is pinned by an O_NOFOLLOW descriptor. Entry paths refer
        // to that descriptor, so concurrent parent renames cannot redirect traversal.
        let base = PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd()));
        for entry in fs::read_dir(&base).map_err(|_| WatchError::Unavailable)? {
            let entry = entry.map_err(|_| WatchError::Unavailable)?;
            *visited += 1;
            if *visited > MAX_ENTRIES {
                return Err(WatchError::TooLarge);
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if matches!(name, ".git" | "target" | "node_modules") {
                continue;
            }
            let path = if relative.is_empty() {
                name.into()
            } else {
                format!("{relative}/{name}")
            };
            if entries.len() >= MAX_ENTRIES || path.len() > 4096 {
                return Err(WatchError::TooLarge);
            }
            let meta = fs::symlink_metadata(entry.path()).map_err(|_| WatchError::Unavailable)?;
            let mut current = stamp(&meta);
            if rust_input(&path) {
                current.digest = digest(&entry.path(), &meta, budget)?;
            }
            entries.insert(path.clone(), current);
            if meta.is_dir() {
                walk(
                    directory(&entry.path())?,
                    &path,
                    depth + 1,
                    entries,
                    budget,
                    visited,
                )?;
            }
        }
        Ok(())
    }
    let mut entries = BTreeMap::new();
    let mut budget = 0;
    walk(directory(root)?, "", 0, &mut entries, &mut budget, &mut 0)?;
    // Explicit open tabs are checked even inside directories omitted from the tree.
    for path in paths {
        if path.is_empty()
            || path.len() > 4096
            || path.contains(['\\', '\0'])
            || path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".." | ".git"))
        {
            return Err(WatchError::InvalidPath);
        }
        let mut dir = directory(root)?;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let child = PathBuf::from(format!("/proc/self/fd/{}/{}", dir.as_raw_fd(), part));
            let Ok(meta) = fs::symlink_metadata(&child) else {
                entries.remove(path);
                break;
            };
            if parts.peek().is_none() || !meta.is_dir() {
                let mut current = stamp(&meta);
                current.digest = digest(&child, &meta, &mut budget)?;
                entries.insert(path.clone(), current);
                break;
            }
            dir = directory(&child)?;
        }
    }
    Ok(entries)
}
#[cfg(not(target_os = "linux"))]
fn scan(_: &Path, _: &[String]) -> Result<BTreeMap<String, Stamp>, WatchError> {
    Err(WatchError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
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
        assert!(!scan(&f.0, &[]).unwrap().contains_key("linked/secret.rs"));
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
        for n in 0..=MAX_ENTRIES {
            std::fs::write(f.0.join(format!("file{n}")), "").unwrap();
        }
        assert!(matches!(
            service.poll(&f.0, Some(&initial.token), &[]),
            Err(WatchError::TooLarge)
        ));
        std::fs::remove_dir_all(&f.0).unwrap();
        std::fs::create_dir(&f.0).unwrap();
        assert_eq!(
            service.poll(&f.0, Some(&initial.token), &[]).unwrap().token,
            initial.token
        );
    }
}
