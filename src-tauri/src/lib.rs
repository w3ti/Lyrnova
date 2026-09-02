pub mod ai_provider;
pub mod app_server;
pub mod backend;
pub mod git;
pub mod plugin_catalog;
pub mod plugin_manifest;
pub mod plugin_package;
pub mod plugin_protocol;
pub mod plugin_runtime;
pub mod plugin_trust;
pub mod plugins;
pub mod process_broker;
pub mod protocol;
pub mod tasks;
pub mod terminal;
pub mod workspace;

use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};
use std::{fs, process::Command};

use ai_provider::{current_ai_provider, resolve_ai_provider};
use git::{GitError, GitService, GitStatusSummary};
use plugin_catalog::{
    PluginCatalogError, PluginCatalogService, TrustedPluginSummary, download_release,
};
use plugin_manifest::permissions_exactly_match;
use plugin_manifest::{PluginCapability, PluginPermission};
use plugin_package::{
    PluginPackageDescriptor, PluginPackageError, PluginPackageInstaller, PluginPackageReview,
    StagedPluginPackage,
};
use plugin_runtime::{PluginRuntimeError, PluginRuntimeService};
use plugins::{AiProviderSummary, PluginError, PluginRegistry, PluginSummary, plugin_storage_root};
use process_broker::{ProcessOutputEvent, ProcessResult};
use serde::{Deserialize, Serialize};
use tasks::{TaskBroker, TaskError, TaskList, TaskReview};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use terminal::{TerminalError, TerminalService};
use workspace::{
    ApplyDocumentPatchRequest, CreateDocumentRequest, DeleteWorkspaceEntryRequest,
    DeletedWorkspaceEntry, DocumentPatchPreview, DocumentRangeSnapshot, DocumentSnapshot,
    MoveWorkspaceEntryRequest, ReadDocumentRangeRequest, SaveDocumentRequest, WorkspaceEntry,
    WorkspaceError, WorkspaceMetadata, WorkspaceRecoveryService, WorkspaceSearchMatch,
    WorkspaceService,
};

#[derive(Clone)]
struct ActiveProject {
    workspace: WorkspaceService,
    git: Option<GitService>,
}

struct ProjectState(RwLock<Option<ActiveProject>>);
struct LoginState(Arc<AtomicBool>);

#[derive(Default)]
struct PluginLifecycleState {
    pending: Mutex<Option<PendingPluginInstall>>,
    mutation: Mutex<()>,
    runtimes: PluginRuntimeService,
    tasks: TaskBroker,
}

struct PendingPluginInstall {
    token: String,
    staged: StagedPluginPackage,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginInstallReview {
    token: String,
    review: PluginPackageReview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "domain", content = "error", rename_all = "snake_case")]
enum PluginInstallFlowError {
    Catalog(PluginCatalogError),
    Package(PluginPackageError),
    Registry(PluginError),
    UnknownSession,
    StateUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "domain", content = "error", rename_all = "snake_case")]
enum TaskFlowError {
    Plugin(PluginError),
    Runtime(PluginRuntimeError),
    Task(TaskError),
    NoWorkspace,
}

impl From<PluginError> for TaskFlowError {
    fn from(error: PluginError) -> Self {
        Self::Plugin(error)
    }
}

impl From<PluginRuntimeError> for TaskFlowError {
    fn from(error: PluginRuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<TaskError> for TaskFlowError {
    fn from(error: TaskError) -> Self {
        Self::Task(error)
    }
}

impl From<PluginPackageError> for PluginInstallFlowError {
    fn from(error: PluginPackageError) -> Self {
        Self::Package(error)
    }
}

impl From<PluginCatalogError> for PluginInstallFlowError {
    fn from(error: PluginCatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl From<PluginError> for PluginInstallFlowError {
    fn from(error: PluginError) -> Self {
        Self::Registry(error)
    }
}

const PROJECT_HISTORY_VERSION: u32 = 1;
const MAX_RECENT_PROJECTS: usize = 10;
const MAX_PROJECT_HISTORY_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct ProjectHistory {
    version: u32,
    recent: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSummary {
    name: String,
    path: String,
    has_git: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceMutationEvent {
    event_id: String,
    initiator: &'static str,
    operation: &'static str,
    path: String,
    destination: Option<String>,
    before_revision: Option<String>,
    after_revision: Option<String>,
}

fn emit_workspace_mutation(
    window: &tauri::WebviewWindow,
    operation: &'static str,
    path: String,
    destination: Option<String>,
    before_revision: Option<String>,
    after_revision: Option<String>,
) {
    let _ = window.emit(
        "workspace-mutated",
        WorkspaceMutationEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            initiator: "local_user",
            operation,
            path,
            destination,
            before_revision,
            after_revision,
        },
    );
}

fn workspace_recovery_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, WorkspaceError> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join("workspace-recovery"))
        .map_err(|_| WorkspaceError::RecoveryUnavailable)
}

fn project_snapshot(state: &tauri::State<'_, ProjectState>) -> Option<ActiveProject> {
    state.0.read().ok()?.clone()
}

fn project_summary(project: &ActiveProject) -> ProjectSummary {
    let root = project.workspace.root();
    ProjectSummary {
        name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Projeto")
            .to_owned(),
        path: root.to_string_lossy().into_owned(),
        has_git: project.git.is_some(),
    }
}

fn remember_project_path(history: &mut ProjectHistory, path: &std::path::Path) {
    let path = path.to_string_lossy().into_owned();
    history.recent.retain(|existing| existing != &path);
    history.recent.insert(0, path);
    history.recent.truncate(MAX_RECENT_PROJECTS);
    history.version = PROJECT_HISTORY_VERSION;
}

fn project_history_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, WorkspaceError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("projects.json"))
        .map_err(|_| WorkspaceError::Io)
}

fn read_project_history(app: &tauri::AppHandle) -> ProjectHistory {
    let Ok(path) = project_history_path(app) else {
        return ProjectHistory::default();
    };
    let Ok(metadata) = fs::metadata(&path) else {
        return ProjectHistory::default();
    };
    if !metadata.is_file() || metadata.len() > MAX_PROJECT_HISTORY_BYTES {
        return ProjectHistory::default();
    }
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ProjectHistory>(&bytes).ok())
        .filter(|history| history.version == PROJECT_HISTORY_VERSION)
        .unwrap_or_default()
}

