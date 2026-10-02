//! Cargo tasks for the bundled Rust plugin. They reuse the toolchain and registry
//! directories approved for analysis, run offline and locked, and build into a
//! private directory owned by Lyrnova instead of the read-only workspace.
use super::*;
use crate::{
    process_broker::{
        ProcessAccess, ProcessCommand, ProcessRequest, ProcessStream, SandboxExtension,
        SandboxMount,
    },
    tasks::BuiltinTask,
};
use std::fs;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const CARGO_TIMEOUT_MS: u64 = 30 * 60 * 1000;
const SANDBOX_CARGO_HOME: &str = "/tmp/cargo";
const SANDBOX_TARGET: &str = "/tmp/target";
const MAX_CARGO_DIAGNOSTICS: usize = 500;
const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_PENDING_LINE_BYTES: usize = 1024 * 1024;

struct CargoTaskSpec {
    id: &'static str,
    label: &'static str,
    subcommand: &'static str,
}

const TASKS: &[CargoTaskSpec] = &[
    CargoTaskSpec {
        id: "cargo-check",
        label: "cargo check",
        subcommand: "check",
    },
    CargoTaskSpec {
        id: "cargo-build",
        label: "cargo build",
        subcommand: "build",
    },
    CargoTaskSpec {
        id: "cargo-test",
        label: "cargo test",
        subcommand: "test",
    },
    CargoTaskSpec {
        id: "cargo-run",
        label: "cargo run",
        subcommand: "run",
    },
];

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// Returns the manifest text when the workspace root is a Cargo project.
fn manifest(root: &Path) -> Option<String> {
    let path = root.join("Cargo.toml");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn has_binary(root: &Path, manifest: &str) -> bool {
    regular_file(&root.join("src/main.rs")) || manifest.lines().any(|line| line.trim() == "[[bin]]")
}

fn extension(tools: &environment::Environment, target: &Path) -> SandboxExtension {
    let prefix = tools.prefix();
    let mut mounts = Vec::new();
    if tools.toolchain.id != "system" {
        mounts.push(SandboxMount {
            host: tools.toolchain.path.clone(),
            sandbox: "/toolchain".into(),
            writable: false,
        });
    }
    for path in &tools.registry {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            mounts.push(SandboxMount {
                host: path.clone(),
                sandbox: format!("{SANDBOX_CARGO_HOME}/registry/{name}"),
                writable: false,
            });
        }
    }
    mounts.push(SandboxMount {
        host: target.to_path_buf(),
        sandbox: SANDBOX_TARGET.into(),
        writable: true,
    });
    let mut environment: BTreeMap<_, _> = [
        ("CARGO_HOME", SANDBOX_CARGO_HOME.to_owned()),
        ("CARGO_TARGET_DIR", SANDBOX_TARGET.to_owned()),
        ("CARGO_NET_OFFLINE", "true".to_owned()),
        ("PATH", format!("{prefix}/bin:/usr/bin:/bin")),
        ("CARGO", format!("{prefix}/bin/cargo")),
        ("RUSTC", format!("{prefix}/bin/rustc")),
        ("RUSTC_WRAPPER", String::new()),
        ("RUSTC_WORKSPACE_WRAPPER", String::new()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    // Doctests use the toolchain's own rustdoc when it is installed.
    if regular_file(&tools.toolchain.path.join("bin/rustdoc")) {
        environment.insert("RUSTDOC".into(), format!("{prefix}/bin/rustdoc"));
    }
    SandboxExtension {
        mounts,
        directories: vec![
            SANDBOX_CARGO_HOME.into(),
            format!("{SANDBOX_CARGO_HOME}/registry"),
        ],
        environment,
    }
}

fn detail(environment: &environment::Environment, locked: bool) -> String {
    let mut detail = format!(
        "{} · offline · target privado do Lyrnova",
        environment.toolchain.label
    );
    if !locked {
        detail.push_str(" · requer Cargo.lock");
    }
    detail
}

impl LanguageService {
    /// Cargo tasks for the active workspace, or none when it is not a Cargo project.
    /// `target` is a private directory outside the workspace that keeps build output.
    pub fn cargo_tasks(
        &self,
        root: &Path,
        target: &Path,
    ) -> Result<Vec<BuiltinTask>, LanguageError> {
        let Some(manifest) = manifest(root) else {
            return Ok(Vec::new());
        };
        let environment = self.selected_environment(root)?;
        let binary = has_binary(root, &manifest);
        let detail = detail(&environment, regular_file(&root.join("Cargo.lock")));
        let extension = extension(&environment, target);
        Ok(TASKS
            .iter()
            .filter(|task| task.subcommand != "run" || binary)
            .map(|task| BuiltinTask {
                id: task.id.into(),
                label: task.label.into(),
                detail: Some(detail.clone()),
                execution: ProcessRequest {
                    command: ProcessCommand::Argv {
                        program: "cargo".into(),
                        args: [
                            task.subcommand,
                            "--offline",
                            "--locked",
                            "--message-format=json",
                        ]
                        .map(String::from)
                        .into(),
                    },
                    cwd: None,
                    environment: [("CARGO_TERM_COLOR".to_owned(), "never".to_owned())].into(),
                    access: ProcessAccess::ReadOnly,
                    network: false,
                    timeout_ms: CARGO_TIMEOUT_MS,
                },
                extension: extension.clone(),
            })
            .collect())
    }
}

/// A compiler diagnostic from a Cargo task, located inside the workspace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CargoDiagnostic {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CargoDiagnostics {
    pub items: Vec<CargoDiagnostic>,
    pub truncated: bool,
}

/// Turns `--message-format=json` stdout back into readable output while
/// collecting navigable diagnostics. Parsing stops at `build-finished`, so text
/// printed later by tests or the program cannot forge compiler messages.
#[derive(Default)]
pub struct CargoOutput {
    pending: String,
    finished: bool,
    diagnostics: CargoDiagnostics,
}

impl CargoOutput {
    pub fn feed(&mut self, data: &str) -> Vec<(ProcessStream, String)> {
        let mut output = Vec::new();
        self.pending.push_str(data);
        while let Some(end) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=end).collect();
            self.line(&line, &mut output);
        }
        if self.pending.len() > MAX_PENDING_LINE_BYTES {
            let line = std::mem::take(&mut self.pending);
            output.push((ProcessStream::Stdout, line));
        }
        output
    }

    pub fn finish(mut self) -> (Vec<(ProcessStream, String)>, CargoDiagnostics) {
        let mut output = Vec::new();
        let line = std::mem::take(&mut self.pending);
        if !line.is_empty() {
            self.line(&line, &mut output);
        }
        (output, self.diagnostics)
    }

    fn line(&mut self, line: &str, output: &mut Vec<(ProcessStream, String)>) {
        let message = (!self.finished && line.starts_with('{'))
            .then(|| serde_json::from_str::<Value>(line).ok())
            .flatten();
        let Some(message) = message.filter(|m| m["reason"].is_string()) else {
            output.push((ProcessStream::Stdout, line.to_owned()));
            return;
        };
        match message["reason"].as_str() {
            Some("compiler-message") => {
                if let Some(rendered) = message["message"]["rendered"].as_str() {
                    output.push((ProcessStream::Stderr, rendered.to_owned()));
                }
                self.collect(&message["message"]);
            }
            Some("build-finished") => self.finished = true,
            _ => {}
        }
    }

    fn collect(&mut self, message: &Value) {
        let severity = match message["level"].as_str() {
            Some(level @ ("error" | "warning")) => level,
            _ => return,
        };
        let Some((path, line, column)) = message["spans"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|span| span["is_primary"] == true)
            .find_map(|span| {
                Some((
                    workspace_path(span["file_name"].as_str()?)?,
                    u32::try_from(span["line_start"].as_u64()?).ok()?,
                    u32::try_from(span["column_start"].as_u64()?).ok()?,
                ))
            })
        else {
            return;
        };
        if self.diagnostics.items.len() >= MAX_CARGO_DIAGNOSTICS {
            self.diagnostics.truncated = true;
            return;
        }
        let mut text = message["message"].as_str().unwrap_or_default().to_owned();
        if text.len() > MAX_DIAGNOSTIC_MESSAGE_BYTES {
            let mut end = MAX_DIAGNOSTIC_MESSAGE_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        self.diagnostics.items.push(CargoDiagnostic {
            path,
            line: line.max(1),
            column: column.max(1),
            severity: severity.into(),
            message: text,
            code: message["code"]["code"].as_str().map(str::to_owned),
        });
    }
}

