use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{OnceLock, RwLock},
};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::plugin_catalog::{PluginCatalogService, cleanup_downloads};
use crate::plugin_manifest::{
    ManifestOrigin, PluginCapability, PluginCompatibility, PluginKind, PluginManifest,
    PluginPermission, PluginRuntime, parse_manifest, permissions_exactly_match,
};
use crate::plugin_package::{
    DiscoveredPluginPackage, InstalledPluginPackage, QuarantinedPluginPackages,
    cleanup_committed_removals, discover_installed_packages,
};
use crate::plugin_runtime::ExternalRuntimeSpec;
use crate::tasks::TaskProvider;

#[cfg(test)]
const CODEX_PLUGIN_ID: &str = "io.github.w3ti.lyrnova.ai.codex";
const PLUGIN_STATE_VERSION: u32 = 4;
const MAX_PLUGIN_STATE_BYTES: u64 = 64 * 1024;
const BUNDLED_MANIFESTS: &[&str] = &[
    include_str!("../../plugins/builtin/rust/plugin.json"),
    include_str!("../../plugins/builtin/web/plugin.json"),
    include_str!("../../plugins/builtin/codex/plugin.json"),
];

static BUNDLED_CATALOG: OnceLock<Result<Vec<PluginManifest>, PluginError>> = OnceLock::new();

#[derive(Clone, Debug)]
struct CatalogPlugin {
    manifest: PluginManifest,
    external_path: Option<PathBuf>,
    publisher_key_id: Option<String>,
}

