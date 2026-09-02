use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use reqwest::{Url, blocking::Client, redirect};
use semver::Version;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    plugin_manifest::{ManifestOrigin, PluginManifest, PluginSource, validate_manifest},
    plugin_package::{MAX_PACKAGE_BYTES, PluginPackageDescriptor},
};

const CATALOG_SCHEMA_VERSION: u32 = 1;
const MAX_CATALOG_BYTES: usize = 512 * 1024;
const MAX_CATALOG_ENTRIES: usize = 256;
const MAX_RELEASE_TAG_BYTES: usize = 128;
const DOWNLOADS_DIRECTORY: &str = ".downloads";
const CATALOG_DOCUMENT: &str = include_str!("../../plugins/catalog/v1.json");

static TRUSTED_CATALOG: OnceLock<Result<Vec<TrustedPluginRelease>, PluginCatalogError>> =
    OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustedPluginCatalogDocument {
    schema_version: u32,
    entries: Vec<TrustedPluginRelease>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TrustedPluginRelease {
    pub(crate) manifest: PluginManifest,
    pub(crate) descriptor: PluginPackageDescriptor,
    release_tag: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrustedPluginSummary {
    pub manifest: PluginManifest,
    pub descriptor: PluginPackageDescriptor,
    pub installed_version: Option<Version>,
    pub download_available: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(crate) enum PluginCatalogError {
    InvalidCatalog,
    UnknownPlugin,
    DownloadUrlDenied,
    DownloadFailed,
    DownloadTooLarge,
    Io,
}

pub(crate) fn catalog_summaries() -> Result<Vec<TrustedPluginSummary>, PluginCatalogError> {
    Ok(trusted_catalog()?
        .iter()
        .map(|entry| TrustedPluginSummary {
            manifest: entry.manifest.clone(),
            descriptor: entry.descriptor.clone(),
            installed_version: None,
            download_available: true,
        })
        .collect())
}

pub(crate) fn trusted_release(id: &str) -> Result<TrustedPluginRelease, PluginCatalogError> {
    trusted_catalog()?
        .iter()
        .find(|entry| entry.manifest.id == id)
        .cloned()
        .ok_or(PluginCatalogError::UnknownPlugin)
}

fn trusted_catalog() -> Result<&'static [TrustedPluginRelease], PluginCatalogError> {
    match TRUSTED_CATALOG.get_or_init(|| {
        let host_version = Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|_| PluginCatalogError::InvalidCatalog)?;
        parse_catalog(CATALOG_DOCUMENT, &host_version)
    }) {
        Ok(entries) => Ok(entries),
        Err(error) => Err(*error),
    }
}

fn parse_catalog(
    document: &str,
    host_version: &Version,
) -> Result<Vec<TrustedPluginRelease>, PluginCatalogError> {
    if document.len() > MAX_CATALOG_BYTES {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    let catalog: TrustedPluginCatalogDocument =
        serde_json::from_str(document).map_err(|_| PluginCatalogError::InvalidCatalog)?;
    if catalog.schema_version != CATALOG_SCHEMA_VERSION
        || catalog.entries.len() > MAX_CATALOG_ENTRIES
    {
        return Err(PluginCatalogError::InvalidCatalog);
    }

    let mut ids = BTreeSet::new();
    for entry in &catalog.entries {
        validate_manifest(&entry.manifest, host_version, ManifestOrigin::External)
            .map_err(|_| PluginCatalogError::InvalidCatalog)?;
        entry
            .descriptor
            .validate()
            .map_err(|_| PluginCatalogError::InvalidCatalog)?;
        let PluginSource::GithubRelease { asset, .. } = &entry.manifest.source else {
            return Err(PluginCatalogError::InvalidCatalog);
        };
        if asset != &entry.descriptor.asset
            || !valid_release_tag(&entry.release_tag)
            || !ids.insert(entry.manifest.id.clone())
            || github_release_url(entry).is_err()
        {
            return Err(PluginCatalogError::InvalidCatalog);
        }
    }
    Ok(catalog.entries)
}

fn valid_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= MAX_RELEASE_TAG_BYTES
        && tag.as_bytes()[0].is_ascii_alphanumeric()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn github_release_url(entry: &TrustedPluginRelease) -> Result<Url, PluginCatalogError> {
    let PluginSource::GithubRelease { repository, .. } = &entry.manifest.source else {
        return Err(PluginCatalogError::InvalidCatalog);
    };
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
    let client = Client::builder()
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
        .map_err(|_| PluginCatalogError::DownloadFailed)?;
    let mut response = client
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

    let directory = create_download_directory(root)?;
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

fn create_download_directory(root: &Path) -> Result<PathBuf, PluginCatalogError> {
    fs::create_dir_all(root).map_err(|_| PluginCatalogError::Io)?;
    let root_metadata = fs::symlink_metadata(root).map_err(|_| PluginCatalogError::Io)?;
    if !root_metadata.file_type().is_dir() {
        return Err(PluginCatalogError::Io);
    }
    let downloads_root = root.join(DOWNLOADS_DIRECTORY);
    match fs::symlink_metadata(&downloads_root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&downloads_root).map_err(|_| PluginCatalogError::Io)?;
        }
        Err(_) => return Err(PluginCatalogError::Io),
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(PluginCatalogError::Io),
    }
    set_private_dir_permissions(&downloads_root)?;
    for _ in 0..8 {
        let directory = downloads_root.join(Uuid::new_v4().simple().to_string());
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

    fn catalog_document(id: &str, asset: &str, release_tag: &str) -> String {
        format!(
            r#"{{
              "schemaVersion": 1,
              "entries": [{{
                "manifest": {{
                  "schemaVersion": 1,
                  "id": "{id}",
                  "name": "Example",
                  "description": "Curated external plugin used by catalog tests.",
                  "version": "0.1.0",
                  "publisher": "example",
                  "license": "GPL-3.0-only",
                  "kind": "tool",
                  "compatibility": {{ "lyrnova": ">=0.1.0, <0.2.0", "pluginApi": 1 }},
                  "runtime": {{ "type": "process", "entrypoint": "bin/example", "protocolVersion": 1 }},
                  "source": {{
                    "type": "github_release",
                    "repository": "https://github.com/example/lyrnova-example",
                    "asset": "{asset}"
                  }},
                  "capabilities": ["tasks"],
                  "permissions": ["workspace_read", "process_spawn"]
                }},
                "descriptor": {{ "asset": "{asset}", "sha256": "{}" }},
                "releaseTag": "{release_tag}"
              }}]
            }}"#,
            "a".repeat(64)
        )
    }

    #[test]
    fn curated_catalog_derives_the_release_url_from_validated_fields() {
        let entries = parse_catalog(
            &catalog_document(
                "io.github.example.lyrnova.tool.example",
                "example.tar.zst",
                "v0.1.0",
            ),
            &Version::new(0, 1, 0),
        )
        .unwrap();
        assert_eq!(
            github_release_url(&entries[0]).unwrap().as_str(),
            "https://github.com/example/lyrnova-example/releases/download/v0.1.0/example.tar.zst"
        );
    }

    #[test]
    fn embedded_catalog_is_valid_and_empty_until_a_release_is_curated() {
        assert_eq!(catalog_summaries(), Ok(Vec::new()));
    }

    #[test]
    fn curated_catalog_rejects_duplicates_mismatches_and_unsafe_tags() {
        let valid = catalog_document(
            "io.github.example.lyrnova.tool.example",
            "example.tar.zst",
            "v0.1.0",
        );
        let mut duplicate: serde_json::Value = serde_json::from_str(&valid).unwrap();
        let entry = duplicate["entries"][0].clone();
        duplicate["entries"].as_array_mut().unwrap().push(entry);
        assert_eq!(
            parse_catalog(&duplicate.to_string(), &Version::new(0, 1, 0)),
            Err(PluginCatalogError::InvalidCatalog)
        );
        assert!(
            parse_catalog(
                &catalog_document(
                    "io.github.example.lyrnova.tool.example",
                    "example.tar.zst",
                    "../escape",
                ),
                &Version::new(0, 1, 0),
            )
            .is_err()
        );
        assert!(
            parse_catalog(
                &valid.replace(
                    "\"descriptor\": { \"asset\": \"example.tar.zst\"",
                    "\"descriptor\": { \"asset\": \"other.tar.zst\"",
                ),
                &Version::new(0, 1, 0),
            )
            .is_err()
        );
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
    fn download_staging_rejects_a_symlinked_root() {
        use std::os::unix::fs::symlink;

        let root = TestDirectory::new();
        let outside = root.0.join("outside");
        let store = root.0.join("store");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(&store).unwrap();
        symlink(&outside, store.join(DOWNLOADS_DIRECTORY)).unwrap();

        assert_eq!(
            create_download_directory(&store),
            Err(PluginCatalogError::Io)
        );
        assert!(outside.exists());
    }
}
