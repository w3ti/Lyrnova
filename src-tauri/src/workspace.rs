use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_WORKSPACE_ENTRIES: usize = 5_000;
const IGNORED_DIRECTORIES: &[&str] = &[".git", "node_modules", "target"];
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct WorkspaceService {
    root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSnapshot {
    pub path: String,
    pub content: String,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDocumentRequest {
    pub path: String,
    pub content: String,
    pub expected_revision: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum WorkspaceError {
    NoWorkspace,
    InvalidPath,
    PathEscapesWorkspace,
    SymbolicLink,
    NotAFile,
    NotUtf8,
    DocumentTooLarge,
    TooManyEntries,
    InvalidProjectName,
    ProjectAlreadyExists,
    Conflict,
    Io,
}

impl WorkspaceService {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|_| WorkspaceError::Io)?;
        if !root.is_dir() {
            return Err(WorkspaceError::InvalidPath);
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list(&self) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        let mut entries = Vec::new();
        self.walk(&self.root, &mut entries)?;
        Ok(entries)
    }

    pub fn read(&self, relative: &str) -> Result<DocumentSnapshot, WorkspaceError> {
        let path = self.existing_file(relative)?;
        let bytes = fs::read(&path).map_err(|_| WorkspaceError::Io)?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(WorkspaceError::DocumentTooLarge);
        }
        let revision = revision(&bytes);
        let content = String::from_utf8(bytes).map_err(|_| WorkspaceError::NotUtf8)?;
        Ok(DocumentSnapshot {
            path: relative.replace('\\', "/"),
            content,
            revision,
        })
    }

    pub fn save(&self, request: SaveDocumentRequest) -> Result<DocumentSnapshot, WorkspaceError> {
        if request.content.len() > MAX_DOCUMENT_BYTES {
            return Err(WorkspaceError::DocumentTooLarge);
        }

        let target = self.existing_file(&request.path)?;
        let current = fs::read(&target).map_err(|_| WorkspaceError::Io)?;
        if revision(&current) != request.expected_revision {
            return Err(WorkspaceError::Conflict);
        }

        let parent = target.parent().ok_or(WorkspaceError::InvalidPath)?;
        let metadata = fs::metadata(&target).map_err(|_| WorkspaceError::Io)?;
        let temp = self.create_temp_file(parent, request.content.as_bytes())?;
        fs::set_permissions(&temp, metadata.permissions()).map_err(|_| {
            let _ = fs::remove_file(&temp);
            WorkspaceError::Io
        })?;
        if fs::rename(&temp, &target).is_err() {
            let _ = fs::remove_file(&temp);
            return Err(WorkspaceError::Io);
        }

        self.read(&request.path)
    }

    fn walk(
        &self,
        directory: &Path,
        entries: &mut Vec<WorkspaceEntry>,
    ) -> Result<(), WorkspaceError> {
        let mut children: Vec<_> = fs::read_dir(directory)
            .map_err(|_| WorkspaceError::Io)?
            .filter_map(Result::ok)
            .collect();
        children.sort_by(|left, right| {
            let directory_rank = |entry: &fs::DirEntry| {
                entry
                    .file_type()
                    .map_or(1, |file_type| usize::from(!file_type.is_dir()))
            };
            directory_rank(left)
                .cmp(&directory_rank(right))
                .then_with(|| {
                    left.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .cmp(&right.file_name().to_string_lossy().to_lowercase())
                })
                .then_with(|| left.file_name().cmp(&right.file_name()))
        });

        for child in children {
            if entries.len() >= MAX_WORKSPACE_ENTRIES {
                return Err(WorkspaceError::TooManyEntries);
            }
            let file_name = child.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            let metadata = child
                .path()
                .symlink_metadata()
                .map_err(|_| WorkspaceError::Io)?;
            if metadata.file_type().is_symlink() {
                continue;
            }

            let relative = child
                .path()
                .strip_prefix(&self.root)
                .map_err(|_| WorkspaceError::PathEscapesWorkspace)?
                .to_string_lossy()
                .replace('\\', "/");

            if metadata.is_dir() {
                if IGNORED_DIRECTORIES.contains(&name) {
                    continue;
                }
                entries.push(WorkspaceEntry {
                    path: relative,
                    name: name.into(),
                    kind: EntryKind::Directory,
                });
                self.walk(&child.path(), entries)?;
            } else if metadata.is_file() {
                entries.push(WorkspaceEntry {
                    path: relative,
                    name: name.into(),
                    kind: EntryKind::File,
                });
            }
        }
        Ok(())
    }

    fn existing_file(&self, relative: &str) -> Result<PathBuf, WorkspaceError> {
        let relative = validate_relative(relative)?;
        self.reject_symlink_components(&relative)?;
        let path = self
            .root
            .join(&relative)
            .canonicalize()
            .map_err(|_| WorkspaceError::NotAFile)?;
        if !path.starts_with(&self.root) {
            return Err(WorkspaceError::PathEscapesWorkspace);
        }
        if !path.is_file() {
            return Err(WorkspaceError::NotAFile);
        }
        Ok(path)
    }

    fn reject_symlink_components(&self, relative: &Path) -> Result<(), WorkspaceError> {
        let mut candidate = self.root.clone();
        for component in relative.components() {
            candidate.push(component.as_os_str());
            match fs::symlink_metadata(&candidate) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(WorkspaceError::SymbolicLink);
                }
                Ok(_) => {}
                Err(_) => return Err(WorkspaceError::NotAFile),
            }
        }
        Ok(())
    }

    fn create_temp_file(&self, parent: &Path, content: &[u8]) -> Result<PathBuf, WorkspaceError> {
        for _ in 0..16 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".lyrnova-save-{}-{sequence}", std::process::id()));
            let file = OpenOptions::new().write(true).create_new(true).open(&path);
            let Ok(mut file) = file else {
                continue;
            };

            if file
                .write_all(content)
                .and_then(|_| file.sync_all())
                .is_err()
            {
                let _ = fs::remove_file(&path);
                return Err(WorkspaceError::Io);
            }
            return Ok(path);
        }
        Err(WorkspaceError::Io)
    }
}