#[derive(Clone, Debug)]
struct PluginRegistryState {
    catalog: Vec<CatalogPlugin>,
    preferences: PluginPreferences,
}

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
    pub publisher_verified: bool,
    pub publisher_key_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AiProviderRuntime {
    Builtin { module: String },
    Process,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveAiProvider {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<PluginCapability>,
    pub runtime: AiProviderRuntime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderSummary {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<PluginCapability>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginPreferences {
    version: u32,
    installed: BTreeMap<String, Version>,
    enabled: BTreeSet<String>,
    grants: BTreeMap<String, BTreeSet<PluginPermission>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyPluginPreferences {
    version: u32,
    installed: BTreeSet<String>,
    enabled: BTreeSet<String>,
    grants: BTreeMap<String, BTreeSet<PluginPermission>>,
}

impl PluginPreferences {
    fn defaults(catalog: &[CatalogPlugin]) -> Self {
        let installed: BTreeMap<_, _> = catalog
            .iter()
            .map(|plugin| &plugin.manifest)
            .filter(|plugin| plugin.kind == PluginKind::Language && plugin.source.is_bundled())
            .map(|plugin| (plugin.id.clone(), plugin.version.clone()))
            .collect();
        let grants = catalog
            .iter()
            .map(|plugin| &plugin.manifest)
            .filter(|plugin| installed.contains_key(&plugin.id))
            .map(|plugin| {
                (
                    plugin.id.clone(),
                    plugin.permissions.iter().copied().collect(),
                )
            })
            .collect();
        Self {
            version: PLUGIN_STATE_VERSION,
            enabled: installed.keys().cloned().collect(),
            installed,
            grants,
        }
    }

    fn fail_closed() -> Self {
        Self {
            version: PLUGIN_STATE_VERSION,
            installed: BTreeMap::new(),
            enabled: BTreeSet::new(),
            grants: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct PluginRegistry {
    state: RwLock<PluginRegistryState>,
    catalog_service: PluginCatalogService,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        let catalog = bundled_catalog()
            .map(bundled_catalog_entries)
            .unwrap_or_default();
        let preferences = if catalog.is_empty() {
            PluginPreferences::fail_closed()
        } else {
            PluginPreferences::defaults(&catalog)
        };
        Self {
            state: RwLock::new(PluginRegistryState {
                catalog,
                preferences,
            }),
            catalog_service: PluginCatalogService::default(),
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
    InvalidInstalledPackage,
    ExternalPackageRemovalFailed,
    ExternalPackageRollbackFailed,
    PermissionApprovalRequired,
    PermissionDenied,
    ExternalRuntimeUnsupported,
    SandboxUnavailable,
    RuntimeWorkspaceUnavailable,
    RuntimeStartFailed,
    RuntimeStopFailed,
    NoAiProvider,
    MultipleAiProviders,
    CapabilityUnavailable,
    StateUnavailable,
    Io,
}

impl PluginRegistry {
    pub(crate) fn with_catalog_service(catalog_service: PluginCatalogService) -> Self {
        Self {
            catalog_service,
            ..Self::default()
        }
    }

    pub fn load(&self, app: &tauri::AppHandle) {
        let _ = self.reload(app);
    }

    pub fn reload(&self, app: &tauri::AppHandle) -> Result<Vec<PluginSummary>, PluginError> {
        let root = plugin_storage_root(app)?;
        cleanup_downloads(&root);
        cleanup_committed_removals(&root);
        let host_version = host_version()?;
        let catalog = match load_catalog_from_storage(&root, &host_version, &self.catalog_service) {
            Ok(catalog) => catalog,
            Err(error) => {
                self.fail_closed_external_packages(app)?;
                return Err(error);
            }
        };
        let preferences = read_preferences(app, &catalog)
            .unwrap_or_else(|| PluginPreferences::defaults(&catalog));
        let preferences = normalize_preferences(preferences, &catalog);
        write_preferences(app, &preferences)?;
        let mut state = self
            .state
            .write()
            .map_err(|_| PluginError::StateUnavailable)?;
        *state = PluginRegistryState {
            catalog,
            preferences,
        };
        Ok(summaries(&state))
    }

    pub fn register_external_install(
        &self,
        app: &tauri::AppHandle,
        installed: &InstalledPluginPackage,
        approved_permissions: &[PluginPermission],
    ) -> Result<Vec<PluginSummary>, PluginError> {
        let root = plugin_storage_root(app)?;
        let catalog =
            match load_catalog_from_storage(&root, &host_version()?, &self.catalog_service) {
                Ok(catalog) => catalog,
                Err(error) => {
                    self.fail_closed_external_packages(app)?;
                    return Err(error);
                }
            };
        let mut state = self
            .state
            .write()
            .map_err(|_| PluginError::StateUnavailable)?;
        let preferences = normalize_preferences(state.preferences.clone(), &catalog);
        let preferences = record_external_install(
            preferences,
            &catalog,
            &installed.manifest.id,
            &installed.manifest.version,
            approved_permissions,
        )?;
        write_preferences(app, &preferences)?;
        *state = PluginRegistryState {
            catalog,
            preferences,
        };
        Ok(summaries(&state))
    }

    fn fail_closed_external_packages(&self, app: &tauri::AppHandle) -> Result<(), PluginError> {
        let catalog = bundled_catalog_entries(bundled_catalog()?);
        let preferences = {
            let state = self
                .state
                .read()
                .map_err(|_| PluginError::StateUnavailable)?;
            normalize_preferences(state.preferences.clone(), &catalog)
        };
        {
            let mut state = self
                .state
                .write()
                .map_err(|_| PluginError::StateUnavailable)?;
            *state = PluginRegistryState {
                catalog,
                preferences: preferences.clone(),
            };
        }
        write_preferences(app, &preferences)
    }

    pub fn list(&self) -> Result<Vec<PluginSummary>, PluginError> {
        let state = self
            .state
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        Ok(summaries(&state))
    }

    pub fn active_ai_provider(
        &self,
        required_capabilities: &[PluginCapability],
        required_permissions: &[PluginPermission],
    ) -> Result<ActiveAiProvider, PluginError> {
        let state = self
            .state
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        let mut enabled = state.catalog.iter().filter(|plugin| {
            let manifest = &plugin.manifest;
            manifest.kind == PluginKind::AiProvider
                && state.preferences.installed.get(&manifest.id) == Some(&manifest.version)
                && state.preferences.enabled.contains(&manifest.id)
                && permissions_exactly_match(
                    &manifest.permissions,
                    state
                        .preferences
                        .grants
                        .get(&manifest.id)
                        .into_iter()
                        .flatten()
                        .copied(),
                )
        });
        let Some(provider) = enabled.next() else {
            return Err(PluginError::NoAiProvider);
        };
        if enabled.next().is_some() {
            return Err(PluginError::MultipleAiProviders);
        }
        if !required_capabilities
            .iter()
            .all(|capability| provider.manifest.capabilities.contains(capability))
        {
            return Err(PluginError::CapabilityUnavailable);
        }
        let grants = state.preferences.grants.get(&provider.manifest.id);
        if !required_permissions.iter().all(|permission| {
            provider.manifest.permissions.contains(permission)
                && grants.is_some_and(|grants| grants.contains(permission))
        }) {
            return Err(PluginError::PermissionDenied);
        }
        let runtime = match &provider.manifest.runtime {
            PluginRuntime::Builtin { module } => AiProviderRuntime::Builtin {
                module: module.clone(),
            },
            PluginRuntime::Process { .. } => AiProviderRuntime::Process,
        };
        Ok(ActiveAiProvider {
            id: provider.manifest.id.clone(),
            name: provider.manifest.name.clone(),
            capabilities: provider.manifest.capabilities.clone(),
            runtime,
        })
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        let Ok(state) = self.state.read() else {
            return false;
        };
        let Some(plugin) = state.catalog.iter().find(|plugin| plugin.manifest.id == id) else {
            return false;
        };
        state.preferences.installed.get(id) == Some(&plugin.manifest.version)
            && state.preferences.enabled.contains(id)
            && permissions_exactly_match(
                &plugin.manifest.permissions,
                state
                    .preferences
                    .grants
                    .get(id)
                    .into_iter()
                    .flatten()
                    .copied(),
            )
    }

    pub fn authorize(&self, id: &str, permission: PluginPermission) -> Result<(), PluginError> {
        let state = self
            .state
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        let plugin = catalog_plugin(&state.catalog, id)?;
        if state.preferences.installed.get(id) != Some(&plugin.manifest.version) {
            return Err(PluginError::NotInstalled);
        }
        if !state.preferences.enabled.contains(id) {
            return Err(PluginError::PluginDisabled);
        }
        if !plugin.manifest.permissions.contains(&permission)
            || !state
                .preferences
                .grants
                .get(id)
                .is_some_and(|grants| grants.contains(&permission))
        {
            return Err(PluginError::PermissionDenied);
        }
        Ok(())
    }

    pub fn external_runtime_spec(
        &self,
        id: &str,
        require_enabled: bool,
    ) -> Result<Option<ExternalRuntimeSpec>, PluginError> {
        let state = self
            .state
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        external_runtime_spec(&state, id, require_enabled)
    }

    pub fn enabled_external_runtime_specs(&self) -> Result<Vec<ExternalRuntimeSpec>, PluginError> {
        let state = self
            .state
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        state
            .catalog
            .iter()
            .filter(|plugin| plugin.external_path.is_some())
            .filter(|plugin| state.preferences.enabled.contains(&plugin.manifest.id))
            .map(|plugin| external_runtime_spec(&state, &plugin.manifest.id, true))
            .filter_map(|result| result.transpose())
            .collect()
    }

    pub fn enabled_task_providers(&self) -> Result<Vec<TaskProvider>, PluginError> {
        let state = self
            .state
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        state
            .catalog
            .iter()
            .filter(|plugin| {
                plugin.external_path.is_some()
                    && plugin
                        .manifest
                        .capabilities
                        .contains(&PluginCapability::Tasks)
                    && state.preferences.installed.get(&plugin.manifest.id)
                        == Some(&plugin.manifest.version)
                    && state.preferences.enabled.contains(&plugin.manifest.id)
            })
            .map(|plugin| task_provider(&state, &plugin.manifest.id))
            .collect()
    }

    pub fn task_provider(&self, id: &str) -> Result<TaskProvider, PluginError> {
        let state = self
            .state
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        task_provider(&state, id)
    }

    pub fn install(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        approved_permissions: &[PluginPermission],
    ) -> Result<Vec<PluginSummary>, PluginError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| PluginError::StateUnavailable)?;
        let manifest = catalog_plugin(&state.catalog, id)?.manifest.clone();
        if !permissions_exactly_match(&manifest.permissions, approved_permissions.iter().copied()) {
            return Err(PluginError::PermissionApprovalRequired);
        }
        let mut next = state.preferences.clone();
        next.installed
            .insert(id.to_owned(), manifest.version.clone());
        next.enabled.insert(id.to_owned());
        next.grants.insert(
            id.to_owned(),
            approved_permissions.iter().copied().collect(),
        );
        write_preferences(app, &next)?;
        state.preferences = next;
        Ok(summaries(&state))
    }

    pub fn uninstall(
        &self,
        app: &tauri::AppHandle,
        id: &str,
    ) -> Result<Vec<PluginSummary>, PluginError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| PluginError::StateUnavailable)?;
        let plugin = catalog_plugin(&state.catalog, id)?.clone();
        if state.preferences.installed.get(id) != Some(&plugin.manifest.version) {
            return Err(PluginError::NotInstalled);
        }
        if let Some(installed_path) = &plugin.external_path {
            let root = plugin_storage_root(app)?;
            let removal = QuarantinedPluginPackages::begin(
                &root,
                id,
                &plugin.manifest.version,
                installed_path,
            )
            .map_err(map_removal_error)?;
            let catalog =
                match load_catalog_from_storage(&root, &host_version()?, &self.catalog_service) {
                    Ok(catalog) => catalog,
                    Err(error) => {
                        return Err(rollback_external_removal(removal, error, app, &mut state));
                    }
                };
            let preferences = normalize_preferences(state.preferences.clone(), &catalog);
            if let Err(error) = write_preferences(app, &preferences) {
                return Err(rollback_external_removal(removal, error, app, &mut state));
            }
            *state = PluginRegistryState {
                catalog,
                preferences,
            };
            removal.commit();
            return Ok(summaries(&state));
        }
        let mut next = state.preferences.clone();
        next.installed.remove(id);
        next.enabled.remove(id);
        next.grants.remove(id);
        write_preferences(app, &next)?;
        state.preferences = next;
        Ok(summaries(&state))
    }

    pub fn set_enabled(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        enabled: bool,
    ) -> Result<Vec<PluginSummary>, PluginError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| PluginError::StateUnavailable)?;
        let manifest = catalog_plugin(&state.catalog, id)?.manifest.clone();
        if state.preferences.installed.get(id) != Some(&manifest.version) {
            return Err(PluginError::NotInstalled);
        }
        if enabled
            && !permissions_exactly_match(
                &manifest.permissions,
                state
                    .preferences
                    .grants
                    .get(id)
                    .into_iter()
                    .flatten()
                    .copied(),
            )
        {
            return Err(PluginError::PermissionApprovalRequired);
        }
        let mut next = state.preferences.clone();
        if enabled {
            next.enabled.insert(id.to_owned());
        } else {
            next.enabled.remove(id);
        }
        write_preferences(app, &next)?;
        state.preferences = next;
        Ok(summaries(&state))
    }

    pub fn open_repository(&self, id: &str) -> Result<(), PluginError> {
        let repository = {
            let state = self
                .state
                .read()
                .map_err(|_| PluginError::StateUnavailable)?;
            catalog_plugin(&state.catalog, id)?
                .manifest
                .source
                .repository()
                .to_owned()
        };
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
            .arg(&repository)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| PluginError::Io)
    }
}

fn map_removal_error(error: crate::plugin_package::PluginPackageError) -> PluginError {
    match error {
        crate::plugin_package::PluginPackageError::InvalidRemovalTarget => {
            PluginError::InvalidInstalledPackage
        }
        crate::plugin_package::PluginPackageError::RemovalRollbackFailed => {
            PluginError::ExternalPackageRollbackFailed
        }
        _ => PluginError::ExternalPackageRemovalFailed,
    }
}

fn rollback_external_removal(
    removal: QuarantinedPluginPackages,
    original_error: PluginError,
    app: &tauri::AppHandle,
    state: &mut PluginRegistryState,
) -> PluginError {
    if removal.rollback().is_ok() {
        return original_error;
    }

    let catalog = bundled_catalog()
        .map(bundled_catalog_entries)
        .unwrap_or_default();
    let preferences = normalize_preferences(state.preferences.clone(), &catalog);
    *state = PluginRegistryState {
        catalog,
        preferences: preferences.clone(),
    };
    let _ = write_preferences(app, &preferences);
    PluginError::ExternalPackageRollbackFailed
}

fn bundled_catalog() -> Result<&'static [PluginManifest], PluginError> {
    match BUNDLED_CATALOG.get_or_init(|| load_catalog_from(BUNDLED_MANIFESTS)) {
        Ok(catalog) => Ok(catalog),
        Err(error) => Err(*error),
    }
}

