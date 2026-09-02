use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{Url, blocking::Client, redirect};
use semver::Version;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    plugin_manifest::{PluginManifest, PluginSource},
    plugin_package::{MAX_PACKAGE_BYTES, PluginPackageAuthentication, PluginPackageDescriptor},
    plugin_trust::{
        CatalogPluginRelease, VerifiedCatalog, authentication_for, parse_authenticated_catalog,
        parse_embedded_catalog, parse_trust, rejects_release_downgrade,
        verify_installed_authentication,
    },
};

pub(crate) use crate::plugin_trust::PluginCatalogError;

const MAX_RELEASE_TAG_BYTES: usize = 128;
const DOWNLOADS_DIRECTORY: &str = ".downloads";
const CATALOG_DIRECTORY: &str = ".catalog";
const CACHED_CATALOG_FILE: &str = "catalog-v2.json";
const CATALOG_DOCUMENT: &str = include_str!("../../plugins/catalog/v2.json");
const TRUST_DOCUMENT: &str = include_str!("../../plugins/trust/catalog-root-v1.json");
const REMOTE_CATALOG_URL: &str =
    "https://github.com/w3ti/Lyrnova/releases/latest/download/plugin-catalog-v2.json";

#[derive(Clone, Debug)]
pub(crate) struct TrustedPluginRelease {
    pub(crate) manifest: PluginManifest,
    pub(crate) descriptor: PluginPackageDescriptor,
    pub(crate) release_tag: String,
    pub(crate) authentication: PluginPackageAuthentication,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrustedPluginSummary {
    pub manifest: PluginManifest,
    pub descriptor: PluginPackageDescriptor,
    pub publisher_key_id: String,
    pub installed_version: Option<Version>,
    pub download_available: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PluginCatalogService {
    catalog: Arc<RwLock<Result<VerifiedCatalog, PluginCatalogError>>>,
    update_lock: Arc<Mutex<()>>,
}

impl Default for PluginCatalogService {
    fn default() -> Self {
        let host_version = host_version();
        let catalog = host_version
            .and_then(|version| parse_embedded_catalog(CATALOG_DOCUMENT, &version, unix_time()));
        Self {
            catalog: Arc::new(RwLock::new(catalog)),
            update_lock: Arc::new(Mutex::new(())),
        }
    }
}

impl PluginCatalogService {
    pub(crate) fn load_cached(&self, root: &Path) -> Result<(), PluginCatalogError> {
        let path = root.join(CATALOG_DIRECTORY).join(CACHED_CATALOG_FILE);
        let bytes = match read_bounded_regular_file(&path) {
            Ok(bytes) => bytes,
            Err(PluginCatalogError::Io) if !path.exists() => return Ok(()),
            Err(error) => return Err(error),
        };
        let trust = parse_trust(TRUST_DOCUMENT)?;
        let next = parse_authenticated_catalog(&bytes, &host_version()?, &trust, unix_time())?;
        let current = self.snapshot()?;
        if next.signed.version < current.signed.version
            || rejects_release_downgrade(&current, &next)
        {
            return Err(PluginCatalogError::CatalogRollback);
        }
        *self.catalog.write().map_err(|_| PluginCatalogError::Io)? = Ok(next);
        Ok(())
    }

    pub(crate) fn summaries(&self) -> Result<Vec<TrustedPluginSummary>, PluginCatalogError> {
        Ok(self
            .snapshot()?
            .signed
            .entries
            .into_iter()
            .map(|entry| TrustedPluginSummary {
                publisher_key_id: entry.publisher_signature.key_id.clone(),
                manifest: entry.manifest,
                descriptor: entry.descriptor,
                installed_version: None,
                download_available: true,
            })
            .collect())
    }

    pub(crate) fn trusted_release(
        &self,
        id: &str,
    ) -> Result<TrustedPluginRelease, PluginCatalogError> {
        let catalog = self.snapshot()?;
        let release = catalog
            .signed
            .entries
            .iter()
            .find(|entry| entry.manifest.id == id)
            .ok_or(PluginCatalogError::UnknownPlugin)?;
        Ok(release_from_entry(catalog.signed.version, release))
    }

    pub(crate) fn verify_installed(
        &self,
        manifest: &PluginManifest,
        descriptor: &PluginPackageDescriptor,
        authentication: &PluginPackageAuthentication,
    ) -> Result<(), PluginCatalogError> {
        verify_installed_authentication(&self.snapshot()?, manifest, descriptor, authentication)
    }

    pub(crate) fn update(&self, root: &Path) -> Result<(), PluginCatalogError> {
        let trust = parse_trust(TRUST_DOCUMENT)?;
        if trust.keys.is_empty() || trust.threshold > trust.keys.len() {
            return Err(PluginCatalogError::NoTrustedCatalogKeys);
        }
        let bytes = download_catalog_document()?;
        self.apply_update(root, &bytes, unix_time())
    }

    fn apply_update(&self, root: &Path, bytes: &[u8], now: u64) -> Result<(), PluginCatalogError> {
        let _guard = self
            .update_lock
            .lock()
            .map_err(|_| PluginCatalogError::Io)?;
        let trust = parse_trust(TRUST_DOCUMENT)?;
        let next = parse_authenticated_catalog(bytes, &host_version()?, &trust, now)?;
        let current = self.snapshot()?;
        if next.signed.version <= current.signed.version
            || rejects_release_downgrade(&current, &next)
        {
            return Err(PluginCatalogError::CatalogRollback);
        }
        persist_catalog(root, bytes)?;
        *self.catalog.write().map_err(|_| PluginCatalogError::Io)? = Ok(next);
        Ok(())
    }

    fn snapshot(&self) -> Result<VerifiedCatalog, PluginCatalogError> {
        self.catalog
            .read()
            .map_err(|_| PluginCatalogError::Io)?
            .clone()
    }
}

fn release_from_entry(version: u64, entry: &CatalogPluginRelease) -> TrustedPluginRelease {
    TrustedPluginRelease {
        manifest: entry.manifest.clone(),
        descriptor: entry.descriptor.clone(),
        release_tag: entry.release_tag.clone(),
        authentication: authentication_for(version, entry),
    }
}

fn host_version() -> Result<Version, PluginCatalogError> {
    Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_| PluginCatalogError::InvalidCatalog)
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(u64::MAX)
}

fn github_release_url(entry: &TrustedPluginRelease) -> Result<Url, PluginCatalogError> {
    let PluginSource::GithubRelease { repository, .. } = &entry.manifest.source else {
        return Err(PluginCatalogError::InvalidCatalog);
    };
    if !valid_release_tag(&entry.release_tag) {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    let mut url = Url::parse(repository).map_err(|_| PluginCatalogError::InvalidCatalog)?;
    if !trusted_initial_url(&url) {
        return Err(PluginCatalogError::DownloadUrlDenied);
    }
    let path = format!(
        "{}/releases/download/{}/{}",
        url.path().trim_end_matches('/'),
        entry.release_tag,
        entry.descriptor.asset
    );
    url.set_path(&path);
    Ok(url)
}

fn valid_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= MAX_RELEASE_TAG_BYTES
        && tag.as_bytes()[0].is_ascii_alphanumeric()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn trusted_initial_url(url: &Url) -> bool {
    trusted_url_basics(url) && url.host_str() == Some("github.com") && url.query().is_none()
}

fn trusted_redirect_url(url: &Url) -> bool {
    trusted_url_basics(url)
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "objects.githubusercontent.com"
                    | "objects-origin.githubusercontent.com"
                    | "github-releases.githubusercontent.com"
                    | "release-assets.githubusercontent.com"
            )
        )
}

