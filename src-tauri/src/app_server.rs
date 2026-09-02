use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::backend::{
    AccountSummary, BackendError, CodexAppServerAdapter, LoginChallenge, MAX_FRAME_BYTES,
    RpcInbound, RpcRequest, RpcResponse, decode_frame, encode_frame,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnectionStatus {
    pub backend: String,
    pub account: Option<AccountSummary>,
}

const MAX_PROMPT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnRequest {
    pub thread_id: Option<String>,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnResult {
    pub thread_id: String,
    pub turn_id: String,
    pub status: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

impl AgentApprovalDecision {
    fn wire_value(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::AcceptForSession => "acceptForSession",
            Self::Decline => "decline",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentApprovalResolution {
    pub approval_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub decision: AgentApprovalDecision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalKind {
    Command,
    FileChange,
    Network,
    WriteStdin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFileChange {
    pub path: String,
    pub kind: String,
    pub diff: String,
}

struct PendingApproval {
    thread_id: String,
    turn_id: String,
    item_id: String,
    sender: mpsc::Sender<AgentApprovalDecision>,
}

#[derive(Clone, Default)]
pub struct ApprovalBroker {
    pending: Arc<Mutex<HashMap<String, PendingApproval>>>,
}

impl ApprovalBroker {
    fn register(
        &self,
        thread_id: &str,
        turn_id: &str,
        item_id: &str,
    ) -> Result<(String, mpsc::Receiver<AgentApprovalDecision>), AgentRuntimeError> {
        let approval_id = Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::channel();
        let pending = PendingApproval {
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.to_owned(),
            item_id: item_id.to_owned(),
            sender,
        };
        self.pending
            .lock()
            .map_err(|_| AgentRuntimeError::ProcessFailed)?
            .insert(approval_id.clone(), pending);
        Ok((approval_id, receiver))
    }

    pub fn resolve(&self, resolution: AgentApprovalResolution) -> Result<(), AgentRuntimeError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| AgentRuntimeError::ProcessFailed)?;
        let approval = pending
            .get(&resolution.approval_id)
            .ok_or(AgentRuntimeError::ApprovalNotFound)?;
        if approval.thread_id != resolution.thread_id
            || approval.turn_id != resolution.turn_id
            || approval.item_id != resolution.item_id
        {
            return Err(AgentRuntimeError::ApprovalMismatch);
        }
        let approval = pending
            .remove(&resolution.approval_id)
            .ok_or(AgentRuntimeError::ApprovalNotFound)?;
        approval
            .sender
            .send(resolution.decision)
            .map_err(|_| AgentRuntimeError::TransportClosed)
    }

    fn discard(&self, approval_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(approval_id);
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLoginMode {
    Browser,
    DeviceCode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentLoginEvent {
    BrowserOpened,
    DeviceCode {
        verification_url: String,
        user_code: String,
    },
    Completed {
        account: Option<AccountSummary>,
    },
    Failed {
        code: AgentRuntimeError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentStreamEvent {
    ThreadStarted {
        thread_id: String,
    },
    TurnStarted {
        thread_id: String,
        turn_id: String,
    },
    MessageDelta {
        thread_id: String,
        turn_id: String,
        item_id: String,
        delta: String,
    },
    ApprovalRequested {
        approval_id: String,
        thread_id: String,
        turn_id: String,
        item_id: String,
        kind: AgentApprovalKind,
        reason: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
        files: Vec<AgentFileChange>,
        network_host: Option<String>,
        network_protocol: Option<String>,
    },
    ApprovalResolved {
        approval_id: String,
        decision: AgentApprovalDecision,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
        status: String,
        message: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AgentRuntimeError {
    CodexUnavailable,
    ProcessFailed,
    TransportClosed,
    InvalidProtocol,
    FrameTooLarge,
    AccountUnavailable,
    InvalidRequest,
    RequestFailed,
    ApprovalRequired,
    ApprovalNotFound,
    ApprovalMismatch,
    PluginDisabled,
    LoginInProgress,
    UnsafeLoginUrl,
}

pub fn read_account(root: &Path) -> Result<AgentConnectionStatus, AgentRuntimeError> {
    let mut transport = AppServerTransport::spawn(root)?;
    let mut adapter = CodexAppServerAdapter::default();

    let initialize = adapter
        .begin_connection(env!("CARGO_PKG_VERSION"))
        .map_err(map_backend_error)?;
    let response = transport.request(&initialize)?;
    let initialized = adapter
        .finish_initialize(response)
        .map_err(map_backend_error)?;
    transport.notification(&initialized)?;

    let request = adapter.account_read().map_err(map_backend_error)?;
    let response = transport.request(&request)?;
    let account = adapter
        .complete_account_read(response)
        .map_err(map_backend_error)?;

    Ok(AgentConnectionStatus {
        backend: "codex_app_server".into(),
        account,
    })
}

pub fn logout(root: &Path) -> Result<AgentConnectionStatus, AgentRuntimeError> {
    let mut transport = AppServerTransport::spawn(root)?;
    let mut adapter = CodexAppServerAdapter::default();
    let initialize = adapter
        .begin_connection(env!("CARGO_PKG_VERSION"))
        .map_err(map_backend_error)?;
    let response = transport.request(&initialize)?;
    let initialized = adapter
        .finish_initialize(response)
        .map_err(map_backend_error)?;
    transport.notification(&initialized)?;
    let request = adapter.logout().map_err(map_backend_error)?;
    let response = transport.request(&request)?;
    adapter.acknowledge(response).map_err(map_backend_error)?;
    Ok(AgentConnectionStatus {
        backend: "codex_app_server".into(),
        account: None,
    })
}

pub fn run_turn(
    root: &Path,
    request: AgentTurnRequest,
    approvals: &ApprovalBroker,
    mut emit: impl FnMut(AgentStreamEvent),
) -> Result<AgentTurnResult, AgentRuntimeError> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() || prompt.len() > MAX_PROMPT_BYTES {
        return Err(AgentRuntimeError::InvalidRequest);
    }
    let cwd = root.to_str().ok_or(AgentRuntimeError::InvalidRequest)?;
    let mut transport = AppServerTransport::spawn(root)?;
    let mut adapter = CodexAppServerAdapter::default();

    let initialize = adapter
        .begin_connection(env!("CARGO_PKG_VERSION"))
        .map_err(map_turn_backend_error)?;
    let response = transport.request(&initialize)?;
    let initialized = adapter
        .finish_initialize(response)
        .map_err(map_turn_backend_error)?;
    transport.notification(&initialized)?;

    let resumed_thread = request
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|thread_id| !thread_id.is_empty());
    let (thread_request, is_resume) = if let Some(thread_id) = resumed_thread {
        (
            adapter
                .resume_thread(thread_id)
                .map_err(map_turn_backend_error)?,
            true,
        )
    } else {
        (
            adapter.start_thread(cwd).map_err(map_turn_backend_error)?,
            false,
        )
    };
    let response = transport.request(&thread_request)?;
    let thread = if is_resume {
        adapter
            .complete_thread_resume(response)
            .map_err(map_turn_backend_error)?
    } else {
        adapter
            .complete_thread_start(response)
            .map_err(map_turn_backend_error)?
    };
    emit(AgentStreamEvent::ThreadStarted {
        thread_id: thread.thread_id.clone(),
    });

    let turn_request = adapter
        .start_turn(&thread.thread_id, prompt, cwd)
        .map_err(map_turn_backend_error)?;
    let (response, buffered) = transport.request_with_notifications(&turn_request)?;
    let turn = adapter
        .complete_turn_start(response)
        .map_err(map_turn_backend_error)?;
    emit(AgentStreamEvent::TurnStarted {
        thread_id: thread.thread_id.clone(),
        turn_id: turn.turn_id.clone(),
    });

    let mut last_error = None;
    let mut item_previews = HashMap::new();
    for notification in buffered {
        if let Some(result) = process_turn_notification(
            notification,
            &thread.thread_id,
            &turn.turn_id,
            &mut last_error,
            &mut item_previews,
            &mut emit,
        )? {
            return Ok(result);
        }
    }

    loop {
        match transport.read_inbound()? {
            RpcInbound::Notification(notification) => {
                if let Some(result) = process_turn_notification(
                    notification,
                    &thread.thread_id,
                    &turn.turn_id,
                    &mut last_error,
                    &mut item_previews,
                    &mut emit,
                )? {
                    return Ok(result);
                }
            }
            RpcInbound::ServerRequest(request) => handle_approval_request(
                request,
                &thread.thread_id,
                &turn.turn_id,
                &item_previews,
                approvals,
                &mut transport,
                &mut emit,
            )?,
            RpcInbound::Response(_) => return Err(AgentRuntimeError::InvalidProtocol),
        }
    }
}

pub fn run_login(
    root: &Path,
    mode: AgentLoginMode,
    mut emit: impl FnMut(AgentLoginEvent),
) -> Result<AgentConnectionStatus, AgentRuntimeError> {
    let mut transport = AppServerTransport::spawn(root)?;
    let mut adapter = CodexAppServerAdapter::default();
    let initialize = adapter
        .begin_connection(env!("CARGO_PKG_VERSION"))
        .map_err(map_backend_error)?;
    let response = transport.request(&initialize)?;
    let initialized = adapter
        .finish_initialize(response)
        .map_err(map_backend_error)?;
    transport.notification(&initialized)?;

    let login_request = match mode {
        AgentLoginMode::Browser => adapter.start_browser_login(),
        AgentLoginMode::DeviceCode => adapter.start_device_code_login(),
    }
    .map_err(map_backend_error)?;
    let response = transport.request(&login_request)?;
    let challenge = match mode {
        AgentLoginMode::Browser => adapter.complete_browser_login(response),
        AgentLoginMode::DeviceCode => adapter.complete_device_code_login(response),
    }
    .map_err(map_backend_error)?;

    let login_id = match challenge {
        LoginChallenge::Browser { login_id, auth_url } => {
            open_login_url(&auth_url)?;
            emit(AgentLoginEvent::BrowserOpened);
            login_id
        }
        LoginChallenge::DeviceCode {
            login_id,
            verification_url,
            user_code,
        } => {
            validate_login_url(&verification_url)?;
            open_login_url(&verification_url)?;
            emit(AgentLoginEvent::DeviceCode {
                verification_url,
                user_code,
            });
            login_id
        }
    };

    loop {
        match transport.read_inbound()? {
            RpcInbound::Notification(notification)
                if notification.method == "account/login/completed" =>
            {
                let completed_id = required_value_string(&notification.params, "loginId")?;
                if completed_id != login_id {
                    continue;
                }
                let success = notification
                    .params
                    .get("success")
                    .and_then(Value::as_bool)
                    .ok_or(AgentRuntimeError::InvalidProtocol)?;
                if !success {
                    return Err(AgentRuntimeError::RequestFailed);
                }
                break;
            }
            RpcInbound::Notification(_) | RpcInbound::Response(_) => continue,
            RpcInbound::ServerRequest(_) => return Err(AgentRuntimeError::InvalidProtocol),
        }
    }

    let request = adapter.account_read().map_err(map_backend_error)?;
    let response = transport.request(&request)?;
    let account = adapter
        .complete_account_read(response)
        .map_err(map_backend_error)?;
    let status = AgentConnectionStatus {
        backend: "codex_app_server".into(),
        account,
    };
    emit(AgentLoginEvent::Completed {
        account: status.account.clone(),
    });
    Ok(status)
}

fn validate_login_url(value: &str) -> Result<(), AgentRuntimeError> {
    let parsed = url::Url::parse(value).map_err(|_| AgentRuntimeError::UnsafeLoginUrl)?;
    let trusted_host = matches!(parsed.host_str(), Some("chatgpt.com" | "auth.openai.com"));
    if parsed.scheme() == "https" && trusted_host {
        Ok(())
    } else {
        Err(AgentRuntimeError::UnsafeLoginUrl)
    }
}

fn open_login_url(value: &str) -> Result<(), AgentRuntimeError> {
    validate_login_url(value)?;
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
        .arg(value)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| AgentRuntimeError::ProcessFailed)
}

fn process_turn_notification(
    notification: crate::backend::RpcNotification,
    thread_id: &str,
    turn_id: &str,
    last_error: &mut Option<String>,
    item_previews: &mut HashMap<String, ItemPreview>,
    emit: &mut impl FnMut(AgentStreamEvent),
) -> Result<Option<AgentTurnResult>, AgentRuntimeError> {
    match notification.method.as_str() {
        "item/started" => {
            require_matching_id(&notification.params, "threadId", thread_id)?;
            require_matching_id(&notification.params, "turnId", turn_id)?;
            if let Some((item_id, preview)) = item_preview(&notification.params)? {
                item_previews.insert(item_id, preview);
            }
        }
        "item/completed" => {
            require_matching_id(&notification.params, "threadId", thread_id)?;
            require_matching_id(&notification.params, "turnId", turn_id)?;
            if let Some(item_id) = notification
                .params
                .get("item")
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
            {
                item_previews.remove(item_id);
            }
        }
        "item/agentMessage/delta" => {
            require_matching_id(&notification.params, "threadId", thread_id)?;
            require_matching_id(&notification.params, "turnId", turn_id)?;
            let item_id = required_value_string(&notification.params, "itemId")?;
            let delta = required_value_string(&notification.params, "delta")?;
            emit(AgentStreamEvent::MessageDelta {
                thread_id: thread_id.to_owned(),
                turn_id: turn_id.to_owned(),
                item_id,
                delta,
            });
        }
        "error" => {
            *last_error = notification
                .params
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .filter(|message| !message.is_empty())
                .map(ToOwned::to_owned);
        }
        "turn/completed" => {
            let completed_turn = notification
                .params
                .get("turn")
                .ok_or(AgentRuntimeError::InvalidProtocol)?;
            let completed_id = required_value_string(completed_turn, "id")?;
            if completed_id != turn_id {
                return Err(AgentRuntimeError::InvalidProtocol);
            }
            let status = required_value_string(completed_turn, "status")?;
            let message = last_error.take().or_else(|| {
                completed_turn
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .filter(|message| !message.is_empty())
                    .map(ToOwned::to_owned)
            });
            emit(AgentStreamEvent::TurnCompleted {
                thread_id: thread_id.to_owned(),
                turn_id: turn_id.to_owned(),
                status: status.clone(),
                message,
            });
            return Ok(Some(AgentTurnResult {
                thread_id: thread_id.to_owned(),
                turn_id: turn_id.to_owned(),
                status,
            }));
        }
        _ => {}
    }
    Ok(None)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ItemPreview {
    command: Option<String>,
    cwd: Option<String>,
    files: Vec<AgentFileChange>,
}

fn item_preview(params: &Value) -> Result<Option<(String, ItemPreview)>, AgentRuntimeError> {
    let item = params
        .get("item")
        .and_then(Value::as_object)
        .ok_or(AgentRuntimeError::InvalidProtocol)?;
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(AgentRuntimeError::InvalidProtocol)?
        .to_owned();
    match item.get("type").and_then(Value::as_str) {
        Some("commandExecution") => Ok(Some((
            item_id,
            ItemPreview {
                command: optional_limited_string(item.get("command"), 16 * 1024),
                cwd: optional_limited_string(item.get("cwd"), 4 * 1024),
                files: Vec::new(),
            },
        ))),
        Some("fileChange") => {
            let files = item
                .get("changes")
                .and_then(Value::as_array)
                .map(|changes| {
                    changes
                        .iter()
                        .take(200)
                        .filter_map(file_change_preview)
                        .collect()
                })
                .unwrap_or_default();
            Ok(Some((
                item_id,
                ItemPreview {
                    command: None,
                    cwd: None,
                    files,
                },
            )))
        }
        Some(_) => Ok(None),
        None => Err(AgentRuntimeError::InvalidProtocol),
    }
}

fn file_change_preview(value: &Value) -> Option<AgentFileChange> {
    Some(AgentFileChange {
        path: optional_limited_string(value.get("path"), 4 * 1024)?,
        kind: optional_limited_string(value.get("kind"), 128).unwrap_or_else(|| "update".into()),
        diff: optional_limited_string(value.get("diff"), 64 * 1024).unwrap_or_default(),
    })
}

fn optional_limited_string(value: Option<&Value>, max_bytes: usize) -> Option<String> {
    let value = value?.as_str()?.trim();
    if value.is_empty() {
        return None;
    }
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut limited = value[..end].to_owned();
    if end < value.len() {
        limited.push('…');
    }
    Some(limited)
}

#[allow(clippy::too_many_arguments)]
fn handle_approval_request(
    request: RpcRequest,
    thread_id: &str,
    turn_id: &str,
    item_previews: &HashMap<String, ItemPreview>,
    approvals: &ApprovalBroker,
    transport: &mut AppServerTransport,
    emit: &mut impl FnMut(AgentStreamEvent),
) -> Result<(), AgentRuntimeError> {
    require_matching_id(&request.params, "threadId", thread_id)?;
    require_matching_id(&request.params, "turnId", turn_id)?;
    let item_id = required_value_string(&request.params, "itemId")?;
    let preview = item_previews.get(&item_id).cloned().unwrap_or_default();
    let reason = optional_limited_string(request.params.get("reason"), 4 * 1024);
    let (kind, command, cwd, files, network_host, network_protocol) = match request.method.as_str()
    {
        "item/commandExecution/requestApproval" => {
            let network = request.params.get("networkApprovalContext");
            let network_host =
                optional_limited_string(network.and_then(|value| value.get("host")), 1024);
            let network_protocol =
                optional_limited_string(network.and_then(|value| value.get("protocol")), 64);
            let kind = if network_host.is_some() {
                AgentApprovalKind::Network
            } else if request.params.get("kind").and_then(Value::as_str) == Some("writeStdin") {
                AgentApprovalKind::WriteStdin
            } else {
                AgentApprovalKind::Command
            };
            (
                kind,
                optional_limited_string(request.params.get("command"), 16 * 1024)
                    .or(preview.command),
                optional_limited_string(request.params.get("cwd"), 4 * 1024).or(preview.cwd),
                Vec::new(),
                network_host,
                network_protocol,
            )
        }
        "item/fileChange/requestApproval" => (
            AgentApprovalKind::FileChange,
            None,
            optional_limited_string(request.params.get("grantRoot"), 4 * 1024),
            preview.files,
            None,
            None,
        ),
        _ => return Err(AgentRuntimeError::ApprovalRequired),
    };

    let (approval_id, receiver) = approvals.register(thread_id, turn_id, &item_id)?;
    emit(AgentStreamEvent::ApprovalRequested {
        approval_id: approval_id.clone(),
        thread_id: thread_id.to_owned(),
        turn_id: turn_id.to_owned(),
        item_id,
        kind,
        reason,
        command,
        cwd,
        files,
        network_host,
        network_protocol,
    });
    let decision = match receiver.recv() {
        Ok(decision) => decision,
        Err(_) => {
            approvals.discard(&approval_id);
            return Err(AgentRuntimeError::TransportClosed);
        }
    };
    transport.notification(&RpcResponse {
        id: request.id,
        result: Some(serde_json::json!({ "decision": decision.wire_value() })),
        error: None,
    })?;
    emit(AgentStreamEvent::ApprovalResolved {
        approval_id,
        decision,
    });
    Ok(())
}

fn required_value_string(value: &Value, key: &str) -> Result<String, AgentRuntimeError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(AgentRuntimeError::InvalidProtocol)
}

fn require_matching_id(value: &Value, key: &str, expected: &str) -> Result<(), AgentRuntimeError> {
    if required_value_string(value, key)? == expected {
        Ok(())
    } else {
        Err(AgentRuntimeError::InvalidProtocol)
    }
}

struct AppServerTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl AppServerTransport {
    fn spawn(root: &Path) -> Result<Self, AgentRuntimeError> {
        let mut child = Command::new("codex")
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| AgentRuntimeError::CodexUnavailable)?;
        let stdin = child.stdin.take().ok_or(AgentRuntimeError::ProcessFailed)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(AgentRuntimeError::ProcessFailed)?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn request(&mut self, request: &RpcRequest) -> Result<RpcResponse, AgentRuntimeError> {
        self.write(request)?;
        loop {
            let frame = self.read_frame()?;
            match decode_frame(&frame).map_err(map_backend_error)? {
                RpcInbound::Response(response) if response.id == request.id => return Ok(response),
                RpcInbound::Response(_) | RpcInbound::Notification(_) => continue,
                RpcInbound::ServerRequest(_) => return Err(AgentRuntimeError::InvalidProtocol),
            }
        }
    }

    fn request_with_notifications(
        &mut self,
        request: &RpcRequest,
    ) -> Result<(RpcResponse, Vec<crate::backend::RpcNotification>), AgentRuntimeError> {
        self.write(request)?;
        let mut notifications = Vec::new();
        loop {
            match self.read_inbound()? {
                RpcInbound::Response(response) if response.id == request.id => {
                    return Ok((response, notifications));
                }
                RpcInbound::Notification(notification) => notifications.push(notification),
                RpcInbound::Response(_) => continue,
                RpcInbound::ServerRequest(_) => {
                    return Err(AgentRuntimeError::ApprovalRequired);
                }
            }
        }
    }

    fn notification<T: Serialize>(&mut self, notification: &T) -> Result<(), AgentRuntimeError> {
        self.write(notification)
    }

    fn write<T: Serialize>(&mut self, message: &T) -> Result<(), AgentRuntimeError> {
        let frame = encode_frame(message).map_err(map_backend_error)?;
        self.stdin
            .write_all(&frame)
            .and_then(|_| self.stdin.flush())
            .map_err(|_| AgentRuntimeError::TransportClosed)
    }

    fn read_frame(&mut self) -> Result<Vec<u8>, AgentRuntimeError> {
        let mut frame = Vec::new();
        let bytes = (&mut self.stdout)
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame)
            .map_err(|_| AgentRuntimeError::TransportClosed)?;
        if bytes == 0 {
            return Err(AgentRuntimeError::TransportClosed);
        }
        if frame.len() > MAX_FRAME_BYTES {
            return Err(AgentRuntimeError::FrameTooLarge);
        }
        Ok(frame)
    }

    fn read_inbound(&mut self) -> Result<RpcInbound, AgentRuntimeError> {
        decode_frame(&self.read_frame()?).map_err(map_backend_error)
    }
}

impl Drop for AppServerTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn map_backend_error(error: BackendError) -> AgentRuntimeError {
    match error {
        BackendError::FrameTooLarge => AgentRuntimeError::FrameTooLarge,
        BackendError::InvalidAccount => AgentRuntimeError::AccountUnavailable,
        BackendError::Remote { .. } => AgentRuntimeError::AccountUnavailable,
        _ => AgentRuntimeError::InvalidProtocol,
    }
}

fn map_turn_backend_error(error: BackendError) -> AgentRuntimeError {
    match error {
        BackendError::FrameTooLarge => AgentRuntimeError::FrameTooLarge,
        BackendError::Remote { .. } => AgentRuntimeError::RequestFailed,
        _ => AgentRuntimeError::InvalidProtocol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{RequestId, RpcNotification};

    #[test]
    fn runtime_errors_are_safe_for_the_frontend() {
        let serialized = serde_json::to_value(AgentRuntimeError::CodexUnavailable).unwrap();
        assert_eq!(serialized["code"], "codex_unavailable");
        assert_eq!(serialized.as_object().unwrap().len(), 1);
    }

    #[test]
    fn request_ids_keep_their_wire_type() {
        let response = RpcResponse {
            id: RequestId::Number(7),
            result: Some(serde_json::json!({})),
            error: None,
        };
        assert_eq!(response.id, RequestId::Number(7));
    }

    #[test]
    fn empty_prompts_are_rejected_before_starting_codex() {
        assert_eq!(
            run_turn(
                Path::new("/workspace"),
                AgentTurnRequest {
                    thread_id: None,
                    prompt: "   ".into(),
                },
                &ApprovalBroker::default(),
                |_| {}
            ),
            Err(AgentRuntimeError::InvalidRequest)
        );
    }

    #[test]
    fn stream_notifications_are_reduced_to_safe_frontend_events() {
        let mut events = Vec::new();
        let mut last_error = None;
        let mut item_previews = HashMap::new();
        assert_eq!(
            process_turn_notification(
                RpcNotification {
                    method: "item/agentMessage/delta".into(),
                    params: serde_json::json!({
                        "threadId": "thr_1",
                        "turnId": "turn_1",
                        "itemId": "item_1",
                        "delta": "Olá"
                    }),
                },
                "thr_1",
                "turn_1",
                &mut last_error,
                &mut item_previews,
                &mut |event| events.push(event),
            )
            .unwrap(),
            None
        );
        let result = process_turn_notification(
            RpcNotification {
                method: "turn/completed".into(),
                params: serde_json::json!({
                    "turn": { "id": "turn_1", "status": "completed", "error": null }
                }),
            },
            "thr_1",
            "turn_1",
            &mut last_error,
            &mut item_previews,
            &mut |event| events.push(event),
        )
        .unwrap()
        .unwrap();

        assert_eq!(result.status, "completed");
        assert!(matches!(
            &events[0],
            AgentStreamEvent::MessageDelta { delta, .. } if delta == "Olá"
        ));
        assert!(matches!(
            &events[1],
            AgentStreamEvent::TurnCompleted { status, .. } if status == "completed"
        ));
    }

    #[test]
    fn login_urls_are_restricted_to_https_openai_hosts() {
        assert_eq!(validate_login_url("https://auth.openai.com/codex"), Ok(()));
        assert_eq!(validate_login_url("https://chatgpt.com/auth"), Ok(()));
        assert_eq!(
            validate_login_url("http://auth.openai.com/codex"),
            Err(AgentRuntimeError::UnsafeLoginUrl)
        );
        assert_eq!(
            validate_login_url("https://openai.example/login"),
            Err(AgentRuntimeError::UnsafeLoginUrl)
        );
        assert_eq!(
            validate_login_url("https://auth.openai.com.evil.example/login"),
            Err(AgentRuntimeError::UnsafeLoginUrl)
        );
    }

    #[test]
    fn approval_broker_binds_resolution_to_thread_turn_and_item_once() {
        let broker = ApprovalBroker::default();
        let (approval_id, receiver) = broker.register("thr_1", "turn_1", "item_1").unwrap();
        let mismatched = AgentApprovalResolution {
            approval_id: approval_id.clone(),
            thread_id: "thr_other".into(),
            turn_id: "turn_1".into(),
            item_id: "item_1".into(),
            decision: AgentApprovalDecision::Accept,
        };
        assert_eq!(
            broker.resolve(mismatched),
            Err(AgentRuntimeError::ApprovalMismatch)
        );

        let resolution = AgentApprovalResolution {
            approval_id: approval_id.clone(),
            thread_id: "thr_1".into(),
            turn_id: "turn_1".into(),
            item_id: "item_1".into(),
            decision: AgentApprovalDecision::Decline,
        };
        broker.resolve(resolution.clone()).unwrap();
        assert_eq!(receiver.recv().unwrap(), AgentApprovalDecision::Decline);
        assert_eq!(
            broker.resolve(resolution),
            Err(AgentRuntimeError::ApprovalNotFound)
        );
    }

    #[test]
    fn item_started_builds_a_bounded_file_change_preview() {
        let preview = item_preview(&serde_json::json!({
            "item": {
                "type": "fileChange",
                "id": "item_patch",
                "changes": [{
                    "path": "src/main.rs",
                    "kind": "update",
                    "diff": "@@ -1 +1 @@"
                }]
            }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(preview.0, "item_patch");
        assert_eq!(preview.1.files[0].path, "src/main.rs");
        assert_eq!(preview.1.files[0].kind, "update");
    }

    #[test]
    fn approval_decisions_use_the_app_server_wire_spelling() {
        assert_eq!(AgentApprovalDecision::Accept.wire_value(), "accept");
        assert_eq!(
            AgentApprovalDecision::AcceptForSession.wire_value(),
            "acceptForSession"
        );
        assert_eq!(AgentApprovalDecision::Decline.wire_value(), "decline");
        assert_eq!(AgentApprovalDecision::Cancel.wire_value(), "cancel");
    }
}