fn host_version() -> Result<Version, PluginError> {
    Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_| PluginError::InvalidManifest)
}

fn load_catalog_from(documents: &[&str]) -> Result<Vec<PluginManifest>, PluginError> {
    let host_version = host_version()?;
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

fn bundled_catalog_entries(manifests: &[PluginManifest]) -> Vec<CatalogPlugin> {
    manifests
        .iter()
        .cloned()
        .map(|manifest| CatalogPlugin {
            manifest,
            external_path: None,
            publisher_key_id: None,
        })
        .collect()
}

fn load_catalog_from_storage(
    root: &Path,
    host_version: &Version,
    catalog_service: &PluginCatalogService,
) -> Result<Vec<CatalogPlugin>, PluginError> {
    let bundled = bundled_catalog()?;
    let packages = discover_installed_packages(root, host_version)
        .map_err(|_| PluginError::InvalidInstalledPackage)?;
    for package in &packages {
        if let Some(authentication) = &package.authentication {
            catalog_service
                .verify_installed(&package.manifest, &package.descriptor, authentication)
                .map_err(|_| PluginError::InvalidInstalledPackage)?;
        }
    }
    merge_catalog(bundled, packages)
}

fn merge_catalog(
    bundled: &[PluginManifest],
    packages: Vec<DiscoveredPluginPackage>,
) -> Result<Vec<CatalogPlugin>, PluginError> {
    let mut catalog = bundled_catalog_entries(bundled);
    let bundled_ids: BTreeSet<_> = bundled.iter().map(|plugin| plugin.id.as_str()).collect();
    let mut newest = BTreeMap::<String, DiscoveredPluginPackage>::new();
    for package in packages {
        if bundled_ids.contains(package.manifest.id.as_str()) {
            return Err(PluginError::DuplicatePlugin);
        }
        match newest.entry(package.manifest.id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(package);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().manifest.version == package.manifest.version {
                    return Err(PluginError::DuplicatePlugin);
                }
                if entry.get().manifest.version < package.manifest.version {
                    entry.insert(package);
                }
            }
        }
    }
    catalog.extend(newest.into_values().map(|package| {
        CatalogPlugin {
            manifest: package.manifest,
            external_path: Some(package.path),
            publisher_key_id: package
                .authentication
                .map(|authentication| authentication.key_id),
        }
    }));
    Ok(catalog)
}

