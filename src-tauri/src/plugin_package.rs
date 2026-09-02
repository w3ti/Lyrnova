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

pub(crate) const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_STREAM_BYTES: u64 = 300 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 4_096;
const MAX_ARCHIVE_PATH_BYTES: usize = 240;
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;
const MAX_DESCRIPTOR_BYTES: u64 = 64 * 1024;
const INSTALL_RECEIPT_VERSION: u32 = 1;
const INSTALL_RECEIPT_NAME: &str = ".lyrnova-install.json";
pub(crate) const PACKAGES_DIRECTORY: &str = "packages";
const REMOVALS_DIRECTORY: &str = ".removals";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPackageDescriptor {
    pub asset: String,
    pub sha256: String,
}

impl PluginPackageDescriptor {
    pub fn read_sidecar(path: &Path) -> Result<Self, PluginPackageError> {
        let metadata =
            fs::symlink_metadata(path).map_err(|_| PluginPackageError::DescriptorUnavailable)?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_DESCRIPTOR_BYTES {
            return Err(PluginPackageError::InvalidDescriptor);
        }
        let file = File::open(path).map_err(|_| PluginPackageError::DescriptorUnavailable)?;
        let mut contents = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_DESCRIPTOR_BYTES + 1)
            .read_to_end(&mut contents)
            .map_err(|_| PluginPackageError::DescriptorUnavailable)?;
        if contents.len() as u64 > MAX_DESCRIPTOR_BYTES {
            return Err(PluginPackageError::InvalidDescriptor);
        }
        let descriptor: Self =
            serde_json::from_slice(&contents).map_err(|_| PluginPackageError::InvalidDescriptor)?;
        validate_descriptor(&descriptor)?;
        Ok(descriptor)
    }

    pub(crate) fn validate(&self) -> Result<(), PluginPackageError> {
        validate_descriptor(self)
    }
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

#[derive(Clone, Debug)]
pub(crate) struct DiscoveredPluginPackage {
    pub manifest: PluginManifest,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginInstallReceipt {
    version: u32,
    plugin_id: String,
    plugin_version: Version,
    descriptor: PluginPackageDescriptor,
    content_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PluginPackageError {
    InvalidDescriptor,
    DescriptorUnavailable,
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
    MissingReceipt,
    InvalidReceipt,
    InvalidInstallLayout,
    ContentIntegrityMismatch,
    InvalidRemovalTarget,
    RemovalRollbackFailed,
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
        let plugin_parent = self
            .root
            .join(PACKAGES_DIRECTORY)
            .join(&self.review.manifest.id);
        create_private_dir_all(&plugin_parent)?;
        let destination = plugin_parent.join(self.review.manifest.version.to_string());
        if destination
            .try_exists()
            .map_err(|_| PluginPackageError::Io)?
        {
            return Err(PluginPackageError::AlreadyInstalled);
        }
        let content_sha256 = content_sha256(&self.content_path)?;
        let receipt = PluginInstallReceipt {
            version: INSTALL_RECEIPT_VERSION,
            plugin_id: self.review.manifest.id.clone(),
            plugin_version: self.review.manifest.version.clone(),
            descriptor: self.review.descriptor.clone(),
            content_sha256,
        };
        write_install_receipt(&self.content_path, &receipt)?;
        sync_directory(&self.content_path)?;
        fs::rename(&self.content_path, &destination).map_err(|_| PluginPackageError::Io)?;
        sync_directory(&plugin_parent)?;

        Ok(InstalledPluginPackage {
            manifest: self.review.manifest.clone(),
            path: destination,
            enabled: false,
        })
    }
}

pub(crate) struct QuarantinedPluginPackages {
    original_path: PathBuf,
    quarantine_path: Option<PathBuf>,
    packages_root: PathBuf,
    removals_root: PathBuf,
}

impl QuarantinedPluginPackages {
    pub(crate) fn begin(
        root: &Path,
        plugin_id: &str,
        plugin_version: &Version,
        installed_path: &Path,
    ) -> Result<Self, PluginPackageError> {
        let packages_root = root.join(PACKAGES_DIRECTORY);
        let original_path = packages_root.join(plugin_id);
        let expected_version_path = original_path.join(plugin_version.to_string());
        if installed_path != expected_version_path {
            return Err(PluginPackageError::InvalidRemovalTarget);
        }
        let metadata = fs::symlink_metadata(&original_path)
            .map_err(|_| PluginPackageError::InvalidRemovalTarget)?;
        if !metadata.file_type().is_dir() {
            return Err(PluginPackageError::InvalidRemovalTarget);
        }

        let removals_root = root.join(REMOVALS_DIRECTORY);
        match fs::symlink_metadata(&removals_root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                create_private_dir(&removals_root)?;
            }
            Err(_) => return Err(PluginPackageError::Io),
            Ok(metadata) if metadata.file_type().is_dir() => {
                set_private_dir_permissions(&removals_root)?;
            }
            Ok(_) => return Err(PluginPackageError::InvalidRemovalTarget),
        }
        let quarantine_path = removals_root.join(Uuid::new_v4().simple().to_string());
        fs::rename(&original_path, &quarantine_path).map_err(|_| PluginPackageError::Io)?;
        if sync_directory(&packages_root).is_err() || sync_directory(&removals_root).is_err() {
            if fs::rename(&quarantine_path, &original_path).is_err() {
                return Err(PluginPackageError::RemovalRollbackFailed);
            }
            let _ = sync_directory(&packages_root);
            return Err(PluginPackageError::Io);
        }