fn remember_project(app: &tauri::AppHandle, root: &std::path::Path) {
    let Ok(path) = project_history_path(app) else {
        return;
    };
    let mut history = read_project_history(app);
    remember_project_path(&mut history, root);
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let Ok(bytes) = serde_json::to_vec_pretty(&history) else {
        return;
    };
    if fs::write(&temporary, bytes).is_ok() && fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
    }
}

fn load_last_project(app: &tauri::AppHandle) -> Option<WorkspaceService> {
    read_project_history(app)
        .recent
        .into_iter()
        .find_map(|path| WorkspaceService::new(path).ok())
}

#[tauri::command]
fn project_current(
    state: tauri::State<'_, ProjectState>,
) -> Result<ProjectSummary, WorkspaceError> {
    let project = project_snapshot(&state).ok_or(WorkspaceError::NoWorkspace)?;
    Ok(project_summary(&project))
}

#[tauri::command]
fn project_open_dialog(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProjectState>,
    terminal: tauri::State<'_, TerminalService>,
    registry: tauri::State<'_, PluginRegistry>,
    plugins: tauri::State<'_, PluginLifecycleState>,
) -> Result<Option<ProjectSummary>, WorkspaceError> {
    let Some(selection) = app
        .dialog()
        .file()
        .set_title("Abrir projeto no Lyrnova")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let root = selection
        .into_path()
        .map_err(|_| WorkspaceError::InvalidPath)?;
    let workspace = WorkspaceService::new(&root)?;
    let project = ActiveProject {
        git: GitService::new(workspace.root()).ok(),
        workspace,
    };
    let _mutation = plugins.mutation.lock().map_err(|_| WorkspaceError::Io)?;
    plugins
        .tasks
        .invalidate_all()
        .map_err(|_| WorkspaceError::Io)?;
    terminal.stop().map_err(|_| WorkspaceError::Io)?;
    plugins
        .runtimes
        .stop_all()
        .map_err(|_| WorkspaceError::Io)?;
    app.state::<ApprovalBroker>().clear_session();
    let summary = project_summary(&project);
    remember_project(&app, project.workspace.root());
    *state.0.write().map_err(|_| WorkspaceError::Io)? = Some(project);
    start_enabled_external_runtimes(&app, &registry, &plugins.runtimes, Some(&root));
    Ok(Some(summary))
}

fn validated_project_name(value: &str) -> Result<&str, WorkspaceError> {
    let name = value.trim();
    let windows_device_name = name
        .split('.')
        .next()
        .map(str::to_ascii_uppercase)
        .is_some_and(|stem| {
            matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || stem
                    .strip_prefix("COM")
                    .or_else(|| stem.strip_prefix("LPT"))
                    .is_some_and(|suffix| {
                        matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                    })
        });
    let valid = !name.is_empty()
        && name.chars().count() <= 64
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && !windows_device_name
        && !name.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        });
    valid
        .then_some(name)
        .ok_or(WorkspaceError::InvalidProjectName)
}

