use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Child,
    sync::Mutex,
};
#[cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, Read},
    process::{Command, Stdio},
};

use semver::Version;
use serde::Serialize;

use crate::{
    plugin_manifest::{PluginPermission, PluginRuntime, permissions_exactly_match},
    plugin_package::discover_installed_packages,
};

const RUNTIME_DIRECTORY: &str = ".runtime";
#[cfg(target_os = "linux")]
const BWRAP_PATH: &str = "/usr/bin/bwrap";
#[cfg(target_os = "linux")]
const SANDBOX_PLUGIN_ROOT: &str = "/plugin";
#[cfg(target_os = "linux")]
const SANDBOX_WORKSPACE_ROOT: &str = "/workspace";
#[cfg(target_os = "linux")]
const MAX_OPEN_FILES: libc::rlim_t = 256;
#[cfg(target_os = "linux")]
const MAX_FILE_BYTES: libc::rlim_t = 64 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_ADDRESS_SPACE_BYTES: libc::rlim_t = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalRuntimeSpec {
    pub id: String,
    pub version: Version,
    pub package_path: PathBuf,
    pub entrypoint: String,
    pub permissions: BTreeSet<PluginPermission>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PluginRuntimeError {
    UnsupportedPlatform,
    SandboxUnavailable,
    PermissionDenied,
    WorkspaceUnavailable,
    InvalidPackage,
    RuntimeDirectoryUnavailable,
    SpawnFailed,
    StopFailed,
    StateUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceAccess {
    None,
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimePolicy {
    network: bool,
    workspace: WorkspaceAccess,
}

impl RuntimePolicy {
    fn authorize(
        permissions: &BTreeSet<PluginPermission>,
        workspace: Option<&Path>,
    ) -> Result<(Self, Option<PathBuf>), PluginRuntimeError> {
        if !permissions.contains(&PluginPermission::ProcessSpawn) {
            return Err(PluginRuntimeError::PermissionDenied);
        }
        let workspace_access = if permissions.contains(&PluginPermission::WorkspaceWrite) {
            WorkspaceAccess::ReadWrite
        } else if permissions.contains(&PluginPermission::WorkspaceRead) {
            WorkspaceAccess::ReadOnly
        } else {
            WorkspaceAccess::None
        };
        let workspace = match (workspace_access, workspace) {
            (WorkspaceAccess::None, _) => None,
            (_, Some(path)) => Some(
                path.canonicalize()
                    .map_err(|_| PluginRuntimeError::WorkspaceUnavailable)?,
            ),
            (_, None) => return Err(PluginRuntimeError::WorkspaceUnavailable),
        };
        Ok((
            Self {
                network: permissions.contains(&PluginPermission::NetworkAccess),
                workspace: workspace_access,
            },
            workspace,
        ))
    }
}

struct RunningPlugin {
    child: Child,
    session_path: PathBuf,
}

impl RunningPlugin {
    fn stop(mut self) -> Result<(), PluginRuntimeError> {
        let running = self
            .child
            .try_wait()
            .map_err(|_| PluginRuntimeError::StopFailed)?
            .is_none();
        if running {
            self.child
                .kill()
                .map_err(|_| PluginRuntimeError::StopFailed)?;
        }
        self.child
            .wait()
            .map_err(|_| PluginRuntimeError::StopFailed)?;
        let _ = fs::remove_dir_all(&self.session_path);
        Ok(())
    }
}

impl Drop for RunningPlugin {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = fs::remove_dir_all(&self.session_path);
    }
}

#[derive(Default)]
pub struct PluginRuntimeService {
    running: Mutex<BTreeMap<String, RunningPlugin>>,
}

impl PluginRuntimeService {
    pub fn cleanup_stale_sessions(storage_root: &Path) {
        if !fs::symlink_metadata(storage_root).is_ok_and(|metadata| metadata.file_type().is_dir()) {
            return;
        }
        let runtime_root = storage_root.join(RUNTIME_DIRECTORY);
        let Ok(metadata) = fs::symlink_metadata(&runtime_root) else {
            return;
        };
        if metadata.file_type().is_dir() {
            let _ = fs::remove_dir_all(runtime_root);
        } else {
            let _ = fs::remove_file(runtime_root);
        }
    }

    pub fn start(
        &self,
        storage_root: &Path,
        spec: ExternalRuntimeSpec,
        workspace: Option<&Path>,
    ) -> Result<(), PluginRuntimeError> {
        let (policy, workspace) = RuntimePolicy::authorize(&spec.permissions, workspace)?;
        #[cfg(not(target_os = "linux"))]
        {
            let RuntimePolicy {
                network,
                workspace: workspace_access,
            } = policy;
            let _ = (storage_root, spec, network, workspace_access, workspace);
            return Err(PluginRuntimeError::UnsupportedPlatform);
        }

        #[cfg(target_os = "linux")]
        {
            revalidate_spec(storage_root, &spec)?;
            validate_bwrap()?;

            let mut running = self
                .running
                .lock()
                .map_err(|_| PluginRuntimeError::StateUnavailable)?;
            if let Some(existing) = running.get_mut(&spec.id) {
                if existing
                    .child
                    .try_wait()
                    .map_err(|_| PluginRuntimeError::StateUnavailable)?
                    .is_none()
                {
                    return Ok(());
                }
                let stale = running.remove(&spec.id).expect("runtime entry exists");
                drop(stale);
            }

            let session_path = create_session(storage_root)?;
            let executable = session_path.join("entrypoint");
            if let Err(error) =
                copy_executable(&spec.package_path.join(&spec.entrypoint), &executable)
            {
                let _ = fs::remove_dir_all(&session_path);
                return Err(error);
            }
            let args = sandbox_arguments(&spec, &executable, &policy, workspace.as_deref());
            let mut command = Command::new(BWRAP_PATH);
            command
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(test)]
            command.stderr(Stdio::inherit());
            apply_resource_limits(&mut command);
            let child = match command.spawn() {
                Ok(child) => child,
                Err(_) => {
                    let _ = fs::remove_dir_all(&session_path);
                    return Err(PluginRuntimeError::SpawnFailed);
                }
            };
            let mut child = child;
            std::thread::sleep(std::time::Duration::from_millis(50));
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(_)) => {
                    let _ = fs::remove_dir_all(&session_path);
                    return Err(PluginRuntimeError::SpawnFailed);
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_dir_all(&session_path);
                    return Err(PluginRuntimeError::SpawnFailed);
                }
            }
            running.insert(
                spec.id,
                RunningPlugin {
                    child,
                    session_path,
                },
            );
            Ok(())
        }
    }

    pub fn stop(&self, plugin_id: &str) -> Result<(), PluginRuntimeError> {
        let runtime = self
            .running
            .lock()
            .map_err(|_| PluginRuntimeError::StateUnavailable)?
            .remove(plugin_id);
        runtime.map_or(Ok(()), RunningPlugin::stop)
    }

    pub fn stop_all(&self) -> Result<(), PluginRuntimeError> {
        let runtimes = {
            let mut running = self
                .running
                .lock()
                .map_err(|_| PluginRuntimeError::StateUnavailable)?;
            std::mem::take(&mut *running)
        };
        let mut result = Ok(());
        for (_, runtime) in runtimes {
            if let Err(error) = runtime.stop() {
                result = Err(error);
            }
        }
        result
    }
}