fn validate_relative(relative: &str) -> Result<PathBuf, WorkspaceError> {
    if relative.is_empty() || relative.contains('\0') {
        return Err(WorkspaceError::InvalidPath);
    }
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(WorkspaceError::InvalidPath);
    }
    Ok(path.to_path_buf())
}

fn revision(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "lyrnova-workspace-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lists_files_but_skips_generated_and_symlink_entries() {
        let workspace = TestWorkspace::new();
        fs::create_dir(workspace.0.join("src")).unwrap();
        fs::create_dir(workspace.0.join("assets")).unwrap();
        fs::create_dir(workspace.0.join("target")).unwrap();
        fs::write(workspace.0.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(workspace.0.join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(workspace.0.join("README.md"), "# Test\n").unwrap();
        fs::write(workspace.0.join("target/output"), "ignored").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("src/main.rs", workspace.0.join("linked.rs")).unwrap();

        let service = WorkspaceService::new(&workspace.0).unwrap();
        let entries = service.list().unwrap();
        let paths: Vec<_> = entries.iter().map(|entry| entry.path.as_str()).collect();

        assert!(paths.contains(&"src"));
        assert!(paths.contains(&"src/main.rs"));
        assert!(!paths.iter().any(|path| path.starts_with("target")));
        assert!(!paths.contains(&"linked.rs"));
        assert_eq!(
            paths,
            ["assets", "src", "src/main.rs", "Cargo.toml", "README.md"]
        );
    }

    #[test]
    fn rejects_absolute_parent_and_symlink_paths() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join("safe.txt"), "safe").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        assert_eq!(
            service.read("../safe.txt"),
            Err(WorkspaceError::InvalidPath)
        );
        assert_eq!(
            service.read("/etc/passwd"),
            Err(WorkspaceError::InvalidPath)
        );

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("safe.txt", workspace.0.join("linked.txt")).unwrap();
            assert_eq!(
                service.read("linked.txt"),
                Err(WorkspaceError::SymbolicLink)
            );
        }
    }

    #[test]
    fn reads_and_atomically_saves_an_existing_utf8_file() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join("notes.md"), "antes\n").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();
        let initial = service.read("notes.md").unwrap();

        let saved = service
            .save(SaveDocumentRequest {
                path: "notes.md".into(),
                content: "depois\n".into(),
                expected_revision: initial.revision,
            })
            .unwrap();

        assert_eq!(saved.content, "depois\n");
        assert_eq!(
            fs::read_to_string(workspace.0.join("notes.md")).unwrap(),
            "depois\n"
        );
    }

    #[test]
    fn refuses_to_overwrite_a_file_changed_after_read() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join("notes.md"), "original\n").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();
        let initial = service.read("notes.md").unwrap();
        fs::write(workspace.0.join("notes.md"), "external\n").unwrap();

        assert_eq!(
            service.save(SaveDocumentRequest {
                path: "notes.md".into(),
                content: "editor\n".into(),
                expected_revision: initial.revision,
            }),
            Err(WorkspaceError::Conflict)
        );
        assert_eq!(
            fs::read_to_string(workspace.0.join("notes.md")).unwrap(),
            "external\n"
        );
    }

    #[test]
    fn refuses_binary_and_oversized_documents() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join("binary"), [0xff, 0xfe]).unwrap();
        fs::write(
            workspace.0.join("large.txt"),
            vec![b'x'; MAX_DOCUMENT_BYTES + 1],
        )
        .unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        assert_eq!(service.read("binary"), Err(WorkspaceError::NotUtf8));
        assert_eq!(
            service.read("large.txt"),
            Err(WorkspaceError::DocumentTooLarge)
        );
    }
}