#[tauri::command]
fn project_create_dialog(
    name: String,
    initialize_git: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, ProjectState>,
    terminal: tauri::State<'_, TerminalService>,
    registry: tauri::State<'_, PluginRegistry>,
    plugins: tauri::State<'_, PluginLifecycleState>,
) -> Result<Option<ProjectSummary>, WorkspaceError> {
    let name = validated_project_name(&name)?;
    let Some(selection) = app
        .dialog()
        .file()
        .set_title("Escolher pasta para o novo projeto")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let parent = selection
        .into_path()
        .map_err(|_| WorkspaceError::InvalidPath)?;
    let root = parent.join(name);
    if root.exists() {
        return Err(WorkspaceError::ProjectAlreadyExists);
    }
    fs::create_dir(&root).map_err(|_| WorkspaceError::Io)?;
    let readme = format!("# {name}\n\nProjeto criado com o Lyrnova.\n");
    if fs::write(root.join("README.md"), readme).is_err()
        || fs::write(root.join(".gitignore"), "target/\nnode_modules/\ndist/\n").is_err()
    {
        let _ = fs::remove_dir_all(&root);
        return Err(WorkspaceError::Io);
    }
    if initialize_git {
        let _ = Command::new("git")
            .args(["init", "--quiet", "--"])
            .arg(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let workspace = WorkspaceService::new(&root)?;
    let project = ActiveProject {
        git: GitService::new(workspace.root()).ok(),
        workspace,
    };
    let _mutation = plugins.mutation.lock().map_err(|_| WorkspaceError::Io)?;
    plugins
        .tasks
        .invalidate_all()
        .map_err(|_| WorkspaceError::Io)?;
    terminal.stop().map_err(|_| WorkspaceError::Io)?;
    plugins
        .runtimes
        .stop_all()
        .map_err(|_| WorkspaceError::Io)?;
    app.state::<ApprovalBroker>().clear_session();
    let summary = project_summary(&project);
    remember_project(&app, project.workspace.root());
    *state.0.write().map_err(|_| WorkspaceError::Io)? = Some(project);
    start_enabled_external_runtimes(&app, &registry, &plugins.runtimes, Some(&root));
    Ok(Some(summary))
}

#[tauri::command]
fn workspace_list(
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
    project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .list()
}

#[tauri::command]
fn workspace_read(
    path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<DocumentSnapshot, WorkspaceError> {
    project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .read(&path)
}

#[tauri::command]
fn workspace_read_range(
    request: ReadDocumentRangeRequest,
    state: tauri::State<'_, ProjectState>,
) -> Result<DocumentRangeSnapshot, WorkspaceError> {
    project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .read_range(request)
}

#[tauri::command]
fn workspace_metadata(
    path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<WorkspaceMetadata, WorkspaceError> {
    project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .metadata(&path)
}

#[tauri::command]
fn workspace_search(
    query: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<WorkspaceSearchMatch>, WorkspaceError> {
    project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .search(&query)
}

#[tauri::command]
fn workspace_save(
    request: SaveDocumentRequest,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
) -> Result<DocumentSnapshot, WorkspaceError> {
    let before_revision = request.expected_revision.clone();
    let path = request.path.clone();
    let snapshot = project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .save(request)?;
    emit_workspace_mutation(
        &window,
        "save",
        path,
        None,
        Some(before_revision),
        Some(snapshot.revision.clone()),
    );
    Ok(snapshot)
}

#[tauri::command]
fn workspace_create_document(
    request: CreateDocumentRequest,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
) -> Result<DocumentSnapshot, WorkspaceError> {
    let path = request.path.clone();
    let snapshot = project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .create_document(request)?;
    emit_workspace_mutation(
        &window,
        "create_file",
        path,
        None,
        None,
        Some(snapshot.revision.clone()),
    );
    Ok(snapshot)
}

#[tauri::command]
fn workspace_create_directory(
    path: String,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
) -> Result<WorkspaceEntry, WorkspaceError> {
    let entry = project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .create_directory(&path)?;
    emit_workspace_mutation(&window, "create_directory", path, None, None, None);
    Ok(entry)
}

#[tauri::command]
fn workspace_move(
    request: MoveWorkspaceEntryRequest,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
) -> Result<WorkspaceEntry, WorkspaceError> {
    let source = request.source.clone();
    let destination = request.destination.clone();
    let entry = project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .move_entry(request)?;
    emit_workspace_mutation(&window, "move", source, Some(destination), None, None);
    Ok(entry)
}

#[tauri::command]
fn workspace_apply_patch(
    request: ApplyDocumentPatchRequest,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
) -> Result<DocumentSnapshot, WorkspaceError> {
    let path = request.path.clone();
    let before_revision = request.expected_revision.clone();
    let snapshot = project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .apply_patch(request)?;
    emit_workspace_mutation(
        &window,
        "apply_patch",
        path,
        None,
        Some(before_revision),
        Some(snapshot.revision.clone()),
    );
    Ok(snapshot)
}

#[tauri::command]
fn workspace_preview_patch(
    request: ApplyDocumentPatchRequest,
    state: tauri::State<'_, ProjectState>,
) -> Result<DocumentPatchPreview, WorkspaceError> {
    project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .preview_patch(&request)
}

#[tauri::command]
fn workspace_delete(
    request: DeleteWorkspaceEntryRequest,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
    recovery: tauri::State<'_, WorkspaceRecoveryService>,
) -> Result<DeletedWorkspaceEntry, WorkspaceError> {
    let project = project_snapshot(&state).ok_or(WorkspaceError::NoWorkspace)?;
    let path = request.path.clone();
    let before_revision = request.expected_revision.clone();
    let deleted = recovery.delete(&project.workspace, &workspace_recovery_root(&app)?, request)?;
    emit_workspace_mutation(&window, "delete", path, None, before_revision, None);
    Ok(deleted)
}

#[tauri::command]
fn workspace_restore(
    recovery_token: String,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
    recovery: tauri::State<'_, WorkspaceRecoveryService>,
) -> Result<WorkspaceEntry, WorkspaceError> {
    let project = project_snapshot(&state).ok_or(WorkspaceError::NoWorkspace)?;
    let entry = recovery.restore(&project.workspace, &recovery_token)?;
    emit_workspace_mutation(&window, "restore", entry.path.clone(), None, None, None);
    Ok(entry)
}

#[tauri::command]
fn git_status(state: tauri::State<'_, ProjectState>) -> Result<GitStatusSummary, GitError> {
    project_snapshot(&state)
        .and_then(|project| project.git)
        .ok_or(GitError::NoWorkspace)?
        .status()
}

#[tauri::command]
fn git_stage(
    path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<GitStatusSummary, GitError> {
    project_snapshot(&state)
        .and_then(|project| project.git)
        .ok_or(GitError::NoWorkspace)?
        .stage(&path)
}

#[tauri::command]
fn git_unstage(
    path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<GitStatusSummary, GitError> {
    project_snapshot(&state)
        .and_then(|project| project.git)
        .ok_or(GitError::NoWorkspace)?
        .unstage(&path)
}

#[tauri::command]
fn git_commit(
    message: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<GitStatusSummary, GitError> {
    project_snapshot(&state)
        .and_then(|project| project.git)
        .ok_or(GitError::NoWorkspace)?
        .commit(&message)
}

#[tauri::command]
fn plugin_list(
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Vec<PluginSummary>, PluginError> {
    registry.list()
}

#[tauri::command]
fn ai_provider_current(
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Option<AiProviderSummary>, AgentRuntimeError> {
    current_ai_provider(registry.inner())
}

#[tauri::command]
fn plugin_install(
    plugin_id: String,
    approved_permissions: Vec<PluginPermission>,
    app: tauri::AppHandle,
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Vec<PluginSummary>, PluginError> {
    registry.install(&app, &plugin_id, &approved_permissions)
}

#[tauri::command]
fn plugin_uninstall(
    plugin_id: String,
    app: tauri::AppHandle,
    plugins: tauri::State<'_, PluginLifecycleState>,
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Vec<PluginSummary>, PluginError> {
    let _mutation = plugins
        .mutation
        .lock()
        .map_err(|_| PluginError::StateUnavailable)?;
    plugins
        .tasks
        .invalidate_plugin(&plugin_id)
        .map_err(|_| PluginError::StateUnavailable)?;
    plugins
        .runtimes
        .stop(&plugin_id)
        .map_err(map_runtime_error)?;
    registry.uninstall(&app, &plugin_id)
}

#[tauri::command]
fn plugin_set_enabled(
    plugin_id: String,
    enabled: bool,
    app: tauri::AppHandle,
    plugins: tauri::State<'_, PluginLifecycleState>,
    project: tauri::State<'_, ProjectState>,
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Vec<PluginSummary>, PluginError> {
    let _mutation = plugins
        .mutation
        .lock()
        .map_err(|_| PluginError::StateUnavailable)?;
    if !enabled {
        plugins
            .tasks
            .invalidate_plugin(&plugin_id)
            .map_err(|_| PluginError::StateUnavailable)?;
        let summaries = registry.set_enabled(&app, &plugin_id, false)?;
        plugins
            .runtimes
            .stop(&plugin_id)
            .map_err(map_runtime_error)?;
        return Ok(summaries);
    }

    let spec = registry.external_runtime_spec(&plugin_id, false)?;
    if let Some(spec) = spec {
        let workspace =
            project_snapshot(&project).map(|project| project.workspace.root().to_owned());
        let root = plugin_storage_root(&app)?;
        plugins
            .runtimes
            .start(&root, spec, workspace.as_deref())
            .map_err(map_runtime_error)?;
    }
    match registry.set_enabled(&app, &plugin_id, true) {
        Ok(summaries) => Ok(summaries),
        Err(error) => {
            let _ = plugins.runtimes.stop(&plugin_id);
            Err(error)
        }
    }
}

fn map_runtime_error(error: PluginRuntimeError) -> PluginError {
    match error {
        PluginRuntimeError::UnsupportedPlatform => PluginError::ExternalRuntimeUnsupported,
        PluginRuntimeError::SandboxUnavailable => PluginError::SandboxUnavailable,
        PluginRuntimeError::PermissionDenied => PluginError::PermissionDenied,
        PluginRuntimeError::WorkspaceUnavailable => PluginError::RuntimeWorkspaceUnavailable,
        PluginRuntimeError::InvalidPackage => PluginError::InvalidInstalledPackage,
        PluginRuntimeError::StopFailed => PluginError::RuntimeStopFailed,
        PluginRuntimeError::RuntimeDirectoryUnavailable
        | PluginRuntimeError::SpawnFailed
        | PluginRuntimeError::ProtocolViolation
        | PluginRuntimeError::TransportClosed
        | PluginRuntimeError::RequestTimeout
        | PluginRuntimeError::RuntimeNotRunning
        | PluginRuntimeError::CapabilityDenied
        | PluginRuntimeError::PluginRejected
        | PluginRuntimeError::StateUnavailable => PluginError::RuntimeStartFailed,
    }
}

fn start_enabled_external_runtimes(
    app: &tauri::AppHandle,
    registry: &PluginRegistry,
    runtimes: &PluginRuntimeService,
    workspace: Option<&std::path::Path>,
) {
    let Ok(root) = plugin_storage_root(app) else {
        return;
    };
    let Ok(specs) = registry.enabled_external_runtime_specs() else {
        return;
    };
    for spec in specs {
        let id = spec.id.clone();
        if runtimes.start(&root, spec, workspace).is_err() {
            let _ = registry.set_enabled(app, &id, false);
        }
    }
}

#[tauri::command]
fn plugin_open_repository(
    plugin_id: String,
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<(), PluginError> {
    registry.open_repository(&plugin_id)
}

#[tauri::command]
fn plugin_catalog_list(
    catalog_service: tauri::State<'_, PluginCatalogService>,
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Vec<TrustedPluginSummary>, PluginInstallFlowError> {
    let mut catalog = catalog_service.summaries()?;
    let installed = registry.list()?;
    if catalog.iter().any(|entry| {
        installed
            .iter()
            .any(|plugin| plugin.bundled && plugin.id == entry.manifest.id)
    }) {
        return Err(PluginCatalogError::InvalidCatalog.into());
    }
    for entry in &mut catalog {
        if let Some(plugin) = installed
            .iter()
            .find(|plugin| plugin.installed && plugin.id == entry.manifest.id)
        {
            entry.installed_version = Some(plugin.version.clone());
            entry.download_available = plugin.version < entry.manifest.version;
        }
    }
    Ok(catalog)
}

#[tauri::command]
async fn plugin_catalog_update(
    app: tauri::AppHandle,
    catalog_service: tauri::State<'_, PluginCatalogService>,
    registry: tauri::State<'_, PluginRegistry>,
    plugins: tauri::State<'_, PluginLifecycleState>,
    project: tauri::State<'_, ProjectState>,
) -> Result<Vec<TrustedPluginSummary>, PluginInstallFlowError> {
    let root = plugin_storage_root(&app)?;
    let service = catalog_service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.update(&root))
        .await
        .map_err(|_| PluginCatalogError::DownloadFailed)??;
    let _mutation = plugins
        .mutation
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    plugins
        .tasks
        .invalidate_all()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    let stop_result = plugins.runtimes.stop_all().map_err(map_runtime_error);
    registry.reload(&app)?;
    stop_result?;
    let workspace = project_snapshot(&project).map(|project| project.workspace.root().to_owned());
    start_enabled_external_runtimes(
        &app,
        registry.inner(),
        &plugins.runtimes,
        workspace.as_deref(),
    );
    plugin_catalog_list(catalog_service, registry)
}

fn ensure_trusted_download_available(
    registry: &PluginRegistry,
    plugin_id: &str,
    version: &semver::Version,
) -> Result<(), PluginInstallFlowError> {
    if let Some(plugin) = registry
        .list()?
        .into_iter()
        .find(|plugin| plugin.id == plugin_id)
    {
        if plugin.bundled {
            return Err(PluginCatalogError::InvalidCatalog.into());
        }
        if plugin.installed && plugin.version >= *version {
            return Err(PluginPackageError::AlreadyInstalled.into());
        }
    }
    Ok(())
}

#[tauri::command]
fn plugin_package_select(
    app: tauri::AppHandle,
    state: tauri::State<'_, PluginLifecycleState>,
) -> Result<Option<PluginInstallReview>, PluginInstallFlowError> {
    let Some(selection) = app
        .dialog()
        .file()
        .set_title("Selecionar pacote de plugin do Lyrnova")
        .add_filter("Pacote de plugin", &["zst"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let package_path = selection
        .into_path()
        .map_err(|_| PluginPackageError::PackageUnavailable)?;
    let asset = package_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(PluginPackageError::InvalidDescriptor)?;
    let descriptor_path = package_path.with_file_name(format!("{asset}.json"));
    let descriptor = PluginPackageDescriptor::read_sidecar(&descriptor_path)?;
    let host_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| PluginPackageError::InvalidManifest)?;
    let _mutation = state
        .mutation
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    let installer = PluginPackageInstaller::new(plugin_storage_root(&app)?, host_version);
    let staged = installer.stage_local(&package_path, descriptor)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let review = PluginInstallReview {
        token: token.clone(),
        review: staged.review().clone(),
    };
    let mut current = state
        .pending
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    *current = Some(PendingPluginInstall { token, staged });
    Ok(Some(review))
}

#[tauri::command]
async fn plugin_package_download(
    plugin_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, PluginLifecycleState>,
    registry: tauri::State<'_, PluginRegistry>,
    catalog_service: tauri::State<'_, PluginCatalogService>,
) -> Result<PluginInstallReview, PluginInstallFlowError> {
    let release = catalog_service.trusted_release(&plugin_id)?;
    ensure_trusted_download_available(
        registry.inner(),
        &release.manifest.id,
        &release.manifest.version,
    )?;
    let root = plugin_storage_root(&app)?;
    let host_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| PluginPackageError::InvalidManifest)?;
    let staged = tauri::async_runtime::spawn_blocking(move || {
        let downloaded = download_release(&root, &release)?;
        let staged = PluginPackageInstaller::new(root, host_version)
            .stage_local(downloaded.path(), release.descriptor.clone())
            .map_err(PluginInstallFlowError::from)?;
        if staged.review().manifest != release.manifest
            || staged.review().descriptor != release.descriptor
        {
            return Err(PluginInstallFlowError::Catalog(
                PluginCatalogError::PublisherSignatureInvalid,
            ));
        }
        Ok(staged.authenticate(release.authentication.clone()))
    })
    .await
    .map_err(|_| PluginCatalogError::DownloadFailed)??;

    let _mutation = state
        .mutation
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    if let Some(authentication) = &staged.review().authentication {
        catalog_service.verify_installed(
            &staged.review().manifest,
            &staged.review().descriptor,
            authentication,
        )?;
    }
    ensure_trusted_download_available(
        registry.inner(),
        &staged.review().manifest.id,
        &staged.review().manifest.version,
    )?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let review = PluginInstallReview {
        token: token.clone(),
        review: staged.review().clone(),
    };
    let mut current = state
        .pending
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    *current = Some(PendingPluginInstall { token, staged });
    Ok(review)
}

#[tauri::command]
fn plugin_package_confirm(
    token: String,
    approved_permissions: Vec<PluginPermission>,
    app: tauri::AppHandle,
    installs: tauri::State<'_, PluginLifecycleState>,
    registry: tauri::State<'_, PluginRegistry>,
    catalog_service: tauri::State<'_, PluginCatalogService>,
) -> Result<Vec<PluginSummary>, PluginInstallFlowError> {
    let _mutation = installs
        .mutation
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    let mut current = installs
        .pending
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    let pending = current
        .as_ref()
        .filter(|pending| pending.token == token)
        .ok_or(PluginInstallFlowError::UnknownSession)?;
    if !permissions_exactly_match(
        &pending.staged.review().manifest.permissions,
        approved_permissions.iter().copied(),
    ) {
        return Err(PluginPackageError::PermissionApprovalRequired.into());
    }
    if let Some(authentication) = &pending.staged.review().authentication {
        catalog_service.verify_installed(
            &pending.staged.review().manifest,
            &pending.staged.review().descriptor,
            authentication,
        )?;
    }
    let plugin_id = pending.staged.review().manifest.id.clone();
    let staged = current
        .take()
        .ok_or(PluginInstallFlowError::UnknownSession)?
        .staged;
    drop(current);

    installs
        .tasks
        .invalidate_plugin(&plugin_id)
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    installs
        .runtimes
        .stop(&plugin_id)
        .map_err(map_runtime_error)?;
    let installed = staged.install(&approved_permissions)?;
    registry
        .register_external_install(&app, &installed, &approved_permissions)
        .map_err(Into::into)
}

#[tauri::command]
fn plugin_package_cancel(
    token: String,
    state: tauri::State<'_, PluginLifecycleState>,
) -> Result<(), PluginInstallFlowError> {
    let mut current = state
        .pending
        .lock()
        .map_err(|_| PluginInstallFlowError::StateUnavailable)?;
    if current
        .as_ref()
        .is_none_or(|pending| pending.token != token)
    {
        return Err(PluginInstallFlowError::UnknownSession);
    }
    current.take();
    Ok(())
}

#[tauri::command]
fn terminal_start(
    window: tauri::WebviewWindow,
    project: tauri::State<'_, ProjectState>,
    terminal: tauri::State<'_, TerminalService>,
) -> Result<(), TerminalError> {
    let project = project_snapshot(&project).ok_or(TerminalError::ProcessFailed)?;
    match terminal.start(project.workspace.root(), window) {
        Err(TerminalError::AlreadyRunning) | Ok(()) => Ok(()),
        Err(error) => Err(error),
    }
}

#[tauri::command]
fn terminal_write(
    input: String,
    terminal: tauri::State<'_, TerminalService>,
) -> Result<(), TerminalError> {
    terminal.write_line(&input)
}

#[tauri::command]
fn terminal_stop(terminal: tauri::State<'_, TerminalService>) -> Result<(), TerminalError> {
    terminal.stop()
}

#[tauri::command]
fn task_list(
    registry: tauri::State<'_, PluginRegistry>,
    plugins: tauri::State<'_, PluginLifecycleState>,
) -> Result<TaskList, TaskFlowError> {
    let mut items = Vec::new();
    for provider in registry.enabled_task_providers()? {
        let response = plugins.runtimes.request(
            &provider.id,
            PluginCapability::Tasks,
            "tasks.list".into(),
            serde_json::json!({}),
        )?;
        items.extend(plugins.tasks.list(&provider, response.result)?);
    }
    items.sort_by(|left, right| {
        left.plugin_name
            .cmp(&right.plugin_name)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    Ok(TaskList {
        items,
        sandbox: plugins.tasks.sandbox_diagnostic(),
    })
}

#[tauri::command]
fn task_review(
    plugin_id: String,
    task_id: String,
    window: tauri::WebviewWindow,
    project: tauri::State<'_, ProjectState>,
    registry: tauri::State<'_, PluginRegistry>,
    plugins: tauri::State<'_, PluginLifecycleState>,
) -> Result<TaskReview, TaskFlowError> {
    let root = project_snapshot(&project)
        .ok_or(TaskFlowError::NoWorkspace)?
        .workspace
        .root()
        .to_owned();
    let provider = registry.task_provider(&plugin_id)?;
    let response = plugins.runtimes.request(
        &provider.id,
        PluginCapability::Tasks,
        "tasks.list".into(),
        serde_json::json!({}),
    )?;
    let (review, audit) = plugins
        .tasks
        .review(&root, &provider, &task_id, response.result)?;
    let _ = window.emit("process-audit", audit);
    Ok(review)
}

#[tauri::command]
async fn task_execute(
    review_token: String,
    action_sha256: String,
    window: tauri::WebviewWindow,
    registry: tauri::State<'_, PluginRegistry>,
    plugins: tauri::State<'_, PluginLifecycleState>,
) -> Result<ProcessResult, TaskFlowError> {
    let plugin_id = plugins.tasks.pending_plugin_id(&review_token)?;
    let provider = match registry.task_provider(&plugin_id) {
        Ok(provider) => provider,
        Err(error) => {
            let _ = plugins.tasks.discard(&review_token);
            return Err(error.into());
        }
    };
    let task_broker = plugins.tasks.clone();
    let output_window = window.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let emit: Arc<dyn Fn(ProcessOutputEvent) + Send + Sync> = Arc::new(move |event| {
            let _ = output_window.emit("task-output", event);
        });
        let (result, audits) =
            task_broker.execute(&review_token, &action_sha256, &provider.permissions, emit)?;
        for audit in audits {
            let _ = window.emit("process-audit", audit);
        }
        Ok(result)
    })
    .await
    .map_err(|_| TaskFlowError::Task(TaskError::StateUnavailable))?
}

#[tauri::command]
fn task_review_discard(
    review_token: String,
    plugins: tauri::State<'_, PluginLifecycleState>,
) -> Result<(), TaskError> {
    plugins.tasks.discard(&review_token)
}

#[tauri::command]
fn task_cancel(
    process_id: String,
    plugins: tauri::State<'_, PluginLifecycleState>,
) -> Result<(), TaskError> {
    plugins.tasks.cancel(&process_id)
}

#[tauri::command]
async fn agent_account_read(
    state: tauri::State<'_, ProjectState>,
    plugins: tauri::State<'_, PluginRegistry>,
) -> Result<AgentConnectionStatus, AgentRuntimeError> {
    resolve_ai_provider(
        plugins.inner(),
        &[PluginCapability::AccountAuth],
        &[
            PluginPermission::ProcessSpawn,
            PluginPermission::NetworkAccess,
        ],
    )?;
    let root = agent_runtime_root(&state);
    tauri::async_runtime::spawn_blocking(move || app_server::read_account(&root))
        .await
        .map_err(|_| AgentRuntimeError::ProcessFailed)?
}

#[tauri::command]
async fn agent_logout(
    state: tauri::State<'_, ProjectState>,
    plugins: tauri::State<'_, PluginRegistry>,
    approvals: tauri::State<'_, ApprovalBroker>,
) -> Result<AgentConnectionStatus, AgentRuntimeError> {
    resolve_ai_provider(
        plugins.inner(),
        &[PluginCapability::AccountAuth],
        &[
            PluginPermission::ProcessSpawn,
            PluginPermission::NetworkAccess,
        ],
    )?;
    let root = agent_runtime_root(&state);
    let result = tauri::async_runtime::spawn_blocking(move || app_server::logout(&root))
        .await
        .map_err(|_| AgentRuntimeError::ProcessFailed)?;
    if result.is_ok() {
        approvals.clear_session();
    }
    result
}

#[tauri::command]
async fn agent_turn_start(
    request: AgentTurnRequest,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
    approvals: tauri::State<'_, ApprovalBroker>,
    plugins: tauri::State<'_, PluginRegistry>,
) -> Result<AgentTurnResult, AgentRuntimeError> {
    resolve_ai_provider(
        plugins.inner(),
        &[
            PluginCapability::AiChat,
            PluginCapability::AiTools,
            PluginCapability::Approvals,
        ],
        &[
            PluginPermission::WorkspaceRead,
            PluginPermission::ProcessSpawn,
            PluginPermission::NetworkAccess,
            PluginPermission::RequestApproval,
        ],
    )?;
    let root = project_snapshot(&state)
        .ok_or(AgentRuntimeError::InvalidRequest)?
        .workspace
        .root()
        .to_owned();
    let approvals = approvals.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        app_server::run_turn(&root, request, &approvals, |event| {
            let _ = window.emit("agent-stream", event);
        })
    })
    .await
    .map_err(|_| AgentRuntimeError::ProcessFailed)?
}

#[tauri::command]
fn agent_approval_resolve(
    request: AgentApprovalResolution,
    approvals: tauri::State<'_, ApprovalBroker>,
    plugins: tauri::State<'_, PluginRegistry>,
) -> Result<(), AgentRuntimeError> {
    resolve_ai_provider(
        plugins.inner(),
        &[PluginCapability::Approvals],
        &[PluginPermission::RequestApproval],
    )?;
    approvals.resolve(request)
}

#[tauri::command]
fn agent_approval_state(
    approvals: tauri::State<'_, ApprovalBroker>,
) -> Result<app_server::AgentApprovalState, AgentRuntimeError> {
    approvals.snapshot()
}

#[tauri::command]
fn agent_approval_session_revoke(
    rule_id: String,
    approvals: tauri::State<'_, ApprovalBroker>,
) -> Result<(), AgentRuntimeError> {
    approvals.revoke_session_rule(&rule_id)
}

#[tauri::command]
fn agent_login_start(
    mode: AgentLoginMode,
    window: tauri::WebviewWindow,
    project: tauri::State<'_, ProjectState>,
    login: tauri::State<'_, LoginState>,
    plugins: tauri::State<'_, PluginRegistry>,
) -> Result<(), AgentRuntimeError> {
    resolve_ai_provider(
        plugins.inner(),
        &[PluginCapability::AccountAuth],
        &[
            PluginPermission::ProcessSpawn,
            PluginPermission::NetworkAccess,
        ],
    )?;
    if login.0.swap(true, Ordering::AcqRel) {
        return Err(AgentRuntimeError::LoginInProgress);
    }
    let root = agent_runtime_root(&project);
    let active = Arc::clone(&login.0);
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(code) = app_server::run_login(&root, mode, |event| {
            let _ = window.emit("agent-login", event);
        }) {
            let _ = window.emit("agent-login", AgentLoginEvent::Failed { code });
        }
        active.store(false, Ordering::Release);
    });
    Ok(())
}

fn development_workspace() -> Option<WorkspaceService> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let root = std::env::current_dir().ok()?;
    if !root.join(".git").is_dir() {
        return None;
    }
    WorkspaceService::new(root).ok()
}

fn agent_runtime_root(state: &tauri::State<'_, ProjectState>) -> std::path::PathBuf {
    project_snapshot(state)
        .map(|project| project.workspace.root().to_owned())
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let catalog_service = PluginCatalogService::default();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(ProjectState(RwLock::new(None)))
        .manage(LoginState(Arc::new(AtomicBool::new(false))))
        .manage(ApprovalBroker::default())
        .manage(PluginRegistry::with_catalog_service(
            catalog_service.clone(),
        ))
        .manage(catalog_service)
        .manage(PluginLifecycleState::default())
        .manage(TerminalService::new())
        .manage(WorkspaceRecoveryService::default())
        .setup(|app| {
            if let Ok(root) = workspace_recovery_root(app.handle()) {
                let _ = WorkspaceRecoveryService::cleanup_stale(&root);
            }
            if let Ok(root) = plugin_storage_root(app.handle()) {
                PluginRuntimeService::cleanup_stale_sessions(&root);
                let _ = app.state::<PluginCatalogService>().load_cached(&root);
            }
            app.state::<PluginRegistry>().load(app.handle());
            let workspace = load_last_project(app.handle()).or_else(development_workspace);
            let workspace_root = workspace
                .as_ref()
                .map(|workspace| workspace.root().to_owned());
            if let Some(workspace) = workspace {
                let project = ActiveProject {
                    git: GitService::new(workspace.root()).ok(),
                    workspace,
                };
                *app.state::<ProjectState>().0.write().map_err(|_| {
                    std::io::Error::other("project state lock poisoned during startup")
                })? = Some(project);
            }
            start_enabled_external_runtimes(
                app.handle(),
                app.state::<PluginRegistry>().inner(),
                &app.state::<PluginLifecycleState>().runtimes,
                workspace_root.as_deref(),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            project_current,
            project_open_dialog,
            project_create_dialog,
            workspace_list,
            workspace_read,
            workspace_read_range,
            workspace_metadata,
            workspace_search,
            workspace_save,
            workspace_create_document,
            workspace_create_directory,
            workspace_move,
            workspace_preview_patch,
            workspace_apply_patch,
            workspace_delete,
            workspace_restore,
            git_status,
            git_stage,
            git_unstage,
            git_commit,
            plugin_list,
            ai_provider_current,
            plugin_install,
            plugin_uninstall,
            plugin_set_enabled,
            plugin_open_repository,
            plugin_catalog_list,
            plugin_catalog_update,
            plugin_package_select,
            plugin_package_download,
            plugin_package_confirm,
            plugin_package_cancel,
            terminal_start,
            terminal_write,
            terminal_stop,
            task_list,
            task_review,
            task_execute,
            task_review_discard,
            task_cancel,
            agent_account_read,
            agent_logout,
            agent_turn_start,
            agent_approval_resolve,
            agent_approval_state,
            agent_approval_session_revoke,
            agent_login_start
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Lyrnova");
}
use app_server::{
    AgentApprovalResolution, AgentConnectionStatus, AgentLoginEvent, AgentLoginMode,
    AgentRuntimeError, AgentTurnRequest, AgentTurnResult, ApprovalBroker,
};

#[cfg(test)]
mod tests {
    use super::{
        MAX_RECENT_PROJECTS, PROJECT_HISTORY_VERSION, PluginInstallFlowError, ProjectHistory,
        remember_project_path, validated_project_name,
    };
    use crate::plugin_catalog::PluginCatalogError;
    use crate::plugin_package::PluginPackageError;
    use crate::workspace::WorkspaceError;

    #[test]
    fn accepts_portable_project_names_and_trims_outer_whitespace() {
        assert_eq!(validated_project_name("  lyrnova-app  "), Ok("lyrnova-app"));
        assert_eq!(validated_project_name("Lyrnova β"), Ok("Lyrnova β"));
    }

    #[test]
    fn rejects_empty_relative_and_reserved_project_names() {
        for name in ["", "   ", ".", "..", "CON", "nul.txt", "COM1", "lpt9.log"] {
            assert_eq!(
                validated_project_name(name),
                Err(WorkspaceError::InvalidProjectName),
                "name {name:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_path_separators_reserved_characters_and_long_names() {
        for name in ["foo/bar", "foo\\bar", "bad:name", "bad?name", "trailing."] {
            assert_eq!(
                validated_project_name(name),
                Err(WorkspaceError::InvalidProjectName),
                "name {name:?} should be rejected"
            );
        }
        let long_name = "a".repeat(65);
        assert_eq!(
            validated_project_name(&long_name),
            Err(WorkspaceError::InvalidProjectName)
        );
    }

    #[test]
    fn recent_projects_are_deduplicated_and_bounded() {
        let mut history = ProjectHistory::default();
        for index in 0..12 {
            remember_project_path(
                &mut history,
                std::path::Path::new(&format!("/projects/project-{index}")),
            );
        }
        remember_project_path(&mut history, std::path::Path::new("/projects/project-5"));

        assert_eq!(history.version, PROJECT_HISTORY_VERSION);
        assert_eq!(history.recent.len(), MAX_RECENT_PROJECTS);
        assert_eq!(history.recent[0], "/projects/project-5");
        assert_eq!(
            history
                .recent
                .iter()
                .filter(|path| path.as_str() == "/projects/project-5")
                .count(),
            1
        );
    }

    #[test]
    fn plugin_install_errors_keep_their_domain_across_ipc() {
        let package = serde_json::to_value(PluginInstallFlowError::Package(
            PluginPackageError::PermissionApprovalRequired,
        ))
        .unwrap();
        let expired = serde_json::to_value(PluginInstallFlowError::UnknownSession).unwrap();
        let catalog = serde_json::to_value(PluginInstallFlowError::Catalog(
            PluginCatalogError::DownloadUrlDenied,
        ))
        .unwrap();

        assert_eq!(
            package,
            serde_json::json!({
                "domain": "package",
                "error": { "code": "permission_approval_required" }
            })
        );
        assert_eq!(expired, serde_json::json!({ "domain": "unknown_session" }));
        assert_eq!(
            catalog,
            serde_json::json!({
                "domain": "catalog",
                "error": { "code": "download_url_denied" }
            })
        );
    }
}
