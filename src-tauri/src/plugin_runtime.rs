use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::{Child, ChildStdin},
    sync::{Mutex, mpsc::Receiver},
    thread::JoinHandle,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, BufReader, Read},
    process::{ChildStdout, Command, Stdio},
    sync::mpsc,
};

use semver::Version;
use serde::Serialize;
use serde_json::Value;

use crate::{
    plugin_manifest::{
        PluginCapability, PluginPermission, PluginRuntime, permissions_exactly_match,
    },
    plugin_package::discover_installed_packages,
    plugin_protocol::{
        EXTERNAL_PLUGIN_PROTOCOL_VERSION, HostPluginFrame, PluginEvent, PluginHostFrame,
        PluginProtocolError, PluginResponse, operation_matches, read_plugin_frame,
        write_host_frame,
    },
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
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE: Duration = Duration::from_millis(250);
const MAX_QUEUED_EVENTS: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalRuntimeSpec {
    pub id: String,
    pub version: Version,
    pub package_path: PathBuf,
    pub entrypoint: String,
    pub capabilities: BTreeSet<PluginCapability>,
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
    ProtocolViolation,
    TransportClosed,
    RequestTimeout,
    RuntimeNotRunning,
    CapabilityDenied,
    PluginRejected,
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
    stdin: Option<ChildStdin>,
    frames: Option<Receiver<Result<PluginHostFrame, PluginProtocolError>>>,
    reader: Option<JoinHandle<()>>,
    spec: ExternalRuntimeSpec,
    events: VecDeque<PluginEvent>,
    session_path: PathBuf,
}

impl RunningPlugin {
    fn stop(mut self) -> Result<(), PluginRuntimeError> {
        if let Some(stdin) = &mut self.stdin {
            let _ = write_host_frame(stdin, &HostPluginFrame::Shutdown);
        }
        self.stdin.take();
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline {
            if self
                .child
                .try_wait()
                .map_err(|_| PluginRuntimeError::StopFailed)?
                .is_some()
            {
                self.frames.take();
                self.join_reader();
                let _ = fs::remove_dir_all(&self.session_path);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.child
            .kill()
            .map_err(|_| PluginRuntimeError::StopFailed)?;
        self.child
            .wait()
            .map_err(|_| PluginRuntimeError::StopFailed)?;
        self.frames.take();
        self.join_reader();
        let _ = fs::remove_dir_all(&self.session_path);
        Ok(())
    }

    fn request(
        &mut self,
        capability: PluginCapability,
        operation: String,
        payload: Value,
    ) -> Result<PluginResponse, PluginRuntimeError> {
        if !self.spec.capabilities.contains(&capability) {
            return Err(PluginRuntimeError::CapabilityDenied);
        }
        let request_id = uuid::Uuid::new_v4().simple().to_string();
        let stdin = self
            .stdin
            .as_mut()
            .ok_or(PluginRuntimeError::TransportClosed)?;
        write_host_frame(
            stdin,
            &HostPluginFrame::Request {
                request_id: request_id.clone(),
                capability,
                operation,
                payload,
            },
        )
        .map_err(map_protocol_error)?;

        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(PluginRuntimeError::RequestTimeout)?;
            let frame = self
                .frames
                .as_ref()
                .ok_or(PluginRuntimeError::TransportClosed)?
                .recv_timeout(remaining)
                .map_err(|error| match error {
                    std::sync::mpsc::RecvTimeoutError::Timeout => {
                        PluginRuntimeError::RequestTimeout
                    }
                    std::sync::mpsc::RecvTimeoutError::Disconnected => {
                        PluginRuntimeError::TransportClosed
                    }
                })?
                .map_err(map_protocol_error)?;
            match frame {
                PluginHostFrame::Response {
                    request_id: response_id,
                    capability: response_capability,
                    result,
                } if response_id == request_id && response_capability == capability => {
                    return Ok(PluginResponse { capability, result });
                }
                PluginHostFrame::Error {
                    request_id: response_id,
                    capability: response_capability,
                    ..
                } if response_id == request_id && response_capability == capability => {
                    return Err(PluginRuntimeError::PluginRejected);
                }
                PluginHostFrame::Event {
                    capability,
                    event,
                    payload,
                } if self.spec.capabilities.contains(&capability)
                    && self.events.len() < MAX_QUEUED_EVENTS =>
                {
                    self.events.push_back(PluginEvent {
                        capability,
                        event,
                        payload,
                    });
                }
                _ => return Err(PluginRuntimeError::ProtocolViolation),
            }
        }
    }

    fn drain_events(&mut self) -> Result<Vec<PluginEvent>, PluginRuntimeError> {
        loop {
            match self
                .frames
                .as_ref()
                .ok_or(PluginRuntimeError::TransportClosed)?
                .try_recv()
            {
                Ok(Ok(PluginHostFrame::Event {
                    capability,
                    event,
                    payload,
                })) if self.spec.capabilities.contains(&capability)
                    && self.events.len() < MAX_QUEUED_EVENTS =>
                {
                    self.events.push_back(PluginEvent {
                        capability,
                        event,
                        payload,
                    });
                }
                Ok(Ok(_)) => return Err(PluginRuntimeError::ProtocolViolation),
                Ok(Err(error)) => return Err(map_protocol_error(error)),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(PluginRuntimeError::TransportClosed);
                }
            }
        }
        Ok(self.events.drain(..).collect())
    }

    fn join_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for RunningPlugin {
    fn drop(&mut self) {
        self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.frames.take();
        self.join_reader();
        let _ = fs::remove_dir_all(&self.session_path);
    }
}

fn map_protocol_error(error: PluginProtocolError) -> PluginRuntimeError {
    match error {
        PluginProtocolError::Io | PluginProtocolError::Closed => {
            PluginRuntimeError::TransportClosed
        }
        PluginProtocolError::FrameTooLarge
        | PluginProtocolError::InvalidFrame
        | PluginProtocolError::InvalidValue => PluginRuntimeError::ProtocolViolation,
    }
}

fn validate_handshake(
    frame: PluginHostFrame,
    expected_capabilities: &BTreeSet<PluginCapability>,
) -> Result<(), PluginRuntimeError> {
    match frame {
        PluginHostFrame::Ready {
            protocol_version,
            capabilities,
        } if protocol_version == EXTERNAL_PLUGIN_PROTOCOL_VERSION
            && capabilities.iter().copied().collect::<BTreeSet<_>>() == *expected_capabilities =>
        {
            Ok(())
        }
        _ => Err(PluginRuntimeError::ProtocolViolation),
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
                    && existing.spec == spec
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
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            #[cfg(test)]
            command.stderr(Stdio::inherit());
            apply_resource_limits(&mut command);
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(_) => {
                    let _ = fs::remove_dir_all(&session_path);
                    return Err(PluginRuntimeError::SpawnFailed);
                }
            };
            let Some(stdin) = child.stdin.take() else {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&session_path);
                return Err(PluginRuntimeError::SpawnFailed);
            };
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&session_path);
                return Err(PluginRuntimeError::SpawnFailed);
            };
            let (frames, reader) = spawn_protocol_reader(stdout);
            let capabilities = spec.capabilities.clone();
            let mut runtime = RunningPlugin {
                child,
                stdin: Some(stdin),
                frames: Some(frames),
                reader: Some(reader),
                spec: spec.clone(),
                events: VecDeque::new(),
                session_path,
            };
            let initialize = HostPluginFrame::Initialize {
                protocol_version: EXTERNAL_PLUGIN_PROTOCOL_VERSION,
                plugin_id: spec.id.clone(),
                plugin_version: spec.version.to_string(),
                capabilities: capabilities.iter().copied().collect(),
                permissions: spec.permissions.iter().copied().collect(),
            };
            let handshake = runtime
                .stdin
                .as_mut()
                .ok_or(PluginRuntimeError::TransportClosed)
                .and_then(|stdin| write_host_frame(stdin, &initialize).map_err(map_protocol_error))
                .and_then(|()| {
                    runtime
                        .frames
                        .as_ref()
                        .ok_or(PluginRuntimeError::TransportClosed)?
                        .recv_timeout(HANDSHAKE_TIMEOUT)
                        .map_err(|error| match error {
                            std::sync::mpsc::RecvTimeoutError::Timeout => {
                                PluginRuntimeError::RequestTimeout
                            }
                            std::sync::mpsc::RecvTimeoutError::Disconnected => {
                                PluginRuntimeError::TransportClosed
                            }
                        })?
                        .map_err(map_protocol_error)
                });
            match handshake.and_then(|frame| validate_handshake(frame, &capabilities)) {
                Ok(()) => {}
                Err(PluginRuntimeError::ProtocolViolation) => {
                    let _ = runtime.stop();
                    return Err(PluginRuntimeError::ProtocolViolation);
                }
                Err(error) => {
                    let _ = runtime.stop();
                    return Err(error);
                }
            }
            running.insert(spec.id.clone(), runtime);
            Ok(())
        }
    }

    pub fn request(
        &self,
        plugin_id: &str,
        capability: PluginCapability,
        operation: String,
        payload: Value,
    ) -> Result<PluginResponse, PluginRuntimeError> {
        if !operation_matches(capability, &operation) {
            return Err(PluginRuntimeError::CapabilityDenied);
        }
        let mut running = self
            .running
            .lock()
            .map_err(|_| PluginRuntimeError::StateUnavailable)?;
        let result = running
            .get_mut(plugin_id)
            .ok_or(PluginRuntimeError::RuntimeNotRunning)?
            .request(capability, operation, payload);
        if result.as_ref().is_err_and(|error| {
            !matches!(
                error,
                PluginRuntimeError::CapabilityDenied | PluginRuntimeError::PluginRejected
            )
        }) {
            let runtime = running.remove(plugin_id);
            drop(running);
            if let Some(runtime) = runtime {
                let _ = runtime.stop();
            }
        }
        result
    }

    pub fn drain_events(&self, plugin_id: &str) -> Result<Vec<PluginEvent>, PluginRuntimeError> {
        let mut running = self
            .running
            .lock()
            .map_err(|_| PluginRuntimeError::StateUnavailable)?;
        let result = running
            .get_mut(plugin_id)
            .ok_or(PluginRuntimeError::RuntimeNotRunning)?
            .drain_events();
        if result.is_err() {
            let runtime = running.remove(plugin_id);
            drop(running);
            if let Some(runtime) = runtime {
                let _ = runtime.stop();
            }
        }
        result
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
fn spawn_protocol_reader(
    stdout: ChildStdout,
) -> (
    Receiver<Result<PluginHostFrame, PluginProtocolError>>,
    JoinHandle<()>,
) {
    let (sender, receiver) = mpsc::sync_channel(64);
    let reader = std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        loop {
            let frame = read_plugin_frame(&mut stdout);
            let terminal = frame.is_err();
            if sender.send(frame).is_err() || terminal {
                break;
            }
        }
    });
    (receiver, reader)
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
        || package
            .manifest
            .capabilities
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            != spec.capabilities
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
            capabilities: [PluginCapability::Tasks].into_iter().collect(),
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

    #[test]
    fn handshake_requires_the_exact_manifest_capabilities() {
        let expected = [PluginCapability::Tasks].into_iter().collect();
        assert_eq!(
            validate_handshake(
                PluginHostFrame::Ready {
                    protocol_version: EXTERNAL_PLUGIN_PROTOCOL_VERSION,
                    capabilities: vec![PluginCapability::Tasks],
                },
                &expected,
            ),
            Ok(())
        );
        assert_eq!(
            validate_handshake(
                PluginHostFrame::Ready {
                    protocol_version: EXTERNAL_PLUGIN_PROTOCOL_VERSION + 1,
                    capabilities: vec![PluginCapability::Tasks],
                },
                &expected,
            ),
            Err(PluginRuntimeError::ProtocolViolation)
        );
        assert_eq!(
            validate_handshake(
                PluginHostFrame::Ready {
                    protocol_version: EXTERNAL_PLUGIN_PROTOCOL_VERSION,
                    capabilities: vec![PluginCapability::Diagnostics],
                },
                &expected,
            ),
            Err(PluginRuntimeError::ProtocolViolation)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_transport_correlates_responses_and_queues_typed_events() {
        let test = TestDirectory::new("protocol");
        let session = test.0.join("session");
        fs::create_dir(&session).unwrap();
        let script = r#"
IFS= read -r initialize || exit 10
printf '%s\n' '{"type":"ready","protocol_version":1,"capabilities":["tasks"]}'
IFS= read -r request || exit 11
request_id=$(printf '%s\n' "$request" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p')
[ -n "$request_id" ] || exit 12
printf '%s\n' '{"type":"event","capability":"tasks","event":"task.output","payload":{"chunk":"running"}}'
printf '{"type":"response","request_id":"%s","capability":"tasks","result":{"accepted":true}}\n' "$request_id"
IFS= read -r shutdown || exit 13
case "$shutdown" in
  *'"type":"shutdown"'*) exit 0 ;;
  *) exit 14 ;;
esac
"#;
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (frames, reader) = spawn_protocol_reader(stdout);
        let capabilities = [PluginCapability::Tasks].into_iter().collect();
        let runtime_spec = ExternalRuntimeSpec {
            id: "io.github.example.protocol".into(),
            version: Version::new(1, 0, 0),
            package_path: test.0.join("package"),
            entrypoint: "entrypoint".into(),
            capabilities,
            permissions: permissions(&[PluginPermission::ProcessSpawn]),
        };
        let mut runtime = RunningPlugin {
            child,
            stdin: Some(stdin),
            frames: Some(frames),
            reader: Some(reader),
            spec: runtime_spec,
            events: VecDeque::new(),
            session_path: session.clone(),
        };
        write_host_frame(
            runtime.stdin.as_mut().unwrap(),
            &HostPluginFrame::Initialize {
                protocol_version: EXTERNAL_PLUGIN_PROTOCOL_VERSION,
                plugin_id: "io.github.example.protocol".into(),
                plugin_version: "1.0.0".into(),
                capabilities: vec![PluginCapability::Tasks],
                permissions: vec![PluginPermission::ProcessSpawn],
            },
        )
        .unwrap();
        let ready = runtime
            .frames
            .as_ref()
            .unwrap()
            .recv_timeout(HANDSHAKE_TIMEOUT)
            .unwrap()
            .unwrap();
        validate_handshake(ready, &runtime.spec.capabilities).unwrap();

        let response = runtime
            .request(
                PluginCapability::Tasks,
                "tasks.list".into(),
                serde_json::json!({}),
            )
            .unwrap();
        assert_eq!(response.capability, PluginCapability::Tasks);
        assert_eq!(response.result, serde_json::json!({ "accepted": true }));
        assert_eq!(
            runtime.drain_events().unwrap(),
            [PluginEvent {
                capability: PluginCapability::Tasks,
                event: "task.output".into(),
                payload: serde_json::json!({ "chunk": "running" }),
            }]
        );
        assert_eq!(
            runtime.request(
                PluginCapability::Diagnostics,
                "diagnostics.read".into(),
                serde_json::json!({}),
            ),
            Err(PluginRuntimeError::CapabilityDenied)
        );
        runtime.stop().unwrap();
        assert!(!session.exists());
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
            capabilities: [PluginCapability::Tasks].into_iter().collect(),
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
                "#!/bin/sh\nif [ -e '{}' ] || [ -e /workspace/host-only ]; then exit 42; fi\nIFS= read -r initialize || exit 43\nprintf '%s\\n' '{{\"type\":\"ready\",\"protocol_version\":1,\"capabilities\":[\"tasks\"]}}'\nIFS= read -r shutdown || exit 44\ncase \"$shutdown\" in *'\"type\":\"shutdown\"'*) exit 0 ;; *) exit 45 ;; esac\n",
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
            capabilities: [PluginCapability::Tasks].into_iter().collect(),
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
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        apply_resource_limits(&mut command);
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (frames, reader) = spawn_protocol_reader(stdout);
        let capabilities = launch.capabilities.clone();
        let mut runtime = RunningPlugin {
            child,
            stdin: Some(stdin),
            frames: Some(frames),
            reader: Some(reader),
            spec: launch.clone(),
            events: VecDeque::new(),
            session_path: session.clone(),
        };
        write_host_frame(
            runtime.stdin.as_mut().unwrap(),
            &HostPluginFrame::Initialize {
                protocol_version: EXTERNAL_PLUGIN_PROTOCOL_VERSION,
                plugin_id: launch.id,
                plugin_version: launch.version.to_string(),
                capabilities: capabilities.iter().copied().collect(),
                permissions: launch.permissions.iter().copied().collect(),
            },
        )
        .unwrap();
        let ready = runtime
            .frames
            .as_ref()
            .unwrap()
            .recv_timeout(HANDSHAKE_TIMEOUT)
            .unwrap()
            .unwrap();
        validate_handshake(ready, &capabilities).unwrap();
        runtime.stop().unwrap();
        assert!(!session.exists());
    }
}
