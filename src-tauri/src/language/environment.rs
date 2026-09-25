//! Explicit, session-scoped access to installed Rust tools and registry sources.
use super::*;
use std::{
    fs,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toolchain {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub standard_library: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentReview {
    pub token: String,
    pub toolchains: Vec<Toolchain>,
    pub registry_paths: Vec<PathBuf>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentChoice {
    pub token: String,
    pub toolchain_id: String,
    pub dependencies: bool,
    pub use_registry: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSummary {
    pub toolchain: String,
    pub standard_library: bool,
    pub dependencies: bool,
    pub registry: bool,
}
#[derive(Clone, Debug)]
pub(super) struct SourceRoot {
    pub host: PathBuf,
    pub sandbox: PathBuf,
}
#[derive(Clone, Debug)]
pub(super) struct Environment {
    pub toolchain: Toolchain,
    pub registry: Vec<PathBuf>,
    pub dependencies: bool,
}
struct Pending {
    workspace: PathBuf,
    review: EnvironmentReview,
    created: Instant,
}
#[derive(Default)]
pub(super) struct Environments {
    pending: Option<Pending>,
    approved: Option<(PathBuf, Environment)>,
}
impl Environment {
    pub fn summary(&self) -> EnvironmentSummary {
        EnvironmentSummary {
            toolchain: self.toolchain.label.clone(),
            standard_library: self.toolchain.standard_library.is_some(),
            dependencies: self.dependencies,
            registry: !self.registry.is_empty(),
        }
    }
    pub fn prefix(&self) -> &str {
        if self.toolchain.id == "system" {
            "/usr"
        } else {
            "/toolchain"
        }
    }
    pub fn standard_library(&self) -> Option<PathBuf> {
        self.toolchain
            .standard_library
            .as_ref()
            .and_then(|p| p.strip_prefix(&self.toolchain.path).ok())
            .map(|relative| Path::new(self.prefix()).join(relative))
    }
    pub fn sources(&self) -> Vec<SourceRoot> {
        let mut sources = Vec::new();
        if let (Some(host), Some(sandbox)) =
            (&self.toolchain.standard_library, self.standard_library())
        {
            sources.push(SourceRoot {
                host: host.clone(),
                sandbox,
            });
        }
        for host in &self.registry {
            if host.file_name().is_some_and(|name| name == "src") {
                sources.push(SourceRoot {
                    host: host.clone(),
                    sandbox: "/tmp/cargo/registry/src".into(),
                });
            }
        }
        sources
    }
    pub fn configuration(&self) -> Value {
        let mut config = configuration();
        config["cargo"]["noDeps"] = json!(!self.dependencies);
        // External source discovery changes the initial crate graph. rust-analyzer
        // must be allowed to refresh metadata, still offline/locked and read-only.
        config["cargo"]["autoreload"] =
            json!(self.dependencies || self.standard_library().is_some());
        config["cargo"]["extraArgs"] = json!(["--offline", "--locked"]);
        if let Some(source) = self.standard_library() {
            config["cargo"]["sysroot"] = json!(self.prefix());
            config["cargo"]["sysrootSrc"] = json!(source);
        }
        config
    }
}

fn executable(path: &Path, prefix: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            && path.canonicalize().is_ok_and(|p| {
                p.starts_with(prefix) && p.file_name().is_none_or(|name| name != "rustup")
            })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, prefix);
        false
    }
}
fn toolchain(path: PathBuf, id: String, label: String) -> Option<Toolchain> {
    if !executable(&path.join("bin/cargo"), &path) || !executable(&path.join("bin/rustc"), &path) {
        return None;
    }
    let standard_library = [
        "lib/rustlib/src/rust/library",
        "lib64/rustlib/src/rust/library",
        "share/rust/src/library",
    ]
    .iter()
    .map(|p| path.join(p))
    .find_map(|p| {
        p.canonicalize().ok().filter(|p| {
            p.starts_with(&path)
                && p.join("core/src/lib.rs").is_file()
                && p.join("std/src/lib.rs").is_file()
        })
    });
    Some(Toolchain {
        id,
        label,
        path,
        standard_library,
    })
}
fn discover(
    root: &Path,
    rustup_home: Option<&Path>,
    cargo_home: Option<&Path>,
) -> EnvironmentReview {
    let mut toolchains = Vec::new();
    if !Path::new("/usr").starts_with(root) {
        if let Some(system) = toolchain("/usr".into(), "system".into(), "Rust do sistema".into()) {
            toolchains.push(system);
        }
    }
    if let Some(base) = rustup_home
        .and_then(|p| p.join("toolchains").canonicalize().ok())
        .filter(|p| !p.starts_with(root))
    {
        if let Ok(entries) = fs::read_dir(&base) {
            let mut paths: Vec<_> = entries
                .take(128)
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect();
            paths.sort();
            for path in paths {
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if name.len() > 128 || name.chars().any(char::is_control) {
                    continue;
                }
                if let Some(tool) = toolchain(path.clone(), format!("rustup:{name}"), name.into()) {
                    toolchains.push(tool);
                }
            }
        }
    }
    let mut registry_paths = Vec::new();
    if let Some(base) = cargo_home
        .and_then(|p| p.canonicalize().ok())
        .filter(|p| !p.starts_with(root))
    {
        // Never expose Cargo's top-level config, credentials, binaries or git databases.
        for name in ["index", "cache", "src"] {
            let path = base.join("registry").join(name);
            if path.is_dir() && path.canonicalize().is_ok_and(|p| p == path) {
                registry_paths.push(path);
            }
        }
    }
    EnvironmentReview {
        token: uuid::Uuid::new_v4().to_string(),
        toolchains,
        registry_paths,
    }
}
fn host_review(root: &Path) -> EnvironmentReview {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|p| p.join(".rustup")));
    let cargo = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|p| p.join(".cargo")));
    discover(
        root,
        rustup.as_deref().filter(|p| p.is_absolute()),
        cargo.as_deref().filter(|p| p.is_absolute()),
    )
}
impl Environments {
    fn review(&mut self, root: &Path, review: EnvironmentReview) -> EnvironmentReview {
        self.pending = Some(Pending {
            workspace: root.into(),
            review: review.clone(),
            created: Instant::now(),
        });
        review
    }
    fn choose(
        &mut self,
        root: &Path,
        choice: EnvironmentChoice,
        current: &EnvironmentReview,
    ) -> Result<Environment, LanguageError> {
        let pending = self.pending.take().ok_or(LanguageError::ReviewExpired)?;
        if pending.workspace != root
            || pending.review.token != choice.token
            || pending.created.elapsed() > Duration::from_secs(600)
        {
            return Err(LanguageError::ReviewExpired);
        }
        let selected = pending
            .review
            .toolchains
            .iter()
            .find(|t| t.id == choice.toolchain_id)
            .ok_or(LanguageError::ToolchainUnavailable)?;
        let valid = current.toolchains.iter().any(|t| {
            t.id == selected.id
                && t.path == selected.path
                && t.standard_library == selected.standard_library
        });
        if !valid
            || (choice.use_registry
                && (!choice.dependencies
                    || pending.review.registry_paths.is_empty()
                    || pending.review.registry_paths != current.registry_paths))
        {
            return Err(LanguageError::ReviewExpired);
        }
        Ok(Environment {
            toolchain: selected.clone(),
            dependencies: choice.dependencies,
            registry: if choice.use_registry {
                pending.review.registry_paths
            } else {
                vec![]
            },
        })
    }
    fn selected(&self, root: &Path) -> Result<Environment, LanguageError> {
        let current = host_review(root);
        if let Some((workspace, environment)) = &self.approved {
            if workspace == root {
                if !current.toolchains.iter().any(|t| {
                    t.id == environment.toolchain.id
                        && t.path == environment.toolchain.path
                        && t.standard_library == environment.toolchain.standard_library
                }) || environment
                    .registry
                    .iter()
                    .any(|p| !current.registry_paths.contains(p))
                {
                    return Err(LanguageError::ReviewExpired);
                }
                return Ok(environment.clone());
            }
        }
        let toolchain = current
            .toolchains
            .into_iter()
            .find(|t| t.id == "system")
            .ok_or(LanguageError::ToolchainUnavailable)?;
        Ok(Environment {
            toolchain,
            registry: vec![],
            dependencies: false,
        })
    }
}
impl LanguageService {
    pub fn review_environment(&self, root: &Path) -> Result<EnvironmentReview, LanguageError> {
        Ok(self
            .environments
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?
            .review(root, host_review(root)))
    }
    pub fn configure_environment(
        &self,
        root: &Path,
        choice: EnvironmentChoice,
    ) -> Result<EnvironmentSummary, LanguageError> {
        let environment = self
            .environments
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?
            .choose(root, choice, &host_review(root))?;
        self.stop()?;
        let summary = environment.summary();
        self.environments
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?
            .approved = Some((root.into(), environment));
        Ok(summary)
    }
    pub(super) fn selected_environment(&self, root: &Path) -> Result<Environment, LanguageError> {
        self.environments
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?
            .selected(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        workspace: PathBuf,
        rustup: PathBuf,
        cargo: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            let root = std::env::temp_dir().join(format!("lyrnova-env-{}", uuid::Uuid::new_v4()));
            let workspace = root.join("workspace");
            let rustup = root.join("rustup");
            let cargo = root.join("cargo");
            fs::create_dir_all(&workspace).unwrap();
            let tools = rustup.join("toolchains/stable-test/bin");
            fs::create_dir_all(&tools).unwrap();
            for name in ["cargo", "rustc"] {
                let path = tools.join(name);
                fs::write(&path, b"fixture: discovery never executes tools").unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
            for name in ["index", "cache", "src"] {
                fs::create_dir_all(cargo.join("registry").join(name)).unwrap();
            }
            fs::write(cargo.join("credentials.toml"), "secret fixture").unwrap();
            Self {
                root,
                workspace,
                rustup,
                cargo,
            }
        }
        fn review(&self) -> EnvironmentReview {
            discover(&self.workspace, Some(&self.rustup), Some(&self.cargo))
        }
        fn choice(&self, token: String) -> EnvironmentChoice {
            EnvironmentChoice {
                token,
                toolchain_id: "rustup:stable-test".into(),
                dependencies: true,
                use_registry: true,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[test]
    fn discovers_only_installed_tools_and_specific_registry_directories() {
        let f = Fixture::new();
        let review = f.review();
        assert!(
            review
                .toolchains
                .iter()
                .any(|t| t.id == "rustup:stable-test")
        );
        assert_eq!(review.registry_paths.len(), 3);
        assert!(
            !review
                .registry_paths
                .iter()
                .any(|p| p.ends_with("credentials.toml"))
        );
        std::os::unix::fs::symlink(&f.workspace, f.rustup.join("toolchains/project")).unwrap();
        let nested = discover(&f.root, Some(&f.rustup), Some(&f.cargo));
        assert!(nested.toolchains.iter().all(|t| t.id == "system"));
        assert!(nested.registry_paths.is_empty());
        fs::remove_dir(f.cargo.join("registry/cache")).unwrap();
        std::os::unix::fs::symlink(&f.workspace, f.cargo.join("registry/cache")).unwrap();
        assert_eq!(f.review().registry_paths.len(), 2);
    }
    #[test]
    fn reviews_are_workspace_bound_single_use_expiring_and_revalidated() {
        let f = Fixture::new();
        let mut environments = Environments::default();
        let review = environments.review(&f.workspace, f.review());
        let result = environments
            .choose(&f.workspace, f.choice(review.token.clone()), &f.review())
            .unwrap();
        assert_eq!(result.prefix(), "/toolchain");
        assert_eq!(
            result.sources()[0].sandbox,
            Path::new("/tmp/cargo/registry/src")
        );
        assert!(
            environments
                .choose(&f.workspace, f.choice(review.token), &f.review())
                .is_err()
        );
        let review = environments.review(&f.workspace, f.review());
        assert!(
            environments
                .choose(&f.root, f.choice(review.token), &f.review())
                .is_err()
        );
        let review = environments.review(&f.workspace, f.review());
        environments.pending.as_mut().unwrap().created = Instant::now() - Duration::from_secs(601);
        assert!(
            environments
                .choose(&f.workspace, f.choice(review.token), &f.review())
                .is_err()
        );
        let review = environments.review(&f.workspace, f.review());
        fs::remove_file(f.rustup.join("toolchains/stable-test/bin/cargo")).unwrap();
        assert!(
            environments
                .choose(&f.workspace, f.choice(review.token), &f.review())
                .is_err()
        );
    }
    #[test]
    fn stdlib_requires_installed_sources_and_configuration_stays_offline_and_read_only() {
        let f = Fixture::new();
        let path = f
            .rustup
            .join("toolchains/stable-test/lib/rustlib/src/rust/library");
        for name in ["core", "std"] {
            fs::create_dir_all(path.join(name).join("src")).unwrap();
            fs::write(path.join(name).join("src/lib.rs"), "").unwrap();
        }
        let mut environments = Environments::default();
        let review = environments.review(&f.workspace, f.review());
        let environment = environments
            .choose(&f.workspace, f.choice(review.token), &f.review())
            .unwrap();
        let config = environment.configuration();
        assert_eq!(config["cargo"]["sysroot"], "/toolchain");
        assert_eq!(
            config["cargo"]["sysrootSrc"],
            "/toolchain/lib/rustlib/src/rust/library"
        );
        assert_eq!(config["cargo"]["noDeps"], false);
        assert_eq!(config["cargo"]["autoreload"], true);
        assert_eq!(
            config["cargo"]["extraArgs"],
            json!(["--offline", "--locked"])
        );
        assert_eq!(config["cargo"]["buildScripts"]["enable"], false);
        assert_eq!(config["procMacro"]["enable"], false);
        assert_eq!(config["checkOnSave"], false);
    }
}
