use std::{collections::BTreeSet, path::Component};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use url::Url;

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PLUGIN_API_VERSION: u32 = 1;
pub const PROCESS_PROTOCOL_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_IDENTIFIER_BYTES: usize = 160;
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestOrigin {
    Bundled,
    External,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Language,
    Runtime,
    Framework,
    Tool,
    AiProvider,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    SyntaxHighlighting,
    Autocomplete,
    Diagnostics,
    Snippets,
    Templates,
    Lsp,
    Dap,
    Tasks,
    Tests,
    AccountAuth,
    AiChat,
    AiTools,
    Approvals,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    WorkspaceRead,
    WorkspaceWrite,
    ProcessSpawn,
    NetworkAccess,
    SecretStorage,
    RequestApproval,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCompatibility {
    pub lyrnova: VersionReq,
    pub plugin_api: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PluginRuntime {
    Builtin {
        module: String,
    },
    Process {
        entrypoint: String,
        protocol_version: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PluginSource {
    Bundled { repository: String },
    GithubRelease { repository: String, asset: String },
}

impl PluginSource {
    pub fn repository(&self) -> &str {
        match self {
            Self::Bundled { repository } | Self::GithubRelease { repository, .. } => repository,
        }
    }

    pub fn is_bundled(&self) -> bool {
        matches!(self, Self::Bundled { .. })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Version,
    pub publisher: String,
    pub license: String,
    pub kind: PluginKind,
    pub compatibility: PluginCompatibility,
    pub runtime: PluginRuntime,
    pub source: PluginSource,
    pub capabilities: Vec<PluginCapability>,
    pub permissions: Vec<PluginPermission>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestError {
    TooLarge,
    InvalidJson,
    UnsupportedSchemaVersion,
    InvalidId,
    InvalidName,
    InvalidDescription,
    InvalidPublisher,
    InvalidLicense,
    IncompatibleLyrnovaVersion,
    UnsupportedPluginApi,
    InvalidRepository,
    SourceMismatch,
    InvalidBuiltinModule,
    UnsafeEntrypoint,
    UnsupportedProcessProtocol,
    ProcessPermissionRequired,
    InvalidReleaseAsset,
    DuplicateCapability,
    DuplicatePermission,
}

pub fn parse_manifest(
    contents: &str,
    host_version: &Version,
    origin: ManifestOrigin,
) -> Result<PluginManifest, ManifestError> {
    if contents.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge);
    }
    let manifest: PluginManifest =
        serde_json::from_str(contents).map_err(|_| ManifestError::InvalidJson)?;
    validate_manifest(&manifest, host_version, origin)?;
    Ok(manifest)
}

pub fn permissions_exactly_match(
    requested: &[PluginPermission],
    granted: impl IntoIterator<Item = PluginPermission>,
) -> bool {
    let granted: Vec<_> = granted.into_iter().collect();
    let granted_set: BTreeSet<_> = granted.iter().copied().collect();
    granted.len() == granted_set.len()
        && requested.len() == granted_set.len()
        && requested
            .iter()
            .all(|permission| granted_set.contains(permission))
}

pub fn validate_manifest(
    manifest: &PluginManifest,
    host_version: &Version,
    origin: ManifestOrigin,
) -> Result<(), ManifestError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(ManifestError::UnsupportedSchemaVersion);
    }
    if !valid_dotted_identifier(&manifest.id, 3) {
        return Err(ManifestError::InvalidId);
    }
    if !valid_text(&manifest.name, 80) {
        return Err(ManifestError::InvalidName);
    }
    if !valid_text(&manifest.description, MAX_TEXT_BYTES) {
        return Err(ManifestError::InvalidDescription);
    }
    if !valid_identifier_segment(&manifest.publisher) {
        return Err(ManifestError::InvalidPublisher);
    }
    if !valid_spdx_expression(&manifest.license) {
        return Err(ManifestError::InvalidLicense);
    }
    if manifest.compatibility.plugin_api != PLUGIN_API_VERSION {
        return Err(ManifestError::UnsupportedPluginApi);
    }
    if !manifest.compatibility.lyrnova.matches(host_version) {
        return Err(ManifestError::IncompatibleLyrnovaVersion);
    }
    validate_repository(manifest.source.repository())?;
    if !matches!(
        (origin, &manifest.source),
        (ManifestOrigin::Bundled, PluginSource::Bundled { .. })
            | (ManifestOrigin::External, PluginSource::GithubRelease { .. })
    ) {
        return Err(ManifestError::SourceMismatch);
    }

    match &manifest.runtime {
        PluginRuntime::Builtin { module } => {
            if !valid_dotted_identifier(module, 2) {
                return Err(ManifestError::InvalidBuiltinModule);
            }
        }
        PluginRuntime::Process {
            entrypoint,
            protocol_version,
        } => {
            if !safe_relative_path(entrypoint) {
                return Err(ManifestError::UnsafeEntrypoint);
            }
            if *protocol_version != PROCESS_PROTOCOL_VERSION {
                return Err(ManifestError::UnsupportedProcessProtocol);
            }
            if !manifest
                .permissions
                .contains(&PluginPermission::ProcessSpawn)
            {
                return Err(ManifestError::ProcessPermissionRequired);
            }
        }
    }

    if let PluginSource::GithubRelease { asset, .. } = &manifest.source {
        if !safe_file_name(asset) {
            return Err(ManifestError::InvalidReleaseAsset);
        }
    }

    if contains_duplicate(&manifest.capabilities) {
        return Err(ManifestError::DuplicateCapability);
    }
    if contains_duplicate(&manifest.permissions) {
        return Err(ManifestError::DuplicatePermission);
    }
    Ok(())
}

fn contains_duplicate<T: Copy + Ord>(values: &[T]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().copied().any(|value| !seen.insert(value))
}

fn valid_text(value: &str, maximum_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum_bytes && !value.chars().any(char::is_control)
}

fn valid_identifier_segment(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_dotted_identifier(value: &str, minimum_segments: usize) -> bool {
    value.len() <= MAX_IDENTIFIER_BYTES
        && value.split('.').count() >= minimum_segments
        && value.split('.').all(valid_identifier_segment)
}

fn valid_spdx_expression(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'+' | b'(' | b')' | b' ')
        })
}