fn trusted_url_basics(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
}

fn http_client() -> Result<Client, PluginCatalogError> {
    Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .user_agent(concat!("Lyrnova/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if trusted_redirect_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirect target denied")
            }
        }))
        .build()
        .map_err(|_| PluginCatalogError::DownloadFailed)
}

fn download_catalog_document() -> Result<Vec<u8>, PluginCatalogError> {
    let url = Url::parse(REMOTE_CATALOG_URL).map_err(|_| PluginCatalogError::InvalidCatalog)?;
    if !trusted_initial_url(&url) {
        return Err(PluginCatalogError::DownloadUrlDenied);
    }
    let mut response = http_client()?
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|_| PluginCatalogError::DownloadFailed)?;
    if !trusted_redirect_url(response.url()) {
        return Err(PluginCatalogError::DownloadUrlDenied);
    }
    if response
        .content_length()
        .is_some_and(|length| length > crate::plugin_trust::MAX_CATALOG_BYTES as u64)
    {
        return Err(PluginCatalogError::DownloadTooLarge);
    }
    let mut bytes = Vec::new();
    copy_bounded(
        &mut response,
        &mut bytes,
        crate::plugin_trust::MAX_CATALOG_BYTES as u64,
    )?;
    Ok(bytes)
}

pub(crate) struct DownloadedPluginPackage {
    directory: PathBuf,
    path: PathBuf,
}