        Ok(Self {
            original_path,
            quarantine_path: Some(quarantine_path),
            packages_root,
            removals_root,
        })
    }

    pub(crate) fn rollback(mut self) -> Result<(), PluginPackageError> {
        let quarantine_path = self
            .quarantine_path
            .take()
            .ok_or(PluginPackageError::RemovalRollbackFailed)?;
        fs::rename(&quarantine_path, &self.original_path)
            .map_err(|_| PluginPackageError::RemovalRollbackFailed)?;
        sync_directory(&self.packages_root)
            .map_err(|_| PluginPackageError::RemovalRollbackFailed)?;
        remove_empty_directory(&self.removals_root);
        Ok(())
    }

    pub(crate) fn commit(mut self) {
        let Some(quarantine_path) = self.quarantine_path.take() else {
            return;
        };
        let _ = fs::remove_dir_all(quarantine_path);
        remove_empty_directory(&self.removals_root);
    }
}

impl Drop for QuarantinedPluginPackages {
    fn drop(&mut self) {
        let Some(quarantine_path) = self.quarantine_path.take() else {
            return;
        };
        let _ = fs::rename(quarantine_path, &self.original_path);
        let _ = sync_directory(&self.packages_root);
        remove_empty_directory(&self.removals_root);
    }
}

pub(crate) fn cleanup_committed_removals(root: &Path) {
    let removals_root = root.join(REMOVALS_DIRECTORY);
    let Ok(metadata) = fs::symlink_metadata(&removals_root) else {
        return;
    };
    if metadata.file_type().is_dir() {
        let _ = fs::remove_dir_all(removals_root);
    } else {
        let _ = fs::remove_file(removals_root);
    }
}

fn remove_empty_directory(path: &Path) {
    let _ = fs::remove_dir(path);
}