/// Relative workspace path for a rustc span, or `None` for external sources.
fn workspace_path(file: &str) -> Option<String> {
    let relative = file.strip_prefix("/workspace/").unwrap_or(file);
    let path = Path::new(relative);
    (!relative.is_empty()
        && relative.len() <= 4096
        && !relative.contains(['\0', '\\'])
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))))
    .then(|| relative.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("lyrnova-cargo-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(path.join("src")).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn system() -> environment::Environment {
        environment::Environment {
            toolchain: environment::Toolchain {
                id: "system".into(),
                label: "Sistema".into(),
                path: "/usr".into(),
                standard_library: None,
            },
            registry: vec![],
            dependencies: false,
        }
    }

    fn compiler_message(file: &str, level: &str, line: u64) -> String {
        json!({
            "reason": "compiler-message",
            "message": {
                "rendered": format!("{level}: problem in {file}\n"),
                "level": level,
                "message": format!("problem in {file}"),
                "code": {"code": "E0425"},
                "spans": [
                    {"file_name": "src/other.rs", "is_primary": false, "line_start": 9, "column_start": 9},
                    {"file_name": file, "is_primary": true, "line_start": line, "column_start": 5}
                ]
            }
        })
        .to_string()
            + "\n"
    }

    #[test]
    fn cargo_json_is_rendered_and_workspace_locations_are_collected() {
        let mut output = CargoOutput::default();
        let first = compiler_message("src/lib.rs", "error", 3);
        let (head, tail) = first.split_at(first.len() / 2);
        assert!(output.feed(head).is_empty());
        let mut shown = output.feed(&format!("{tail}not json\n"));
        shown.extend(output.feed(&compiler_message("/workspace/src/main.rs", "warning", 1)));
        shown.extend(output.feed(&compiler_message(
            "/tmp/cargo/registry/src/dep/lib.rs",
            "error",
            1,
        )));
        shown.extend(output.feed(&compiler_message("../outside.rs", "error", 1)));
        shown.extend(output.feed(&compiler_message("src/lib.rs", "note", 1)));
        shown.extend(output.feed("{\"reason\":\"compiler-artifact\"}\n{\"reason\":\"build-finished\",\"success\":false}\n"));
        // After the build, test or program output is shown verbatim and never parsed.
        let forged = compiler_message("src/forged.rs", "error", 1);
        shown.extend(output.feed(&forged));
        let (rest, diagnostics) = output.finish();
        assert!(rest.is_empty());
        assert_eq!(
            shown[0],
            (
                ProcessStream::Stderr,
                "error: problem in src/lib.rs\n".into()
            )
        );
        assert_eq!(shown[1], (ProcessStream::Stdout, "not json\n".into()));
        assert_eq!(shown.last().unwrap(), &(ProcessStream::Stdout, forged));
        assert!(
            !shown
                .iter()
                .any(|(_, text)| text.contains("compiler-artifact"))
        );
        assert_eq!(
            diagnostics.items,
            vec![
                CargoDiagnostic {
                    path: "src/lib.rs".into(),
                    line: 3,
                    column: 5,
                    severity: "error".into(),
                    message: "problem in src/lib.rs".into(),
                    code: Some("E0425".into()),
                },
                CargoDiagnostic {
                    path: "src/main.rs".into(),
                    line: 1,
                    column: 5,
                    severity: "warning".into(),
                    message: "problem in /workspace/src/main.rs".into(),
                    code: Some("E0425".into()),
                },
            ]
        );
    }

    #[test]
    fn cargo_diagnostics_are_bounded() {
        let mut output = CargoOutput::default();
        for _ in 0..=MAX_CARGO_DIAGNOSTICS {
            output.feed(&compiler_message("src/lib.rs", "error", 1));
        }
        let unterminated = "x".repeat(MAX_PENDING_LINE_BYTES + 1);
        assert_eq!(
            output.feed(&unterminated),
            vec![(ProcessStream::Stdout, unterminated)]
        );
        let (_, diagnostics) = output.finish();
        assert_eq!(diagnostics.items.len(), MAX_CARGO_DIAGNOSTICS);
        assert!(diagnostics.truncated);
    }

    #[test]
    fn only_cargo_roots_with_regular_manifests_offer_tasks() {
        let f = Fixture::new();
        assert!(manifest(&f.0).is_none());
        std::os::unix::fs::symlink("/etc/hostname", f.0.join("Cargo.toml")).unwrap();
        assert!(manifest(&f.0).is_none());
        fs::remove_file(f.0.join("Cargo.toml")).unwrap();
        fs::write(f.0.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        let text = manifest(&f.0).unwrap();
        assert!(!has_binary(&f.0, &text));
        fs::write(f.0.join("src/main.rs"), "fn main() {}\n").unwrap();
        assert!(has_binary(&f.0, &text));
    }

    #[test]
    fn tasks_build_into_a_private_writable_target_with_read_only_tools() {
        let target = Path::new("/var/cache/lyrnova-target");
        let prepared = extension(&system(), target);
        assert_eq!(
            prepared.mounts,
            vec![SandboxMount {
                host: target.into(),
                sandbox: SANDBOX_TARGET.into(),
                writable: true,
            }]
        );
        assert_eq!(prepared.environment["CARGO_TARGET_DIR"], SANDBOX_TARGET);
        assert_eq!(prepared.environment["CARGO_NET_OFFLINE"], "true");
        assert_eq!(prepared.environment["RUSTC_WRAPPER"], "");
        assert_eq!(prepared.environment["PATH"], "/usr/bin:/usr/bin:/bin");

        let mut custom = system();
        custom.toolchain.id = "stable".into();
        custom.toolchain.path = "/opt/rust/stable".into();
        custom.registry = vec!["/home/user/.cargo/registry/src".into()];
        let prepared = extension(&custom, target);
        assert!(
            prepared
                .mounts
                .iter()
                .any(|mount| mount.sandbox == "/toolchain" && !mount.writable)
        );
        assert!(
            prepared
                .mounts
                .iter()
                .any(|mount| mount.sandbox == "/tmp/cargo/registry/src" && !mount.writable)
        );
        assert_eq!(prepared.environment["CARGO"], "/toolchain/bin/cargo");
        assert!(detail(&custom, false).ends_with("requer Cargo.lock"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cargo_test_runs_in_the_sandbox_without_writing_the_workspace() {
        use crate::process_broker::{
            ProcessAuthority, ProcessBroker, ProcessOrigin, ProcessOutcome, SandboxStrength,
        };
        if !Path::new("/usr/bin/cargo").is_file()
            || ProcessBroker::sandbox_diagnostic().isolated_network != SandboxStrength::Strong
        {
            eprintln!("skipping: system Cargo or Bubblewrap unavailable");
            return;
        }
        let f = Fixture::new();
        let target = Fixture::new();
        fs::write(
            f.0.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            f.0.join("Cargo.lock"),
            "version = 3\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            f.0.join("src/lib.rs"),
            "#[test]\nfn sandboxed() { assert_eq!(2 + 2, 4); }\n",
        )
        .unwrap();
        let request = ProcessRequest {
            command: ProcessCommand::Argv {
                program: "cargo".into(),
                args: ["test", "--offline", "--locked"].map(String::from).into(),
            },
            cwd: None,
            environment: [("CARGO_TERM_COLOR".to_owned(), "never".to_owned())].into(),
            access: ProcessAccess::ReadOnly,
            network: false,
            timeout_ms: CARGO_TIMEOUT_MS,
        };
        let broker = ProcessBroker::default();
        let (review, _) = broker
            .review_with_extension(
                &f.0,
                request,
                ProcessOrigin::LocalUser,
                ProcessAuthority::default(),
                extension(&system(), &target.0),
            )
            .unwrap();
        let (result, _) = broker
            .execute(
                &review.review_token,
                &review.action_sha256,
                std::sync::Arc::new(|_| {}),
            )
            .unwrap();
        assert_eq!(result.outcome, ProcessOutcome::Exited);
        assert_eq!(result.exit_code, Some(0), "{}", result.stderr);
        assert!(
            result.stdout.contains("test sandboxed ... ok"),
            "{}",
            result.stdout
        );
        assert!(!f.0.join("target").exists());
        assert!(target.0.join("debug").is_dir());
    }
}
