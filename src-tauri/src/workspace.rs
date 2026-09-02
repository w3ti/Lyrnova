use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_DOCUMENT_RANGE_BYTES: usize = 256 * 1024;
const MAX_WORKSPACE_ENTRIES: usize = 5_000;
const MAX_SEARCH_QUERY_BYTES: usize = 256;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_SEARCH_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PATCH_EDITS: usize = 1_024;
const IGNORED_DIRECTORIES: &[&str] = &[".git", "node_modules", "target"];
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct WorkspaceService {
    root: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
pub struct ReadDocumentRangeRequest {
    pub path: String,
    pub start_byte: usize,
    pub max_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRangeSnapshot {
    pub path: String,
    pub content: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub total_bytes: usize,
    pub eof: bool,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMetadata {
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified_unix_ms: Option<u64>,
    pub binary: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDocumentRequest {
    pub path: String,
    pub content: String,
    pub expected_revision: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDocumentRequest {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveWorkspaceEntryRequest {
    pub source: String,
    pub destination: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorkspaceEntryRequest {
    pub path: String,
    pub expected_revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPatchEdit {
    pub start_byte: usize,
    pub end_byte: usize,
    pub expected: String,
    pub replacement: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyDocumentPatchRequest {
    pub path: String,
    pub expected_revision: String,
    pub edits: Vec<DocumentPatchEdit>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPatchPreview {
    pub path: String,
    pub current_revision: String,
    pub result_revision: String,
    pub content: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceSearchMatchKind {
    Path,
    Content,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchMatch {
    pub path: String,
    pub kind: WorkspaceSearchMatchKind,
    pub entry_kind: EntryKind,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub preview: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedWorkspaceEntry {
    pub recovery_token: String,
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Clone, Debug)]
struct RecoveryRecord {
    workspace_root: PathBuf,
    relative: PathBuf,
    stored: PathBuf,
    kind: EntryKind,
}

impl WorkspaceRecoveryService {
    pub fn cleanup_stale(recovery_root: &Path) -> Result<(), WorkspaceError> {
        let metadata = match fs::symlink_metadata(recovery_root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(WorkspaceError::RecoveryUnavailable),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(WorkspaceError::RecoveryUnavailable);
        }
        for entry in fs::read_dir(recovery_root).map_err(|_| WorkspaceError::RecoveryUnavailable)? {
            let entry = entry.map_err(|_| WorkspaceError::RecoveryUnavailable)?;
            let name = entry.file_name();
            let Some(token) = name.to_str() else {
                continue;
            };
            if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            let metadata = entry
                .path()
                .symlink_metadata()
                .map_err(|_| WorkspaceError::RecoveryUnavailable)?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(entry.path())
                    .map_err(|_| WorkspaceError::RecoveryUnavailable)?;
            }
        }
        Ok(())
    }

    pub fn delete(
        &self,
        workspace: &WorkspaceService,
        recovery_root: &Path,
        request: DeleteWorkspaceEntryRequest,
    ) -> Result<DeletedWorkspaceEntry, WorkspaceError> {
        let relative = validate_relative(&request.path)?;
        let (source, kind) = workspace.existing_entry(&relative)?;
        if kind == EntryKind::File
            && let Some(expected) = request.expected_revision.as_deref()
            && file_revision(&source)? != expected
        {
            return Err(WorkspaceError::Conflict);
        }
        create_private_directory_all(recovery_root)?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let ticket = recovery_root.join(&token);
        fs::create_dir(&ticket).map_err(|_| WorkspaceError::RecoveryUnavailable)?;
        set_private_directory_permissions(&ticket)?;
        let stored = ticket.join("content");
        if let Err(error) = rename_no_replace(&source, &stored) {
            let _ = fs::remove_dir(&ticket);
            return Err(error);
        }
        let record = RecoveryRecord {
            workspace_root: workspace.root.clone(),
            relative: relative.clone(),
            stored,
            kind,
        };
        let mut records = match self.records.lock() {
            Ok(records) => records,
            Err(_) => {
                let _ = rename_no_replace(&record.stored, &source);
                let _ = fs::remove_dir(&ticket);
                return Err(WorkspaceError::RecoveryUnavailable);
            }
        };
        records.insert(token.clone(), record);
        Ok(DeletedWorkspaceEntry {
            recovery_token: token,
            path: request.path.replace('\\', "/"),
            kind,
        })
    }

    pub fn restore(
        &self,
        workspace: &WorkspaceService,
        recovery_token: &str,
    ) -> Result<WorkspaceEntry, WorkspaceError> {
        if recovery_token.is_empty()
            || recovery_token.len() > 64
            || !recovery_token.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(WorkspaceError::UnknownRecovery);
        }
        let mut records = self
            .records
            .lock()
            .map_err(|_| WorkspaceError::RecoveryUnavailable)?;
        let record = records
            .get(recovery_token)
            .ok_or(WorkspaceError::UnknownRecovery)?;
        if record.workspace_root != workspace.root {
            return Err(WorkspaceError::UnknownRecovery);
        }
        let metadata = fs::symlink_metadata(&record.stored)
            .map_err(|_| WorkspaceError::RecoveryUnavailable)?;
        if metadata.file_type().is_symlink()
            || (record.kind == EntryKind::File && !metadata.is_file())
            || (record.kind == EntryKind::Directory && !metadata.is_dir())
        {
            return Err(WorkspaceError::RecoveryUnavailable);
        }
        let destination = workspace.new_entry_target(&record.relative)?;
        rename_no_replace(&record.stored, &destination)?;
        let relative = record.relative.clone();
        let ticket = record.stored.parent().map(Path::to_path_buf);
        records.remove(recovery_token);
        drop(records);
        if let Some(ticket) = ticket {
            let _ = fs::remove_dir(ticket);
        }
        workspace.entry_at(&relative)
    }
}

#[derive(Default)]
pub struct WorkspaceRecoveryService {
    records: Mutex<BTreeMap<String, RecoveryRecord>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum WorkspaceError {
    NoWorkspace,
    InvalidPath,
    PathEscapesWorkspace,
    SymbolicLink,
    NotAFile,
    BinaryFile,
    NotUtf8,
    DocumentTooLarge,
    TooManyEntries,
    InvalidProjectName,
    ProjectAlreadyExists,
    EntryAlreadyExists,
    NotADirectory,
    InvalidSearchQuery,
    InvalidRange,
    InvalidPatch,
    RecoveryUnavailable,
    UnknownRecovery,
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

    pub fn search(&self, query: &str) -> Result<Vec<WorkspaceSearchMatch>, WorkspaceError> {
        let query = query.trim();
        if query.is_empty()
            || query.len() > MAX_SEARCH_QUERY_BYTES
            || query.chars().any(char::is_control)
        {
            return Err(WorkspaceError::InvalidSearchQuery);
        }
        let normalized_query = query.to_lowercase();
        let entries = self.list()?;
        let mut matches = Vec::new();
        let mut scanned_bytes = 0u64;

        for entry in &entries {
            if entry.path.to_lowercase().contains(&normalized_query) {
                matches.push(WorkspaceSearchMatch {
                    path: entry.path.clone(),
                    kind: WorkspaceSearchMatchKind::Path,
                    entry_kind: entry.kind,
                    line: None,
                    column: None,
                    preview: entry.path.clone(),
                });
                if matches.len() >= MAX_SEARCH_RESULTS {
                    return Ok(matches);
                }
            }
            if entry.kind != EntryKind::File {
                continue;
            }
            let Ok(path) = self.existing_file(&entry.path) else {
                continue;
            };
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if metadata.len() > MAX_DOCUMENT_BYTES as u64
                || scanned_bytes.saturating_add(metadata.len()) > MAX_SEARCH_TOTAL_BYTES
            {
                continue;
            }
            scanned_bytes = scanned_bytes.saturating_add(metadata.len());
            let Some(content) = read_searchable_text(&path)? else {
                continue;
            };
            for (line_index, line) in content.lines().enumerate() {
                let normalized_line = line.to_lowercase();
                let Some(byte_column) = normalized_line.find(&normalized_query) else {
                    continue;
                };
                let column = normalized_line[..byte_column]
                    .chars()
                    .count()
                    .saturating_add(1);
                matches.push(WorkspaceSearchMatch {
                    path: entry.path.clone(),
                    kind: WorkspaceSearchMatchKind::Content,
                    entry_kind: EntryKind::File,
                    line: u32::try_from(line_index.saturating_add(1)).ok(),
                    column: u32::try_from(column).ok(),
                    preview: bounded_preview(line),
                });
                if matches.len() >= MAX_SEARCH_RESULTS {
                    return Ok(matches);
                }
            }
        }
        Ok(matches)
    }

    pub fn read(&self, relative: &str) -> Result<DocumentSnapshot, WorkspaceError> {
        let path = self.existing_file(relative)?;
        let bytes = fs::read(&path).map_err(|_| WorkspaceError::Io)?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(WorkspaceError::DocumentTooLarge);
        }
        if bytes.contains(&0) {
            return Err(WorkspaceError::BinaryFile);
        }
        let revision = revision(&bytes);
        let content = String::from_utf8(bytes).map_err(|_| WorkspaceError::NotUtf8)?;
        Ok(DocumentSnapshot {
            path: relative.replace('\\', "/"),
            content,
            revision,
        })
    }

    pub fn read_range(
        &self,
        request: ReadDocumentRangeRequest,
    ) -> Result<DocumentRangeSnapshot, WorkspaceError> {
        if request.max_bytes == 0 || request.max_bytes > MAX_DOCUMENT_RANGE_BYTES {
            return Err(WorkspaceError::InvalidRange);
        }
        let snapshot = self.read(&request.path)?;
        if request.start_byte > snapshot.content.len()
            || !snapshot.content.is_char_boundary(request.start_byte)
        {
            return Err(WorkspaceError::InvalidRange);
        }
        let requested_end = request
            .start_byte
            .saturating_add(request.max_bytes)
            .min(snapshot.content.len());
        let mut end_byte = requested_end;
        while end_byte > request.start_byte && !snapshot.content.is_char_boundary(end_byte) {
            end_byte -= 1;
        }
        Ok(DocumentRangeSnapshot {
            path: snapshot.path,
            content: snapshot.content[request.start_byte..end_byte].to_owned(),
            start_byte: request.start_byte,
            end_byte,
            total_bytes: snapshot.content.len(),
            eof: end_byte == snapshot.content.len(),
            revision: snapshot.revision,
        })
    }

    pub fn metadata(&self, relative: &str) -> Result<WorkspaceMetadata, WorkspaceError> {
        let relative = validate_relative(relative)?;
        let (path, kind) = self.existing_entry(&relative)?;
        let metadata = fs::metadata(&path).map_err(|_| WorkspaceError::Io)?;
        let binary = if kind == EntryKind::File {
            let mut file = fs::File::open(&path).map_err(|_| WorkspaceError::Io)?;
            let mut sample = vec![0; 8 * 1024];
            let read = file.read(&mut sample).map_err(|_| WorkspaceError::Io)?;
            sample.truncate(read);
            let invalid_utf8 =
                std::str::from_utf8(&sample).is_err_and(|error| error.error_len().is_some());
            Some(sample.contains(&0) || invalid_utf8)
        } else {
            None
        };
        let modified_unix_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|duration| u64::try_from(duration.as_millis()).ok());
        Ok(WorkspaceMetadata {
            path: relative.to_string_lossy().replace('\\', "/"),
            kind,
            size: metadata.len(),
            modified_unix_ms,
            binary,
        })
    }

    pub fn save(&self, request: SaveDocumentRequest) -> Result<DocumentSnapshot, WorkspaceError> {
        if request.content.len() > MAX_DOCUMENT_BYTES {
            return Err(WorkspaceError::DocumentTooLarge);
        }
        if request.content.contains('\0') {
            return Err(WorkspaceError::BinaryFile);
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
        let unchanged = fs::symlink_metadata(&target).is_ok_and(|current_metadata| {
            current_metadata.is_file()
                && !current_metadata.file_type().is_symlink()
                && same_file_identity(&metadata, &current_metadata)
        }) && file_revision(&target)
            .is_ok_and(|revision| revision == request.expected_revision);
        if !unchanged {
            let _ = fs::remove_file(&temp);
            return Err(WorkspaceError::Conflict);
        }
        if fs::rename(&temp, &target).is_err() {
            let _ = fs::remove_file(&temp);
            return Err(WorkspaceError::Io);
        }

        self.read(&request.path)
    }

    pub fn create_document(
        &self,
        request: CreateDocumentRequest,
    ) -> Result<DocumentSnapshot, WorkspaceError> {
        if request.content.len() > MAX_DOCUMENT_BYTES {
            return Err(WorkspaceError::DocumentTooLarge);
        }
        if request.content.contains('\0') {
            return Err(WorkspaceError::BinaryFile);
        }
        let relative = validate_relative(&request.path)?;
        let target = self.new_entry_target(&relative)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    WorkspaceError::EntryAlreadyExists
                } else {
                    WorkspaceError::Io
                }
            })?;
        if file
            .write_all(request.content.as_bytes())
            .and_then(|()| file.sync_all())
            .is_err()
        {
            drop(file);
            let _ = fs::remove_file(&target);
            return Err(WorkspaceError::Io);
        }
        drop(file);
        self.read(&request.path)
    }

    pub fn create_directory(&self, relative: &str) -> Result<WorkspaceEntry, WorkspaceError> {
        let relative = validate_relative(relative)?;
        let target = self.new_entry_target(&relative)?;
        fs::create_dir(&target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                WorkspaceError::EntryAlreadyExists
            } else {
                WorkspaceError::Io
            }
        })?;
        self.entry_at(&relative)
    }

    pub fn move_entry(
        &self,
        request: MoveWorkspaceEntryRequest,
    ) -> Result<WorkspaceEntry, WorkspaceError> {
        let source_relative = validate_relative(&request.source)?;
        let destination_relative = validate_relative(&request.destination)?;
        if source_relative == destination_relative
            || destination_relative.starts_with(&source_relative)
        {
            return Err(WorkspaceError::InvalidPath);
        }
        let (source, _) = self.existing_entry(&source_relative)?;
        let destination = self.new_entry_target(&destination_relative)?;
        rename_no_replace(&source, &destination)?;
        self.entry_at(&destination_relative)
    }

    pub fn apply_patch(
        &self,
        request: ApplyDocumentPatchRequest,
    ) -> Result<DocumentSnapshot, WorkspaceError> {
        let preview = self.preview_patch(&request)?;
        self.save(SaveDocumentRequest {
            path: request.path,
            content: preview.content,
            expected_revision: request.expected_revision,
        })
    }

    pub fn preview_patch(
        &self,
        request: &ApplyDocumentPatchRequest,
    ) -> Result<DocumentPatchPreview, WorkspaceError> {
        if request.edits.is_empty() || request.edits.len() > MAX_PATCH_EDITS {
            return Err(WorkspaceError::InvalidPatch);
        }
        let snapshot = self.read(&request.path)?;
        if snapshot.revision != request.expected_revision {
            return Err(WorkspaceError::Conflict);
        }
        let mut output = String::with_capacity(snapshot.content.len());
        let mut cursor = 0usize;
        for edit in &request.edits {
            if edit.start_byte < cursor
                || edit.start_byte > edit.end_byte
                || edit.end_byte > snapshot.content.len()
                || !snapshot.content.is_char_boundary(edit.start_byte)
                || !snapshot.content.is_char_boundary(edit.end_byte)
                || snapshot.content[edit.start_byte..edit.end_byte] != edit.expected
            {
                return Err(WorkspaceError::InvalidPatch);
            }
            output.push_str(&snapshot.content[cursor..edit.start_byte]);
            output.push_str(&edit.replacement);
            if output.len() > MAX_DOCUMENT_BYTES {
                return Err(WorkspaceError::DocumentTooLarge);
            }
            cursor = edit.end_byte;
        }
        output.push_str(&snapshot.content[cursor..]);
        if output.len() > MAX_DOCUMENT_BYTES {
            return Err(WorkspaceError::DocumentTooLarge);
        }
        Ok(DocumentPatchPreview {
            path: request.path.clone(),
            current_revision: snapshot.revision,
            result_revision: revision(output.as_bytes()),
            content: output,
        })
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

    fn existing_entry(&self, relative: &Path) -> Result<(PathBuf, EntryKind), WorkspaceError> {
        self.reject_symlink_components(relative)?;
        let path = self
            .root
            .join(relative)
            .canonicalize()
            .map_err(|_| WorkspaceError::InvalidPath)?;
        if !path.starts_with(&self.root) {
            return Err(WorkspaceError::PathEscapesWorkspace);
        }
        let metadata = fs::symlink_metadata(&path).map_err(|_| WorkspaceError::InvalidPath)?;
        let kind = if metadata.is_file() {
            EntryKind::File
        } else if metadata.is_dir() {
            EntryKind::Directory
        } else {
            return Err(WorkspaceError::InvalidPath);
        };
        Ok((path, kind))
    }

    fn existing_file(&self, relative: &str) -> Result<PathBuf, WorkspaceError> {
        let relative = validate_relative(relative)?;
        let (path, kind) = self
            .existing_entry(&relative)
            .map_err(|error| match error {
                WorkspaceError::InvalidPath => WorkspaceError::NotAFile,
                error => error,
            })?;
        if kind != EntryKind::File {
            return Err(WorkspaceError::NotAFile);
        }
        Ok(path)
    }

    fn new_entry_target(&self, relative: &Path) -> Result<PathBuf, WorkspaceError> {
        let name = relative.file_name().ok_or(WorkspaceError::InvalidPath)?;
        let parent_relative = relative.parent().ok_or(WorkspaceError::InvalidPath)?;
        let parent = if parent_relative.as_os_str().is_empty() {
            self.root.clone()
        } else {
            let (path, kind) =
                self.existing_entry(parent_relative)
                    .map_err(|error| match error {
                        WorkspaceError::NotAFile => WorkspaceError::InvalidPath,
                        error => error,
                    })?;
            if kind != EntryKind::Directory {
                return Err(WorkspaceError::NotADirectory);
            }
            path
        };
        let target = parent.join(name);
        match fs::symlink_metadata(&target) {
            Ok(_) => Err(WorkspaceError::EntryAlreadyExists),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(target),
            Err(_) => Err(WorkspaceError::Io),
        }
    }

    fn entry_at(&self, relative: &Path) -> Result<WorkspaceEntry, WorkspaceError> {
        let (_, kind) = self.existing_entry(relative)?;
        let path = relative
            .to_str()
            .ok_or(WorkspaceError::InvalidPath)?
            .replace('\\', "/");
        let name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(WorkspaceError::InvalidPath)?
            .to_owned();
        Ok(WorkspaceEntry { path, name, kind })
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
    if relative.is_empty()
        || relative.contains('\0')
        || relative.contains('\\')
        || relative.contains(':')
        || relative.chars().any(char::is_control)
        || relative
            .split('/')
            .any(|component| !is_portable_path_component(component))
    {
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

fn is_portable_path_component(component: &str) -> bool {
    if component.is_empty()
        || component.len() > 255
        || matches!(component, "." | "..")
        || component.ends_with('.')
        || component.ends_with(' ')
        || component
            .chars()
            .any(|character| matches!(character, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn read_searchable_text(path: &Path) -> Result<Option<String>, WorkspaceError> {
    let file = fs::File::open(path).map_err(|_| WorkspaceError::Io)?;
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| WorkspaceError::Io)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Ok(None);
    }
    if bytes.contains(&0) {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
}

fn bounded_preview(line: &str) -> String {
    let line = line.trim();
    let mut preview: String = line.chars().take(240).collect();
    if line.chars().count() > 240 {
        preview.push('…');
    }
    preview
}

fn file_revision(path: &Path) -> Result<String, WorkspaceError> {
    let mut file = fs::File::open(path).map_err(|_| WorkspaceError::Io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| WorkspaceError::Io)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn create_private_directory_all(path: &Path) -> Result<(), WorkspaceError> {
    fs::create_dir_all(path).map_err(|_| WorkspaceError::RecoveryUnavailable)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| WorkspaceError::RecoveryUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WorkspaceError::RecoveryUnavailable);
    }
    set_private_directory_permissions(path)
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), WorkspaceError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| WorkspaceError::RecoveryUnavailable)
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), WorkspaceError> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), WorkspaceError> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| WorkspaceError::InvalidPath)?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| WorkspaceError::InvalidPath)?;
    // SAFETY: both values are valid NUL-terminated paths and remain alive for the syscall.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EEXIST) => Err(WorkspaceError::EntryAlreadyExists),
        Some(libc::EXDEV) => Err(WorkspaceError::RecoveryUnavailable),
        _ => Err(WorkspaceError::Io),
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), WorkspaceError> {
    match fs::symlink_metadata(destination) {
        Ok(_) => return Err(WorkspaceError::EntryAlreadyExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(WorkspaceError::Io),
    }
    fs::rename(source, destination).map_err(|_| WorkspaceError::Io)
}

fn revision(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    true
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
    fn reads_bounded_ranges_without_splitting_utf8_characters() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join("utf8.txt"), "abácd").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        let first = service
            .read_range(ReadDocumentRangeRequest {
                path: "utf8.txt".into(),
                start_byte: 0,
                max_bytes: 3,
            })
            .unwrap();
        assert_eq!(first.content, "ab");
        assert_eq!(first.end_byte, 2);
        assert!(!first.eof);
        let second = service
            .read_range(ReadDocumentRangeRequest {
                path: "utf8.txt".into(),
                start_byte: first.end_byte,
                max_bytes: 4,
            })
            .unwrap();
        assert_eq!(second.content, "ácd");
        assert!(second.eof);
        assert_eq!(
            service.read_range(ReadDocumentRangeRequest {
                path: "utf8.txt".into(),
                start_byte: 3,
                max_bytes: 1,
            }),
            Err(WorkspaceError::InvalidRange)
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
        fs::write(workspace.0.join("nul-binary"), b"prefix\0suffix").unwrap();
        fs::write(
            workspace.0.join("large.txt"),
            vec![b'x'; MAX_DOCUMENT_BYTES + 1],
        )
        .unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        assert_eq!(service.read("binary"), Err(WorkspaceError::NotUtf8));
        assert_eq!(service.read("nul-binary"), Err(WorkspaceError::BinaryFile));
        assert_eq!(
            service.read("large.txt"),
            Err(WorkspaceError::DocumentTooLarge)
        );
        assert_eq!(service.metadata("binary").unwrap().binary, Some(true));
        assert_eq!(service.metadata("nul-binary").unwrap().binary, Some(true));
        assert_eq!(service.metadata("large.txt").unwrap().binary, Some(false));
        assert_eq!(
            service.create_document(CreateDocumentRequest {
                path: "new-binary".into(),
                content: "prefix\0suffix".into(),
            }),
            Err(WorkspaceError::BinaryFile)
        );
        assert!(!workspace.0.join("new-binary").exists());
    }

    #[test]
    fn searches_paths_and_utf8_content_with_fixed_limits() {
        let workspace = TestWorkspace::new();
        fs::create_dir(workspace.0.join("src")).unwrap();
        fs::create_dir(workspace.0.join("target")).unwrap();
        fs::write(
            workspace.0.join("src/main.rs"),
            "fn main() {\n    println!(\"Lyrnova\");\n}\n",
        )
        .unwrap();
        fs::write(workspace.0.join("src/binary.bin"), [0xff, 0xfe]).unwrap();
        fs::write(workspace.0.join("target/lyrnova.txt"), "ignored").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        let matches = service.search("lyrnova").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "src/main.rs");
        assert_eq!(matches[0].kind, WorkspaceSearchMatchKind::Content);
        assert_eq!(matches[0].line, Some(2));
        assert_eq!(matches[0].column, Some(15));
        assert_eq!(
            service.search("main.rs").unwrap()[0].kind,
            WorkspaceSearchMatchKind::Path
        );
        assert_eq!(
            service.search("\n"),
            Err(WorkspaceError::InvalidSearchQuery)
        );
    }

    #[test]
    fn creates_directories_and_documents_without_replacing_entries() {
        let workspace = TestWorkspace::new();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        let directory = service.create_directory("src").unwrap();
        assert_eq!(directory.kind, EntryKind::Directory);
        let document = service
            .create_document(CreateDocumentRequest {
                path: "src/main.rs".into(),
                content: "fn main() {}\n".into(),
            })
            .unwrap();
        assert_eq!(document.path, "src/main.rs");
        assert_eq!(
            service.create_document(CreateDocumentRequest {
                path: "src/main.rs".into(),
                content: "replacement".into(),
            }),
            Err(WorkspaceError::EntryAlreadyExists)
        );
        assert_eq!(
            service.create_document(CreateDocumentRequest {
                path: "missing/main.rs".into(),
                content: String::new(),
            }),
            Err(WorkspaceError::InvalidPath)
        );
    }

    #[test]
    fn moves_entries_without_overwrite_or_descendant_cycles() {
        let workspace = TestWorkspace::new();
        fs::create_dir(workspace.0.join("src")).unwrap();
        fs::write(workspace.0.join("src/old.rs"), "old").unwrap();
        fs::write(workspace.0.join("occupied.rs"), "occupied").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        let moved = service
            .move_entry(MoveWorkspaceEntryRequest {
                source: "src/old.rs".into(),
                destination: "src/new.rs".into(),
            })
            .unwrap();
        assert_eq!(moved.path, "src/new.rs");
        assert_eq!(
            service.move_entry(MoveWorkspaceEntryRequest {
                source: "src/new.rs".into(),
                destination: "occupied.rs".into(),
            }),
            Err(WorkspaceError::EntryAlreadyExists)
        );
        assert_eq!(
            service.move_entry(MoveWorkspaceEntryRequest {
                source: "src".into(),
                destination: "src/nested".into(),
            }),
            Err(WorkspaceError::InvalidPath)
        );
    }

    #[test]
    fn applies_non_overlapping_utf8_patches_with_revision_and_text_preconditions() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join("notes.md"), "Olá mundo\nlinha dois\n").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();
        let initial = service.read("notes.md").unwrap();

        let request = ApplyDocumentPatchRequest {
            path: "notes.md".into(),
            expected_revision: initial.revision.clone(),
            edits: vec![
                DocumentPatchEdit {
                    start_byte: 5,
                    end_byte: 10,
                    expected: "mundo".into(),
                    replacement: "Lyrnova".into(),
                },
                DocumentPatchEdit {
                    start_byte: 17,
                    end_byte: 21,
                    expected: "dois".into(),
                    replacement: "2".into(),
                },
            ],
        };
        let preview = service.preview_patch(&request).unwrap();
        assert_eq!(preview.content, "Olá Lyrnova\nlinha 2\n");
        assert_eq!(
            fs::read_to_string(workspace.0.join("notes.md")).unwrap(),
            "Olá mundo\nlinha dois\n"
        );
        let updated = service.apply_patch(request).unwrap();
        assert_eq!(updated.content, "Olá Lyrnova\nlinha 2\n");

        assert_eq!(
            service.apply_patch(ApplyDocumentPatchRequest {
                path: "notes.md".into(),
                expected_revision: updated.revision,
                edits: vec![DocumentPatchEdit {
                    start_byte: 1,
                    end_byte: 2,
                    expected: String::new(),
                    replacement: "x".into(),
                }],
            }),
            Err(WorkspaceError::InvalidPatch)
        );
        assert_eq!(
            service.apply_patch(ApplyDocumentPatchRequest {
                path: "notes.md".into(),
                expected_revision: initial.revision,
                edits: vec![DocumentPatchEdit {
                    start_byte: 0,
                    end_byte: 0,
                    expected: String::new(),
                    replacement: "x".into(),
                }],
            }),
            Err(WorkspaceError::Conflict)
        );
    }

    #[test]
    fn patch_preserves_unmodified_crlf_line_endings() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join("windows.txt"), b"a\r\nb\r\n").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();
        let initial = service.read("windows.txt").unwrap();

        service
            .apply_patch(ApplyDocumentPatchRequest {
                path: "windows.txt".into(),
                expected_revision: initial.revision,
                edits: vec![DocumentPatchEdit {
                    start_byte: 3,
                    end_byte: 4,
                    expected: "b".into(),
                    replacement: "B".into(),
                }],
            })
            .unwrap();

        assert_eq!(
            fs::read(workspace.0.join("windows.txt")).unwrap(),
            b"a\r\nB\r\n"
        );
    }

    #[test]
    fn deletion_is_revision_checked_and_recoverable_by_opaque_token() {
        let workspace = TestWorkspace::new();
        let recovery = TestWorkspace::new();
        fs::write(workspace.0.join("notes.md"), "original\n").unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();
        let snapshot = service.read("notes.md").unwrap();
        let recoveries = WorkspaceRecoveryService::default();

        assert_eq!(
            recoveries.delete(
                &service,
                &recovery.0,
                DeleteWorkspaceEntryRequest {
                    path: "notes.md".into(),
                    expected_revision: Some("0".repeat(64)),
                },
            ),
            Err(WorkspaceError::Conflict)
        );
        let deleted = recoveries
            .delete(
                &service,
                &recovery.0,
                DeleteWorkspaceEntryRequest {
                    path: "notes.md".into(),
                    expected_revision: Some(snapshot.revision),
                },
            )
            .unwrap();
        assert!(!workspace.0.join("notes.md").exists());
        assert_eq!(deleted.kind, EntryKind::File);
        let restored = recoveries
            .restore(&service, &deleted.recovery_token)
            .unwrap();
        assert_eq!(restored.path, "notes.md");
        assert_eq!(
            fs::read_to_string(workspace.0.join("notes.md")).unwrap(),
            "original\n"
        );
        assert_eq!(
            recoveries.restore(&service, &deleted.recovery_token),
            Err(WorkspaceError::UnknownRecovery)
        );
    }

    #[test]
    fn stale_recovery_tickets_are_bounded_to_host_generated_directories() {
        let recovery = TestWorkspace::new();
        let token = "a".repeat(32);
        fs::create_dir(recovery.0.join(&token)).unwrap();
        fs::write(recovery.0.join(&token).join("content"), "deleted").unwrap();
        fs::create_dir(recovery.0.join("operator-data")).unwrap();

        WorkspaceRecoveryService::cleanup_stale(&recovery.0).unwrap();

        assert!(!recovery.0.join(token).exists());
        assert!(recovery.0.join("operator-data").exists());
    }

    #[test]
    fn mutation_paths_reject_traversal_and_symlinked_parents() {
        let workspace = TestWorkspace::new();
        let service = WorkspaceService::new(&workspace.0).unwrap();
        for path in [
            "",
            ".",
            "..",
            "../escape",
            "/tmp/escape",
            "a//b",
            "a/./b",
            "CON",
            "aux.txt",
            "file.",
            "bad?.txt",
        ] {
            assert_eq!(validate_relative(path), Err(WorkspaceError::InvalidPath));
        }
        assert_eq!(
            validate_relative(&"x".repeat(256)),
            Err(WorkspaceError::InvalidPath)
        );

        #[cfg(unix)]
        {
            let outside = TestWorkspace::new();
            std::os::unix::fs::symlink(&outside.0, workspace.0.join("linked")).unwrap();
            assert_eq!(
                service.create_document(CreateDocumentRequest {
                    path: "linked/escape.txt".into(),
                    content: "escape".into(),
                }),
                Err(WorkspaceError::SymbolicLink)
            );
            assert!(!outside.0.join("escape.txt").exists());
        }
    }

    #[test]
    fn generated_unsafe_paths_never_reach_the_workspace() {
        let workspace = TestWorkspace::new();
        let outside = TestWorkspace::new();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        for index in 0..512 {
            let paths = [
                format!("../escape-{index}"),
                format!("safe//escape-{index}"),
                format!("safe/./escape-{index}"),
                format!("safe/../../escape-{index}"),
                format!("C:/escape-{index}"),
                format!("safe\\escape-{index}"),
            ];
            for path in paths {
                assert_eq!(
                    service.create_document(CreateDocumentRequest {
                        path,
                        content: "must not escape".into(),
                    }),
                    Err(WorkspaceError::InvalidPath)
                );
            }
        }
        assert!(fs::read_dir(&outside.0).unwrap().next().is_none());
    }

    #[test]
    fn generated_invalid_patches_never_write_partial_output() {
        let workspace = TestWorkspace::new();
        let original = "Olá mundo\n";
        fs::write(workspace.0.join("notes.md"), original).unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();
        let revision = service.read("notes.md").unwrap().revision;
        let invalid_edits = [
            vec![DocumentPatchEdit {
                start_byte: 8,
                end_byte: 4,
                expected: String::new(),
                replacement: "x".into(),
            }],
            vec![DocumentPatchEdit {
                start_byte: 0,
                end_byte: original.len() + 1,
                expected: original.into(),
                replacement: "x".into(),
            }],
            vec![DocumentPatchEdit {
                start_byte: 3,
                end_byte: 4,
                expected: String::new(),
                replacement: "x".into(),
            }],
            vec![
                DocumentPatchEdit {
                    start_byte: 0,
                    end_byte: 2,
                    expected: "Ol".into(),
                    replacement: "x".into(),
                },
                DocumentPatchEdit {
                    start_byte: 1,
                    end_byte: 2,
                    expected: "l".into(),
                    replacement: "y".into(),
                },
            ],
            vec![DocumentPatchEdit {
                start_byte: 0,
                end_byte: 2,
                expected: "no".into(),
                replacement: "x".into(),
            }],
        ];

        for edits in invalid_edits {
            assert_eq!(
                service.apply_patch(ApplyDocumentPatchRequest {
                    path: "notes.md".into(),
                    expected_revision: revision.clone(),
                    edits,
                }),
                Err(WorkspaceError::InvalidPatch)
            );
            assert_eq!(
                fs::read_to_string(workspace.0.join("notes.md")).unwrap(),
                original
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn mutations_revalidate_a_parent_replaced_by_a_symlink() {
        let workspace = TestWorkspace::new();
        let outside = TestWorkspace::new();
        fs::create_dir(workspace.0.join("parent")).unwrap();
        let service = WorkspaceService::new(&workspace.0).unwrap();

        fs::rename(workspace.0.join("parent"), workspace.0.join("old-parent")).unwrap();
        std::os::unix::fs::symlink(&outside.0, workspace.0.join("parent")).unwrap();
        assert_eq!(
            service.create_document(CreateDocumentRequest {
                path: "parent/escape.txt".into(),
                content: "must not escape".into(),
            }),
            Err(WorkspaceError::SymbolicLink)
        );
        assert!(!outside.0.join("escape.txt").exists());
    }
}