pub(crate) fn discover_installed_packages(
    root: &Path,
    host_version: &Version,
) -> Result<Vec<DiscoveredPluginPackage>, PluginPackageError> {
    let packages_root = root.join(PACKAGES_DIRECTORY);
    match fs::symlink_metadata(&packages_root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(PluginPackageError::Io),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(PluginPackageError::InvalidInstallLayout);
        }
        Ok(_) => {}
    }

    let mut discovered = Vec::new();
    for id_entry in sorted_directory_entries(&packages_root)? {
        if !id_entry
            .file_type()
            .map_err(|_| PluginPackageError::Io)?
            .is_dir()
        {
            return Err(PluginPackageError::InvalidInstallLayout);
        }
        let id = id_entry
            .file_name()
            .into_string()
            .map_err(|_| PluginPackageError::InvalidInstallLayout)?;
        for version_entry in sorted_directory_entries(&id_entry.path())? {
            if discovered.len() >= MAX_ENTRIES
                || !version_entry
                    .file_type()
                    .map_err(|_| PluginPackageError::Io)?
                    .is_dir()
            {
                return Err(PluginPackageError::InvalidInstallLayout);
            }
            let version = version_entry
                .file_name()
                .into_string()
                .ok()
                .and_then(|value| Version::parse(&value).ok())
                .ok_or(PluginPackageError::InvalidInstallLayout)?;
            discovered.push(discover_installed_package(
                &version_entry.path(),
                &id,
                &version,
                host_version,
            )?);
        }
    }
    Ok(discovered)
}

fn discover_installed_package(
    path: &Path,
    expected_id: &str,
    expected_version: &Version,
    host_version: &Version,
) -> Result<DiscoveredPluginPackage, PluginPackageError> {
    let receipt_path = path.join(INSTALL_RECEIPT_NAME);
    let receipt_metadata =
        fs::symlink_metadata(&receipt_path).map_err(|_| PluginPackageError::MissingReceipt)?;
    if !receipt_metadata.file_type().is_file() || receipt_metadata.len() > MAX_RECEIPT_BYTES {
        return Err(PluginPackageError::InvalidReceipt);
    }
    let receipt: PluginInstallReceipt = serde_json::from_slice(
        &fs::read(&receipt_path).map_err(|_| PluginPackageError::InvalidReceipt)?,
    )
    .map_err(|_| PluginPackageError::InvalidReceipt)?;
    if receipt.version != INSTALL_RECEIPT_VERSION
        || receipt.plugin_id != expected_id
        || &receipt.plugin_version != expected_version
        || validate_descriptor(&receipt.descriptor).is_err()
        || !valid_sha256(&receipt.content_sha256)
    {
        return Err(PluginPackageError::InvalidReceipt);
    }
    if content_sha256(path)? != receipt.content_sha256 {
        return Err(PluginPackageError::ContentIntegrityMismatch);
    }

    let manifest_contents = fs::read_to_string(path.join("plugin.json"))
        .map_err(|_| PluginPackageError::InvalidManifest)?;
    let manifest = parse_manifest(&manifest_contents, host_version, ManifestOrigin::External)
        .map_err(|_| PluginPackageError::InvalidManifest)?;
    if manifest.id != expected_id || &manifest.version != expected_version {
        return Err(PluginPackageError::InvalidInstallLayout);
    }
    let PluginSource::GithubRelease { asset, .. } = &manifest.source else {
        return Err(PluginPackageError::InvalidManifest);
    };
    if asset != &receipt.descriptor.asset {
        return Err(PluginPackageError::AssetMismatch);
    }
    let PluginRuntime::Process { entrypoint, .. } = &manifest.runtime else {
        return Err(PluginPackageError::UnsupportedRuntime);
    };
    if !fs::symlink_metadata(path.join(entrypoint))
        .is_ok_and(|metadata| metadata.file_type().is_file())
    {
        return Err(PluginPackageError::MissingEntrypoint);
    }
    Ok(DiscoveredPluginPackage {
        manifest,
        path: path.to_owned(),
    })
}

fn write_install_receipt(
    content_path: &Path,
    receipt: &PluginInstallReceipt,
) -> Result<(), PluginPackageError> {
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|_| PluginPackageError::Io)?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(PluginPackageError::Io);
    }
    let path = content_path.join(INSTALL_RECEIPT_NAME);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| PluginPackageError::Io)?;
    set_private_file_permissions(&path)?;
    file.write_all(&bytes).map_err(|_| PluginPackageError::Io)?;
    file.sync_all().map_err(|_| PluginPackageError::Io)
}

