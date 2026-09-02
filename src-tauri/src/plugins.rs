use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{OnceLock, RwLock},
};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::plugin_manifest::{
    ManifestOrigin, PluginCapability, PluginCompatibility, PluginKind, PluginManifest,
    PluginPermission, parse_manifest, permissions_exactly_match,
};

pub const CODEX_PLUGIN_ID: &str = "io.github.w3ti.lyrnova.ai.codex";
const PLUGIN_STATE_VERSION: u32 = 3;
const MAX_PLUGIN_STATE_BYTES: u64 = 64 * 1024;
const BUNDLED_MANIFESTS: &[&str] = &[
    include_str!("../../plugins/builtin/rust/plugin.json"),
    include_str!("../../plugins/builtin/web/plugin.json"),
    include_str!("../../plugins/builtin/codex/plugin.json"),
];

static CATALOG: OnceLock<Result<Vec<PluginManifest>, PluginError>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Version,
    pub publisher: String,
    pub license: String,
    pub kind: PluginKind,
    pub compatibility: PluginCompatibility,
    pub installed: bool,
    pub enabled: bool,
    pub official: bool,
    pub bundled: bool,
    pub repository: String,
    pub capabilities: Vec<PluginCapability>,
    pub permissions: Vec<PluginPermission>,
    pub granted_permissions: Vec<PluginPermission>,
    pub requires_permission_review: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PluginPreferences {
    version: u32,
    installed: BTreeSet<String>,
    enabled: BTreeSet<String>,
    grants: BTreeMap<String, BTreeSet<PluginPermission>>,
}

impl PluginPreferences {
    fn defaults(catalog: &[PluginManifest]) -> Self {
        let installed: BTreeSet<_> = catalog
            .iter()
            .filter(|plugin| plugin.kind == PluginKind::Language)
            .map(|plugin| plugin.id.clone())
            .collect();
        let grants = catalog
            .iter()
            .filter(|plugin| installed.contains(&plugin.id))
            .map(|plugin| {
                (
                    plugin.id.clone(),
                    plugin.permissions.iter().copied().collect(),
                )
            })
            .collect();
        Self {
            version: PLUGIN_STATE_VERSION,
            enabled: installed.clone(),
            installed,
            grants,
        }
    }

