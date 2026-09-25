use super::*;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Harness {
    service: TerminalService,
    root: PathBuf,
    id: String,
    events: mpsc::Receiver<TerminalEvent>,
}

impl Harness {
    fn paused() -> Self {
        let root = std::env::temp_dir().join(format!("lyrnova-pty-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let service = TerminalService::new();
        let (sender, events) = mpsc::channel();
        let session = service
            .start_with_sink(&root, 80, 24, move |event| sender.send(event).is_ok())
            .unwrap();
        Self {
            service,
            root,
            id: session.session_id,
            events,
        }
    }

    fn ready() -> Self {
        let harness = Self::paused();
        harness.service.ack(&harness.id, 0).unwrap();
        harness.send(b"stty -echo; PS1=''; printf '\\nREADY\\n'\r");
        harness.until_output("\r\nREADY\r\n");
        harness
    }

    fn send(&self, data: &[u8]) {
        self.service.write(&self.id, data).unwrap();
    }

    fn until_output(&self, expected: &str) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut bytes = Vec::new();
        while Instant::now() < deadline {
            if let Ok(event) = self.events.recv_timeout(Duration::from_millis(50)) {
                match event {
                    TerminalEvent::Output {
                        session_id,
                        sequence,
                        data,
                    } => {
                        assert_eq!(session_id, self.id);
                        bytes.extend(data);
                        let _ = self.service.ack(&self.id, sequence);
                        if String::from_utf8_lossy(&bytes).contains(expected) {
                            return bytes;
                        }
                    }
                    TerminalEvent::Exit { .. } => panic!("PTY exited before expected output"),
                }
            }
        }
        panic!(
            "Missing output {expected:?}: {:?}",
            String::from_utf8_lossy(&bytes)
        );
    }

    fn file(&self, name: &str) -> String {
        for _ in 0..250 {
            if let Ok(value) = fs::read_to_string(self.root.join(name))
                && !value.is_empty()
            {
                return value;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("missing fixture {name}");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.service.stop();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn running(pid: &str) -> bool {
    fs::read_to_string(format!("/proc/{}/stat", pid.trim()))
        .is_ok_and(|stat| stat.rsplit_once(')').unwrap().1.split_whitespace().next() != Some("Z"))
}

fn assert_stopped(pid: &str) {
    for _ in 0..100 {
        if !running(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("PTY process {pid} survived cleanup");
}

#[test]
fn pty_is_interactive_resizes_and_preserves_unicode_and_ansi() {
    let harness = Harness::ready();
    harness.send(
        b"test -t 0 && test -t 1 && test -t 2 && printf 'TTY-OK\\n'; stty size > initial-size\r",
    );
    harness.until_output("TTY-OK\r\n");
    assert_eq!(harness.file("initial-size").trim(), "24 80");
    harness.service.resize(&harness.id, 100, 32).unwrap();
    harness.send("stty size > resized; printf '\\033[31mOlá 世界\\033[0m\\n'\r".as_bytes());
    harness.until_output("\u{1b}[31mOlá 世界\u{1b}[0m\r\n");
    assert_eq!(harness.file("resized").trim(), "32 100");
}

#[test]
fn ctrl_c_interrupts_foreground_job_and_ctrl_d_exits_shell() {
    let harness = Harness::ready();
    harness.send(b"sh -c 'echo $$ > foreground.pid; exec sleep 30'\r");
    let pid = harness.file("foreground.pid");
    assert!(running(&pid));
    harness.send(b"\x03");
    harness.send(b"printf 'INTERRUPTED\\n'\r");
    harness.until_output("INTERRUPTED\r\n");
    assert_stopped(&pid);
    harness.send(b"\x04");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match harness.events.recv_timeout(Duration::from_millis(50)) {
            Ok(TerminalEvent::Exit { exit_code, .. }) => {
                assert_eq!(exit_code, Some(0));
                return;
            }
            Ok(TerminalEvent::Output { sequence, .. }) => {
                let _ = harness.service.ack(&harness.id, sequence);
            }
            Err(_) => {}
        }
    }
    panic!("Ctrl+D did not close the PTY");
}

#[test]
fn stop_kills_foreground_and_background_job_groups() {
    let harness = Harness::ready();
    let mut unrelated = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    harness.send(
        b"sleep 30 & echo $! > background.pid; sh -c 'echo $$ > foreground.pid; exec sleep 30'\r",
    );
    let background = harness.file("background.pid");
    let foreground = harness.file("foreground.pid");
    assert!(running(&background) && running(&foreground));
    harness.service.stop().unwrap();
    let unrelated_survived = unrelated.try_wait().unwrap().is_none();
    let _ = unrelated.kill();
    let _ = unrelated.wait();
    assert!(
        unrelated_survived,
        "terminal cleanup affected an unrelated process"
    );
    assert_stopped(&background);
    assert_stopped(&foreground);
}

#[test]
fn exited_shell_does_not_leave_background_job_running() {
    let harness = Harness::ready();
    harness.send(b"sleep 30 & echo $! > background.pid; disown; exit\r");
    let background = harness.file("background.pid");
    assert_stopped(&background);
}

#[test]
fn paused_session_bounds_input_and_rejects_invalid_sizes_and_stale_tokens() {
    let harness = Harness::paused();
    assert_eq!(
        harness
            .service
            .write(&harness.id, &vec![0; MAX_INPUT_BYTES + 1]),
        Err(TerminalError::InvalidInput)
    );
    assert_eq!(
        harness.service.resize(&harness.id, 0, 10),
        Err(TerminalError::InvalidSize)
    );
    assert_eq!(
        harness.service.resize(&harness.id, 80, 501),
        Err(TerminalError::InvalidSize)
    );
    for _ in 0..MAX_PENDING_INPUT / MAX_INPUT_BYTES {
        harness.send(&vec![b'x'; MAX_INPUT_BYTES]);
    }
    assert_eq!(
        harness.service.write(&harness.id, b"x"),
        Err(TerminalError::Busy)
    );
    assert!(
        harness
            .events
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
    harness.service.stop().unwrap();
    let fresh = harness
        .service
        .start_with_sink(&harness.root, 80, 24, |_| true)
        .unwrap();
    assert_ne!(fresh.session_id, harness.id);
    assert_eq!(
        harness.service.write(&harness.id, b"bad\r"),
        Err(TerminalError::StaleSession)
    );
    assert_eq!(
        harness.service.resize(&harness.id, 80, 20),
        Err(TerminalError::StaleSession)
    );
    assert_eq!(
        harness.service.ack(&harness.id, 1),
        Err(TerminalError::StaleSession)
    );
    assert_eq!(
        harness.service.stop_session(&harness.id),
        Err(TerminalError::StaleSession)
    );
    assert!(harness.service.ack(&fresh.session_id, 0).is_ok());
}

#[test]
fn output_window_applies_backpressure_and_stop_remains_responsive() {
    let harness = Harness::ready();
    harness.send(b"yes\r");
    let mut output = 0;
    let mut bytes = 0;
    while let Ok(TerminalEvent::Output { data, .. }) =
        harness.events.recv_timeout(Duration::from_millis(200))
    {
        bytes += data.len();
        output += 1;
    }
    assert!(output > 0 && output <= 8);
    assert!(bytes <= 64 * 1024);
    let started = Instant::now();
    harness.service.stop().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
}
