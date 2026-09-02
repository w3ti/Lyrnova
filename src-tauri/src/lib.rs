pub mod app_server;
pub mod backend;
pub mod git;
pub mod plugin_manifest;
pub mod plugin_package;
pub mod plugins;
pub mod protocol;
pub mod terminal;
pub mod workspace;

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};
use std::{fs, process::Command};

use git::{GitError, GitService, GitStatusSummary};
use plugin_manifest::PluginPermission;
use plugins::{CODEX_PLUGIN_ID, PluginError, PluginRegistry, PluginSummary};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use terminal::{TerminalError, TerminalService};
use workspace::{
    DocumentSnapshot, SaveDocumentRequest, WorkspaceEntry, WorkspaceError, WorkspaceService,
};

#[derive(Clone)]
struct ActiveProject {
    workspace: WorkspaceService,
    git: Option<GitService>,
}

struct ProjectState(RwLock<Option<ActiveProject>>);
struct LoginState(Arc<AtomicBool>);

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
    terminal.stop().map_err(|_| WorkspaceError::Io)?;
    let summary = project_summary(&project);
    remember_project(&app, project.workspace.root());
    *state.0.write().map_err(|_| WorkspaceError::Io)? = Some(project);
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
    terminal.stop().map_err(|_| WorkspaceError::Io)?;
    let summary = project_summary(&project);
    remember_project(&app, project.workspace.root());
    *state.0.write().map_err(|_| WorkspaceError::Io)? = Some(project);
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
fn workspace_save(
    request: SaveDocumentRequest,
    state: tauri::State<'_, ProjectState>,
) -> Result<DocumentSnapshot, WorkspaceError> {
    project_snapshot(&state)
        .ok_or(WorkspaceError::NoWorkspace)?
        .workspace
        .save(request)
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
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Vec<PluginSummary>, PluginError> {
    registry.uninstall(&app, &plugin_id)
}

#[tauri::command]
fn plugin_set_enabled(
    plugin_id: String,
    enabled: bool,
    app: tauri::AppHandle,
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<Vec<PluginSummary>, PluginError> {
    registry.set_enabled(&app, &plugin_id, enabled)
}

#[tauri::command]
fn plugin_open_repository(
    plugin_id: String,
    registry: tauri::State<'_, PluginRegistry>,
) -> Result<(), PluginError> {
    registry.open_repository(&plugin_id)
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
async fn agent_account_read(
    state: tauri::State<'_, ProjectState>,
    plugins: tauri::State<'_, PluginRegistry>,
) -> Result<AgentConnectionStatus, AgentRuntimeError> {
    require_codex_permissions(
        &plugins,
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
) -> Result<AgentConnectionStatus, AgentRuntimeError> {
    require_codex_permissions(
        &plugins,
        &[
            PluginPermission::ProcessSpawn,
            PluginPermission::NetworkAccess,
        ],
    )?;
    let root = agent_runtime_root(&state);
    tauri::async_runtime::spawn_blocking(move || app_server::logout(&root))
        .await
        .map_err(|_| AgentRuntimeError::ProcessFailed)?
}

#[tauri::command]
async fn agent_turn_start(
    request: AgentTurnRequest,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ProjectState>,
    approvals: tauri::State<'_, ApprovalBroker>,
    plugins: tauri::State<'_, PluginRegistry>,
) -> Result<AgentTurnResult, AgentRuntimeError> {
    require_codex_permissions(
        &plugins,
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
    require_codex_permissions(&plugins, &[PluginPermission::RequestApproval])?;
    approvals.resolve(request)
}

#[tauri::command]
fn agent_login_start(
    mode: AgentLoginMode,
    window: tauri::WebviewWindow,
    project: tauri::State<'_, ProjectState>,
    login: tauri::State<'_, LoginState>,
    plugins: tauri::State<'_, PluginRegistry>,
) -> Result<(), AgentRuntimeError> {
    require_codex_permissions(
        &plugins,
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

fn require_codex_permissions(
    plugins: &tauri::State<'_, PluginRegistry>,
    permissions: &[PluginPermission],
) -> Result<(), AgentRuntimeError> {
    if !plugins.is_enabled(CODEX_PLUGIN_ID) {
        return Err(AgentRuntimeError::PluginDisabled);
    }
    permissions.iter().try_for_each(|permission| {
        plugins
            .authorize(CODEX_PLUGIN_ID, *permission)
            .map_err(|_| AgentRuntimeError::PluginPermissionDenied)
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(ProjectState(RwLock::new(None)))
        .manage(LoginState(Arc::new(AtomicBool::new(false))))
        .manage(ApprovalBroker::default())
        .manage(PluginRegistry::default())
        .manage(TerminalService::new())
        .setup(|app| {
            app.state::<PluginRegistry>().load(app.handle());
            let workspace = load_last_project(app.handle()).or_else(development_workspace);
            if let Some(workspace) = workspace {
                let project = ActiveProject {
                    git: GitService::new(workspace.root()).ok(),
                    workspace,
                };
                *app.state::<ProjectState>().0.write().map_err(|_| {
                    std::io::Error::other("project state lock poisoned during startup")
                })? = Some(project);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            project_current,
            project_open_dialog,
            project_create_dialog,
            workspace_list,
            workspace_read,
            workspace_save,
            git_status,
            git_stage,
            git_unstage,
            git_commit,
            plugin_list,
            plugin_install,
            plugin_uninstall,
            plugin_set_enabled,
            plugin_open_repository,
            terminal_start,
            terminal_write,
            terminal_stop,
            agent_account_read,
            agent_logout,
            agent_turn_start,
            agent_approval_resolve,
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
        MAX_RECENT_PROJECTS, PROJECT_HISTORY_VERSION, ProjectHistory, remember_project_path,
        validated_project_name,
    };
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
}