fn validate_repository(value: &str) -> Result<(), ManifestError> {
    let url = Url::parse(value).map_err(|_| ManifestError::InvalidRepository)?;
    let segments: Vec<_> = url
        .path_segments()
        .ok_or(ManifestError::InvalidRepository)?
        .filter(|segment| !segment.is_empty())
        .collect();
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().ends_with('/')
        || segments.len() != 2
        || !segments.iter().all(|segment| {
            !segment.is_empty()
                && segment.len() <= 100
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(ManifestError::InvalidRepository);
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 240
        || value.contains('\\')
        || value.contains(':')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
    {
        return false;
    }
    std::path::Path::new(value)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn safe_file_name(value: &str) -> bool {
    safe_relative_path(value) && !value.contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MANIFEST: &str = r#"{
      "schemaVersion": 1,
      "id": "io.github.w3ti.lyrnova.tool.example",
      "name": "Example",
      "description": "Manifest used by the contract tests.",
      "version": "0.1.0",
      "publisher": "w3ti",
      "license": "GPL-3.0-only",
      "kind": "tool",
      "compatibility": { "lyrnova": ">=0.1.0, <0.2.0", "pluginApi": 1 },
      "runtime": { "type": "process", "entrypoint": "bin/example", "protocolVersion": 1 },
      "source": {
        "type": "github_release",
        "repository": "https://github.com/w3ti/lyrnova-example",
        "asset": "lyrnova-example.tar.zst"
      },
      "capabilities": ["tasks"],
      "permissions": ["process_spawn"]
    }"#;

    fn host_version() -> Version {
        Version::new(0, 1, 0)
    }

    fn replaced(from: &str, to: &str) -> String {
        VALID_MANIFEST.replacen(from, to, 1)
    }

    #[test]
    fn accepts_a_strict_versioned_process_manifest() {
        let manifest =
            parse_manifest(VALID_MANIFEST, &host_version(), ManifestOrigin::External).unwrap();
        assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.permissions, [PluginPermission::ProcessSpawn]);
    }

    #[test]
    fn rejects_unknown_fields_and_capabilities() {
        let unknown_field = replaced(
            "\"schemaVersion\": 1,",
            "\"schemaVersion\": 1, \"surprise\": true,",
        );
        assert_eq!(
            parse_manifest(&unknown_field, &host_version(), ManifestOrigin::External),
            Err(ManifestError::InvalidJson)
        );

        let unknown_capability = replaced("\"tasks\"", "\"raw_shell\"");
        assert_eq!(
            parse_manifest(
                &unknown_capability,
                &host_version(),
                ManifestOrigin::External
            ),
            Err(ManifestError::InvalidJson)
        );

        let unknown_runtime_field = replaced(
            "\"protocolVersion\": 1",
            "\"protocolVersion\": 1, \"shell\": true",
        );
        assert_eq!(
            parse_manifest(
                &unknown_runtime_field,
                &host_version(),
                ManifestOrigin::External
            ),
            Err(ManifestError::InvalidJson)
        );

        let embedded_checksum = replaced(
            "\"asset\": \"lyrnova-example.tar.zst\"",
            "\"asset\": \"lyrnova-example.tar.zst\", \"sha256\": \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
        );
        assert_eq!(
            parse_manifest(
                &embedded_checksum,
                &host_version(),
                ManifestOrigin::External
            ),
            Err(ManifestError::InvalidJson)
        );
    }

    #[test]
    fn rejects_traversal_and_processes_without_permission() {
        let traversal = replaced("bin/example", "../example");
        assert_eq!(
            parse_manifest(&traversal, &host_version(), ManifestOrigin::External),
            Err(ManifestError::UnsafeEntrypoint)
        );

        let non_normal = replaced("bin/example", "bin//example");
        assert_eq!(
            parse_manifest(&non_normal, &host_version(), ManifestOrigin::External),
            Err(ManifestError::UnsafeEntrypoint)
        );

        let missing_permission = replaced("[\"process_spawn\"]", "[]");
        assert_eq!(
            parse_manifest(
                &missing_permission,
                &host_version(),
                ManifestOrigin::External
            ),
            Err(ManifestError::ProcessPermissionRequired)
        );
    }

    #[test]
    fn rejects_incompatible_hosts() {
        let incompatible = replaced(">=0.1.0, <0.2.0", ">=1.0.0");
        assert_eq!(
            parse_manifest(&incompatible, &host_version(), ManifestOrigin::External),
            Err(ManifestError::IncompatibleLyrnovaVersion)
        );
    }

    #[test]
    fn rejects_duplicate_authority_declarations() {
        let capabilities = replaced("[\"tasks\"]", "[\"tasks\", \"tasks\"]");
        assert_eq!(
            parse_manifest(&capabilities, &host_version(), ManifestOrigin::External),
            Err(ManifestError::DuplicateCapability)
        );

        let permissions = replaced(
            "[\"process_spawn\"]",
            "[\"process_spawn\", \"process_spawn\"]",
        );
        assert_eq!(
            parse_manifest(&permissions, &host_version(), ManifestOrigin::External),
            Err(ManifestError::DuplicatePermission)
        );
    }

    #[test]
    fn json_schema_enums_match_the_rust_contract() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../docs/plugins/plugin-manifest.schema.json"
        ))
        .unwrap();
        let schema_values = |name: &str| -> BTreeSet<String> {
            schema["$defs"][name]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect()
        };
        let serialized = |values: &[serde_json::Value]| -> BTreeSet<String> {
            values
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect()
        };

        let capabilities = [
            PluginCapability::SyntaxHighlighting,
            PluginCapability::Autocomplete,
            PluginCapability::Diagnostics,
            PluginCapability::Snippets,
            PluginCapability::Templates,
            PluginCapability::Lsp,
            PluginCapability::Dap,
            PluginCapability::Tasks,
            PluginCapability::Tests,
            PluginCapability::AccountAuth,
            PluginCapability::AiChat,
            PluginCapability::AiTools,
            PluginCapability::Approvals,
        ]
        .map(|value| serde_json::to_value(value).unwrap());
        let permissions = [
            PluginPermission::WorkspaceRead,
            PluginPermission::WorkspaceWrite,
            PluginPermission::ProcessSpawn,
            PluginPermission::NetworkAccess,
            PluginPermission::SecretStorage,
            PluginPermission::RequestApproval,
        ]
        .map(|value| serde_json::to_value(value).unwrap());

        assert_eq!(schema_values("capability"), serialized(&capabilities));
        assert_eq!(schema_values("permission"), serialized(&permissions));
    }

    #[test]
    fn external_manifests_cannot_claim_to_be_bundled() {
        let bundled = include_str!("../../plugins/builtin/rust/plugin.json");
        assert_eq!(
            parse_manifest(bundled, &host_version(), ManifestOrigin::External),
            Err(ManifestError::SourceMismatch)
        );
        assert_eq!(
            parse_manifest(VALID_MANIFEST, &host_version(), ManifestOrigin::Bundled),
            Err(ManifestError::SourceMismatch)
        );
    }
}
