use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

use serde::Serialize;
use tauri::{Emitter, WebviewWindow};

#[cfg(target_os = "linux")]
mod pty;

const MAX_INPUT_BYTES: usize = 16 * 1024;
const MAX_PENDING_INPUT: usize = 128 * 1024;
const MAX_DIMENSION: u16 = 500;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum TerminalError {
    NotRunning,
    StaleSession,
    InvalidInput,
    InvalidSize,
    Busy,
    Unsupported,
    ProcessFailed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSummary {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum TerminalEvent {
    Output {
        session_id: String,
        sequence: u64,
        data: Vec<u8>,
    },
    Exit {
        session_id: String,
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
}

enum TerminalCommand {
    Input(Vec<u8>),
    Resize(u16, u16),
    Ack(u64),
}

struct TerminalSession {
    summary: TerminalSummary,
    commands: mpsc::SyncSender<TerminalCommand>,
    stop: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    pending_input: Arc<AtomicUsize>,
    done: mpsc::Receiver<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Polling and writes are nonblocking. Never hang a UI operation on an
        // uninterruptible kernel process; the worker retains ownership/reaping.
        let _ = self.done.recv_timeout(std::time::Duration::from_secs(1));
        if let Some(worker) = self.worker.take().filter(|worker| worker.is_finished()) {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
pub struct TerminalService {
    session: Mutex<Option<TerminalSession>>,
}

impl TerminalService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(
        &self,
        root: &Path,
        cols: u16,
        rows: u16,
        window: WebviewWindow,
    ) -> Result<TerminalSummary, TerminalError> {
        self.start_with_sink(root, cols, rows, move |event| {
            window.emit("terminal-event", event).is_ok()
        })
    }

    fn start_with_sink(
        &self,
        root: &Path,
        cols: u16,
        rows: u16,
        sink: impl Fn(TerminalEvent) -> bool + Send + 'static,
    ) -> Result<TerminalSummary, TerminalError> {
        validate_size(cols, rows)?;
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (root, sink);
            Err(TerminalError::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            let mut session = self
                .session
                .lock()
                .map_err(|_| TerminalError::ProcessFailed)?;
            if let Some(existing) = session
                .as_ref()
                .filter(|value| value.alive.load(Ordering::Acquire))
            {
                return Ok(existing.summary.clone());
            }
            drop(session.take());
            let process = pty::PtyProcess::spawn(root, cols, rows)
                .map_err(|_| TerminalError::ProcessFailed)?;
            let summary = TerminalSummary {
                session_id: uuid::Uuid::new_v4().to_string(),
                cols,
                rows,
            };
            let (commands, receiver) = mpsc::sync_channel(64);
            let stop = Arc::new(AtomicBool::new(false));
            let alive = Arc::new(AtomicBool::new(true));
            let pending_input = Arc::new(AtomicUsize::new(0));
            let (done_tx, done) = mpsc::channel();
            let (worker_stop, worker_alive, worker_input) =
                (stop.clone(), alive.clone(), pending_input.clone());
            let id = summary.session_id.clone();
            let worker = thread::spawn(move || {
                process.run(id, receiver, worker_stop, worker_input, sink);
                worker_alive.store(false, Ordering::Release);
                let _ = done_tx.send(());
            });
            *session = Some(TerminalSession {
                summary: summary.clone(),
                commands,
                stop,
                alive,
                pending_input,
                done,
                worker: Some(worker),
            });
            Ok(summary)
        }
    }

    pub fn write(&self, session_id: &str, input: &[u8]) -> Result<(), TerminalError> {
        if input.is_empty() || input.len() > MAX_INPUT_BYTES {
            return Err(TerminalError::InvalidInput);
        }
        self.with_session(session_id, |session| {
            // Explicit CAS loop: `fetch_update` is deprecated on newer stable
            // toolchains, while its `try_update` replacement exceeds the MSRV.
            let mut current = session.pending_input.load(Ordering::Acquire);
            loop {
                let next = current
                    .checked_add(input.len())
                    .filter(|next| *next <= MAX_PENDING_INPUT)
                    .ok_or(TerminalError::Busy)?;
                match session.pending_input.compare_exchange_weak(
                    current,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
            if session
                .commands
                .try_send(TerminalCommand::Input(input.to_vec()))
                .is_err()
            {
                session
                    .pending_input
                    .fetch_sub(input.len(), Ordering::AcqRel);
                return Err(TerminalError::Busy);
            }
            Ok(())
        })
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), TerminalError> {
        validate_size(cols, rows)?;
        self.with_session(session_id, |session| {
            session
                .commands
                .try_send(TerminalCommand::Resize(cols, rows))
                .map_err(|_| TerminalError::Busy)
        })
    }

    pub fn ack(&self, session_id: &str, sequence: u64) -> Result<(), TerminalError> {
        self.with_session(session_id, |session| {
            session
                .commands
                .try_send(TerminalCommand::Ack(sequence))
                .map_err(|_| TerminalError::Busy)
        })
    }

    fn with_session(
        &self,
        id: &str,
        action: impl FnOnce(&TerminalSession) -> Result<(), TerminalError>,
    ) -> Result<(), TerminalError> {
        let session = self
            .session
            .lock()
            .map_err(|_| TerminalError::ProcessFailed)?;
        let session = session.as_ref().ok_or(TerminalError::NotRunning)?;
        if session.summary.session_id != id {
            return Err(TerminalError::StaleSession);
        }
        if !session.alive.load(Ordering::Acquire) {
            return Err(TerminalError::NotRunning);
        }
        action(session)
    }

    pub fn stop(&self) -> Result<(), TerminalError> {
        drop(
            self.session
                .lock()
                .map_err(|_| TerminalError::ProcessFailed)?
                .take(),
        );
        Ok(())
    }

    pub fn stop_session(&self, session_id: &str) -> Result<(), TerminalError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| TerminalError::ProcessFailed)?;
        let current = session.as_ref().ok_or(TerminalError::NotRunning)?;
        if current.summary.session_id != session_id {
            return Err(TerminalError::StaleSession);
        }
        drop(session.take());
        Ok(())
    }
}

fn validate_size(cols: u16, rows: u16) -> Result<(), TerminalError> {
    if !(2..=MAX_DIMENSION).contains(&cols) || !(1..=MAX_DIMENSION).contains(&rows) {
        Err(TerminalError::InvalidSize)
    } else {
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