fn content_sha256(root: &Path) -> Result<String, PluginPackageError> {
    let mut entries = Vec::new();
    collect_content_entries(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    let mut expanded_bytes = 0u64;
    for (relative, path, is_directory) in entries {
        digest.update(if is_directory { b"d" } else { b"f" });
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        let metadata = fs::symlink_metadata(&path).map_err(|_| PluginPackageError::Io)?;
        digest.update(security_mode(&metadata).to_be_bytes());
        if is_directory {
            continue;
        }
        if metadata.len() > MAX_ENTRY_BYTES {
            return Err(PluginPackageError::EntryTooLarge);
        }
        expanded_bytes = expanded_bytes
            .checked_add(metadata.len())
            .ok_or(PluginPackageError::ExpandedSizeTooLarge)?;
        if expanded_bytes > MAX_EXPANDED_BYTES {
            return Err(PluginPackageError::ExpandedSizeTooLarge);
        }
        digest.update(metadata.len().to_be_bytes());
        let mut file = File::open(path).map_err(|_| PluginPackageError::Io)?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|_| PluginPackageError::Io)?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(unix)]
fn security_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn security_mode(_metadata: &fs::Metadata) -> u32 {
    0
}

fn collect_content_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<(String, PathBuf, bool)>,
) -> Result<(), PluginPackageError> {
    for entry in sorted_directory_entries(directory)? {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PluginPackageError::InvalidInstallLayout)?;
        if relative == Path::new(INSTALL_RECEIPT_NAME) {
            continue;
        }
        validate_archive_path(relative)?;
        let relative = relative
            .to_str()
            .ok_or(PluginPackageError::UnsafeEntryPath)?
            .to_owned();
        let file_type = entry.file_type().map_err(|_| PluginPackageError::Io)?;
        if !file_type.is_file() && !file_type.is_dir() {
            return Err(PluginPackageError::UnsupportedEntryType);
        }
        if entries.len() >= MAX_ENTRIES {
            return Err(PluginPackageError::TooManyEntries);
        }
        entries.push((relative, path.clone(), file_type.is_dir()));
        if file_type.is_dir() {
            collect_content_entries(root, &path, entries)?;
        }
    }
    Ok(())
}

