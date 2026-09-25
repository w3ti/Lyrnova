use super::{
    LanguageDocument, LanguageError, State, configuration, initialization, parse_diagnostics,
    protocol::{Decoder, frame},
    uri,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn server_path(root: &Path) -> Result<PathBuf, LanguageError> {
    // Only standalone ELF servers from absolute host PATH entries, never project tools
    // or rustup shims (which could download/execute a workspace-selected toolchain).
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if !directory.is_absolute() {
            continue;
        }
        let Ok(path) = directory.join("rust-analyzer").canonicalize() else {
            continue;
        };
        if path.starts_with(root) || path.file_name().is_some_and(|n| n == "rustup") {
            continue;
        }
        if !fs::metadata(&path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0) {
            continue;
        }
        let mut magic = [0; 4];
        if File::open(&path)
            .and_then(|mut f| f.read_exact(&mut magic))
            .is_ok()
            && magic == *b"\x7fELF"
        {
            return Ok(path);
        }
    }
    Err(LanguageError::ServerUnavailable)
}

fn configuration_masks(root: &Path) -> Result<Vec<PathBuf>, LanguageError> {
    // Workspace configuration has precedence over LSP settings in rust-analyzer.
    // Hide existing overrides in source directories from this diagnostics-only adapter.
    let mut pending = vec![root.to_path_buf()];
    let mut masks = Vec::new();
    let mut visited = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|_| LanguageError::InvalidDocument)? {
            let entry = entry.map_err(|_| LanguageError::InvalidDocument)?;
            visited += 1;
            if visited > 50_000 {
                return Err(LanguageError::TooLarge);
            }
            let kind = entry
                .file_type()
                .map_err(|_| LanguageError::InvalidDocument)?;
            let name = entry.file_name();
            if name == "rust-analyzer.toml" {
                if !kind.is_file() {
                    return Err(LanguageError::InvalidDocument);
                }
                masks.push(entry.path());
                if masks.len() > 256 {
                    return Err(LanguageError::TooLarge);
                }
            } else if kind.is_dir()
                && ![".git", "target", "node_modules"]
                    .iter()
                    .any(|ignored| name == *ignored)
            {
                pending.push(entry.path());
            }
        }
    }
    Ok(masks)
}

