//! Cargo tasks for the bundled Rust plugin. They reuse the toolchain and registry
//! directories approved for analysis, run offline and locked, and build into a
//! private directory owned by Lyrnova instead of the read-only workspace.
use super::*;
use crate::{
    process_broker::{
        ProcessAccess, ProcessCommand, ProcessRequest, SandboxExtension, SandboxMount,
    },
    tasks::BuiltinTask,
};
use std::fs;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const CARGO_TIMEOUT_MS: u64 = 30 * 60 * 1000;
const SANDBOX_CARGO_HOME: &str = "/tmp/cargo";
const SANDBOX_TARGET: &str = "/tmp/target";

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
                        args: [task.subcommand, "--offline", "--locked"]
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
