use std::io::{BufRead, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin_manifest::{PluginCapability, PluginPermission};

pub(crate) const EXTERNAL_PLUGIN_PROTOCOL_VERSION: u32 = 1;
pub(crate) const MAX_PLUGIN_FRAME_BYTES: usize = 256 * 1024;
const MAX_VALUE_DEPTH: usize = 16;
const MAX_COLLECTION_ITEMS: usize = 4_096;
const MAX_STRING_BYTES: usize = 128 * 1024;
const MAX_NAME_BYTES: usize = 128;
const MAX_ERROR_MESSAGE_BYTES: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum HostPluginFrame {
    Initialize {
        protocol_version: u32,
        plugin_id: String,
        plugin_version: String,
        capabilities: Vec<PluginCapability>,
        permissions: Vec<PluginPermission>,
    },
    Request {
        request_id: String,
        capability: PluginCapability,
        operation: String,
        payload: Value,
    },
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PluginHostFrame {
    Ready {
        protocol_version: u32,
        capabilities: Vec<PluginCapability>,
    },
    Response {
        request_id: String,
        capability: PluginCapability,
        result: Value,
    },
    Error {
        request_id: String,
        capability: PluginCapability,
        code: String,
        message: String,
    },
    Event {
        capability: PluginCapability,
        event: String,
        payload: Value,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginEvent {
    pub capability: PluginCapability,
    pub event: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginResponse {
    pub capability: PluginCapability,
    pub result: Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginProtocolError {
    Io,
    Closed,
    FrameTooLarge,
    InvalidFrame,
    InvalidValue,
}

pub(crate) fn write_host_frame(
    writer: &mut impl Write,
    frame: &HostPluginFrame,
) -> Result<(), PluginProtocolError> {
    validate_host_frame(frame)?;
    let bytes = serde_json::to_vec(frame).map_err(|_| PluginProtocolError::InvalidFrame)?;
    if bytes.len() > MAX_PLUGIN_FRAME_BYTES {
        return Err(PluginProtocolError::FrameTooLarge);
    }
    writer
        .write_all(&bytes)
        .and_then(|()| writer.write_all(b"\n"))
        .and_then(|()| writer.flush())
        .map_err(|_| PluginProtocolError::Io)
}

pub(crate) fn read_plugin_frame(
    reader: &mut impl BufRead,
) -> Result<PluginHostFrame, PluginProtocolError> {
    let mut bytes = Vec::new();
    let read = (&mut *reader)
        .take(MAX_PLUGIN_FRAME_BYTES as u64 + 2)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| PluginProtocolError::Io)?;
    if read == 0 {
        return Err(PluginProtocolError::Closed);
    }
    if bytes.len() > MAX_PLUGIN_FRAME_BYTES + 1 || bytes.last() != Some(&b'\n') {
        return Err(PluginProtocolError::FrameTooLarge);
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.len() > MAX_PLUGIN_FRAME_BYTES {
        return Err(PluginProtocolError::InvalidFrame);
    }
    let frame = serde_json::from_slice(&bytes).map_err(|_| PluginProtocolError::InvalidFrame)?;
    validate_plugin_frame(&frame)?;
    Ok(frame)
}

fn validate_host_frame(frame: &HostPluginFrame) -> Result<(), PluginProtocolError> {
    match frame {
        HostPluginFrame::Initialize {
            plugin_id,
            plugin_version,
            capabilities,
            permissions,
            ..
        } => {
            if !valid_name(plugin_id)
                || plugin_version.is_empty()
                || plugin_version.len() > MAX_NAME_BYTES
                || contains_duplicate(capabilities)
                || contains_duplicate(permissions)
            {
                return Err(PluginProtocolError::InvalidFrame);
            }
        }
        HostPluginFrame::Request {
            capability,
            request_id,
            operation,
            payload,
        } => {
            if !valid_name(request_id)
                || !valid_name(operation)
                || !operation_matches(*capability, operation)
            {
                return Err(PluginProtocolError::InvalidFrame);
            }
            validate_value(payload, 0)?;
        }
        HostPluginFrame::Shutdown => {}
    }
    Ok(())
}

fn validate_plugin_frame(frame: &PluginHostFrame) -> Result<(), PluginProtocolError> {
    match frame {
        PluginHostFrame::Ready { capabilities, .. } => {
            if contains_duplicate(capabilities) {
                return Err(PluginProtocolError::InvalidFrame);
            }
        }
        PluginHostFrame::Response {
            request_id, result, ..
        } => {
            if !valid_name(request_id) {
                return Err(PluginProtocolError::InvalidFrame);
            }
            validate_value(result, 0)?;
        }
        PluginHostFrame::Error {
            request_id,
            code,
            message,
            ..
        } => {
            if !valid_name(request_id)
                || !valid_name(code)
                || message.is_empty()
                || message.len() > MAX_ERROR_MESSAGE_BYTES
                || message.contains('\0')
            {
                return Err(PluginProtocolError::InvalidFrame);
            }
        }
        PluginHostFrame::Event { event, payload, .. } => {
            if !valid_name(event) {
                return Err(PluginProtocolError::InvalidFrame);
            }
            validate_value(payload, 0)?;
        }
    }
    Ok(())
}

fn validate_value(value: &Value, depth: usize) -> Result<(), PluginProtocolError> {
    if depth > MAX_VALUE_DEPTH {
        return Err(PluginProtocolError::InvalidValue);
    }
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(number) if number.is_i64() || number.is_u64() => Ok(()),
        Value::Number(_) => Err(PluginProtocolError::InvalidValue),
        Value::String(value) => {
            if value.len() <= MAX_STRING_BYTES && !value.contains('\0') {
                Ok(())
            } else {
                Err(PluginProtocolError::InvalidValue)
            }
        }
        Value::Array(values) => {
            if values.len() > MAX_COLLECTION_ITEMS {
                return Err(PluginProtocolError::InvalidValue);
            }
            values
                .iter()
                .try_for_each(|value| validate_value(value, depth + 1))
        }
        Value::Object(values) => {
            if values.len() > MAX_COLLECTION_ITEMS
                || values.keys().any(|key| !valid_payload_key(key))
            {
                return Err(PluginProtocolError::InvalidValue);
            }
            values
                .values()
                .try_for_each(|value| validate_value(value, depth + 1))
        }
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_NAME_BYTES
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn valid_payload_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_NAME_BYTES && !value.contains('\0')
}

fn capability_name(capability: PluginCapability) -> &'static str {
    match capability {
        PluginCapability::SyntaxHighlighting => "syntax_highlighting",
        PluginCapability::Autocomplete => "autocomplete",
        PluginCapability::Diagnostics => "diagnostics",
        PluginCapability::Snippets => "snippets",
        PluginCapability::Templates => "templates",
        PluginCapability::Lsp => "lsp",
        PluginCapability::Dap => "dap",
        PluginCapability::Tasks => "tasks",
        PluginCapability::Tests => "tests",
        PluginCapability::AccountAuth => "account_auth",
        PluginCapability::AiChat => "ai_chat",
        PluginCapability::AiTools => "ai_tools",
        PluginCapability::Approvals => "approvals",
    }
}

pub(crate) fn operation_matches(capability: PluginCapability, operation: &str) -> bool {
    operation
        .strip_prefix(capability_name(capability))
        .is_some_and(|suffix| suffix.starts_with('.') && suffix.len() > 1)
}

fn contains_duplicate<T: Ord + Copy>(values: &[T]) -> bool {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.windows(2).any(|pair| pair[0] == pair[1])
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn host_frames_are_jsonl_and_keep_the_capability_boundary() {
        let mut output = Vec::new();
        write_host_frame(
            &mut output,
            &HostPluginFrame::Request {
                request_id: "request-1".into(),
                capability: PluginCapability::Tasks,
                operation: "tasks.list".into(),
                payload: serde_json::json!({ "workspace": "/workspace" }),
            },
        )
        .unwrap();
        assert!(output.ends_with(b"\n"));
        let value: Value = serde_json::from_slice(&output[..output.len() - 1]).unwrap();
        assert_eq!(value["type"], "request");
        assert_eq!(value["capability"], "tasks");
        assert_eq!(value["operation"], "tasks.list");

        assert_eq!(
            write_host_frame(
                &mut Vec::new(),
                &HostPluginFrame::Request {
                    request_id: "request-2".into(),
                    capability: PluginCapability::Tasks,
                    operation: "diagnostics.read".into(),
                    payload: serde_json::json!({}),
                },
            ),
            Err(PluginProtocolError::InvalidFrame)
        );
    }

    #[test]
    fn plugin_frames_reject_unknown_fields_duplicates_floats_and_oversize() {
        for invalid in [
            b"{\"type\":\"ready\",\"protocol_version\":1,\"capabilities\":[\"tasks\"],\"extra\":true}\n".as_slice(),
            b"{\"type\":\"ready\",\"protocol_version\":1,\"capabilities\":[\"tasks\",\"tasks\"]}\n".as_slice(),
            b"{\"type\":\"event\",\"capability\":\"tasks\",\"event\":\"task.output\",\"payload\":1.5}\n".as_slice(),
        ] {
            assert!(read_plugin_frame(&mut BufReader::new(Cursor::new(invalid))).is_err());
        }

        let oversized = vec![b'x'; MAX_PLUGIN_FRAME_BYTES + 2];
        assert_eq!(
            read_plugin_frame(&mut BufReader::new(Cursor::new(oversized))),
            Err(PluginProtocolError::FrameTooLarge)
        );
    }

    #[test]
    fn plugin_response_and_event_frames_are_strictly_typed() {
        let response = b"{\"type\":\"response\",\"request_id\":\"request-1\",\"capability\":\"diagnostics\",\"result\":{\"items\":[]}}\n";
        assert_eq!(
            read_plugin_frame(&mut BufReader::new(Cursor::new(response))).unwrap(),
            PluginHostFrame::Response {
                request_id: "request-1".into(),
                capability: PluginCapability::Diagnostics,
                result: serde_json::json!({ "items": [] }),
            }
        );

        let event = b"{\"type\":\"event\",\"capability\":\"tasks\",\"event\":\"task.output\",\"payload\":{\"chunk\":\"ok\"}}\n";
        assert_eq!(
            read_plugin_frame(&mut BufReader::new(Cursor::new(event))).unwrap(),
            PluginHostFrame::Event {
                capability: PluginCapability::Tasks,
                event: "task.output".into(),
                payload: serde_json::json!({ "chunk": "ok" }),
            }
        );
    }

    #[test]
    fn nested_payloads_are_bounded() {
        let mut value = Value::Null;
        for _ in 0..=MAX_VALUE_DEPTH {
            value = serde_json::json!({ "nested": value });
        }
        assert_eq!(
            validate_value(&value, 0),
            Err(PluginProtocolError::InvalidValue)
        );
    }
}