fn catalog_plugin<'a>(
    catalog: &'a [CatalogPlugin],
    id: &str,
) -> Result<&'a CatalogPlugin, PluginError> {
    catalog
        .iter()
        .find(|plugin| plugin.manifest.id == id)
        .ok_or(PluginError::UnknownPlugin)
}

fn external_runtime_spec(
    state: &PluginRegistryState,
    id: &str,
    require_enabled: bool,
) -> Result<Option<ExternalRuntimeSpec>, PluginError> {
    let plugin = catalog_plugin(&state.catalog, id)?;
    if state.preferences.installed.get(id) != Some(&plugin.manifest.version) {
        return Err(PluginError::NotInstalled);
    }
    if require_enabled && !state.preferences.enabled.contains(id) {
        return Err(PluginError::PluginDisabled);
    }
    let granted = state
        .preferences
        .grants
        .get(id)
        .cloned()
        .unwrap_or_default();
    if !permissions_exactly_match(&plugin.manifest.permissions, granted.iter().copied()) {
        return Err(PluginError::PermissionApprovalRequired);
    }
    let Some(package_path) = &plugin.external_path else {
        return Ok(None);
    };
    let crate::plugin_manifest::PluginRuntime::Process { entrypoint, .. } =
        &plugin.manifest.runtime
    else {
        return Err(PluginError::ExternalRuntimeUnsupported);
    };
    Ok(Some(ExternalRuntimeSpec {
        id: plugin.manifest.id.clone(),
        version: plugin.manifest.version.clone(),
        package_path: package_path.clone(),
        entrypoint: entrypoint.clone(),
        capabilities: plugin.manifest.capabilities.iter().copied().collect(),
        permissions: granted,
    }))
}

