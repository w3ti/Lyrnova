use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::Mutex,
    thread,
};

use serde::Serialize;
use tauri::{Emitter, WebviewWindow};

const MAX_INPUT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum TerminalError {
    AlreadyRunning,
    NotRunning,
    InvalidInput,
    ProcessFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalOutput {
    stream: &'static str,
    data: String,
}

struct TerminalSession {
    child: Child,
    stdin: ChildStdin,
}

pub struct TerminalService {
    session: Mutex<Option<TerminalSession>>,
}

impl Default for TerminalService {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalService {
    pub fn new() -> Self {
        Self {
            session: Mutex::new(None),
        }
    }

    pub fn start(&self, root: &Path, window: WebviewWindow) -> Result<(), TerminalError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| TerminalError::ProcessFailed)?;
        if let Some(existing) = session.as_mut() {
            if existing.child.try_wait().ok().flatten().is_none() {
                return Err(TerminalError::AlreadyRunning);
            }
            *session = None;
        }

        let mut child = Command::new("/bin/bash")
            .args(["--noprofile", "--norc"])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| TerminalError::ProcessFailed)?;
        let stdin = child.stdin.take().ok_or(TerminalError::ProcessFailed)?;
        let stdout = child.stdout.take().ok_or(TerminalError::ProcessFailed)?;
        let stderr = child.stderr.take().ok_or(TerminalError::ProcessFailed)?;
        stream_output(stdout, "stdout", window.clone());
        stream_output(stderr, "stderr", window);
        *session = Some(TerminalSession { child, stdin });
        Ok(())
    }

    pub fn write_line(&self, input: &str) -> Result<(), TerminalError> {
        validate_input(input)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| TerminalError::ProcessFailed)?;
        let terminal = session.as_mut().ok_or(TerminalError::NotRunning)?;
        terminal
            .stdin
            .write_all(input.as_bytes())
            .and_then(|_| terminal.stdin.write_all(b"\n"))
            .and_then(|_| terminal.stdin.flush())
            .map_err(|_| TerminalError::ProcessFailed)
    }

    pub fn stop(&self) -> Result<(), TerminalError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| TerminalError::ProcessFailed)?;
        let Some(mut terminal) = session.take() else {
            return Ok(());
        };
        let _ = terminal.child.kill();
        let _ = terminal.child.wait();
        Ok(())
    }
}

impl Drop for TerminalService {
    fn drop(&mut self) {
        if let Ok(session) = self.session.get_mut()
            && let Some(terminal) = session.as_mut()
        {
            let _ = terminal.child.kill();
            let _ = terminal.child.wait();
        }
    }
}

fn validate_input(input: &str) -> Result<(), TerminalError> {
    if input.len() > MAX_INPUT_BYTES || input.contains(['\0', '\n', '\r']) {
        Err(TerminalError::InvalidInput)
    } else {
        Ok(())
    }
}

fn stream_output(
    mut reader: impl Read + Send + 'static,
    stream: &'static str,
    window: WebviewWindow,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(bytes) = reader.read(&mut buffer) {
            if bytes == 0 {
                break;
            }
            let data = String::from_utf8_lossy(&buffer[..bytes]).into_owned();
            let _ = window.emit("terminal-output", TerminalOutput { stream, data });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_accepts_one_explicit_line_at_a_time() {
        assert_eq!(validate_input("cargo test --offline"), Ok(()));
        assert_eq!(
            validate_input("printf ok\nprintf bad"),
            Err(TerminalError::InvalidInput)
        );
        assert_eq!(
            validate_input("bad\0input"),
            Err(TerminalError::InvalidInput)
        );
    }
}