impl DownloadedPluginPackage {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DownloadedPluginPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
        if let Some(downloads_root) = self.directory.parent() {
            let _ = fs::remove_dir(downloads_root);
        }
    }
}

pub(crate) fn download_release(
    root: &Path,
    release: &TrustedPluginRelease,
) -> Result<DownloadedPluginPackage, PluginCatalogError> {
    let url = github_release_url(release)?;
    let mut response = http_client()?
        .get(url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|_| PluginCatalogError::DownloadFailed)?;
    if !trusted_redirect_url(response.url()) {
        return Err(PluginCatalogError::DownloadUrlDenied);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PACKAGE_BYTES)
    {
        return Err(PluginCatalogError::DownloadTooLarge);
    }

    let directory = create_private_child_directory(root, DOWNLOADS_DIRECTORY)?;
    let path = directory.join(&release.descriptor.asset);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| PluginCatalogError::Io)?;
        set_private_file_permissions(&path)?;
        copy_bounded(&mut response, &mut file, MAX_PACKAGE_BYTES)?;
        file.sync_all().map_err(|_| PluginCatalogError::Io)
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    Ok(DownloadedPluginPackage { directory, path })
}

pub(crate) fn cleanup_downloads(root: &Path) {
    let downloads_root = root.join(DOWNLOADS_DIRECTORY);
    let Ok(metadata) = fs::symlink_metadata(&downloads_root) else {
        return;
    };
    if metadata.file_type().is_dir() {
        let _ = fs::remove_dir_all(downloads_root);
    } else {
        let _ = fs::remove_file(downloads_root);
    }
}

fn create_private_child_directory(root: &Path, child: &str) -> Result<PathBuf, PluginCatalogError> {
    fs::create_dir_all(root).map_err(|_| PluginCatalogError::Io)?;
    if !fs::symlink_metadata(root)
        .map_err(|_| PluginCatalogError::Io)?
        .file_type()
        .is_dir()
    {
        return Err(PluginCatalogError::Io);
    }
    let parent = root.join(child);
    ensure_private_directory(&parent)?;
    for _ in 0..8 {
        let directory = parent.join(Uuid::new_v4().simple().to_string());
        match fs::create_dir(&directory) {
            Ok(()) => {
                set_private_dir_permissions(&directory)?;
                return Ok(directory);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(PluginCatalogError::Io),
        }
    }
    Err(PluginCatalogError::Io)
}

fn ensure_private_directory(path: &Path) -> Result<(), PluginCatalogError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| PluginCatalogError::Io)?;
        }
        Err(_) => return Err(PluginCatalogError::Io),
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(PluginCatalogError::Io),
    }
    set_private_dir_permissions(path)
}