fn task_provider(state: &PluginRegistryState, id: &str) -> Result<TaskProvider, PluginError> {
    let plugin = catalog_plugin(&state.catalog, id)?;
    if plugin.external_path.is_none()
        || !plugin
            .manifest
            .capabilities
            .contains(&PluginCapability::Tasks)
    {
        return Err(PluginError::CapabilityUnavailable);
    }
    if state.preferences.installed.get(id) != Some(&plugin.manifest.version) {
        return Err(PluginError::NotInstalled);
    }
    if !state.preferences.enabled.contains(id) {
        return Err(PluginError::PluginDisabled);
    }
    let permissions = state
        .preferences
        .grants
        .get(id)
        .cloned()
        .unwrap_or_default();
    if !permissions_exactly_match(&plugin.manifest.permissions, permissions.iter().copied()) {
        return Err(PluginError::PermissionApprovalRequired);
    }
    Ok(TaskProvider {
        id: plugin.manifest.id.clone(),
        name: plugin.manifest.name.clone(),
        permissions,
    })
}

fn summaries(state: &PluginRegistryState) -> Vec<PluginSummary> {
    state
        .catalog
        .iter()
        .map(|plugin| {
            let manifest = &plugin.manifest;
            let granted_permissions: Vec<_> = state
                .preferences
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
            let installed =
                state.preferences.installed.get(&manifest.id) == Some(&manifest.version);
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
                installed,
                enabled: installed
                    && state.preferences.enabled.contains(&manifest.id)
                    && permissions_approved,
                official: manifest.publisher == "w3ti" && manifest.source.is_bundled(),
                bundled: manifest.source.is_bundled(),
                repository: manifest.source.repository().to_owned(),
                capabilities: manifest.capabilities.clone(),
                permissions: manifest.permissions.clone(),
                granted_permissions,
                requires_permission_review: installed && !permissions_approved,
                publisher_verified: plugin.publisher_key_id.is_some(),
                publisher_key_id: plugin.publisher_key_id.clone(),
            }
        })
        .collect()
}

fn normalize_preferences(
    mut preferences: PluginPreferences,
    catalog: &[CatalogPlugin],
) -> PluginPreferences {
    preferences.version = PLUGIN_STATE_VERSION;
    let known: BTreeMap<_, _> = catalog
        .iter()
        .map(|plugin| (plugin.manifest.id.clone(), plugin.manifest.version.clone()))
        .collect();
    let installed_ids: Vec<_> = preferences.installed.keys().cloned().collect();
    for id in installed_ids {
        match known.get(&id) {
            None => {
                preferences.installed.remove(&id);
                preferences.enabled.remove(&id);
                preferences.grants.remove(&id);
            }
            Some(version) if preferences.installed.get(&id) != Some(version) => {
                preferences.installed.insert(id.clone(), version.clone());
                preferences.enabled.remove(&id);
                preferences.grants.remove(&id);
            }
            Some(_) => {}
        }
    }
    for plugin in catalog
        .iter()
        .filter(|plugin| plugin.external_path.is_some())
    {
        if !preferences.installed.contains_key(&plugin.manifest.id) {
            preferences
                .installed
                .insert(plugin.manifest.id.clone(), plugin.manifest.version.clone());
            preferences.enabled.remove(&plugin.manifest.id);
            preferences.grants.remove(&plugin.manifest.id);
        }
    }
    preferences
        .grants
        .retain(|id, _| preferences.installed.contains_key(id));
    for (id, grants) in &mut preferences.grants {
        if let Some(plugin) = catalog.iter().find(|plugin| &plugin.manifest.id == id) {
            grants.retain(|permission| plugin.manifest.permissions.contains(permission));
        }
    }
    preferences.enabled.retain(|id| {
        preferences.installed.contains_key(id)
            && catalog
                .iter()
                .find(|plugin| &plugin.manifest.id == id)
                .is_some_and(|plugin| {
                    permissions_exactly_match(
                        &plugin.manifest.permissions,
                        preferences.grants.get(id).into_iter().flatten().copied(),
                    )
                })
    });
    preferences
}

fn record_external_install(
    mut preferences: PluginPreferences,
    catalog: &[CatalogPlugin],
    id: &str,
    version: &Version,
    approved_permissions: &[PluginPermission],
) -> Result<PluginPreferences, PluginError> {
    let plugin = catalog_plugin(catalog, id)?;
    if plugin.external_path.is_none() || &plugin.manifest.version != version {
        return Err(PluginError::InvalidInstalledPackage);
    }
    if !permissions_exactly_match(
        &plugin.manifest.permissions,
        approved_permissions.iter().copied(),
    ) {
        return Err(PluginError::PermissionApprovalRequired);
    }
    preferences.installed.insert(id.to_owned(), version.clone());
    preferences.enabled.remove(id);
    preferences.grants.insert(
        id.to_owned(),
        approved_permissions.iter().copied().collect(),
    );
    Ok(preferences)
}

pub(crate) fn plugin_storage_root(app: &tauri::AppHandle) -> Result<PathBuf, PluginError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("plugins"))
        .map_err(|_| PluginError::Io)
}

fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, PluginError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("plugins.json"))
        .map_err(|_| PluginError::Io)
}

