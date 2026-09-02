use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::plugin_manifest::{
    ManifestOrigin, PluginManifest, PluginPermission, PluginRuntime, PluginSource, parse_manifest,
    permissions_exactly_match,
};

const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_STREAM_BYTES: u64 = 300 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 4_096;
const MAX_ARCHIVE_PATH_BYTES: usize = 240;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPackageDescriptor {
    pub asset: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPackageReview {
    pub manifest: PluginManifest,
    pub descriptor: PluginPackageDescriptor,
    pub package_bytes: u64,
    pub entry_count: usize,
    pub expanded_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledPluginPackage {
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PluginPackageError {
    InvalidDescriptor,
    PackageUnavailable,
    PackageNotRegularFile,
    PackageTooLarge,
    ChecksumMismatch,
    InvalidArchive,
    ArchiveStreamTooLarge,
    TooManyEntries,
    EntryTooLarge,
    ExpandedSizeTooLarge,
    UnsafeEntryPath,
    DuplicateEntry,
    UnsupportedEntryType,
    MissingManifest,
    InvalidManifest,
    AssetMismatch,
    UnsupportedRuntime,
    MissingEntrypoint,
    PermissionApprovalRequired,
    AlreadyInstalled,
    StateUnavailable,
    Io,
}

#[derive(Clone, Copy)]
struct PackageLimits {
    package_bytes: u64,
    archive_stream_bytes: u64,
    expanded_bytes: u64,
    entry_bytes: u64,
    entries: usize,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            package_bytes: MAX_PACKAGE_BYTES,
            archive_stream_bytes: MAX_ARCHIVE_STREAM_BYTES,
            expanded_bytes: MAX_EXPANDED_BYTES,
            entry_bytes: MAX_ENTRY_BYTES,
            entries: MAX_ENTRIES,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PluginPackageInstaller {
    root: PathBuf,
    host_version: Version,
    mutation: Arc<Mutex<()>>,
}

impl PluginPackageInstaller {
    pub fn new(root: impl Into<PathBuf>, host_version: Version) -> Self {
        Self {
            root: root.into(),
            host_version,
            mutation: Arc::new(Mutex::new(())),
        }
    }

    pub fn stage_local(
        &self,
        package_path: &Path,
        descriptor: PluginPackageDescriptor,
    ) -> Result<StagedPluginPackage, PluginPackageError> {
        self.stage_local_with_limits(package_path, descriptor, PackageLimits::default())
    }

    fn stage_local_with_limits(
        &self,
        package_path: &Path,
        descriptor: PluginPackageDescriptor,
        limits: PackageLimits,
    ) -> Result<StagedPluginPackage, PluginPackageError> {
        validate_descriptor(&descriptor)?;
        if package_path.file_name().and_then(|name| name.to_str())
            != Some(descriptor.asset.as_str())
        {
            return Err(PluginPackageError::AssetMismatch);
        }

        let path_metadata = fs::symlink_metadata(package_path)
            .map_err(|_| PluginPackageError::PackageUnavailable)?;
        if !path_metadata.file_type().is_file() {
            return Err(PluginPackageError::PackageNotRegularFile);
        }

        let mut package =
            File::open(package_path).map_err(|_| PluginPackageError::PackageUnavailable)?;
        let metadata = package.metadata().map_err(|_| PluginPackageError::Io)?;
        if !metadata.file_type().is_file() {
            return Err(PluginPackageError::PackageNotRegularFile);
        }
        if metadata.len() > limits.package_bytes {
            return Err(PluginPackageError::PackageTooLarge);
        }
        if sha256(&mut package)? != descriptor.sha256 {
            return Err(PluginPackageError::ChecksumMismatch);
        }
        package
            .seek(SeekFrom::Start(0))
            .map_err(|_| PluginPackageError::Io)?;

        let staging_root = self.root.join(".staging");
        create_private_dir_all(&staging_root)?;
        let mut cleanup = StagingCleanup::create(&staging_root)?;
        let content_path = cleanup.path().join("content");
        create_private_dir(&content_path)?;

        let extraction = extract_package(package, &content_path, limits)?;
        let manifest_path = content_path.join("plugin.json");
        let manifest_contents =
            fs::read_to_string(&manifest_path).map_err(|_| PluginPackageError::MissingManifest)?;
        let manifest = parse_manifest(
            &manifest_contents,
            &self.host_version,
            ManifestOrigin::External,
        )
        .map_err(|_| PluginPackageError::InvalidManifest)?;

        let PluginSource::GithubRelease { asset, .. } = &manifest.source else {
            return Err(PluginPackageError::InvalidManifest);
        };
        if asset != &descriptor.asset {
            return Err(PluginPackageError::AssetMismatch);
        }
        let PluginRuntime::Process { entrypoint, .. } = &manifest.runtime else {
            return Err(PluginPackageError::UnsupportedRuntime);
        };
        let entrypoint_metadata = fs::symlink_metadata(content_path.join(entrypoint))
            .map_err(|_| PluginPackageError::MissingEntrypoint)?;
        if !entrypoint_metadata.file_type().is_file() {
            return Err(PluginPackageError::MissingEntrypoint);
        }

        let review = PluginPackageReview {
            manifest,
            descriptor,
            package_bytes: metadata.len(),
            entry_count: extraction.entry_count,
            expanded_bytes: extraction.expanded_bytes,
        };
        let staging_path = cleanup.take();
        Ok(StagedPluginPackage {
            root: self.root.clone(),
            staging_path,
            content_path,
            review,
            mutation: Arc::clone(&self.mutation),
        })
    }
}

pub struct StagedPluginPackage {
    root: PathBuf,
    staging_path: PathBuf,
    content_path: PathBuf,
    review: PluginPackageReview,
    mutation: Arc<Mutex<()>>,
}

impl StagedPluginPackage {
    pub fn review(&self) -> &PluginPackageReview {
        &self.review
    }

    pub fn install(
        self,
        approved_permissions: &[PluginPermission],
    ) -> Result<InstalledPluginPackage, PluginPackageError> {
        if !permissions_exactly_match(
            &self.review.manifest.permissions,
            approved_permissions.iter().copied(),
        ) {
            return Err(PluginPackageError::PermissionApprovalRequired);
        }

        let _mutation = self
            .mutation
            .lock()
            .map_err(|_| PluginPackageError::StateUnavailable)?;
        let plugin_parent = self.root.join("packages").join(&self.review.manifest.id);
        create_private_dir_all(&plugin_parent)?;
        let destination = plugin_parent.join(self.review.manifest.version.to_string());
        if destination
            .try_exists()
            .map_err(|_| PluginPackageError::Io)?
        {
            return Err(PluginPackageError::AlreadyInstalled);
        }
        fs::rename(&self.content_path, &destination).map_err(|_| PluginPackageError::Io)?;
        sync_directory(&plugin_parent)?;

        Ok(InstalledPluginPackage {
            manifest: self.review.manifest.clone(),
            path: destination,
            enabled: false,
        })
    }
}

impl Drop for StagedPluginPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging_path);
    }
}

struct ExtractionSummary {
    entry_count: usize,
    expanded_bytes: u64,
}

fn extract_package(
    package: File,
    destination: &Path,
    limits: PackageLimits,
) -> Result<ExtractionSummary, PluginPackageError> {
    let decoder = zstd::stream::read::Decoder::new(package)
        .map_err(|_| PluginPackageError::InvalidArchive)?;
    let stream_limit_exceeded = Arc::new(AtomicBool::new(false));
    let reader = SizeLimitedReader::new(
        decoder,
        limits.archive_stream_bytes,
        Arc::clone(&stream_limit_exceeded),
    );
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|_| archive_error(&stream_limit_exceeded))?;
    let mut seen = BTreeSet::new();
    let mut entry_count = 0usize;
    let mut expanded_bytes = 0u64;
    let mut found_manifest = false;

    for next in entries {
        let mut entry = next.map_err(|_| archive_error(&stream_limit_exceeded))?;
        entry_count = entry_count
            .checked_add(1)
            .ok_or(PluginPackageError::TooManyEntries)?;
        if entry_count > limits.entries {
            return Err(PluginPackageError::TooManyEntries);
        }
        let path = entry
            .path()
            .map_err(|_| archive_error(&stream_limit_exceeded))?
            .into_owned();
        validate_archive_path(&path)?;
        if !seen.insert(path.clone()) {
            return Err(PluginPackageError::DuplicateEntry);
        }

        let entry_type = entry.header().entry_type();
        if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(PluginPackageError::UnsupportedEntryType);
        }
        let output_path = destination.join(&path);
        if entry_type.is_dir() {
            create_private_dir_all(&output_path)?;
            continue;
        }

        let size = entry.size();
        if size > limits.entry_bytes {
            return Err(PluginPackageError::EntryTooLarge);
        }
        expanded_bytes = expanded_bytes
            .checked_add(size)
            .ok_or(PluginPackageError::ExpandedSizeTooLarge)?;
        if expanded_bytes > limits.expanded_bytes {
            return Err(PluginPackageError::ExpandedSizeTooLarge);
        }
        if path == Path::new("plugin.json") {
            found_manifest = true;
        }

        if let Some(parent) = output_path.parent() {
            create_private_dir_all(parent)?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(|_| PluginPackageError::Io)?;
        set_private_file_permissions(&output_path)?;
        let copied =
            io::copy(&mut entry, &mut output).map_err(|_| archive_error(&stream_limit_exceeded))?;
        if copied != size {
            return Err(PluginPackageError::InvalidArchive);
        }
        output.flush().map_err(|_| PluginPackageError::Io)?;
        output.sync_all().map_err(|_| PluginPackageError::Io)?;
    }

    if !found_manifest {
        return Err(PluginPackageError::MissingManifest);
    }
    Ok(ExtractionSummary {
        entry_count,
        expanded_bytes,
    })
}

