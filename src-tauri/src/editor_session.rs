//! Private editor recovery snapshots. These files never write into a workspace.
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_DRAFT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentState {
    pub path: String,
    pub revision: Option<String>,
    pub draft: Option<String>,
    pub line: u32,
    pub column: u32,
    pub scroll_top: u32,
    pub scroll_left: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorSession {
    pub version: u32,
    pub documents: Vec<DocumentState>,
    pub active: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSession {
    pub token: String,
    pub session: Option<EditorSession>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionError {
    Unavailable,
    Invalid,
    TooLarge,
    Conflict,
    Busy,
    WorkspaceChanged,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate(session: &EditorSession) -> Result<(), SessionError> {
    if session.version != 1 || session.documents.len() > 64 {
        return Err(SessionError::Invalid);
    }
    let mut paths = HashSet::new();
    for doc in &session.documents {
        let path = Path::new(&doc.path);
        if doc.path.is_empty()
            || doc.path.len() > 4096
            || doc.path.contains(['\0', '\\'])
            || doc
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".." || part == ".git")
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || !paths.insert(&doc.path)
            || doc.line == 0
            || doc.column == 0
            || doc
                .revision
                .as_ref()
                .is_some_and(|r| r.len() != 64 || !r.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(SessionError::Invalid);
        }
        if doc
            .draft
            .as_ref()
            .is_some_and(|d| d.len() > MAX_DRAFT_BYTES)
        {
            return Err(SessionError::TooLarge);
        }
    }
    if session.active.as_ref().is_some_and(|p| !paths.contains(p)) {
        return Err(SessionError::Invalid);
    }
    Ok(())
}

struct SessionLock(File);
impl Drop for SessionLock {
    fn drop(&mut self) {
        // A concurrent process spawn can briefly inherit the descriptor before exec.
        // Explicit unlock prevents that child from extending this transaction's lock.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, workspace: &Path) -> PathBuf {
        use std::os::unix::ffi::OsStrExt;
        self.root
            .join(format!("{}.json", digest(workspace.as_os_str().as_bytes())))
    }

    fn lock(&self, workspace: &Path) -> Result<SessionLock, SessionError> {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.root)
            .map_err(|_| SessionError::Unavailable)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.path(workspace).with_extension("lock"))
            .map_err(|_| SessionError::Unavailable)?;
        // Advisory lock shared by all app instances; dropped with this descriptor.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(SessionError::Busy);
        }
        Ok(SessionLock(lock))
    }

    fn read(&self, workspace: &Path) -> Result<LoadedSession, SessionError> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(self.path(workspace))
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedSession {
                    token: "missing".into(),
                    session: None,
                });
            }
            Err(_) => return Err(SessionError::Unavailable),
        };
        if !file
            .metadata()
            .map_err(|_| SessionError::Unavailable)?
            .is_file()
        {
            return Err(SessionError::Invalid);
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SessionError::Unavailable)?;
        if bytes.len() > MAX_BYTES {
            return Err(SessionError::TooLarge);
        }
        let session = serde_json::from_slice(&bytes).map_err(|_| SessionError::Invalid)?;
        validate(&session)?;
        Ok(LoadedSession {
            token: digest(&bytes),
            session: Some(session),
        })
    }

    pub fn load(&self, workspace: &Path) -> Result<LoadedSession, SessionError> {
        let _lock = self.lock(workspace)?;
        self.read(workspace)
    }

    pub fn save(
        &self,
        workspace: &Path,
        token: &str,
        session: &EditorSession,
    ) -> Result<String, SessionError> {
        validate(session)?;
        let bytes = serde_json::to_vec(session).map_err(|_| SessionError::Invalid)?;
        if bytes.len() > MAX_BYTES {
            return Err(SessionError::TooLarge);
        }
        let _lock = self.lock(workspace)?;
        if self.read(workspace)?.token != token {
            return Err(SessionError::Conflict);
        }
        let temp = self.root.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, self.path(workspace))?;
            File::open(&self.root)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|_: std::io::Error| SessionError::Unavailable)?;
        Ok(digest(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("lyrnova-session-{}", uuid::Uuid::new_v4())))
        }
        fn store(&self) -> SessionStore {
            SessionStore::new(self.0.join("sessions"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn session() -> EditorSession {
        EditorSession {
            version: 1,
            active: Some("README.md".into()),
            documents: vec![DocumentState {
                path: "README.md".into(),
                revision: Some("a".repeat(64)),
                draft: Some("private draft".into()),
                line: 3,
                column: 2,
                scroll_top: 100,
                scroll_left: 0,
            }],
        }
    }
    #[test]
    fn roundtrip_private_atomic_snapshot_and_workspace_isolation() {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new();
        let store = f.store();
        let workspace = f.0.join("project");
        assert!(store.load(&workspace).unwrap().session.is_none());
        let token = store.save(&workspace, "missing", &session()).unwrap();
        let loaded = store.load(&workspace).unwrap();
        assert_eq!(loaded.token, token);
        assert_eq!(loaded.session, Some(session()));
        assert_eq!(
            fs::metadata(store.path(&workspace))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&store.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(!workspace.exists());
        assert!(store.load(&f.0.join("other")).unwrap().session.is_none());
        assert_eq!(
            store.save(&workspace, "missing", &session()),
            Err(SessionError::Conflict)
        );
        let empty = EditorSession {
            version: 1,
            active: None,
            documents: vec![],
        };
        store.save(&workspace, &token, &empty).unwrap();
        assert_eq!(store.load(&workspace).unwrap().session, Some(empty));
    }
    #[test]
    fn invalid_snapshots_never_replace_recoverable_drafts() {
        let f = Fixture::new();
        let store = f.store();
        let workspace = f.0.join("project");
        let token = store.save(&workspace, "missing", &session()).unwrap();
        for path in [
            "../secret",
            "/etc/passwd",
            "a/../b",
            ".git/config",
            "a//b",
            "a\\b",
            "",
        ] {
            let mut bad = session();
            bad.documents[0].path = path.into();
            bad.active = Some(path.into());
            assert_eq!(
                store.save(&workspace, &token, &bad),
                Err(SessionError::Invalid)
            );
        }
        let mut large = session();
        large.documents[0].draft = Some("x".repeat(MAX_DRAFT_BYTES + 1));
        assert_eq!(
            store.save(&workspace, &token, &large),
            Err(SessionError::TooLarge)
        );
        let mut duplicate = session();
        duplicate.documents.push(duplicate.documents[0].clone());
        assert_eq!(
            store.save(&workspace, &token, &duplicate),
            Err(SessionError::Invalid)
        );
        assert_eq!(store.load(&workspace).unwrap().session, Some(session()));
    }
    #[test]
    fn corrupt_future_and_symlink_files_are_preserved() {
        let f = Fixture::new();
        let store = f.store();
        let workspace = f.0.join("project");
        store.load(&workspace).unwrap();
        for bytes in [
            b"{broken".as_slice(),
            b"{\"version\":2,\"documents\":[],\"active\":null}",
        ] {
            fs::write(store.path(&workspace), bytes).unwrap();
            assert!(store.load(&workspace).is_err());
            assert!(store.save(&workspace, "missing", &session()).is_err());
            assert_eq!(fs::read(store.path(&workspace)).unwrap(), bytes);
        }
        fs::remove_file(store.path(&workspace)).unwrap();
        let target = f.0.join("secret");
        fs::write(&target, b"untouched").unwrap();
        std::os::unix::fs::symlink(&target, store.path(&workspace)).unwrap();
        assert!(store.save(&workspace, "missing", &session()).is_err());
        assert_eq!(fs::read(target).unwrap(), b"untouched");
    }

    #[test]
    fn transaction_unlocks_even_when_a_spawn_inherits_its_descriptor() {
        let f = Fixture::new();
        let store = f.store();
        let workspace = f.0.join("project");
        let lock = store.lock(&workspace).unwrap();
        let inherited = lock.0.try_clone().unwrap();
        assert!(matches!(store.load(&workspace), Err(SessionError::Busy)));
        drop(lock);
        // Duplicated descriptors share an open file description, just as after fork.
        assert!(store.load(&workspace).unwrap().session.is_none());
        drop(inherited);
    }

    #[test]
    fn bounds_total_snapshot_and_rejects_oversized_reads_without_truncation() {
        let f = Fixture::new();
        let store = f.store();
        let workspace = f.0.join("project");
        let token = store.save(&workspace, "missing", &session()).unwrap();
        let mut large = session();
        for index in 0..5 {
            let mut doc = large.documents[0].clone();
            doc.path = format!("file-{index}");
            doc.draft = Some("x".repeat(MAX_DRAFT_BYTES));
            large.documents.push(doc);
        }
        assert_eq!(
            store.save(&workspace, &token, &large),
            Err(SessionError::TooLarge)
        );
        assert_eq!(store.load(&workspace).unwrap().session, Some(session()));
        let bytes = vec![b'x'; MAX_BYTES + 1];
        fs::write(store.path(&workspace), &bytes).unwrap();
        assert!(matches!(
            store.load(&workspace),
            Err(SessionError::TooLarge)
        ));
        assert_eq!(
            fs::metadata(store.path(&workspace)).unwrap().len(),
            bytes.len() as u64
        );
    }
}