pub(super) fn spawn(root: &Path) -> Result<Child, LanguageError> {
    let server = server_path(root)?;
    let masks = configuration_masks(root)?;
    if !Path::new("/usr/bin/bwrap").is_file() {
        return Err(LanguageError::SandboxUnavailable);
    }
    let mut command = Command::new("/usr/bin/bwrap");
    command.env_clear().args([
        "--die-with-parent",
        "--new-session",
        "--unshare-all",
        "--cap-drop",
        "ALL",
        "--clearenv",
    ]);
    for (key, value) in [
        ("HOME", "/tmp/home"),
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "C.UTF-8"),
        ("CARGO_HOME", "/tmp/cargo"),
        ("CARGO_TARGET_DIR", "/tmp/target"),
        ("CARGO_NET_OFFLINE", "true"),
    ] {
        command.args(["--setenv", key, value]);
    }
    for path in ["/usr", "/bin", "/lib", "/lib64", "/etc/ld.so.cache"] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    command
        .args([
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--dir",
            "/tmp/home",
            "--dir",
            "/tmp/cargo",
            "--dir",
            "/server",
        ])
        .arg("--ro-bind")
        .arg(server)
        .arg("/server/rust-analyzer")
        .arg("--ro-bind")
        .arg(root)
        .arg("/workspace");
    for path in masks {
        let target = Path::new("/workspace").join(
            path.strip_prefix(root)
                .map_err(|_| LanguageError::InvalidDocument)?,
        );
        command.args(["--ro-bind", "/dev/null"]).arg(target);
    }
    command
        .args(["--chdir", "/workspace", "--", "/server/rust-analyzer"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe libc calls execute between fork and exec.
    unsafe {
        command.pre_exec(|| {
            for (resource, limit) in [
                (libc::RLIMIT_CORE, 0),
                (libc::RLIMIT_NOFILE, 256),
                (libc::RLIMIT_FSIZE, 64 * 1024 * 1024),
                (libc::RLIMIT_AS, 4 * 1024 * 1024 * 1024),
            ] {
                let limits = libc::rlimit {
                    rlim_cur: limit,
                    rlim_max: limit,
                };
                if libc::setrlimit(resource, &limits) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().map_err(|_| LanguageError::SpawnFailed)
}

fn nonblocking(fd: i32) -> Result<(), LanguageError> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(LanguageError::SpawnFailed);
    }
    Ok(())
}

#[derive(Default)]
struct Output {
    queue: VecDeque<Vec<u8>>,
    offset: usize,
    bytes: usize,
}
impl Output {
    fn push(&mut self, message: Value) -> Result<(), LanguageError> {
        let bytes = frame(&message)?;
        if self.bytes + bytes.len() > 16 * 1024 * 1024 {
            return Err(LanguageError::TooLarge);
        }
        self.bytes += bytes.len();
        self.queue.push_back(bytes);
        Ok(())
    }
    fn flush(&mut self, stdin: &mut ChildStdin) -> Result<(), LanguageError> {
        for _ in 0..64 {
            let Some(bytes) = self.queue.front() else {
                break;
            };
            match stdin.write(&bytes[self.offset..]) {
                Ok(0) => return Err(LanguageError::ServerExited),
                Ok(n) => {
                    self.offset += n;
                    self.bytes -= n;
                    if self.offset == bytes.len() {
                        self.queue.pop_front();
                        self.offset = 0;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(LanguageError::ServerExited),
            }
        }
        Ok(())
    }
}

fn respond(message: &Value, output: &mut Output) -> Result<(), LanguageError> {
    let id = &message["id"];
    if !(id.is_i64() || id.as_str().is_some_and(|s| s.len() <= 128)) {
        return Err(LanguageError::ProtocolViolation);
    }
    match message["method"].as_str() {
        Some("workspace/configuration") => {
            let items = message["params"]["items"].as_array().filter(|v| v.len() <= 64).ok_or(LanguageError::ProtocolViolation)?;
            let config = configuration();
            let values: Vec<_> = items.iter().map(|item| {
                let section = item["section"].as_str().unwrap_or("rust-analyzer");
                if section == "rust-analyzer" { return config.clone(); }
                section.strip_prefix("rust-analyzer.").map(|key| key.split('.').fold(&config, |value, part| &value[part]).clone()).unwrap_or(Value::Null)
            }).collect();
            output.push(json!({"jsonrpc":"2.0", "id":id, "result":values}))
        }
        Some("window/workDoneProgress/create") => output.push(json!({"jsonrpc":"2.0", "id":id, "result":null})),
        Some("workspace/applyEdit") => output.push(json!({"jsonrpc":"2.0", "id":id, "result":{"applied":false, "failureReason":"Read-only diagnostics client"}})),
        _ => output.push(json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601, "message":"Unsupported operation"}})),
    }
}

fn handle(
    message: Value,
    initialized: &mut bool,
    state: &Arc<Mutex<State>>,
    output: &mut Output,
) -> Result<(), LanguageError> {
    if message.get("method").is_some() && message.get("id").is_some() {
        return respond(&message, output);
    }
    if message.get("method").is_none() && message.get("id") == Some(&json!(1)) {
        if *initialized || message.get("error").is_some() {
            return Err(LanguageError::ProtocolViolation);
        }
        let caps = &message["result"]["capabilities"];
        if !caps.is_object() || caps.get("positionEncoding").is_some_and(|v| v != "utf-16") {
            return Err(LanguageError::ProtocolViolation);
        }
        let sync = caps["textDocumentSync"]
            .as_u64()
            .or_else(|| caps["textDocumentSync"]["change"].as_u64());
        if !matches!(sync, Some(1 | 2)) {
            return Err(LanguageError::ProtocolViolation);
        }
        output.push(json!({"jsonrpc":"2.0", "method":"initialized", "params":{}}))?;
        *initialized = true;
        state
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?
            .snapshot
            .state = "running".into();
    } else if *initialized && message["method"] == "textDocument/publishDiagnostics" {
        let mut state = state.lock().map_err(|_| LanguageError::StateUnavailable)?;
        if let Some(mut diagnostics) = parse_diagnostics(&message["params"], &state.documents) {
            state
                .snapshot
                .diagnostics
                .retain(|d| d.path != diagnostics.path);
            let count: usize = state
                .snapshot
                .diagnostics
                .iter()
                .map(|d| d.items.len())
                .sum();
            let remaining = 1000_usize.saturating_sub(count);
            if diagnostics.items.len() > remaining {
                diagnostics.items.truncate(remaining);
                diagnostics.truncated = true;
            }
            state.snapshot.diagnostics.push(diagnostics);
        }
    }
    Ok(())
}

fn synchronize(
    state: &Arc<Mutex<State>>,
    sent: &mut BTreeMap<String, Arc<LanguageDocument>>,
    output: &mut Output,
) -> Result<(), LanguageError> {
    let docs = state
        .lock()
        .map_err(|_| LanguageError::StateUnavailable)?
        .documents
        .clone();
    for path in sent.keys().filter(|p| !docs.contains_key(*p)) {
        output.push(json!({"jsonrpc":"2.0", "method":"textDocument/didClose", "params":{"textDocument":{"uri":uri(path)}}}))?;
    }
    for (path, doc) in &docs {
        match sent.get(path) {
            None => output.push(json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument":{"uri":uri(path), "languageId":"rust", "version":doc.version, "text":doc.text}}}))?,
            Some(old) if old.version != doc.version => output.push(json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{"textDocument":{"uri":uri(path), "version":doc.version}, "contentChanges":[{"text":doc.text}]}}))?,
            _ => {}
        }
    }
    *sent = docs;
    Ok(())
}

fn session_loop(
    child: &mut Child,
    stdin: &mut ChildStdin,
    stdout: &mut ChildStdout,
    state: &Arc<Mutex<State>>,
    cancel: &AtomicBool,
) -> Result<(), LanguageError> {
    nonblocking(stdin.as_raw_fd())?;
    nonblocking(stdout.as_raw_fd())?;
    let mut output = Output::default();
    output.push(initialization())?;
    let mut decoder = Decoder::default();
    let mut initialized = false;
    let started = Instant::now();
    let mut stalled = Instant::now();
    let mut sent = BTreeMap::new();
    let mut buffer = [0; 8192];
    let mut last_input = Instant::now();
    while !cancel.load(Ordering::Acquire) {
        if !initialized && started.elapsed() > Duration::from_secs(15) {
            return Err(LanguageError::Timeout);
        }
        if output.bytes > 0 && stalled.elapsed() > Duration::from_secs(10) {
            return Err(LanguageError::Timeout);
        }
        if initialized && output.queue.is_empty() {
            synchronize(state, &mut sent, &mut output)?;
        }
        let before = output.bytes;
        output.flush(stdin)?;
        if output.bytes == 0 || output.bytes < before {
            stalled = Instant::now();
        }
        for _ in 0..32 {
            match stdout.read(&mut buffer) {
                Ok(0) => {
                    return Err(if initialized {
                        LanguageError::ServerExited
                    } else {
                        LanguageError::SandboxUnavailable
                    });
                }
                Ok(n) => {
                    decoder.push(&buffer[..n])?;
                    last_input = Instant::now();
                    while let Some(message) = decoder.next()? {
                        handle(message, &mut initialized, state, &mut output)?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(LanguageError::ServerExited),
            }
        }
        if decoder.pending() && last_input.elapsed() > Duration::from_secs(10) {
            return Err(LanguageError::Timeout);
        }
        if child
            .try_wait()
            .map_err(|_| LanguageError::ServerExited)?
            .is_some()
        {
            return Err(LanguageError::ServerExited);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Complete any partially written frame, then request shutdown; never wait indefinitely.
    let deadline = Instant::now() + Duration::from_millis(200);
    output.push(json!({"jsonrpc":"2.0", "id":2, "method":"shutdown", "params":null}))?;
    while Instant::now() < deadline {
        output.flush(stdin)?;
        match stdout.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                decoder.push(&buffer[..n])?;
                while let Some(message) = decoder.next()? {
                    if message.get("method").is_none() && message["id"] == 2 {
                        output.push(json!({"jsonrpc":"2.0", "method":"exit"}))?;
                        output.flush(stdin)?;
                        return Ok(());
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

pub(super) fn run(mut child: Child, state: Arc<Mutex<State>>, cancel: Arc<AtomicBool>) {
    let result = match (child.stdin.take(), child.stdout.take()) {
        (Some(mut stdin), Some(mut stdout)) => {
            session_loop(&mut child, &mut stdin, &mut stdout, &state, &cancel)
        }
        _ => Err(LanguageError::SpawnFailed),
    };
    let _ = child.kill();
    let _ = child.wait();
    if let Ok(mut state) = state.lock() {
        state.snapshot.diagnostics.clear();
        state.snapshot.state = if cancel.load(Ordering::Acquire) {
            "stopped"
        } else {
            "error"
        }
        .into();
        state.snapshot.error = result.err();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_configuration_is_masked_without_following_links() {
        let root =
            std::env::temp_dir().join(format!("lyrnova-lsp-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("member")).unwrap();
        fs::write(root.join("rust-analyzer.toml"), "checkOnSave = true").unwrap();
        fs::write(
            root.join("member/rust-analyzer.toml"),
            "[cargo.buildScripts]\nenable = true",
        )
        .unwrap();
        assert_eq!(configuration_masks(&root).unwrap().len(), 2);
        fs::remove_file(root.join("rust-analyzer.toml")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("rust-analyzer.toml")).unwrap();
        assert_eq!(
            configuration_masks(&root),
            Err(LanguageError::InvalidDocument)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn server_requests_cannot_execute_commands_or_apply_edits() {
        for method in [
            "workspace/executeCommand",
            "client/registerCapability",
            "workspace/applyEdit",
        ] {
            let mut output = Output::default();
            respond(&json!({"jsonrpc":"2.0", "id":99, "method":method, "params":{"command":"sh", "edit":{}}}), &mut output).unwrap();
            let mut decoder = Decoder::default();
            decoder.push(&output.queue.pop_front().unwrap()).unwrap();
            let response = decoder.next().unwrap().unwrap();
            assert_eq!(response["id"], 99);
            if method == "workspace/applyEdit" {
                assert_eq!(response["result"]["applied"], false);
            } else {
                assert_eq!(response["error"]["code"], -32601);
            }
        }
    }
    #[test]
    fn configuration_replies_cannot_enable_project_code_execution() {
        let mut output = Output::default();
        respond(&json!({"id":"config", "method":"workspace/configuration", "params":{"items":[{"section":"rust-analyzer"},{"section":"rust-analyzer.procMacro.enable"},{"section":"other"}]}}), &mut output).unwrap();
        let mut decoder = Decoder::default();
        decoder.push(&output.queue.pop_front().unwrap()).unwrap();
        let response = decoder.next().unwrap().unwrap();
        assert_eq!(response["result"][0]["checkOnSave"], false);
        assert_eq!(response["result"][1], false);
        assert_eq!(response["result"][2], Value::Null);
    }
}
