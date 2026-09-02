use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Mock,
    CodexAppServer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackendCapabilities {
    pub chatgpt_browser_login: bool,
    pub chatgpt_device_code_login: bool,
    pub account_details: bool,
    pub streamed_events: bool,
    pub approvals: bool,
    pub cancellation: bool,
}

impl BackendCapabilities {
    pub const fn codex_app_server() -> Self {
        Self {
            chatgpt_browser_login: true,
            chatgpt_device_code_login: true,
            account_details: true,
            streamed_events: true,
            approvals: true,
            cancellation: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ChatGpt,
    ApiKey,
    Other,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub auth_mode: AuthMode,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub thread_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSummary {
    pub turn_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginChallenge {
    Browser {
        login_id: String,
        auth_url: String,
    },
    DeviceCode {
        login_id: String,
        verification_url: String,
        user_code: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendError {
    FrameTooLarge,
    EmptyFrame,
    InvalidJson,
    InvalidMessage,
    InvalidState,
    UnknownRequest,
    UnexpectedResponse,
    Remote { code: i64, message: String },
    InvalidAccount,
    InvalidLoginChallenge,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    Text(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RpcRequest {
    pub method: String,
    pub id: RequestId,
    pub params: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RpcNotification {
    pub method: String,
    pub params: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RpcResponse {
    pub id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RpcInbound {
    Response(RpcResponse),
    Notification(RpcNotification),
    ServerRequest(RpcRequest),
}

pub fn encode_frame<T: Serialize>(message: &T) -> Result<Vec<u8>, BackendError> {
    let mut frame = serde_json::to_vec(message).map_err(|_| BackendError::InvalidJson)?;
    if frame.len() > MAX_FRAME_BYTES {
        return Err(BackendError::FrameTooLarge);
    }
    frame.push(b'\n');
    Ok(frame)
}

pub fn decode_frame(line: &[u8]) -> Result<RpcInbound, BackendError> {
    if line.is_empty() || line == b"\n" || line == b"\r\n" {
        return Err(BackendError::EmptyFrame);
    }
    if line.len() > MAX_FRAME_BYTES {
        return Err(BackendError::FrameTooLarge);
    }

    let value: Value = serde_json::from_slice(line).map_err(|_| BackendError::InvalidJson)?;
    let object = value.as_object().ok_or(BackendError::InvalidMessage)?;
    let has_method = object.get("method").is_some();
    let has_id = object.get("id").is_some();
    let has_result = object.get("result").is_some();
    let has_error = object.get("error").is_some();

    match (has_method, has_id, has_result, has_error) {
        (true, true, false, false) => serde_json::from_value(value)
            .map(RpcInbound::ServerRequest)
            .map_err(|_| BackendError::InvalidMessage),
        (true, false, false, false) => serde_json::from_value(value)
            .map(RpcInbound::Notification)
            .map_err(|_| BackendError::InvalidMessage),
        (false, true, true, false) | (false, true, false, true) => serde_json::from_value(value)
            .map(RpcInbound::Response)
            .map_err(|_| BackendError::InvalidMessage),
        _ => Err(BackendError::InvalidMessage),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Initializing,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingMethod {
    Initialize,
    AccountRead,
    BrowserLogin,
    DeviceCodeLogin,
    CancelLogin,
    Logout,
    ThreadStart,
    ThreadResume,
    TurnStart,
}

#[derive(Debug)]
pub struct CodexAppServerAdapter {
    state: ConnectionState,
    next_request_id: u64,
    pending: BTreeMap<RequestId, PendingMethod>,
}

impl Default for CodexAppServerAdapter {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            next_request_id: 1,
            pending: BTreeMap::new(),
        }
    }
}

impl CodexAppServerAdapter {
    pub fn kind(&self) -> BackendKind {
        BackendKind::CodexAppServer
    }

    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::codex_app_server()
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    pub fn begin_connection(
        &mut self,
        version: impl Into<String>,
    ) -> Result<RpcRequest, BackendError> {
        if self.state != ConnectionState::Disconnected {
            return Err(BackendError::InvalidState);
        }

        self.state = ConnectionState::Initializing;
        Ok(self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "lyrnova",
                    "title": "Lyrnova",
                    "version": version.into()
                }
            }),
            PendingMethod::Initialize,
        ))
    }

    pub fn finish_initialize(
        &mut self,
        response: RpcResponse,
    ) -> Result<RpcNotification, BackendError> {
        self.complete(response, PendingMethod::Initialize)?;
        if self.state != ConnectionState::Initializing {
            return Err(BackendError::InvalidState);
        }
        self.state = ConnectionState::Ready;
        Ok(RpcNotification {
            method: "initialized".into(),
            params: json!({}),
        })
    }

    pub fn account_read(&mut self) -> Result<RpcRequest, BackendError> {
        self.ready_request(
            "account/read",
            json!({ "refreshToken": false }),
            PendingMethod::AccountRead,
        )
    }

    pub fn start_browser_login(&mut self) -> Result<RpcRequest, BackendError> {
        self.ready_request(
            "account/login/start",
            json!({
                "type": "chatgpt",
                "useHostedLoginSuccessPage": true,
                "appBrand": "chatgpt"
            }),
            PendingMethod::BrowserLogin,
        )
    }

    pub fn start_device_code_login(&mut self) -> Result<RpcRequest, BackendError> {
        self.ready_request(
            "account/login/start",
            json!({ "type": "chatgptDeviceCode" }),
            PendingMethod::DeviceCodeLogin,
        )
    }

    pub fn cancel_login(
        &mut self,
        login_id: impl Into<String>,
    ) -> Result<RpcRequest, BackendError> {
        self.ready_request(
            "account/login/cancel",
            json!({ "loginId": login_id.into() }),
            PendingMethod::CancelLogin,
        )
    }

    pub fn logout(&mut self) -> Result<RpcRequest, BackendError> {
        self.ready_request("account/logout", json!({}), PendingMethod::Logout)
    }

    pub fn start_thread(&mut self, cwd: &str) -> Result<RpcRequest, BackendError> {
        self.ready_request(
            "thread/start",
            json!({
                "cwd": cwd,
                "approvalPolicy": "on-request",
                "sandbox": "readOnly",
                "serviceName": "lyrnova"
            }),
            PendingMethod::ThreadStart,
        )
    }

    pub fn resume_thread(&mut self, thread_id: &str) -> Result<RpcRequest, BackendError> {
        self.ready_request(
            "thread/resume",
            json!({ "threadId": thread_id }),
            PendingMethod::ThreadResume,
        )
    }

    pub fn start_turn(
        &mut self,
        thread_id: &str,
        prompt: &str,
        cwd: &str,
    ) -> Result<RpcRequest, BackendError> {
        self.ready_request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt }],
                "cwd": cwd,
                "approvalPolicy": "on-request",
                "sandboxPolicy": { "type": "readOnly" }
            }),
            PendingMethod::TurnStart,
        )
    }

    pub fn complete_account_read(
        &mut self,
        response: RpcResponse,
    ) -> Result<Option<AccountSummary>, BackendError> {
        let result = self.complete(response, PendingMethod::AccountRead)?;
        account_from_read(&result)
    }

    pub fn complete_browser_login(
        &mut self,
        response: RpcResponse,
    ) -> Result<LoginChallenge, BackendError> {
        let result = self.complete(response, PendingMethod::BrowserLogin)?;
        login_challenge(&result, "chatgpt")
    }

    pub fn complete_device_code_login(
        &mut self,
        response: RpcResponse,
    ) -> Result<LoginChallenge, BackendError> {
        let result = self.complete(response, PendingMethod::DeviceCodeLogin)?;
        login_challenge(&result, "chatgptDeviceCode")
    }

    pub fn acknowledge(&mut self, response: RpcResponse) -> Result<(), BackendError> {
        let pending = self
            .pending
            .get(&response.id)
            .copied()
            .ok_or(BackendError::UnknownRequest)?;
        if !matches!(pending, PendingMethod::CancelLogin | PendingMethod::Logout) {
            return Err(BackendError::UnexpectedResponse);
        }
        self.complete(response, pending).map(|_| ())
    }

    pub fn complete_thread_start(
        &mut self,
        response: RpcResponse,
    ) -> Result<ThreadSummary, BackendError> {
        let result = self.complete(response, PendingMethod::ThreadStart)?;
        thread_summary(&result)
    }

    pub fn complete_thread_resume(
        &mut self,
        response: RpcResponse,
    ) -> Result<ThreadSummary, BackendError> {
        let result = self.complete(response, PendingMethod::ThreadResume)?;
        thread_summary(&result)
    }

    pub fn complete_turn_start(
        &mut self,
        response: RpcResponse,
    ) -> Result<TurnSummary, BackendError> {
        let result = self.complete(response, PendingMethod::TurnStart)?;
        let turn_id = result
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or(BackendError::InvalidMessage)?;
        Ok(TurnSummary {
            turn_id: turn_id.to_owned(),
        })
    }

    fn ready_request(
        &mut self,
        method: &str,
        params: Value,
        pending: PendingMethod,
    ) -> Result<RpcRequest, BackendError> {
        if self.state != ConnectionState::Ready {
            return Err(BackendError::InvalidState);
        }
        Ok(self.request(method, params, pending))
    }

    fn request(&mut self, method: &str, params: Value, pending: PendingMethod) -> RpcRequest {
        let id = RequestId::Number(self.next_request_id);
        self.next_request_id += 1;
        self.pending.insert(id.clone(), pending);
        RpcRequest {
            method: method.into(),
            id,
            params,
        }
    }

    fn complete(
        &mut self,
        response: RpcResponse,
        expected: PendingMethod,
    ) -> Result<Value, BackendError> {
        let pending = self
            .pending
            .remove(&response.id)
            .ok_or(BackendError::UnknownRequest)?;
        if pending != expected {
            return Err(BackendError::UnexpectedResponse);
        }
        if let Some(error) = response.error {
            return Err(BackendError::Remote {
                code: error.code,
                message: error.message,
            });
        }
        response.result.ok_or(BackendError::InvalidMessage)
    }
}

pub fn account_from_updated(params: &Value) -> Result<Option<AccountSummary>, BackendError> {
    let auth_mode = params
        .get("authMode")
        .and_then(Value::as_str)
        .map(auth_mode);
    let Some(auth_mode) = auth_mode else {
        return Ok(None);
    };

    Ok(Some(AccountSummary {
        auth_mode,
        email: None,
        plan_type: optional_string(params, "planType")?,
    }))
}

fn account_from_read(result: &Value) -> Result<Option<AccountSummary>, BackendError> {
    let Some(account) = result.get("account") else {
        return Err(BackendError::InvalidAccount);
    };
    if account.is_null() {
        return Ok(None);
    }
    let account_type = account
        .get("type")
        .and_then(Value::as_str)
        .ok_or(BackendError::InvalidAccount)?;

    Ok(Some(AccountSummary {
        auth_mode: auth_mode(account_type),
        email: optional_string(account, "email")?,
        plan_type: optional_string(account, "planType")?,
    }))
}

fn thread_summary(result: &Value) -> Result<ThreadSummary, BackendError> {
    let thread_id = result
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(BackendError::InvalidMessage)?;
    Ok(ThreadSummary {
        thread_id: thread_id.to_owned(),
    })
}

fn login_challenge(result: &Value, expected_type: &str) -> Result<LoginChallenge, BackendError> {
    if result.get("type").and_then(Value::as_str) != Some(expected_type) {
        return Err(BackendError::InvalidLoginChallenge);
    }
    let login_id = required_string(result, "loginId")?;

    match expected_type {
        "chatgpt" => Ok(LoginChallenge::Browser {
            login_id,
            auth_url: required_string(result, "authUrl")?,
        }),
        "chatgptDeviceCode" => Ok(LoginChallenge::DeviceCode {
            login_id,
            verification_url: required_string(result, "verificationUrl")?,
            user_code: required_string(result, "userCode")?,
        }),
        _ => Err(BackendError::InvalidLoginChallenge),
    }
}

fn auth_mode(value: &str) -> AuthMode {
    match value {
        "chatgpt" | "chatgptAuthTokens" => AuthMode::ChatGpt,
        "apiKey" | "apikey" => AuthMode::ApiKey,
        _ => AuthMode::Other,
    }
}

fn required_string(value: &Value, field: &str) -> Result<String, BackendError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.len() <= 4096)
        .map(str::to_owned)
        .ok_or(BackendError::InvalidLoginChallenge)
}