fn archive_error(limit_exceeded: &AtomicBool) -> PluginPackageError {
    if limit_exceeded.load(Ordering::Relaxed) {
        PluginPackageError::ArchiveStreamTooLarge
    } else {
        PluginPackageError::InvalidArchive
    }
}

fn validate_descriptor(descriptor: &PluginPackageDescriptor) -> Result<(), PluginPackageError> {
    if !safe_asset_name(&descriptor.asset)
        || !descriptor.asset.ends_with(".tar.zst")
        || descriptor.sha256.len() != 64
        || !descriptor
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PluginPackageError::InvalidDescriptor);
    }
    Ok(())
}

fn safe_asset_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ARCHIVE_PATH_BYTES
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_archive_path(path: &Path) -> Result<(), PluginPackageError> {
    let Some(value) = path.to_str() else {
        return Err(PluginPackageError::UnsafeEntryPath);
    };
    if value.is_empty()
        || value.len() > MAX_ARCHIVE_PATH_BYTES
        || value.contains('\\')
        || value.contains(':')
        || value.chars().any(char::is_control)
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(PluginPackageError::UnsafeEntryPath);
    }
    Ok(())
}

fn sha256(reader: &mut File) -> Result<String, PluginPackageError> {
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| PluginPackageError::Io)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