fn read_preferences(
    app: &tauri::AppHandle,
    catalog: &[CatalogPlugin],
) -> Option<PluginPreferences> {
    let path = preferences_path(app).ok()?;
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLUGIN_STATE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    parse_preferences(&bytes, catalog)
}

fn parse_preferences(bytes: &[u8], catalog: &[CatalogPlugin]) -> Option<PluginPreferences> {
    if let Ok(preferences) = serde_json::from_slice::<PluginPreferences>(bytes)
        && preferences.version == PLUGIN_STATE_VERSION
    {
        return Some(preferences);
    }
    let legacy: LegacyPluginPreferences = serde_json::from_slice(bytes).ok()?;
    if legacy.version != 3 {
        return None;
    }
    let installed = legacy
        .installed
        .into_iter()
        .filter_map(|id| {
            catalog
                .iter()
                .find(|plugin| plugin.manifest.id == id)
                .map(|plugin| (id, plugin.manifest.version.clone()))
        })
        .collect();
    Some(PluginPreferences {
        version: PLUGIN_STATE_VERSION,
        installed,
        enabled: legacy.enabled,
        grants: legacy.grants,
    })
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

    fn bundled_entries() -> Vec<CatalogPlugin> {
        bundled_catalog_entries(&load_catalog_from(BUNDLED_MANIFESTS).unwrap())
    }

    fn external_package(version: &str, permissions: &str) -> DiscoveredPluginPackage {
        let document = format!(
            r#"{{
              "schemaVersion": 1,
              "id": "io.github.example.lyrnova.tool.external",
              "name": "External",
              "description": "External plugin used by registry tests.",
              "version": "{version}",
              "publisher": "example",
              "license": "GPL-3.0-only",
              "kind": "tool",
              "compatibility": {{ "lyrnova": ">=0.1.0, <0.2.0", "pluginApi": 1 }},
              "runtime": {{ "type": "process", "entrypoint": "bin/external", "protocolVersion": 1 }},
              "source": {{
                "type": "github_release",
                "repository": "https://github.com/example/lyrnova-external",
                "asset": "external.tar.zst"
              }},
              "capabilities": ["tasks"],
              "permissions": {permissions}
            }}"#
        );
        DiscoveredPluginPackage {
            manifest: parse_manifest(&document, &Version::new(0, 1, 0), ManifestOrigin::External)
                .unwrap(),
            descriptor: crate::plugin_package::PluginPackageDescriptor {
                asset: "external.tar.zst".into(),
                sha256: "a".repeat(64),
            },
            authentication: None,
            path: PathBuf::from(format!("/plugins/external/{version}")),
        }
    }

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
        let catalog = bundled_entries();
        let preferences = PluginPreferences::defaults(&catalog);
        assert!(
            preferences
                .installed
                .contains_key("io.github.w3ti.lyrnova.language.rust")
        );
        assert!(
            preferences
                .installed
                .contains_key("io.github.w3ti.lyrnova.language.web")
        );
        assert!(!preferences.installed.contains_key(CODEX_PLUGIN_ID));
        assert!(!preferences.enabled.contains(CODEX_PLUGIN_ID));
        assert!(!preferences.grants.contains_key(CODEX_PLUGIN_ID));
    }

    #[test]
    fn ai_provider_resolution_uses_kind_capabilities_and_grants_instead_of_a_fixed_id() {
        let mut provider = bundled_entries()
            .into_iter()
            .find(|plugin| plugin.manifest.kind == PluginKind::AiProvider)
            .unwrap();
        provider.manifest.id = "io.github.example.lyrnova.ai.provider".into();
        provider.manifest.name = "Example AI".into();
        let mut preferences = PluginPreferences::fail_closed();
        preferences.installed.insert(
            provider.manifest.id.clone(),
            provider.manifest.version.clone(),
        );
        preferences.enabled.insert(provider.manifest.id.clone());
        preferences.grants.insert(
            provider.manifest.id.clone(),
            provider.manifest.permissions.iter().copied().collect(),
        );
        let registry = PluginRegistry {
            state: RwLock::new(PluginRegistryState {
                catalog: vec![provider],
                preferences,
            }),
            catalog_service: PluginCatalogService::default(),
        };

        let active = registry
            .active_ai_provider(
                &[PluginCapability::AccountAuth, PluginCapability::AiChat],
                &[
                    PluginPermission::ProcessSpawn,
                    PluginPermission::NetworkAccess,
                ],
            )
            .unwrap();
        assert_eq!(active.id, "io.github.example.lyrnova.ai.provider");
        assert_eq!(active.name, "Example AI");
        assert_eq!(
            registry.active_ai_provider(&[PluginCapability::Diagnostics], &[]),
            Err(PluginError::CapabilityUnavailable)
        );
    }

    #[test]
    fn a_default_registry_exposes_no_active_ai_provider() {
        let registry = PluginRegistry::default();
        assert_eq!(
            registry.active_ai_provider(&[PluginCapability::AiChat], &[]),
            Err(PluginError::NoAiProvider)
        );
    }

    #[test]
    fn multiple_active_ai_providers_fail_closed() {
        let first = bundled_entries()
            .into_iter()
            .find(|plugin| plugin.manifest.kind == PluginKind::AiProvider)
            .unwrap();
        let mut second = first.clone();
        second.manifest.id = "io.github.example.lyrnova.ai.second".into();
        second.manifest.name = "Second AI".into();
        let mut preferences = PluginPreferences::fail_closed();
        for provider in [&first, &second] {
            preferences.installed.insert(
                provider.manifest.id.clone(),
                provider.manifest.version.clone(),
            );
            preferences.enabled.insert(provider.manifest.id.clone());
            preferences.grants.insert(
                provider.manifest.id.clone(),
                provider.manifest.permissions.iter().copied().collect(),
            );
        }
        let registry = PluginRegistry {
            state: RwLock::new(PluginRegistryState {
                catalog: vec![first, second],
                preferences,
            }),
            catalog_service: PluginCatalogService::default(),
        };

        assert_eq!(
            registry.active_ai_provider(&[PluginCapability::AiChat], &[]),
            Err(PluginError::MultipleAiProviders)
        );
        assert_eq!(
            registry.active_ai_provider(&[], &[]),
            Err(PluginError::MultipleAiProviders)
        );
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
        let catalog = bundled_entries();
        let codex_version = catalog
            .iter()
            .find(|plugin| plugin.manifest.id == CODEX_PLUGIN_ID)
            .unwrap()
            .manifest
            .version
            .clone();
        let mut preferences = PluginPreferences::fail_closed();
        preferences
            .installed
            .insert(CODEX_PLUGIN_ID.into(), codex_version);
        preferences.enabled.insert(CODEX_PLUGIN_ID.into());
        preferences.grants.insert(
            CODEX_PLUGIN_ID.into(),
            [PluginPermission::WorkspaceRead].into_iter().collect(),
        );

        let normalized = normalize_preferences(preferences, &catalog);
        assert!(normalized.installed.contains_key(CODEX_PLUGIN_ID));
        assert!(!normalized.enabled.contains(CODEX_PLUGIN_ID));
    }

    #[test]
    fn authorization_requires_install_enable_declaration_and_grant() {
        let catalog = bundled_entries();
        let codex = catalog
            .iter()
            .find(|plugin| plugin.manifest.id == CODEX_PLUGIN_ID)
            .unwrap();
        let mut preferences = PluginPreferences::fail_closed();
        preferences
            .installed
            .insert(CODEX_PLUGIN_ID.into(), codex.manifest.version.clone());
        preferences.enabled.insert(CODEX_PLUGIN_ID.into());
        preferences.grants.insert(
            CODEX_PLUGIN_ID.into(),
            codex.manifest.permissions.iter().copied().collect(),
        );
        let registry = PluginRegistry {
            state: RwLock::new(PluginRegistryState {
                catalog,
                preferences,
            }),
            catalog_service: PluginCatalogService::default(),
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

    #[test]
    fn external_packages_are_installed_but_disabled_without_grants() {
        let catalog = merge_catalog(
            &load_catalog_from(BUNDLED_MANIFESTS).unwrap(),
            vec![external_package(
                "0.1.0",
                r#"["workspace_read", "process_spawn"]"#,
            )],
        )
        .unwrap();
        let preferences = normalize_preferences(PluginPreferences::defaults(&catalog), &catalog);
        let external = "io.github.example.lyrnova.tool.external";

        assert_eq!(
            preferences.installed.get(external),
            Some(&Version::new(0, 1, 0))
        );
        assert!(!preferences.enabled.contains(external));
        assert!(!preferences.grants.contains_key(external));

        let summaries = summaries(&PluginRegistryState {
            catalog,
            preferences,
        });
        let summary = summaries
            .iter()
            .find(|plugin| plugin.id == external)
            .unwrap();
        assert!(summary.installed);
        assert!(!summary.enabled);
        assert!(!summary.bundled);
        assert!(summary.requires_permission_review);
    }

    #[test]
    fn runtime_specs_use_only_exact_persisted_grants() {
        let catalog = merge_catalog(
            &load_catalog_from(BUNDLED_MANIFESTS).unwrap(),
            vec![external_package(
                "0.1.0",
                r#"["workspace_read", "process_spawn"]"#,
            )],
        )
        .unwrap();
        let external = "io.github.example.lyrnova.tool.external";
        let mut preferences =
            normalize_preferences(PluginPreferences::defaults(&catalog), &catalog);
        preferences.grants.insert(
            external.into(),
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ]
            .into_iter()
            .collect(),
        );
        let state = PluginRegistryState {
            catalog,
            preferences,
        };

        let spec = external_runtime_spec(&state, external, false)
            .unwrap()
            .unwrap();
        assert_eq!(spec.id, external);
        assert_eq!(spec.entrypoint, "bin/external");
        assert_eq!(spec.permissions.len(), 2);
        assert_eq!(
            external_runtime_spec(&state, external, true),
            Err(PluginError::PluginDisabled)
        );

        let mut invalid_state = state;
        invalid_state
            .preferences
            .grants
            .get_mut(external)
            .unwrap()
            .remove(&PluginPermission::ProcessSpawn);
        assert_eq!(
            external_runtime_spec(&invalid_state, external, false),
            Err(PluginError::PermissionApprovalRequired)
        );
    }

    #[test]
    fn task_providers_require_an_enabled_external_capability_and_exact_grants() {
        let catalog = merge_catalog(
            &load_catalog_from(BUNDLED_MANIFESTS).unwrap(),
            vec![external_package(
                "0.1.0",
                r#"["workspace_read", "process_spawn"]"#,
            )],
        )
        .unwrap();
        let external = "io.github.example.lyrnova.tool.external";
        let mut preferences =
            normalize_preferences(PluginPreferences::defaults(&catalog), &catalog);
        preferences.enabled.insert(external.into());
        preferences.grants.insert(
            external.into(),
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ]
            .into_iter()
            .collect(),
        );
        let mut state = PluginRegistryState {
            catalog,
            preferences,
        };

        let provider = task_provider(&state, external).unwrap();
        assert_eq!(provider.id, external);
        assert_eq!(provider.name, "External");
        assert_eq!(provider.permissions.len(), 2);

        state.preferences.enabled.remove(external);
        assert_eq!(
            task_provider(&state, external),
            Err(PluginError::PluginDisabled)
        );
        assert_eq!(
            task_provider(&state, "io.github.w3ti.lyrnova.language.rust"),
            Err(PluginError::CapabilityUnavailable)
        );
    }

    #[test]
    fn registering_an_external_install_persists_grants_but_not_enablement() {
        let catalog = merge_catalog(
            &load_catalog_from(BUNDLED_MANIFESTS).unwrap(),
            vec![external_package(
                "0.1.0",
                r#"["workspace_read", "process_spawn"]"#,
            )],
        )
        .unwrap();
        let external = "io.github.example.lyrnova.tool.external";
        let preferences = record_external_install(
            normalize_preferences(PluginPreferences::defaults(&catalog), &catalog),
            &catalog,
            external,
            &Version::new(0, 1, 0),
            &[
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ],
        )
        .unwrap();

        assert!(permissions_exactly_match(
            &[
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ],
            preferences
                .grants
                .get(external)
                .into_iter()
                .flatten()
                .copied(),
        ));
        assert!(!preferences.enabled.contains(external));
    }

    #[test]
    fn newest_external_version_wins_and_requires_a_new_review() {
        let bundled = load_catalog_from(BUNDLED_MANIFESTS).unwrap();
        let old = external_package("0.1.0", r#"["workspace_read", "process_spawn"]"#);
        let external = old.manifest.id.clone();
        let old_catalog = merge_catalog(&bundled, vec![old]).unwrap();
        let mut preferences =
            normalize_preferences(PluginPreferences::defaults(&old_catalog), &old_catalog);
        preferences.enabled.insert(external.clone());
        preferences.grants.insert(
            external.clone(),
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ]
            .into_iter()
            .collect(),
        );

        let updated_catalog = merge_catalog(
            &bundled,
            vec![
                external_package("0.1.0", r#"["workspace_read", "process_spawn"]"#),
                external_package(
                    "0.2.0",
                    r#"["workspace_read", "workspace_write", "process_spawn"]"#,
                ),
            ],
        )
        .unwrap();
        let normalized = normalize_preferences(preferences, &updated_catalog);

        assert_eq!(
            normalized.installed.get(&external),
            Some(&Version::new(0, 2, 0))
        );
        assert!(!normalized.enabled.contains(&external));
        assert!(!normalized.grants.contains_key(&external));
    }

    #[test]
    fn external_package_cannot_shadow_a_bundled_plugin() {
        let bundled = load_catalog_from(BUNDLED_MANIFESTS).unwrap();
        let mut external = external_package("0.1.0", r#"["process_spawn"]"#);
        external.manifest = bundled[0].clone();
        assert!(matches!(
            merge_catalog(&bundled, vec![external]),
            Err(PluginError::DuplicatePlugin)
        ));
    }

    #[test]
    fn removing_external_catalog_entries_clears_their_authority() {
        let bundled = load_catalog_from(BUNDLED_MANIFESTS).unwrap();
        let external_catalog = merge_catalog(
            &bundled,
            vec![external_package(
                "0.1.0",
                r#"["workspace_read", "process_spawn"]"#,
            )],
        )
        .unwrap();
        let external = "io.github.example.lyrnova.tool.external";
        let mut preferences = normalize_preferences(
            PluginPreferences::defaults(&external_catalog),
            &external_catalog,
        );
        preferences.enabled.insert(external.into());
        preferences.grants.insert(
            external.into(),
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ]
            .into_iter()
            .collect(),
        );

        let normalized = normalize_preferences(preferences, &bundled_catalog_entries(&bundled));
        assert!(!normalized.installed.contains_key(external));
        assert!(!normalized.enabled.contains(external));
        assert!(!normalized.grants.contains_key(external));
    }

    #[test]
    fn version_three_preferences_migrate_with_catalog_versions() {
        let catalog = bundled_entries();
        let legacy = br#"{
          "version": 3,
          "installed": ["io.github.w3ti.lyrnova.language.rust"],
          "enabled": ["io.github.w3ti.lyrnova.language.rust"],
          "grants": {
            "io.github.w3ti.lyrnova.language.rust": ["workspace_read"]
          }
        }"#;
        let migrated = parse_preferences(legacy, &catalog).unwrap();

        assert_eq!(migrated.version, PLUGIN_STATE_VERSION);
        assert_eq!(
            migrated
                .installed
                .get("io.github.w3ti.lyrnova.language.rust"),
            Some(&Version::new(0, 1, 0))
        );
        assert!(
            migrated
                .enabled
                .contains("io.github.w3ti.lyrnova.language.rust")
        );
    }
}