    fn fail_closed() -> Self {
        Self {
            version: PLUGIN_STATE_VERSION,
            installed: BTreeSet::new(),
            enabled: BTreeSet::new(),
            grants: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct PluginRegistry {
    preferences: RwLock<PluginPreferences>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        let preferences = catalog()
            .map(PluginPreferences::defaults)
            .unwrap_or_else(|_| PluginPreferences::fail_closed());
        Self {
            preferences: RwLock::new(preferences),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PluginError {
    UnknownPlugin,
    NotInstalled,
    PluginDisabled,
    InvalidManifest,
    DuplicatePlugin,
    PermissionApprovalRequired,
    PermissionDenied,
    StateUnavailable,
    Io,
}

impl PluginRegistry {
    pub fn load(&self, app: &tauri::AppHandle) {
        let Some(preferences) = read_preferences(app) else {
            return;
        };
        if let Ok(mut current) = self.preferences.write() {
            *current = preferences;
        }
    }

    pub fn list(&self) -> Result<Vec<PluginSummary>, PluginError> {
        let catalog = catalog()?;
        let preferences = self
            .preferences
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        Ok(catalog
            .iter()
            .map(|manifest| {
                let granted_permissions: Vec<_> = preferences
                    .grants
                    .get(&manifest.id)
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect();
                let permissions_approved = permissions_exactly_match(
                    &manifest.permissions,
                    granted_permissions.iter().copied(),
                );
                PluginSummary {
                    schema_version: manifest.schema_version,
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                    version: manifest.version.clone(),
                    publisher: manifest.publisher.clone(),
                    license: manifest.license.clone(),
                    kind: manifest.kind,
                    compatibility: manifest.compatibility.clone(),
                    installed: preferences.installed.contains(&manifest.id),
                    enabled: preferences.enabled.contains(&manifest.id) && permissions_approved,
                    official: manifest.publisher == "w3ti" && manifest.source.is_bundled(),
                    bundled: manifest.source.is_bundled(),
                    repository: manifest.source.repository().to_owned(),
                    capabilities: manifest.capabilities.clone(),
                    permissions: manifest.permissions.clone(),
                    granted_permissions,
                    requires_permission_review: !permissions_approved,
                }
            })
            .collect())
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        let Ok(manifest) = catalog_plugin(id) else {
            return false;
        };
        self.preferences.read().is_ok_and(|preferences| {
            preferences.installed.contains(id)
                && preferences.enabled.contains(id)
                && permissions_exactly_match(
                    &manifest.permissions,
                    preferences.grants.get(id).into_iter().flatten().copied(),
                )
        })
    }

    pub fn authorize(&self, id: &str, permission: PluginPermission) -> Result<(), PluginError> {
        let manifest = catalog_plugin(id)?;
        let preferences = self
            .preferences
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        if !preferences.installed.contains(id) {
            return Err(PluginError::NotInstalled);
        }
        if !preferences.enabled.contains(id) {
            return Err(PluginError::PluginDisabled);
        }
        if !manifest.permissions.contains(&permission)
            || !preferences
                .grants
                .get(id)
                .is_some_and(|grants| grants.contains(&permission))
        {
            return Err(PluginError::PermissionDenied);
        }
        Ok(())
    }

    pub fn install(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        approved_permissions: &[PluginPermission],
    ) -> Result<Vec<PluginSummary>, PluginError> {
        let manifest = catalog_plugin(id)?;
        if !permissions_exactly_match(&manifest.permissions, approved_permissions.iter().copied()) {
            return Err(PluginError::PermissionApprovalRequired);
        }
        {
            let mut preferences = self
                .preferences
                .write()
                .map_err(|_| PluginError::StateUnavailable)?;
            let mut next = preferences.clone();
            next.installed.insert(id.to_owned());
            next.enabled.insert(id.to_owned());
            next.grants.insert(
                id.to_owned(),
                approved_permissions.iter().copied().collect(),
            );
            write_preferences(app, &next)?;
            *preferences = next;
        }
        self.list()
    }

    pub fn uninstall(
        &self,
        app: &tauri::AppHandle,
        id: &str,
    ) -> Result<Vec<PluginSummary>, PluginError> {
        catalog_plugin(id)?;
        {
            let mut preferences = self
                .preferences
                .write()
                .map_err(|_| PluginError::StateUnavailable)?;
            let mut next = preferences.clone();
            if !next.installed.remove(id) {
                return Err(PluginError::NotInstalled);
            }
            next.enabled.remove(id);
            next.grants.remove(id);
            write_preferences(app, &next)?;
            *preferences = next;
        }
        self.list()
    }

    pub fn set_enabled(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        enabled: bool,
    ) -> Result<Vec<PluginSummary>, PluginError> {
        let manifest = catalog_plugin(id)?;
        {
            let mut preferences = self
                .preferences
                .write()
                .map_err(|_| PluginError::StateUnavailable)?;
            if !preferences.installed.contains(id) {
                return Err(PluginError::NotInstalled);
            }
            if enabled
                && !permissions_exactly_match(
                    &manifest.permissions,
                    preferences.grants.get(id).into_iter().flatten().copied(),
                )
            {
                return Err(PluginError::PermissionApprovalRequired);
            }
            let mut next = preferences.clone();
            if enabled {
                next.enabled.insert(id.to_owned());
            } else {
                next.enabled.remove(id);
            }
            write_preferences(app, &next)?;
            *preferences = next;
        }
        self.list()
    }

    pub fn open_repository(&self, id: &str) -> Result<(), PluginError> {
        let repository = catalog_plugin(id)?.source.repository();
        #[cfg(target_os = "linux")]
        let mut command = Command::new("xdg-open");
        #[cfg(target_os = "macos")]
        let mut command = Command::new("open");
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("rundll32.exe");
            command.arg("url.dll,FileProtocolHandler");
            command
        };
        command
            .arg(repository)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| PluginError::Io)
    }
}

fn catalog() -> Result<&'static [PluginManifest], PluginError> {
    match CATALOG.get_or_init(|| load_catalog_from(BUNDLED_MANIFESTS)) {
        Ok(catalog) => Ok(catalog),
        Err(error) => Err(*error),
    }
}

fn load_catalog_from(documents: &[&str]) -> Result<Vec<PluginManifest>, PluginError> {
    let host_version =
        Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_| PluginError::InvalidManifest)?;
    let mut ids = BTreeSet::new();
    let mut manifests = Vec::with_capacity(documents.len());
    for document in documents {
        let manifest = parse_manifest(document, &host_version, ManifestOrigin::Bundled)
            .map_err(|_| PluginError::InvalidManifest)?;
        if !ids.insert(manifest.id.clone()) {
            return Err(PluginError::DuplicatePlugin);
        }
        manifests.push(manifest);
    }
    Ok(manifests)
}

fn catalog_plugin(id: &str) -> Result<&'static PluginManifest, PluginError> {
    catalog()?
        .iter()
        .find(|plugin| plugin.id == id)
        .ok_or(PluginError::UnknownPlugin)
}

fn normalize_preferences(
    mut preferences: PluginPreferences,
    catalog: &[PluginManifest],
) -> PluginPreferences {
    let known: BTreeSet<_> = catalog.iter().map(|plugin| plugin.id.clone()).collect();
    preferences.installed.retain(|id| known.contains(id));
    preferences
        .grants
        .retain(|id, _| preferences.installed.contains(id));
    for (id, grants) in &mut preferences.grants {
        if let Some(manifest) = catalog.iter().find(|plugin| &plugin.id == id) {
            grants.retain(|permission| manifest.permissions.contains(permission));
        }
    }
    preferences.enabled.retain(|id| {
        preferences.installed.contains(id)
            && catalog
                .iter()
                .find(|plugin| &plugin.id == id)
                .is_some_and(|manifest| {
                    permissions_exactly_match(
                        &manifest.permissions,
                        preferences.grants.get(id).into_iter().flatten().copied(),
                    )
                })
    });
    preferences
}

fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, PluginError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("plugins.json"))
        .map_err(|_| PluginError::Io)
}

fn read_preferences(app: &tauri::AppHandle) -> Option<PluginPreferences> {
    let path = preferences_path(app).ok()?;
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLUGIN_STATE_BYTES {
        return None;
    }
    let preferences: PluginPreferences = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    if preferences.version != PLUGIN_STATE_VERSION {
        return None;
    }
    Some(normalize_preferences(preferences, catalog().ok()?))
}

fn write_preferences(
    app: &tauri::AppHandle,
    preferences: &PluginPreferences,
) -> Result<(), PluginError> {
    let path = preferences_path(app)?;
    let parent = path.parent().ok_or(PluginError::Io)?;
    fs::create_dir_all(parent).map_err(|_| PluginError::Io)?;
    let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(preferences).map_err(|_| PluginError::Io)?;
    fs::write(&temporary, bytes).map_err(|_| PluginError::Io)?;
    fs::rename(&temporary, path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        PluginError::Io
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_strict_typed_and_unique() {
        let catalog = load_catalog_from(BUNDLED_MANIFESTS).unwrap();
        assert_eq!(catalog.len(), 3);
        assert!(catalog.iter().all(|plugin| plugin.source.is_bundled()));
        assert!(catalog.iter().all(|plugin| plugin.publisher == "w3ti"));
        assert!(
            catalog
                .iter()
                .find(|plugin| plugin.id == CODEX_PLUGIN_ID)
                .unwrap()
                .permissions
                .contains(&PluginPermission::RequestApproval)
        );
    }

    #[test]
    fn duplicate_catalog_ids_fail_closed() {
        assert_eq!(
            load_catalog_from(&[BUNDLED_MANIFESTS[0], BUNDLED_MANIFESTS[0]]),
            Err(PluginError::DuplicatePlugin)
        );
    }

    #[test]
    fn defaults_install_languages_without_an_ai_provider() {
        let catalog = load_catalog_from(BUNDLED_MANIFESTS).unwrap();
        let preferences = PluginPreferences::defaults(&catalog);
        assert!(
            preferences
                .installed
                .contains("io.github.w3ti.lyrnova.language.rust")
        );
        assert!(
            preferences
                .installed
                .contains("io.github.w3ti.lyrnova.language.web")
        );
        assert!(!preferences.installed.contains(CODEX_PLUGIN_ID));
        assert!(!preferences.enabled.contains(CODEX_PLUGIN_ID));
        assert!(!preferences.grants.contains_key(CODEX_PLUGIN_ID));
    }

    #[test]
    fn permission_approval_must_exactly_match_the_manifest() {
        let requested = [PluginPermission::WorkspaceRead];
        assert!(permissions_exactly_match(&requested, requested));
        assert!(!permissions_exactly_match(&requested, []));
        assert!(!permissions_exactly_match(
            &requested,
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::WorkspaceWrite,
            ]
        ));
        assert!(!permissions_exactly_match(
            &requested,
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::WorkspaceRead,
            ]
        ));
    }

    #[test]
    fn a_permission_change_disables_the_plugin_until_reviewed() {
        let catalog = load_catalog_from(BUNDLED_MANIFESTS).unwrap();
        let mut preferences = PluginPreferences::fail_closed();
        preferences.installed.insert(CODEX_PLUGIN_ID.into());
        preferences.enabled.insert(CODEX_PLUGIN_ID.into());
        preferences.grants.insert(
            CODEX_PLUGIN_ID.into(),
            [PluginPermission::WorkspaceRead].into_iter().collect(),
        );

        let normalized = normalize_preferences(preferences, &catalog);
        assert!(normalized.installed.contains(CODEX_PLUGIN_ID));
        assert!(!normalized.enabled.contains(CODEX_PLUGIN_ID));
    }

    #[test]
    fn authorization_requires_install_enable_declaration_and_grant() {
        let catalog = load_catalog_from(BUNDLED_MANIFESTS).unwrap();
        let mut preferences = PluginPreferences::fail_closed();
        preferences.installed.insert(CODEX_PLUGIN_ID.into());
        preferences.enabled.insert(CODEX_PLUGIN_ID.into());
        preferences.grants.insert(
            CODEX_PLUGIN_ID.into(),
            catalog
                .iter()
                .find(|plugin| plugin.id == CODEX_PLUGIN_ID)
                .unwrap()
                .permissions
                .iter()
                .copied()
                .collect(),
        );
        let registry = PluginRegistry {
            preferences: RwLock::new(preferences),
        };

        assert_eq!(
            registry.authorize(CODEX_PLUGIN_ID, PluginPermission::NetworkAccess),
            Ok(())
        );
        assert_eq!(
            registry.authorize(CODEX_PLUGIN_ID, PluginPermission::WorkspaceWrite),
            Err(PluginError::PermissionDenied)
        );
    }
}