fn persist_catalog(root: &Path, bytes: &[u8]) -> Result<(), PluginCatalogError> {
    fs::create_dir_all(root).map_err(|_| PluginCatalogError::Io)?;
    if !fs::symlink_metadata(root)
        .map_err(|_| PluginCatalogError::Io)?
        .file_type()
        .is_dir()
    {
        return Err(PluginCatalogError::Io);
    }
    let directory = root.join(CATALOG_DIRECTORY);
    ensure_private_directory(&directory)?;
    let target = directory.join(CACHED_CATALOG_FILE);
    if fs::symlink_metadata(&target).is_ok_and(|metadata| !metadata.file_type().is_file()) {
        return Err(PluginCatalogError::Io);
    }
    let temporary = directory.join(format!(".{}.tmp", Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| PluginCatalogError::Io)?;
        set_private_file_permissions(&temporary)?;
        file.write_all(bytes).map_err(|_| PluginCatalogError::Io)?;
        file.sync_all().map_err(|_| PluginCatalogError::Io)?;
        fs::rename(&temporary, &target).map_err(|_| PluginCatalogError::Io)?;
        File::open(&directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| PluginCatalogError::Io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn read_bounded_regular_file(path: &Path) -> Result<Vec<u8>, PluginCatalogError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PluginCatalogError::Io)?;
    if !metadata.file_type().is_file()
        || metadata.len() > crate::plugin_trust::MAX_CATALOG_BYTES as u64
    {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .map_err(|_| PluginCatalogError::Io)?
        .take(crate::plugin_trust::MAX_CATALOG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PluginCatalogError::Io)?;
    if bytes.len() > crate::plugin_trust::MAX_CATALOG_BYTES {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    Ok(bytes)
}

fn copy_bounded(
    reader: &mut impl Read,
    writer: &mut impl Write,
    limit: u64,
) -> Result<u64, PluginCatalogError> {
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| PluginCatalogError::DownloadFailed)?;
        if read == 0 {
            return Ok(total);
        }
        total = total
            .checked_add(read as u64)
            .ok_or(PluginCatalogError::DownloadTooLarge)?;
        if total > limit {
            return Err(PluginCatalogError::DownloadTooLarge);
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|_| PluginCatalogError::Io)?;
    }
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), PluginCatalogError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| PluginCatalogError::Io)
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), PluginCatalogError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), PluginCatalogError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| PluginCatalogError::Io)
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), PluginCatalogError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lyrnova-plugin-catalog-test-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn embedded_catalog_is_valid_and_empty_until_a_release_is_curated() {
        assert_eq!(PluginCatalogService::default().summaries(), Ok(Vec::new()));
    }

    #[test]
    fn remote_updates_fail_closed_before_network_without_an_official_root_key() {
        let root = TestDirectory::new();
        assert_eq!(
            PluginCatalogService::default().update(&root.0),
            Err(PluginCatalogError::NoTrustedCatalogKeys)
        );
        assert!(!root.0.join(CATALOG_DIRECTORY).exists());
    }

    #[test]
    fn redirects_are_restricted_to_https_github_release_hosts() {
        for allowed in [
            "https://github.com/owner/repo/releases/download/v1/a.tar.zst",
            "https://objects.githubusercontent.com/object?sig=secret",
            "https://objects-origin.githubusercontent.com/object?sig=secret",
            "https://github-releases.githubusercontent.com/object?sig=secret",
            "https://release-assets.githubusercontent.com/object?sig=secret",
        ] {
            assert!(trusted_redirect_url(&Url::parse(allowed).unwrap()));
        }
        for denied in [
            "http://github.com/owner/repo",
            "https://github.com.evil.example/object",
            "https://raw.githubusercontent.com/owner/repo/main/plugin.tar.zst",
            "https://user:password@github.com/object",
        ] {
            assert!(!trusted_redirect_url(&Url::parse(denied).unwrap()));
        }
    }

    #[test]
    fn streaming_copy_stops_before_writing_beyond_the_limit() {
        let mut output = Vec::new();
        assert_eq!(
            copy_bounded(&mut io::Cursor::new(vec![7u8; 33]), &mut output, 32),
            Err(PluginCatalogError::DownloadTooLarge)
        );
        assert!(output.len() <= 32);
    }

    #[test]
    fn startup_cleanup_removes_an_interrupted_download() {
        let root = TestDirectory::new();
        let download = root.0.join(DOWNLOADS_DIRECTORY).join("interrupted");
        fs::create_dir_all(&download).unwrap();
        fs::write(download.join("partial.tar.zst"), b"partial").unwrap();
        cleanup_downloads(&root.0);
        assert!(!root.0.join(DOWNLOADS_DIRECTORY).exists());
    }

    #[cfg(unix)]
    #[test]
    fn staging_and_cache_reject_symlinked_internal_directories() {
        use std::os::unix::fs::symlink;

        let root = TestDirectory::new();
        let outside = root.0.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.0.join(DOWNLOADS_DIRECTORY)).unwrap();
        assert_eq!(
            create_private_child_directory(&root.0, DOWNLOADS_DIRECTORY),
            Err(PluginCatalogError::Io)
        );
        symlink(&outside, root.0.join(CATALOG_DIRECTORY)).unwrap();
        assert_eq!(persist_catalog(&root.0, b"{}"), Err(PluginCatalogError::Io));
    }
}