impl Drop for PluginRuntimeService {
    fn drop(&mut self) {
        if let Ok(running) = self.running.get_mut() {
            for (_, runtime) in std::mem::take(running) {
                let _ = runtime.stop();
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn revalidate_spec(
    storage_root: &Path,
    spec: &ExternalRuntimeSpec,
) -> Result<(), PluginRuntimeError> {
    if !fs::symlink_metadata(storage_root).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        return Err(PluginRuntimeError::InvalidPackage);
    }
    let host_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| PluginRuntimeError::InvalidPackage)?;
    let packages = discover_installed_packages(storage_root, &host_version)
        .map_err(|_| PluginRuntimeError::InvalidPackage)?;
    let package = packages
        .iter()
        .find(|package| {
            package.manifest.id == spec.id
                && package.manifest.version == spec.version
                && package.path == spec.package_path
        })
        .ok_or(PluginRuntimeError::InvalidPackage)?;
    let PluginRuntime::Process { entrypoint, .. } = &package.manifest.runtime else {
        return Err(PluginRuntimeError::InvalidPackage);
    };
    if entrypoint != &spec.entrypoint
        || !permissions_exactly_match(
            &package.manifest.permissions,
            spec.permissions.iter().copied(),
        )
    {
        return Err(PluginRuntimeError::InvalidPackage);
    }

    validate_spec(spec)
}

#[cfg(target_os = "linux")]
fn validate_spec(spec: &ExternalRuntimeSpec) -> Result<(), PluginRuntimeError> {
    let package =
        fs::symlink_metadata(&spec.package_path).map_err(|_| PluginRuntimeError::InvalidPackage)?;
    if !package.file_type().is_dir() {
        return Err(PluginRuntimeError::InvalidPackage);
    }
    let entrypoint = spec.package_path.join(&spec.entrypoint);
    let metadata =
        fs::symlink_metadata(entrypoint).map_err(|_| PluginRuntimeError::InvalidPackage)?;
    if !metadata.file_type().is_file() {
        return Err(PluginRuntimeError::InvalidPackage);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_bwrap() -> Result<(), PluginRuntimeError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(BWRAP_PATH).map_err(|_| PluginRuntimeError::SandboxUnavailable)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(PluginRuntimeError::SandboxUnavailable);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn create_session(storage_root: &Path) -> Result<PathBuf, PluginRuntimeError> {
    if !fs::symlink_metadata(storage_root).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        return Err(PluginRuntimeError::RuntimeDirectoryUnavailable);
    }
    let root = storage_root.join(RUNTIME_DIRECTORY);
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_dir() => set_private_dir_permissions(&root)?,
        Ok(_) => return Err(PluginRuntimeError::RuntimeDirectoryUnavailable),
        Err(error) if error.kind() == io::ErrorKind::NotFound => create_private_dir_all(&root)?,
        Err(_) => return Err(PluginRuntimeError::RuntimeDirectoryUnavailable),
    }
    let session = root.join(uuid::Uuid::new_v4().simple().to_string());
    fs::create_dir(&session).map_err(|_| PluginRuntimeError::RuntimeDirectoryUnavailable)?;
    set_private_dir_permissions(&session)?;
    Ok(session)
}

#[cfg(target_os = "linux")]
fn copy_executable(source: &Path, destination: &Path) -> Result<(), PluginRuntimeError> {
    let source = open_entrypoint(source)?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| PluginRuntimeError::RuntimeDirectoryUnavailable)?;
    let copied = io::copy(&mut source.take(MAX_EXECUTABLE_BYTES + 1), &mut destination)
        .map_err(|_| PluginRuntimeError::RuntimeDirectoryUnavailable)?;
    if copied > MAX_EXECUTABLE_BYTES {
        return Err(PluginRuntimeError::InvalidPackage);
    }
    destination
        .sync_all()
        .map_err(|_| PluginRuntimeError::RuntimeDirectoryUnavailable)?;
    set_executable_permissions(destination)
}

#[cfg(target_os = "linux")]
fn open_entrypoint(path: &Path) -> Result<File, PluginRuntimeError> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| PluginRuntimeError::InvalidPackage)?;
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.file_type().is_file())
    {
        return Err(PluginRuntimeError::InvalidPackage);
    }
    Ok(file)
}