struct SizeLimitedReader<R> {
    inner: R,
    remaining: u64,
    exceeded: Arc<AtomicBool>,
}

impl<R> SizeLimitedReader<R> {
    fn new(inner: R, remaining: u64, exceeded: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            remaining,
            exceeded,
        }
    }
}

impl<R: Read> Read for SizeLimitedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0u8; 1];
            if self.inner.read(&mut probe)? == 0 {
                return Ok(0);
            }
            self.exceeded.store(true, Ordering::Relaxed);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decompressed plugin archive exceeds its limit",
            ));
        }
        let allowed =
            usize::try_from(self.remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

struct StagingCleanup(Option<PathBuf>);

impl StagingCleanup {
    fn create(parent: &Path) -> Result<Self, PluginPackageError> {
        for _ in 0..8 {
            let path = parent.join(Uuid::new_v4().simple().to_string());
            match fs::create_dir(&path) {
                Ok(()) => {
                    set_private_dir_permissions(&path)?;
                    return Ok(Self(Some(path)));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(PluginPackageError::Io),
            }
        }
        Err(PluginPackageError::Io)
    }

    fn path(&self) -> &Path {
        self.0.as_deref().expect("staging directory is present")
    }

    fn take(&mut self) -> PathBuf {
        self.0.take().expect("staging directory is present")
    }
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn create_private_dir(path: &Path) -> Result<(), PluginPackageError> {
    fs::create_dir(path).map_err(|_| PluginPackageError::Io)?;
    set_private_dir_permissions(path)
}

fn create_private_dir_all(path: &Path) -> Result<(), PluginPackageError> {
    fs::create_dir_all(path).map_err(|_| PluginPackageError::Io)?;
    set_private_dir_permissions(path)
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), PluginPackageError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| PluginPackageError::Io)
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), PluginPackageError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), PluginPackageError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| PluginPackageError::Io)
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), PluginPackageError> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), PluginPackageError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| PluginPackageError::Io)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), PluginPackageError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use tar::{EntryType, Header};

    use super::*;

    const ASSET: &str = "example-plugin.tar.zst";

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lyrnova-plugin-package-test-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    enum TestEntry {
        File(String, Vec<u8>),
        Directory(String),
        Symlink(String, String),
        Fifo(String),
        RawFile(String, Vec<u8>),
    }

    fn manifest(asset: &str) -> String {
        format!(
            r#"{{
              "schemaVersion": 1,
              "id": "io.github.w3ti.lyrnova.tool.example",
              "name": "Example",
              "description": "External package used by installer tests.",
              "version": "0.1.0",
              "publisher": "w3ti",
              "license": "GPL-3.0-only",
              "kind": "tool",
              "compatibility": {{ "lyrnova": ">=0.1.0, <0.2.0", "pluginApi": 1 }},
              "runtime": {{ "type": "process", "entrypoint": "bin/example", "protocolVersion": 1 }},
              "source": {{
                "type": "github_release",
                "repository": "https://github.com/w3ti/lyrnova-example",
                "asset": "{asset}"
              }},
              "capabilities": ["tasks"],
              "permissions": ["workspace_read", "process_spawn"]
            }}"#
        )
    }

    fn valid_entries() -> Vec<TestEntry> {
        vec![
            TestEntry::File("plugin.json".into(), manifest(ASSET).into_bytes()),
            TestEntry::Directory("bin".into()),
            TestEntry::File("bin/example".into(), b"#!/bin/sh\nexit 0\n".to_vec()),
        ]
    }

    fn write_package(directory: &Path, asset: &str, entries: Vec<TestEntry>) -> PathBuf {
        let package_path = directory.join(asset);
        let file = File::create(&package_path).unwrap();
        let encoder = zstd::stream::write::Encoder::new(file, 0).unwrap();
        let mut builder = tar::Builder::new(encoder);

        for entry in entries {
            match entry {
                TestEntry::File(path, contents) => {
                    append_entry(
                        &mut builder,
                        &path,
                        EntryType::Regular,
                        None,
                        &contents,
                        false,
                    );
                }
                TestEntry::Directory(path) => {
                    append_entry(&mut builder, &path, EntryType::Directory, None, &[], false);
                }
                TestEntry::Symlink(path, target) => {
                    append_entry(
                        &mut builder,
                        &path,
                        EntryType::Symlink,
                        Some(&target),
                        &[],
                        false,
                    );
                }
                TestEntry::Fifo(path) => {
                    append_entry(&mut builder, &path, EntryType::Fifo, None, &[], false);
                }
                TestEntry::RawFile(path, contents) => {
                    append_entry(
                        &mut builder,
                        &path,
                        EntryType::Regular,
                        None,
                        &contents,
                        true,
                    );
                }
            }
        }
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();
        package_path
    }

    fn append_entry<W: Write>(
        builder: &mut tar::Builder<W>,
        path: &str,
        entry_type: EntryType,
        link_name: Option<&str>,
        contents: &[u8],
        raw_path: bool,
    ) {
        let mut header = Header::new_gnu();
        header.set_entry_type(entry_type);
        header.set_mode(0o777);
        header.set_size(contents.len() as u64);
        if raw_path {
            let name = &mut header.as_mut_bytes()[..100];
            name.fill(0);
            name[..path.len()].copy_from_slice(path.as_bytes());
        } else {
            header.set_path(path).unwrap();
        }
        if let Some(link_name) = link_name {
            header.set_link_name(link_name).unwrap();
        }
        header.set_cksum();
        builder.append(&header, Cursor::new(contents)).unwrap();
    }

    fn descriptor(package_path: &Path) -> PluginPackageDescriptor {
        let mut package = File::open(package_path).unwrap();
        PluginPackageDescriptor {
            asset: package_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            sha256: sha256(&mut package).unwrap(),
        }
    }

    fn installer(root: &TestDirectory) -> PluginPackageInstaller {
        PluginPackageInstaller::new(root.path().join("store"), Version::new(0, 1, 0))
    }

    fn staging_entry_count(root: &TestDirectory) -> usize {
        fs::read_dir(root.path().join("store/.staging"))
            .map(|entries| entries.count())
            .unwrap_or(0)
    }

    fn assert_stage_error(
        result: Result<StagedPluginPackage, PluginPackageError>,
        expected: PluginPackageError,
    ) {
        match result {
            Ok(_) => panic!("package staging unexpectedly succeeded"),
            Err(actual) => assert_eq!(actual, expected),
        }
    }

    #[test]
    fn stages_reviews_and_atomically_installs_without_enabling() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let staged = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap();

        assert_eq!(staged.review().entry_count, 3);
        assert_eq!(
            staged.review().manifest.permissions,
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ]
        );
        assert_eq!(staging_entry_count(&root), 1);

        let installed = staged
            .install(&[
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ])
            .unwrap();
        assert!(!installed.enabled);
        assert!(installed.path.join("plugin.json").is_file());
        assert!(installed.path.join("bin/example").is_file());
        assert_eq!(staging_entry_count(&root), 0);

        #[cfg(unix)]
        assert_eq!(
            fs::metadata(installed.path.join("bin/example"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn dropping_or_rejecting_a_review_cleans_staging() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let staged = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap();
        drop(staged);
        assert_eq!(staging_entry_count(&root), 0);

        let staged = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap();
        assert_eq!(
            staged.install(&[PluginPermission::WorkspaceRead]),
            Err(PluginPackageError::PermissionApprovalRequired)
        );
        assert_eq!(staging_entry_count(&root), 0);
        assert!(!root.path().join("store/packages").exists());
    }

    #[test]
    fn verifies_descriptor_asset_and_archive_checksum_before_staging() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let mut expected = descriptor(&package_path);
        expected.sha256 = "0".repeat(64);
        assert_stage_error(
            installer(&root).stage_local(&package_path, expected),
            PluginPackageError::ChecksumMismatch,
        );

        let mut expected = descriptor(&package_path);
        expected.asset = "different.tar.zst".into();
        assert_stage_error(
            installer(&root).stage_local(&package_path, expected),
            PluginPackageError::AssetMismatch,
        );

        let mut expected = descriptor(&package_path);
        expected.sha256 = "A".repeat(64);
        assert_stage_error(
            installer(&root).stage_local(&package_path, expected),
            PluginPackageError::InvalidDescriptor,
        );

        let directory_asset = root.path().join("directory.tar.zst");
        fs::create_dir(&directory_asset).unwrap();
        assert_stage_error(
            installer(&root).stage_local(
                &directory_asset,
                PluginPackageDescriptor {
                    asset: "directory.tar.zst".into(),
                    sha256: "0".repeat(64),
                },
            ),
            PluginPackageError::PackageNotRegularFile,
        );
        assert_eq!(staging_entry_count(&root), 0);
    }

    #[test]
    fn rejects_traversal_links_and_duplicate_entries() {
        let root = TestDirectory::new();
        for (suffix, hostile, expected_error) in [
            (
                "traversal",
                TestEntry::RawFile("../escape".into(), b"no".to_vec()),
                PluginPackageError::UnsafeEntryPath,
            ),
            (
                "symlink",
                TestEntry::Symlink("bin/link".into(), "../outside".into()),
                PluginPackageError::UnsupportedEntryType,
            ),
            (
                "fifo",
                TestEntry::Fifo("bin/pipe".into()),
                PluginPackageError::UnsupportedEntryType,
            ),
            (
                "duplicate",
                TestEntry::File("bin/example".into(), b"duplicate".to_vec()),
                PluginPackageError::DuplicateEntry,
            ),
        ] {
            let asset = format!("example-{suffix}.tar.zst");
            let mut entries = valid_entries();
            let updated_manifest = manifest(&asset).into_bytes();
            entries[0] = TestEntry::File("plugin.json".into(), updated_manifest);
            entries.push(hostile);
            let package_path = write_package(root.path(), &asset, entries);
            assert_stage_error(
                installer(&root).stage_local(&package_path, descriptor(&package_path)),
                expected_error,
            );
            assert_eq!(staging_entry_count(&root), 0);
        }
        assert!(!root.path().join("escape").exists());
    }

    #[test]
    fn enforces_compressed_entry_expanded_stream_and_count_limits() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let package_size = fs::metadata(&package_path).unwrap().len();
        let base = PackageLimits::default();

        let cases = [
            (
                PackageLimits {
                    package_bytes: package_size - 1,
                    ..base
                },
                PluginPackageError::PackageTooLarge,
            ),
            (
                PackageLimits {
                    entry_bytes: 32,
                    ..base
                },
                PluginPackageError::EntryTooLarge,
            ),
            (
                PackageLimits {
                    expanded_bytes: 100,
                    entry_bytes: u64::MAX,
                    ..base
                },
                PluginPackageError::ExpandedSizeTooLarge,
            ),
            (
                PackageLimits {
                    archive_stream_bytes: 128,
                    ..base
                },
                PluginPackageError::ArchiveStreamTooLarge,
            ),
            (
                PackageLimits { entries: 2, ..base },
                PluginPackageError::TooManyEntries,
            ),
        ];

        for (limits, expected_error) in cases {
            assert_stage_error(
                installer(&root).stage_local_with_limits(
                    &package_path,
                    descriptor(&package_path),
                    limits,
                ),
                expected_error,
            );
            assert_eq!(staging_entry_count(&root), 0);
        }
    }

    #[test]
    fn rejects_invalid_manifest_asset_runtime_and_missing_entrypoint() {
        let root = TestDirectory::new();
        let invalid_cases = [
            (
                "asset",
                manifest("another.tar.zst"),
                true,
                PluginPackageError::AssetMismatch,
            ),
            (
                "runtime",
                manifest("example-runtime.tar.zst").replace(
                    r#""runtime": { "type": "process", "entrypoint": "bin/example", "protocolVersion": 1 }"#,
                    r#""runtime": { "type": "builtin", "module": "tool.example" }"#,
                ),
                true,
                PluginPackageError::UnsupportedRuntime,
            ),
            (
                "entrypoint",
                manifest("example-entrypoint.tar.zst"),
                false,
                PluginPackageError::MissingEntrypoint,
            ),
        ];

        for (suffix, document, include_entrypoint, expected_error) in invalid_cases {
            let asset = format!("example-{suffix}.tar.zst");
            let mut entries = vec![TestEntry::File("plugin.json".into(), document.into_bytes())];
            if include_entrypoint {
                entries.push(TestEntry::File("bin/example".into(), b"binary".to_vec()));
            }
            let package_path = write_package(root.path(), &asset, entries);
            assert_stage_error(
                installer(&root).stage_local(&package_path, descriptor(&package_path)),
                expected_error,
            );
            assert_eq!(staging_entry_count(&root), 0);
        }
    }

    #[test]
    fn refuses_to_replace_an_installed_version() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let approved = [
            PluginPermission::WorkspaceRead,
            PluginPermission::ProcessSpawn,
        ];
        installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap()
            .install(&approved)
            .unwrap();

        let second = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap();
        assert_eq!(
            second.install(&approved),
            Err(PluginPackageError::AlreadyInstalled)
        );
        assert_eq!(staging_entry_count(&root), 0);
    }
}