fn sorted_directory_entries(path: &Path) -> Result<Vec<fs::DirEntry>, PluginPackageError> {
    let mut entries: Vec<_> = fs::read_dir(path)
        .map_err(|_| PluginPackageError::Io)?
        .collect::<Result<_, _>>()
        .map_err(|_| PluginPackageError::Io)?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
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
        || !valid_sha256(&descriptor.sha256)
    {
        return Err(PluginPackageError::InvalidDescriptor);
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    fn reads_only_a_small_regular_valid_descriptor_sidecar() {
        let root = TestDirectory::new();
        let sidecar = root.path().join("example.tar.zst.json");
        let expected = PluginPackageDescriptor {
            asset: "example.tar.zst".into(),
            sha256: "a".repeat(64),
        };
        fs::write(&sidecar, serde_json::to_vec(&expected).unwrap()).unwrap();
        assert_eq!(
            PluginPackageDescriptor::read_sidecar(&sidecar),
            Ok(expected)
        );

        fs::write(&sidecar, br#"{"asset":"../escape.tar.zst","sha256":"bad"}"#).unwrap();
        assert_eq!(
            PluginPackageDescriptor::read_sidecar(&sidecar),
            Err(PluginPackageError::InvalidDescriptor)
        );

        fs::write(&sidecar, vec![b' '; MAX_DESCRIPTOR_BYTES as usize + 1]).unwrap();
        assert_eq!(
            PluginPackageDescriptor::read_sidecar(&sidecar),
            Err(PluginPackageError::InvalidDescriptor)
        );
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

    #[test]
    fn external_removal_quarantines_every_version_and_supports_rollback() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let installed = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap()
            .install(&[
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ])
            .unwrap();
        let store = root.path().join("store");
        let plugin_root = installed.path.parent().unwrap().to_owned();
        let older_version = plugin_root.join("0.0.9");
        fs::create_dir(&older_version).unwrap();
        fs::write(older_version.join("legacy"), b"old").unwrap();

        let removal = QuarantinedPluginPackages::begin(
            &store,
            &installed.manifest.id,
            &installed.manifest.version,
            &installed.path,
        )
        .unwrap();
        assert!(!plugin_root.exists());
        drop(removal);
        assert!(installed.path.exists());
        assert!(older_version.exists());

        let removal = QuarantinedPluginPackages::begin(
            &store,
            &installed.manifest.id,
            &installed.manifest.version,
            &installed.path,
        )
        .unwrap();
        removal.commit();
        assert!(!plugin_root.exists());
        assert!(!store.join(REMOVALS_DIRECTORY).exists());
    }

    #[test]
    fn startup_cleanup_finishes_an_interrupted_committed_removal() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let installed = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap()
            .install(&[
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ])
            .unwrap();
        let store = root.path().join("store");
        let removal = QuarantinedPluginPackages::begin(
            &store,
            &installed.manifest.id,
            &installed.manifest.version,
            &installed.path,
        )
        .unwrap();
        std::mem::forget(removal);

        assert!(store.join(REMOVALS_DIRECTORY).is_dir());
        cleanup_committed_removals(&store);
        assert!(!store.join(REMOVALS_DIRECTORY).exists());
        assert!(!installed.path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn external_removal_rejects_a_symlinked_quarantine() {
        use std::os::unix::fs::symlink;

        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let installed = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap()
            .install(&[
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ])
            .unwrap();
        let store = root.path().join("store");
        let outside = root.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, store.join(REMOVALS_DIRECTORY)).unwrap();

        assert!(matches!(
            QuarantinedPluginPackages::begin(
                &store,
                &installed.manifest.id,
                &installed.manifest.version,
                &installed.path,
            ),
            Err(PluginPackageError::InvalidRemovalTarget)
        ));
        assert!(installed.path.exists());
        assert!(outside.exists());
    }

    #[test]
    fn restart_discovery_revalidates_the_receipt_and_content_tree() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let approved = [
            PluginPermission::WorkspaceRead,
            PluginPermission::ProcessSpawn,
        ];
        let installed = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap()
            .install(&approved)
            .unwrap();

        assert!(installed.path.join(INSTALL_RECEIPT_NAME).is_file());
        let discovered =
            discover_installed_packages(&root.path().join("store"), &Version::new(0, 1, 0))
                .unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].manifest, installed.manifest);
        assert_eq!(discovered[0].path, installed.path);

        fs::write(installed.path.join("bin/example"), b"tampered").unwrap();
        assert!(matches!(
            discover_installed_packages(&root.path().join("store"), &Version::new(0, 1, 0),),
            Err(PluginPackageError::ContentIntegrityMismatch)
        ));
    }

    #[test]
    fn restart_discovery_fails_closed_without_a_valid_receipt() {
        let root = TestDirectory::new();
        let package_path = write_package(root.path(), ASSET, valid_entries());
        let installed = installer(&root)
            .stage_local(&package_path, descriptor(&package_path))
            .unwrap()
            .install(&[
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ])
            .unwrap();
        fs::remove_file(installed.path.join(INSTALL_RECEIPT_NAME)).unwrap();

        assert!(matches!(
            discover_installed_packages(&root.path().join("store"), &Version::new(0, 1, 0),),
            Err(PluginPackageError::MissingReceipt)
        ));
    }
}