#[cfg(target_os = "linux")]
fn sandbox_arguments(
    spec: &ExternalRuntimeSpec,
    executable: &Path,
    policy: &RuntimePolicy,
    workspace: Option<&Path>,
) -> Vec<OsString> {
    let mut args = vec![
        "--die-with-parent".into(),
        "--new-session".into(),
        "--unshare-all".into(),
    ];
    if policy.network {
        args.push("--share-net".into());
    }
    args.extend([
        "--cap-drop".into(),
        "ALL".into(),
        "--clearenv".into(),
        "--setenv".into(),
        "HOME".into(),
        "/tmp/home".into(),
        "--setenv".into(),
        "TMPDIR".into(),
        "/tmp".into(),
        "--setenv".into(),
        "PATH".into(),
        "/usr/bin:/bin".into(),
        "--setenv".into(),
        "LANG".into(),
        "C.UTF-8".into(),
        "--setenv".into(),
        "LYRNOVA_PLUGIN_ID".into(),
        spec.id.clone().into(),
        "--setenv".into(),
        "LYRNOVA_PLUGIN_VERSION".into(),
        spec.version.to_string().into(),
        "--setenv".into(),
        "LYRNOVA_PLUGIN_PROTOCOL_VERSION".into(),
        "1".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--dir".into(),
        "/tmp/home".into(),
        "--dir".into(),
        "/run".into(),
        "--dir".into(),
        "/etc".into(),
    ]);

    for path in ["/usr", "/bin", "/lib", "/lib64"] {
        if Path::new(path).exists() {
            push_read_only_bind(&mut args, Path::new(path), Path::new(path));
        }
    }
    for path in [
        "/etc/ld.so.cache",
        "/etc/ld.so.conf",
        "/etc/ld.so.conf.d",
        "/etc/localtime",
    ] {
        if Path::new(path).exists() {
            push_read_only_bind(&mut args, Path::new(path), Path::new(path));
        }
    }
    if policy.network {
        for path in [
            "/etc/resolv.conf",
            "/etc/hosts",
            "/etc/nsswitch.conf",
            "/etc/gai.conf",
            "/etc/ssl/certs",
            "/etc/pki",
        ] {
            if Path::new(path).exists() {
                push_read_only_bind(&mut args, Path::new(path), Path::new(path));
            }
        }
    }

    push_read_only_bind(
        &mut args,
        &spec.package_path,
        Path::new(SANDBOX_PLUGIN_ROOT),
    );
    let sandbox_entrypoint = Path::new(SANDBOX_PLUGIN_ROOT).join(&spec.entrypoint);
    push_read_only_bind(&mut args, executable, &sandbox_entrypoint);

    match (policy.workspace, workspace) {
        (WorkspaceAccess::ReadOnly, Some(path)) => {
            push_read_only_bind(&mut args, path, Path::new(SANDBOX_WORKSPACE_ROOT));
        }
        (WorkspaceAccess::ReadWrite, Some(path)) => {
            args.push("--bind".into());
            args.push(path.as_os_str().to_owned());
            args.push(SANDBOX_WORKSPACE_ROOT.into());
        }
        (WorkspaceAccess::None, None) => {
            args.push("--dir".into());
            args.push(SANDBOX_WORKSPACE_ROOT.into());
        }
        _ => unreachable!("runtime policy and workspace must agree"),
    }
    args.extend([
        "--setenv".into(),
        "LYRNOVA_WORKSPACE".into(),
        SANDBOX_WORKSPACE_ROOT.into(),
        "--chdir".into(),
        SANDBOX_PLUGIN_ROOT.into(),
        "--".into(),
        sandbox_entrypoint.into_os_string(),
    ]);
    args
}