fn optional_string(value: &Value, field: &str) -> Result<Option<String>, BackendError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.len() <= 512 => Ok(Some(text.clone())),
        _ => Err(BackendError::InvalidAccount),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initialized_adapter() -> CodexAppServerAdapter {
        let mut adapter = CodexAppServerAdapter::default();
        let request = adapter.begin_connection("0.1.0").unwrap();
        let notification = adapter
            .finish_initialize(RpcResponse {
                id: request.id,
                result: Some(json!({ "userAgent": "codex" })),
                error: None,
            })
            .unwrap();
        assert_eq!(notification.method, "initialized");
        adapter
    }

    #[test]
    fn handshake_is_required_before_account_calls() {
        let mut adapter = CodexAppServerAdapter::default();
        assert_eq!(adapter.account_read(), Err(BackendError::InvalidState));

        let request = adapter.begin_connection("0.1.0").unwrap();
        assert_eq!(request.method, "initialize");
        assert_eq!(request.params["clientInfo"]["name"], "lyrnova");
        assert_eq!(adapter.state(), ConnectionState::Initializing);
    }

    #[test]
    fn browser_login_request_contains_no_credential() {
        let mut adapter = initialized_adapter();
        let request = adapter.start_browser_login().unwrap();
        let serialized = String::from_utf8(encode_frame(&request).unwrap()).unwrap();

        assert_eq!(request.method, "account/login/start");
        assert_eq!(request.params["type"], "chatgpt");
        assert!(!serialized.contains("token"));
        assert!(!serialized.contains("password"));
        assert!(serialized.ends_with('\n'));
    }

    #[test]
    fn account_read_exposes_only_ui_safe_identity() {
        let mut adapter = initialized_adapter();
        let request = adapter.account_read().unwrap();
        let account = adapter
            .complete_account_read(RpcResponse {
                id: request.id,
                result: Some(json!({
                    "account": {
                        "type": "chatgpt",
                        "email": "dev@example.com",
                        "planType": "plus",
                        "accessToken": "must-not-cross-the-boundary"
                    },
                    "requiresOpenaiAuth": true
                })),
                error: None,
            })
            .unwrap()
            .unwrap();

        assert_eq!(account.auth_mode, AuthMode::ChatGpt);
        assert_eq!(account.email.as_deref(), Some("dev@example.com"));
        assert_eq!(account.plan_type.as_deref(), Some("plus"));
        assert!(
            !serde_json::to_string(&account)
                .unwrap()
                .contains("accessToken")
        );
    }

    #[test]
    fn parses_browser_and_device_challenges() {
        let mut adapter = initialized_adapter();
        let browser = adapter.start_browser_login().unwrap();
        assert_eq!(
            adapter
                .complete_browser_login(RpcResponse {
                    id: browser.id,
                    result: Some(json!({
                        "type": "chatgpt",
                        "loginId": "login-browser",
                        "authUrl": "https://chatgpt.com/auth"
                    })),
                    error: None,
                })
                .unwrap(),
            LoginChallenge::Browser {
                login_id: "login-browser".into(),
                auth_url: "https://chatgpt.com/auth".into(),
            }
        );

        let device = adapter.start_device_code_login().unwrap();
        assert_eq!(
            adapter
                .complete_device_code_login(RpcResponse {
                    id: device.id,
                    result: Some(json!({
                        "type": "chatgptDeviceCode",
                        "loginId": "login-device",
                        "verificationUrl": "https://auth.openai.com/codex/device",
                        "userCode": "ABCD-1234"
                    })),
                    error: None,
                })
                .unwrap(),
            LoginChallenge::DeviceCode {
                login_id: "login-device".into(),
                verification_url: "https://auth.openai.com/codex/device".into(),
                user_code: "ABCD-1234".into(),
            }
        );
    }

    #[test]
    fn account_update_supports_sign_in_and_sign_out() {
        let signed_in = account_from_updated(&json!({
            "authMode": "chatgpt",
            "planType": "pro"
        }))
        .unwrap()
        .unwrap();
        assert_eq!(signed_in.auth_mode, AuthMode::ChatGpt);
        assert_eq!(signed_in.plan_type.as_deref(), Some("pro"));
        assert_eq!(
            account_from_updated(&json!({ "authMode": null })).unwrap(),
            None
        );
    }

    #[test]
    fn coding_turn_starts_with_read_only_sandbox_and_on_request_approvals() {
        let mut adapter = initialized_adapter();
        let thread_request = adapter.start_thread("/workspace/project").unwrap();
        assert_eq!(thread_request.params["approvalPolicy"], "on-request");
        assert_eq!(thread_request.params["sandbox"], "readOnly");
        let thread = adapter
            .complete_thread_start(RpcResponse {
                id: thread_request.id,
                result: Some(json!({ "thread": { "id": "thr_123" } })),
                error: None,
            })
            .unwrap();

        let turn_request = adapter
            .start_turn(
                &thread.thread_id,
                "Explain this project",
                "/workspace/project",
            )
            .unwrap();
        assert_eq!(turn_request.params["threadId"], "thr_123");
        assert_eq!(turn_request.params["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(turn_request.params["approvalPolicy"], "on-request");
        assert_eq!(
            turn_request.params["input"][0],
            json!({ "type": "text", "text": "Explain this project" })
        );
        let turn = adapter
            .complete_turn_start(RpcResponse {
                id: turn_request.id,
                result: Some(json!({
                    "turn": { "id": "turn_456", "status": "inProgress", "items": [] }
                })),
                error: None,
            })
            .unwrap();
        assert_eq!(turn.turn_id, "turn_456");
    }

    #[test]
    fn stored_thread_can_be_resumed() {
        let mut adapter = initialized_adapter();
        let request = adapter.resume_thread("thr_saved").unwrap();
        assert_eq!(request.method, "thread/resume");
        assert_eq!(request.params, json!({ "threadId": "thr_saved" }));
        assert_eq!(
            adapter
                .complete_thread_resume(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "thread": { "id": "thr_saved" } })),
                    error: None,
                })
                .unwrap()
                .thread_id,
            "thr_saved"
        );
    }

    #[test]
    fn jsonl_decoder_classifies_protocol_messages() {
        assert!(matches!(
            decode_frame(br#"{"id":7,"result":{}}"#).unwrap(),
            RpcInbound::Response(_)
        ));
        assert!(matches!(
            decode_frame(br#"{"method":"turn/started","params":{}}"#).unwrap(),
            RpcInbound::Notification(_)
        ));
        assert!(matches!(
            decode_frame(br#"{"method":"approval/request","id":9,"params":{}}"#).unwrap(),
            RpcInbound::ServerRequest(_)
        ));
    }

    #[test]
    fn jsonl_decoder_rejects_ambiguous_and_oversized_messages() {
        assert_eq!(
            decode_frame(br#"{"id":1,"result":{},"error":{"code":1,"message":"x"}}"#),
            Err(BackendError::InvalidMessage)
        );
        assert_eq!(
            decode_frame(&vec![b'x'; MAX_FRAME_BYTES + 1]),
            Err(BackendError::FrameTooLarge)
        );
    }

    #[test]
    fn remote_errors_are_not_treated_as_success() {
        let mut adapter = initialized_adapter();
        let request = adapter.account_read().unwrap();
        assert_eq!(
            adapter.complete_account_read(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError {
                    code: -32000,
                    message: "not authenticated".into(),
                }),
            }),
            Err(BackendError::Remote {
                code: -32000,
                message: "not authenticated".into(),
            })
        );
    }
}