#[cfg(target_os = "linux")]
fn push_read_only_bind(args: &mut Vec<OsString>, source: &Path, destination: &Path) {
    args.push("--ro-bind".into());
    args.push(source.as_os_str().to_owned());
    args.push(destination.as_os_str().to_owned());
}

#[cfg(target_os = "linux")]
fn apply_resource_limits(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // SAFETY: the closure only calls async-signal-safe libc functions and does
    // not allocate or take locks between fork and exec.
    unsafe {
        command.pre_exec(|| {
            set_limit(libc::RLIMIT_CORE, 0)?;
            set_limit(libc::RLIMIT_NOFILE, MAX_OPEN_FILES)?;
            set_limit(libc::RLIMIT_FSIZE, MAX_FILE_BYTES)?;
            set_limit(libc::RLIMIT_AS, MAX_ADDRESS_SPACE_BYTES)?;
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(target_os = "linux")]
fn set_limit(resource: libc::__rlimit_resource_t, value: libc::rlim_t) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: `limit` points to a valid rlimit value for the duration of the call.
    if unsafe { libc::setrlimit(resource, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn create_private_dir_all(path: &Path) -> Result<(), PluginRuntimeError> {
    fs::create_dir_all(path).map_err(|_| PluginRuntimeError::RuntimeDirectoryUnavailable)?;
    set_private_dir_permissions(path)
}

#[cfg(target_os = "linux")]
fn set_private_dir_permissions(path: &Path) -> Result<(), PluginRuntimeError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| PluginRuntimeError::RuntimeDirectoryUnavailable)
}

#[cfg(target_os = "linux")]
fn set_executable_permissions(file: File) -> Result<(), PluginRuntimeError> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o500))
        .map_err(|_| PluginRuntimeError::RuntimeDirectoryUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    struct TestDirectory(PathBuf);

    #[cfg(target_os = "linux")]
    impl TestDirectory {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "lyrnova-plugin-runtime-{name}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn permissions(values: &[PluginPermission]) -> BTreeSet<PluginPermission> {
        values.iter().copied().collect()
    }

    fn spec(permissions: &[PluginPermission]) -> ExternalRuntimeSpec {
        ExternalRuntimeSpec {
            id: "io.github.example.runtime".into(),
            version: Version::new(1, 2, 3),
            package_path: PathBuf::from("/packages/runtime"),
            entrypoint: "bin/runtime".into(),
            permissions: self::permissions(permissions),
        }
    }

    #[cfg(target_os = "linux")]
    fn bubblewrap_namespaces_available(share_network: bool) -> bool {
        let mut command = Command::new(BWRAP_PATH);
        command.arg("--unshare-all");
        if share_network {
            command.arg("--share-net");
        }
        command.args(["--cap-drop", "ALL", "--proc", "/proc", "--dev", "/dev"]);
        for path in ["/usr", "/bin", "/lib", "/lib64"] {
            if Path::new(path).exists() {
                command.args(["--ro-bind", path, path]);
            }
        }
        command
            .args(["--", "/usr/bin/true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn process_spawn_is_mandatory_at_the_policy_boundary() {
        assert_eq!(
            RuntimePolicy::authorize(
                &permissions(&[PluginPermission::WorkspaceRead]),
                Some(Path::new("/tmp")),
            ),
            Err(PluginRuntimeError::PermissionDenied)
        );
    }

    #[test]
    fn workspace_permission_fails_closed_without_an_active_workspace() {
        assert_eq!(
            RuntimePolicy::authorize(
                &permissions(&[
                    PluginPermission::ProcessSpawn,
                    PluginPermission::WorkspaceRead,
                ]),
                None,
            ),
            Err(PluginRuntimeError::WorkspaceUnavailable)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_only_policy_mounts_only_the_current_workspace_and_unshares_network() {
        let workspace = Path::new("/tmp").canonicalize().unwrap();
        let permissions = permissions(&[
            PluginPermission::ProcessSpawn,
            PluginPermission::WorkspaceRead,
        ]);
        let (policy, authorized_workspace) =
            RuntimePolicy::authorize(&permissions, Some(&workspace)).unwrap();
        let args = sandbox_arguments(
            &spec(&[
                PluginPermission::ProcessSpawn,
                PluginPermission::WorkspaceRead,
            ]),
            Path::new("/runtime/entrypoint"),
            &policy,
            authorized_workspace.as_deref(),
        );
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();

        assert!(args.iter().any(|arg| arg == "--unshare-all"));
        assert!(!args.iter().any(|arg| arg == "--share-net"));
        assert!(args.windows(3).any(|values| {
            values[0] == "--ro-bind"
                && values[1] == workspace.to_string_lossy()
                && values[2] == SANDBOX_WORKSPACE_ROOT
        }));
        assert!(!args.iter().any(|arg| arg == "/home"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_and_network_require_their_explicit_grants() {
        let workspace = Path::new("/tmp").canonicalize().unwrap();
        let granted = [
            PluginPermission::ProcessSpawn,
            PluginPermission::WorkspaceWrite,
            PluginPermission::NetworkAccess,
        ];
        let (policy, authorized_workspace) =
            RuntimePolicy::authorize(&permissions(&granted), Some(&workspace)).unwrap();
        let args = sandbox_arguments(
            &spec(&granted),
            Path::new("/runtime/entrypoint"),
            &policy,
            authorized_workspace.as_deref(),
        );
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();

        assert!(args.iter().any(|arg| arg == "--share-net"));
        assert!(args.windows(3).any(|values| {
            values[0] == "--bind"
                && values[1] == workspace.to_string_lossy()
                && values[2] == SANDBOX_WORKSPACE_ROOT
        }));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn symlinked_entrypoints_and_runtime_roots_fail_closed() {
        use std::os::unix::fs::symlink;

        let test = TestDirectory::new("links");
        let package = test.0.join("package");
        let storage = test.0.join("storage");
        fs::create_dir(&package).unwrap();
        fs::create_dir(&storage).unwrap();
        fs::write(package.join("real"), "#!/bin/sh\n").unwrap();
        symlink(package.join("real"), package.join("entrypoint")).unwrap();
        let linked_spec = ExternalRuntimeSpec {
            id: "io.github.example.links".into(),
            version: Version::new(1, 0, 0),
            package_path: package.clone(),
            entrypoint: "entrypoint".into(),
            permissions: permissions(&[PluginPermission::ProcessSpawn]),
        };
        assert_eq!(
            validate_spec(&linked_spec),
            Err(PluginRuntimeError::InvalidPackage)
        );

        symlink(&package, storage.join(RUNTIME_DIRECTORY)).unwrap();
        assert_eq!(
            create_session(&storage),
            Err(PluginRuntimeError::RuntimeDirectoryUnavailable)
        );

        let real_storage = test.0.join("real-storage");
        let linked_storage = test.0.join("linked-storage");
        fs::create_dir(&real_storage).unwrap();
        fs::create_dir(real_storage.join(RUNTIME_DIRECTORY)).unwrap();
        fs::write(real_storage.join(RUNTIME_DIRECTORY).join("keep"), "safe").unwrap();
        symlink(&real_storage, &linked_storage).unwrap();
        PluginRuntimeService::cleanup_stale_sessions(&linked_storage);
        assert!(real_storage.join(RUNTIME_DIRECTORY).join("keep").is_file());
        assert_eq!(
            create_session(&linked_storage),
            Err(PluginRuntimeError::RuntimeDirectoryUnavailable)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bubblewrap_launches_and_stops_a_minimal_runtime() {
        if validate_bwrap().is_err() {
            return;
        }
        let network_permission = if bubblewrap_namespaces_available(false) {
            None
        } else if bubblewrap_namespaces_available(true) {
            Some(PluginPermission::NetworkAccess)
        } else {
            return;
        };
        let test = TestDirectory::new("launch");
        let package = test.0.join("package");
        let storage = test.0.join("storage");
        let host_only = test.0.join("host-only");
        fs::create_dir(&package).unwrap();
        fs::create_dir(&storage).unwrap();
        fs::write(&host_only, "must not be visible").unwrap();
        fs::write(
            package.join("entrypoint"),
            format!(
                "#!/bin/sh\nif [ -e '{}' ] || [ -e /workspace/host-only ]; then exit 42; fi\nwhile :; do sleep 1; done\n",
                host_only.display()
            ),
        )
        .unwrap();
        let mut granted = vec![PluginPermission::ProcessSpawn];
        granted.extend(network_permission);
        let launch = ExternalRuntimeSpec {
            id: "io.github.example.launch".into(),
            version: Version::new(1, 0, 0),
            package_path: package,
            entrypoint: "entrypoint".into(),
            permissions: permissions(&granted),
        };
        let (policy, workspace) = RuntimePolicy::authorize(&launch.permissions, None).unwrap();
        let session = create_session(&storage).unwrap();
        let executable = session.join("entrypoint");
        copy_executable(&launch.package_path.join("entrypoint"), &executable).unwrap();
        let args = sandbox_arguments(&launch, &executable, &policy, workspace.as_deref());
        let mut command = Command::new(BWRAP_PATH);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_resource_limits(&mut command);
        let mut child = command.spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
