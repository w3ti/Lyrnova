use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(target_os = "linux")]
use std::{ffi::OsString, io};

const BWRAP_PATH: &str = "/usr/bin/bwrap";
const SANDBOX_WORKSPACE: &str = "/workspace";
const MAX_ARGUMENTS: usize = 256;
const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
const MAX_TOTAL_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_SCRIPT_BYTES: usize = 64 * 1024;
const MAX_ENVIRONMENT_ENTRIES: usize = 32;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 8 * 1024;
const MAX_CAPTURE_BYTES_PER_STREAM: usize = 1024 * 1024;
// RLIMIT_NPROC is charged to the real UID across the host, including processes that
// can be outside a container's PID namespace. Keep this conservative enough to stop
// unbounded forks without starving normal parallel builds; cgroup v2 will provide a
// precise per-task quota in the follow-up described by ADR-0016.
const MAX_PROCESSES_PER_USER: libc::rlim_t = 4_096;
const MIN_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const REVIEW_TTL: Duration = Duration::from_secs(5 * 60);
const TERMINATION_GRACE: Duration = Duration::from_millis(300);

const ALLOWED_ENVIRONMENT: &[&str] = &[
    "CARGO_TERM_COLOR",
    "CI",
    "COLUMNS",
    "LINES",
    "NO_COLOR",
    "RUST_BACKTRACE",
    "RUST_LOG",
    "TERM",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessAccess {
    ReadOnly,
    WorkspaceWrite,
    Escalated,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessShell {
    Bash,
    Sh,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessCommand {
    Argv { program: String, args: Vec<String> },
    Shell { shell: ProcessShell, script: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessRequest {
    pub command: ProcessCommand,
    pub cwd: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    pub access: ProcessAccess,
    #[serde(default)]
    pub network: bool,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessOrigin {
    LocalUser,
    Agent { provider_id: String },
    Plugin { plugin_id: String },
}

impl ProcessOrigin {
    fn label(&self) -> Result<String, ProcessBrokerError> {
        match self {
            Self::LocalUser => Ok("local_user".into()),
            Self::Agent { provider_id } => {
                validate_origin_id(provider_id)?;
                Ok(format!("agent:{provider_id}"))
            }
            Self::Plugin { plugin_id } => {
                validate_origin_id(plugin_id)?;
                Ok(format!("plugin:{plugin_id}"))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessAuthority {
    pub workspace_write: bool,
    pub network: bool,
    pub escalated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRisk {
    Standard,
    ApprovalRequired,
    Destructive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxStrength {
    Strong,
    Unavailable,
    HostEscalated,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessReview {
    pub review_token: String,
    pub process_id: String,
    pub action_sha256: String,
    pub origin: String,
    pub command: String,
    pub cwd: String,
    pub access: ProcessAccess,
    pub network: bool,
    pub environment_keys: Vec<String>,
    pub timeout_ms: u64,
    pub risk: ProcessRisk,
    pub sandbox: SandboxStrength,
    pub expires_in_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOutputEvent {
    pub process_id: String,
    pub stream: ProcessStream,
    pub data: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOutcome {
    Exited,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResult {
    pub process_id: String,
    pub outcome: ProcessOutcome,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessAuditEvent {
    pub event_id: String,
    pub process_id: String,
    pub origin: String,
    pub phase: &'static str,
    pub command_sha256: String,
    pub access: ProcessAccess,
    pub network: bool,
    pub outcome: Option<ProcessOutcome>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSandboxDiagnostic {
    pub isolated_network: SandboxStrength,
    pub shared_network: SandboxStrength,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ProcessBrokerError {
    InvalidRequest,
    InvalidCommand,
    InvalidCwd,
    InvalidEnvironment,
    PermissionDenied,
    ReviewNotFound,
    ReviewExpired,
    ApprovalMismatch,
    SandboxUnavailable,
    UnsupportedPlatform,
    SpawnFailed,
    ProcessNotRunning,
    StateUnavailable,
    WaitFailed,
}

#[derive(Clone, Debug)]
enum PreparedProgram {
    SearchPath(String),
    Workspace(PathBuf),
    System(PathBuf),
    Shell(ProcessShell),
}

#[derive(Clone, Debug)]
struct ProcessPlan {
    process_id: String,
    origin: String,
    workspace: PathBuf,
    cwd: PathBuf,
    sandbox_cwd: PathBuf,
    request: ProcessRequest,
    program: PreparedProgram,
    command_display: String,
    command_sha256: String,
    action_sha256: String,
    risk: ProcessRisk,
    sandbox: SandboxStrength,
}

struct PendingReview {
    plan: ProcessPlan,
    expires_at: Instant,
}

struct RunningControl {
    process_group: i32,
    cancelled: AtomicBool,
}

#[derive(Default)]
pub struct ProcessBroker {
    pending: Mutex<BTreeMap<String, PendingReview>>,
    running: Mutex<BTreeMap<String, Arc<RunningControl>>>,
}

impl ProcessBroker {
    pub fn sandbox_diagnostic() -> ProcessSandboxDiagnostic {
        ProcessSandboxDiagnostic {
            isolated_network: sandbox_strength(false),
            shared_network: sandbox_strength(true),
        }
    }

    pub fn review(
        &self,
        workspace: &Path,
        request: ProcessRequest,
        origin: ProcessOrigin,
        authority: ProcessAuthority,
    ) -> Result<(ProcessReview, ProcessAuditEvent), ProcessBrokerError> {
        let plan = prepare_plan(workspace, request, origin, authority)?;
        let review_token = uuid::Uuid::new_v4().simple().to_string();
        let review = ProcessReview {
            review_token: review_token.clone(),
            process_id: plan.process_id.clone(),
            action_sha256: plan.action_sha256.clone(),
            origin: plan.origin.clone(),
            command: plan.command_display.clone(),
            cwd: relative_display(&plan.workspace, &plan.cwd),
            access: plan.request.access,
            network: plan.request.network,
            environment_keys: plan.request.environment.keys().cloned().collect(),
            timeout_ms: plan.request.timeout_ms,
            risk: plan.risk,
            sandbox: plan.sandbox,
            expires_in_ms: REVIEW_TTL.as_millis() as u64,
        };
        let audit = plan.audit("reviewed", None);
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| ProcessBrokerError::StateUnavailable)?;
        pending.retain(|_, review| review.expires_at > Instant::now());
        pending.insert(
            review_token,
            PendingReview {
                plan,
                expires_at: Instant::now() + REVIEW_TTL,
            },
        );
        Ok((review, audit))
    }

    pub fn execute(
        &self,
        review_token: &str,
        action_sha256: &str,
        emit: Arc<dyn Fn(ProcessOutputEvent) + Send + Sync>,
    ) -> Result<(ProcessResult, Vec<ProcessAuditEvent>), ProcessBrokerError> {
        let mut reviews = self
            .pending
            .lock()
            .map_err(|_| ProcessBrokerError::StateUnavailable)?;
        let reviewed = reviews
            .get(review_token)
            .ok_or(ProcessBrokerError::ReviewNotFound)?;
        if reviewed.plan.action_sha256 != action_sha256 {
            return Err(ProcessBrokerError::ApprovalMismatch);
        }
        let pending = reviews
            .remove(review_token)
            .ok_or(ProcessBrokerError::ReviewNotFound)?;
        drop(reviews);
        if pending.expires_at <= Instant::now() {
            return Err(ProcessBrokerError::ReviewExpired);
        }
        let plan = pending.plan;
        let started = plan.audit("started", None);
        let started_at = Instant::now();
        let mut child = spawn_plan(&plan)?;
        let process_group =
            i32::try_from(child.id()).map_err(|_| ProcessBrokerError::SpawnFailed)?;
        let control = Arc::new(RunningControl {
            process_group,
            cancelled: AtomicBool::new(false),
        });
        self.running
            .lock()
            .map_err(|_| ProcessBrokerError::StateUnavailable)?
            .insert(plan.process_id.clone(), Arc::clone(&control));

        let stdout = child.stdout.take().ok_or(ProcessBrokerError::SpawnFailed)?;
        let stderr = child.stderr.take().ok_or(ProcessBrokerError::SpawnFailed)?;
        let stdout_reader = stream_reader(
            stdout,
            plan.process_id.clone(),
            ProcessStream::Stdout,
            Arc::clone(&emit),
        );
        let stderr_reader =
            stream_reader(stderr, plan.process_id.clone(), ProcessStream::Stderr, emit);

        let timeout = Duration::from_millis(plan.request.timeout_ms);
        let wait_result = wait_for_process(&mut child, &control, timeout);
        if wait_result.is_err() {
            terminate_group(control.process_group, libc::SIGKILL);
            let _ = child.wait();
        }
        let removal_result = self
            .running
            .lock()
            .map_err(|_| ProcessBrokerError::StateUnavailable)
            .map(|mut running| {
                running.remove(&plan.process_id);
            });
        let stdout = join_capture(stdout_reader);
        let stderr = join_capture(stderr_reader);
        let (status, outcome) = wait_result?;
        removal_result?;
        let stdout = stdout?;
        let stderr = stderr?;
        let result = ProcessResult {
            process_id: plan.process_id.clone(),
            outcome,
            exit_code: status.and_then(|status| status.code()),
            stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
            duration_ms: started_at
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        };
        let completed = plan.audit("completed", Some(outcome));
        Ok((result, vec![started, completed]))
    }

    pub fn cancel(&self, process_id: &str) -> Result<(), ProcessBrokerError> {
        let control = self
            .running
            .lock()
            .map_err(|_| ProcessBrokerError::StateUnavailable)?
            .get(process_id)
            .cloned()
            .ok_or(ProcessBrokerError::ProcessNotRunning)?;
        control.cancelled.store(true, Ordering::Release);
        terminate_group(control.process_group, libc::SIGTERM);
        Ok(())
    }

    pub fn discard_review(&self, review_token: &str) -> Result<(), ProcessBrokerError> {
        self.pending
            .lock()
            .map_err(|_| ProcessBrokerError::StateUnavailable)?
            .remove(review_token)
            .ok_or(ProcessBrokerError::ReviewNotFound)?;
        Ok(())
    }
}

impl Drop for ProcessBroker {
    fn drop(&mut self) {
        if let Ok(running) = self.running.get_mut() {
            for control in running.values() {
                terminate_group(control.process_group, libc::SIGKILL);
            }
            running.clear();
        }
    }
}

impl ProcessPlan {
    fn audit(&self, phase: &'static str, outcome: Option<ProcessOutcome>) -> ProcessAuditEvent {
        ProcessAuditEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            process_id: self.process_id.clone(),
            origin: self.origin.clone(),
            phase,
            command_sha256: self.command_sha256.clone(),
            access: self.request.access,
            network: self.request.network,
            outcome,
        }
    }
}

fn prepare_plan(
    workspace: &Path,
    request: ProcessRequest,
    origin: ProcessOrigin,
    authority: ProcessAuthority,
) -> Result<ProcessPlan, ProcessBrokerError> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| ProcessBrokerError::InvalidCwd)?;
    if !workspace.is_dir() {
        return Err(ProcessBrokerError::InvalidCwd);
    }
    authorize(&request, authority)?;
    validate_timeout(request.timeout_ms)?;
    validate_environment(&request.environment)?;
    let cwd = resolve_cwd(&workspace, request.cwd.as_deref())?;
    let sandbox_cwd = Path::new(SANDBOX_WORKSPACE).join(
        cwd.strip_prefix(&workspace)
            .map_err(|_| ProcessBrokerError::InvalidCwd)?,
    );
    let (program, command_display, destructive) = prepare_command(&workspace, &request.command)?;
    let risk = if destructive {
        ProcessRisk::Destructive
    } else if request.access != ProcessAccess::ReadOnly
        || request.network
        || matches!(request.command, ProcessCommand::Shell { .. })
    {
        ProcessRisk::ApprovalRequired
    } else {
        ProcessRisk::Standard
    };
    let sandbox = match request.access {
        ProcessAccess::Escalated => SandboxStrength::HostEscalated,
        _ => sandbox_strength(request.network),
    };
    let origin = origin.label()?;
    let command_sha256 = format!("{:x}", Sha256::digest(command_display.as_bytes()));
    let cwd_display = relative_display(&workspace, &cwd);
    let approval_action = serde_json::json!({
        "version": 1,
        "origin": origin,
        "command": command_display,
        "cwd": cwd_display,
        "access": request.access,
        "network": request.network,
        "environment": request.environment,
        "timeoutMs": request.timeout_ms,
        "risk": risk,
        "sandbox": sandbox,
    });
    let approval_bytes =
        serde_json::to_vec(&approval_action).map_err(|_| ProcessBrokerError::InvalidRequest)?;
    let action_sha256 = format!("{:x}", Sha256::digest(approval_bytes));
    Ok(ProcessPlan {
        process_id: uuid::Uuid::new_v4().simple().to_string(),
        origin,
        workspace,
        cwd,
        sandbox_cwd,
        request,
        program,
        command_display,
        command_sha256,
        action_sha256,
        risk,
        sandbox,
    })
}

fn authorize(
    request: &ProcessRequest,
    authority: ProcessAuthority,
) -> Result<(), ProcessBrokerError> {
    if request.network && !authority.network {
        return Err(ProcessBrokerError::PermissionDenied);
    }
    match request.access {
        ProcessAccess::ReadOnly => Ok(()),
        ProcessAccess::WorkspaceWrite if authority.workspace_write => Ok(()),
        ProcessAccess::Escalated if authority.escalated => Ok(()),
        _ => Err(ProcessBrokerError::PermissionDenied),
    }
}

fn validate_timeout(timeout_ms: u64) -> Result<(), ProcessBrokerError> {
    let timeout = Duration::from_millis(timeout_ms);
    if (MIN_TIMEOUT..=MAX_TIMEOUT).contains(&timeout) {
        Ok(())
    } else {
        Err(ProcessBrokerError::InvalidRequest)
    }
}

fn validate_environment(environment: &BTreeMap<String, String>) -> Result<(), ProcessBrokerError> {
    if environment.len() > MAX_ENVIRONMENT_ENTRIES {
        return Err(ProcessBrokerError::InvalidEnvironment);
    }
    let allowed: BTreeSet<_> = ALLOWED_ENVIRONMENT.iter().copied().collect();
    for (key, value) in environment {
        if !allowed.contains(key.as_str())
            || value.len() > MAX_ENVIRONMENT_VALUE_BYTES
            || value.contains('\0')
        {
            return Err(ProcessBrokerError::InvalidEnvironment);
        }
    }
    Ok(())
}

fn resolve_cwd(workspace: &Path, relative: Option<&str>) -> Result<PathBuf, ProcessBrokerError> {
    let relative = relative.unwrap_or("");
    if relative.contains(['\0', '\\']) {
        return Err(ProcessBrokerError::InvalidCwd);
    }
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        if relative.as_os_str().is_empty() {
            return Ok(workspace.to_owned());
        }
        return Err(ProcessBrokerError::InvalidCwd);
    }
    reject_symlink_components(workspace, relative)?;
    let cwd = workspace
        .join(relative)
        .canonicalize()
        .map_err(|_| ProcessBrokerError::InvalidCwd)?;
    if cwd.starts_with(workspace) && cwd.is_dir() {
        Ok(cwd)
    } else {
        Err(ProcessBrokerError::InvalidCwd)
    }
}

fn reject_symlink_components(root: &Path, relative: &Path) -> Result<(), ProcessBrokerError> {
    let mut candidate = root.to_owned();
    for component in relative.components() {
        candidate.push(component.as_os_str());
        let metadata =
            fs::symlink_metadata(&candidate).map_err(|_| ProcessBrokerError::InvalidCwd)?;
        if metadata.file_type().is_symlink() {
            return Err(ProcessBrokerError::InvalidCwd);
        }
    }
    Ok(())
}

fn prepare_command(
    workspace: &Path,
    command: &ProcessCommand,
) -> Result<(PreparedProgram, String, bool), ProcessBrokerError> {
    match command {
        ProcessCommand::Argv { program, args } => {
            validate_arguments(args)?;
            let prepared = prepare_program(workspace, program)?;
            let mut display = shell_quote(program);
            for argument in args {
                display.push(' ');
                display.push_str(&shell_quote(argument));
            }
            let destructive = is_destructive_program(program);
            Ok((prepared, display, destructive))
        }
        ProcessCommand::Shell { shell, script } => {
            if script.is_empty() || script.len() > MAX_SCRIPT_BYTES || script.contains('\0') {
                return Err(ProcessBrokerError::InvalidCommand);
            }
            let destructive = contains_destructive_shell_word(script);
            Ok((PreparedProgram::Shell(*shell), script.clone(), destructive))
        }
    }
}

fn validate_arguments(args: &[String]) -> Result<(), ProcessBrokerError> {
    if args.len() > MAX_ARGUMENTS {
        return Err(ProcessBrokerError::InvalidCommand);
    }
    let mut total = 0usize;
    for argument in args {
        if argument.len() > MAX_ARGUMENT_BYTES || argument.contains('\0') {
            return Err(ProcessBrokerError::InvalidCommand);
        }
        total = total.saturating_add(argument.len());
        if total > MAX_TOTAL_ARGUMENT_BYTES {
            return Err(ProcessBrokerError::InvalidCommand);
        }
    }
    Ok(())
}

fn prepare_program(workspace: &Path, program: &str) -> Result<PreparedProgram, ProcessBrokerError> {
    if program.is_empty() || program.len() > 4096 || program.contains('\0') {
        return Err(ProcessBrokerError::InvalidCommand);
    }
    let path = Path::new(program);
    if !program.contains('/') {
        if program
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
        {
            return Ok(PreparedProgram::SearchPath(program.into()));
        }
        return Err(ProcessBrokerError::InvalidCommand);
    }
    if path.is_absolute() {
        let canonical = path
            .canonicalize()
            .map_err(|_| ProcessBrokerError::InvalidCommand)?;
        if ![Path::new("/usr/bin"), Path::new("/bin")]
            .iter()
            .any(|root| canonical.starts_with(root))
            || !canonical.is_file()
        {
            return Err(ProcessBrokerError::InvalidCommand);
        }
        return Ok(PreparedProgram::System(canonical));
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ProcessBrokerError::InvalidCommand);
    }
    reject_symlink_components(workspace, path).map_err(|_| ProcessBrokerError::InvalidCommand)?;
    let canonical = workspace
        .join(path)
        .canonicalize()
        .map_err(|_| ProcessBrokerError::InvalidCommand)?;
    if !canonical.starts_with(workspace) || !canonical.is_file() {
        return Err(ProcessBrokerError::InvalidCommand);
    }
    Ok(PreparedProgram::Workspace(path.to_owned()))
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@%_+=:,./-".contains(&byte))
    {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn is_destructive_program(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    matches!(
        name,
        "dd" | "mkfs" | "rm" | "rmdir" | "shred" | "sudo" | "wipefs"
    )
}

fn contains_destructive_shell_word(script: &str) -> bool {
    script
        .split(|character: char| character.is_whitespace() || ";|&()".contains(character))
        .any(is_destructive_program)
}

fn validate_origin_id(value: &str) -> Result<(), ProcessBrokerError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        Err(ProcessBrokerError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn relative_display(workspace: &Path, cwd: &Path) -> String {
    let relative = cwd.strip_prefix(workspace).unwrap_or(Path::new(""));
    if relative.as_os_str().is_empty() {
        ".".into()
    } else {
        relative.to_string_lossy().replace('\\', "/")
    }
}

fn sandbox_strength(network: bool) -> SandboxStrength {
    #[cfg(not(target_os = "linux"))]
    {
        SandboxStrength::Unsupported
    }
    #[cfg(target_os = "linux")]
    {
        static ISOLATED_NETWORK: OnceLock<SandboxStrength> = OnceLock::new();
        static SHARED_NETWORK: OnceLock<SandboxStrength> = OnceLock::new();
        *(if network {
            &SHARED_NETWORK
        } else {
            &ISOLATED_NETWORK
        })
        .get_or_init(|| probe_bwrap(network))
    }
}

#[cfg(target_os = "linux")]
fn probe_bwrap(network: bool) -> SandboxStrength {
    if !fs::metadata(BWRAP_PATH).is_ok_and(|metadata| metadata.is_file()) {
        return SandboxStrength::Unavailable;
    }
    let mut command = Command::new(BWRAP_PATH);
    command.arg("--unshare-all");
    if network {
        command.arg("--share-net");
    }
    command.args(["--cap-drop", "ALL", "--proc", "/proc", "--dev", "/dev"]);
    for path in ["/usr", "/bin", "/lib", "/lib64"] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    if command
        .args(["--", "/usr/bin/true"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        SandboxStrength::Strong
    } else {
        SandboxStrength::Unavailable
    }
}

fn spawn_plan(plan: &ProcessPlan) -> Result<Child, ProcessBrokerError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = plan;
        Err(ProcessBrokerError::UnsupportedPlatform)
    }
    #[cfg(target_os = "linux")]
    {
        let mut command = if plan.request.access == ProcessAccess::Escalated {
            host_command(plan)?
        } else {
            if plan.sandbox != SandboxStrength::Strong {
                return Err(ProcessBrokerError::SandboxUnavailable);
            }
            sandbox_command(plan)?
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_child_security(&mut command);
        command.spawn().map_err(|_| ProcessBrokerError::SpawnFailed)
    }
}

#[cfg(target_os = "linux")]
fn host_command(plan: &ProcessPlan) -> Result<Command, ProcessBrokerError> {
    let (program, args) = host_program_and_args(plan)?;
    let mut command = Command::new(program);
    command.args(args).current_dir(&plan.cwd).env_clear();
    apply_fixed_environment(&mut command, &plan.request.environment);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn sandbox_command(plan: &ProcessPlan) -> Result<Command, ProcessBrokerError> {
    let mut command = Command::new(BWRAP_PATH);
    command.args(sandbox_arguments(plan)?);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn host_program_and_args(
    plan: &ProcessPlan,
) -> Result<(OsString, Vec<OsString>), ProcessBrokerError> {
    match &plan.request.command {
        ProcessCommand::Argv { args, .. } => Ok((
            prepared_host_program(&plan.program, &plan.workspace)?,
            args.iter().map(OsString::from).collect(),
        )),
        ProcessCommand::Shell { shell, script } => {
            Ok((shell_path(*shell).into(), shell_arguments(*shell, script)))
        }
    }
}

#[cfg(target_os = "linux")]
fn sandbox_arguments(plan: &ProcessPlan) -> Result<Vec<OsString>, ProcessBrokerError> {
    let mut args = vec!["--die-with-parent".into(), "--unshare-all".into()];
    if plan.request.network {
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
    ]);
    for (key, value) in &plan.request.environment {
        args.extend(["--setenv".into(), key.into(), value.into()]);
    }
    args.extend([
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
            push_ro_bind(&mut args, Path::new(path), Path::new(path));
        }
    }
    for path in [
        "/etc/ld.so.cache",
        "/etc/ld.so.conf",
        "/etc/ld.so.conf.d",
        "/etc/localtime",
    ] {
        if Path::new(path).exists() {
            push_ro_bind(&mut args, Path::new(path), Path::new(path));
        }
    }
    if plan.request.network {
        for path in [
            "/etc/resolv.conf",
            "/etc/hosts",
            "/etc/nsswitch.conf",
            "/etc/ssl/certs",
            "/etc/pki",
        ] {
            if Path::new(path).exists() {
                push_ro_bind(&mut args, Path::new(path), Path::new(path));
            }
        }
    }
    match plan.request.access {
        ProcessAccess::ReadOnly => {
            push_ro_bind(&mut args, &plan.workspace, Path::new(SANDBOX_WORKSPACE))
        }
        ProcessAccess::WorkspaceWrite => args.extend([
            "--bind".into(),
            plan.workspace.as_os_str().to_owned(),
            SANDBOX_WORKSPACE.into(),
        ]),
        ProcessAccess::Escalated => return Err(ProcessBrokerError::InvalidRequest),
    }
    let (program, command_args) = sandbox_program_and_args(plan)?;
    args.extend([
        "--chdir".into(),
        plan.sandbox_cwd.as_os_str().to_owned(),
        "--".into(),
        program,
    ]);
    args.extend(command_args);
    Ok(args)
}

#[cfg(target_os = "linux")]
fn sandbox_program_and_args(
    plan: &ProcessPlan,
) -> Result<(OsString, Vec<OsString>), ProcessBrokerError> {
    match &plan.request.command {
        ProcessCommand::Argv { args, .. } => {
            let program = match &plan.program {
                PreparedProgram::SearchPath(program) => program.into(),
                PreparedProgram::Workspace(relative) => {
                    Path::new(SANDBOX_WORKSPACE).join(relative).into_os_string()
                }
                PreparedProgram::System(path) => path.as_os_str().to_owned(),
                PreparedProgram::Shell(_) => return Err(ProcessBrokerError::InvalidCommand),
            };
            Ok((program, args.iter().map(OsString::from).collect()))
        }
        ProcessCommand::Shell { shell, script } => {
            Ok((shell_path(*shell).into(), shell_arguments(*shell, script)))
        }
    }
}

#[cfg(target_os = "linux")]
fn prepared_host_program(
    program: &PreparedProgram,
    workspace: &Path,
) -> Result<OsString, ProcessBrokerError> {
    match program {
        PreparedProgram::SearchPath(program) => Ok(program.into()),
        PreparedProgram::Workspace(relative) => Ok(workspace.join(relative).into_os_string()),
        PreparedProgram::System(path) => Ok(path.as_os_str().to_owned()),
        PreparedProgram::Shell(shell) => Ok(shell_path(*shell).into()),
    }
}

#[cfg(target_os = "linux")]
fn shell_path(shell: ProcessShell) -> &'static str {
    match shell {
        ProcessShell::Bash => "/bin/bash",
        ProcessShell::Sh => "/bin/sh",
    }
}

#[cfg(target_os = "linux")]
fn shell_arguments(shell: ProcessShell, script: &str) -> Vec<OsString> {
    match shell {
        ProcessShell::Bash => vec![
            "--noprofile".into(),
            "--norc".into(),
            "-c".into(),
            script.into(),
        ],
        ProcessShell::Sh => vec!["-c".into(), script.into()],
    }
}

#[cfg(target_os = "linux")]
fn push_ro_bind(args: &mut Vec<OsString>, source: &Path, destination: &Path) {
    args.extend([
        "--ro-bind".into(),
        source.as_os_str().to_owned(),
        destination.as_os_str().to_owned(),
    ]);
}

#[cfg(target_os = "linux")]
fn apply_fixed_environment(command: &mut Command, environment: &BTreeMap<String, String>) {
    command
        .env("HOME", "/tmp/lyrnova-process-home")
        .env("TMPDIR", "/tmp")
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .envs(environment);
}

#[cfg(target_os = "linux")]
fn apply_child_security(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
    // SAFETY: only async-signal-safe libc calls run between fork and exec.
    unsafe {
        command.pre_exec(|| {
            set_limit(libc::RLIMIT_CORE, 0)?;
            set_limit(libc::RLIMIT_NOFILE, 256)?;
            set_limit(libc::RLIMIT_NPROC, MAX_PROCESSES_PER_USER)?;
            set_limit(libc::RLIMIT_FSIZE, 64 * 1024 * 1024)?;
            set_limit(libc::RLIMIT_AS, 2 * 1024 * 1024 * 1024)?;
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
    // SAFETY: `limit` is valid for the duration of this call.
    if unsafe { libc::setrlimit(resource, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn wait_for_process(
    child: &mut Child,
    control: &RunningControl,
    timeout: Duration,
) -> Result<(Option<ExitStatus>, ProcessOutcome), ProcessBrokerError> {
    let deadline = Instant::now() + timeout;
    loop {
        let outcome = if control.cancelled.load(Ordering::Acquire) {
            Some(ProcessOutcome::Cancelled)
        } else if Instant::now() >= deadline {
            Some(ProcessOutcome::TimedOut)
        } else {
            None
        };
        if let Some(outcome) = outcome {
            terminate_group(control.process_group, libc::SIGTERM);
            let grace = Instant::now() + TERMINATION_GRACE;
            while Instant::now() < grace {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|_| ProcessBrokerError::WaitFailed)?
                {
                    return Ok((Some(status), outcome));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            terminate_group(control.process_group, libc::SIGKILL);
            let status = child.wait().map_err(|_| ProcessBrokerError::WaitFailed)?;
            return Ok((Some(status), outcome));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|_| ProcessBrokerError::WaitFailed)?
        {
            return Ok((Some(status), ProcessOutcome::Exited));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_group(process_group: i32, signal: i32) {
    #[cfg(unix)]
    // SAFETY: a negative PID addresses the process group created for this child.
    unsafe {
        libc::kill(-process_group, signal);
    }
    #[cfg(not(unix))]
    let _ = (process_group, signal);
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn stream_reader(
    mut reader: impl Read + Send + 'static,
    process_id: String,
    stream: ProcessStream,
    emit: Arc<dyn Fn(ProcessOutputEvent) + Send + Sync>,
) -> JoinHandle<CapturedOutput> {
    std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut truncated = false;
        let mut buffer = [0u8; 8 * 1024];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let remaining = MAX_CAPTURE_BYTES_PER_STREAM.saturating_sub(captured.len());
            let kept = remaining.min(read);
            if kept > 0 {
                captured.extend_from_slice(&buffer[..kept]);
                emit(ProcessOutputEvent {
                    process_id: process_id.clone(),
                    stream,
                    data: String::from_utf8_lossy(&buffer[..kept]).into_owned(),
                });
            }
            truncated |= kept < read;
        }
        CapturedOutput {
            bytes: captured,
            truncated,
        }
    })
}

fn join_capture(reader: JoinHandle<CapturedOutput>) -> Result<CapturedOutput, ProcessBrokerError> {
    reader.join().map_err(|_| ProcessBrokerError::WaitFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lyrnova-process-broker-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(command: ProcessCommand) -> ProcessRequest {
        ProcessRequest {
            command,
            cwd: None,
            environment: BTreeMap::new(),
            access: ProcessAccess::ReadOnly,
            network: false,
            timeout_ms: 5_000,
        }
    }

    #[test]
    fn structured_arguments_are_quoted_for_review_but_never_joined_for_execution() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let (review, audit) = broker
            .review(
                &workspace.0,
                request(ProcessCommand::Argv {
                    program: "printf".into(),
                    args: vec!["%s".into(), "$(touch escaped)".into()],
                }),
                ProcessOrigin::LocalUser,
                ProcessAuthority::default(),
            )
            .unwrap();

        assert_eq!(review.command, "printf %s '$(touch escaped)'");
        assert_eq!(review.risk, ProcessRisk::Standard);
        assert_eq!(audit.phase, "reviewed");
        assert_eq!(audit.origin, "local_user");
    }

    #[test]
    fn shell_commands_are_exact_and_destructive_words_escalate_risk() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let script = "printf 'exact value\\n'; rm -rf build";
        let (review, _) = broker
            .review(
                &workspace.0,
                request(ProcessCommand::Shell {
                    shell: ProcessShell::Bash,
                    script: script.into(),
                }),
                ProcessOrigin::Agent {
                    provider_id: "io.github.example.ai".into(),
                },
                ProcessAuthority::default(),
            )
            .unwrap();

        assert_eq!(review.command, script);
        assert_eq!(review.risk, ProcessRisk::Destructive);
    }

    #[test]
    fn write_network_and_escalated_modes_require_independent_authority() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let mut write = request(ProcessCommand::Argv {
            program: "true".into(),
            args: Vec::new(),
        });
        write.access = ProcessAccess::WorkspaceWrite;
        assert_eq!(
            broker.review(
                &workspace.0,
                write.clone(),
                ProcessOrigin::LocalUser,
                ProcessAuthority::default(),
            ),
            Err(ProcessBrokerError::PermissionDenied)
        );
        write.network = true;
        assert_eq!(
            broker.review(
                &workspace.0,
                write,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    workspace_write: true,
                    network: false,
                    escalated: false,
                },
            ),
            Err(ProcessBrokerError::PermissionDenied)
        );
    }

    #[test]
    fn environment_is_an_allowlist_and_never_accepts_loader_or_home_overrides() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        for key in ["HOME", "PATH", "LD_PRELOAD", "AWS_SECRET_ACCESS_KEY"] {
            let mut denied = request(ProcessCommand::Argv {
                program: "true".into(),
                args: Vec::new(),
            });
            denied.environment.insert(key.into(), "secret".into());
            assert_eq!(
                broker.review(
                    &workspace.0,
                    denied,
                    ProcessOrigin::LocalUser,
                    ProcessAuthority::default(),
                ),
                Err(ProcessBrokerError::InvalidEnvironment)
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn execution_starts_from_a_small_fixed_environment() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let mut command = request(ProcessCommand::Argv {
            program: "/usr/bin/env".into(),
            args: Vec::new(),
        });
        command.access = ProcessAccess::Escalated;
        command.environment.insert("TERM".into(), "dumb".into());
        let (review, _) = broker
            .review(
                &workspace.0,
                command,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    escalated: true,
                    ..ProcessAuthority::default()
                },
            )
            .unwrap();
        let (result, _) = broker
            .execute(
                &review.review_token,
                &review.action_sha256,
                Arc::new(|_| {}),
            )
            .unwrap();
        let environment: BTreeMap<_, _> = result
            .stdout
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();

        assert_eq!(environment.get("HOME"), Some(&"/tmp/lyrnova-process-home"));
        assert_eq!(environment.get("PATH"), Some(&"/usr/bin:/bin"));
        assert_eq!(environment.get("TERM"), Some(&"dumb"));
        assert!(!environment.contains_key("USER"));
        assert!(!environment.keys().any(|key| key.contains("SECRET")));
    }

    #[cfg(unix)]
    #[test]
    fn cwd_traversal_and_symlinks_are_rejected_before_review() {
        let workspace = TestWorkspace::new();
        let outside = TestWorkspace::new();
        std::os::unix::fs::symlink(&outside.0, workspace.0.join("linked")).unwrap();
        let broker = ProcessBroker::default();
        for cwd in ["..", "linked", "/tmp", "nested\\escape"] {
            let mut denied = request(ProcessCommand::Argv {
                program: "true".into(),
                args: Vec::new(),
            });
            denied.cwd = Some(cwd.into());
            assert_eq!(
                broker.review(
                    &workspace.0,
                    denied,
                    ProcessOrigin::LocalUser,
                    ProcessAuthority::default(),
                ),
                Err(ProcessBrokerError::InvalidCwd)
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_policy_mounts_only_the_workspace_and_clears_the_environment() {
        let workspace = TestWorkspace::new();
        let plan = prepare_plan(
            &workspace.0,
            request(ProcessCommand::Argv {
                program: "true".into(),
                args: Vec::new(),
            }),
            ProcessOrigin::LocalUser,
            ProcessAuthority::default(),
        )
        .unwrap();
        let args = sandbox_arguments(&plan).unwrap();
        let args: Vec<_> = args.iter().map(|value| value.to_string_lossy()).collect();

        assert!(args.iter().any(|value| value == "--unshare-all"));
        assert!(args.iter().any(|value| value == "--clearenv"));
        assert!(!args.iter().any(|value| value == "--share-net"));
        assert!(args.windows(3).any(|values| {
            values[0] == "--ro-bind"
                && values[1] == workspace.0.canonicalize().unwrap().to_string_lossy()
                && values[2] == SANDBOX_WORKSPACE
        }));
        assert!(!args.iter().any(|value| value == "/home"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unavailable_sandbox_fails_closed_before_starting_the_command() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let (review, _) = broker
            .review(
                &workspace.0,
                request(ProcessCommand::Argv {
                    program: "touch".into(),
                    args: vec!["must-not-exist".into()],
                }),
                ProcessOrigin::LocalUser,
                ProcessAuthority::default(),
            )
            .unwrap();

        if review.sandbox == SandboxStrength::Unavailable {
            assert_eq!(
                broker.execute(
                    &review.review_token,
                    &review.action_sha256,
                    Arc::new(|_| {})
                ),
                Err(ProcessBrokerError::SandboxUnavailable)
            );
            assert!(!workspace.0.join("must-not-exist").exists());
        } else {
            let (result, _) = broker
                .execute(
                    &review.review_token,
                    &review.action_sha256,
                    Arc::new(|_| {}),
                )
                .unwrap();
            assert_eq!(result.outcome, ProcessOutcome::Exited);
            assert_ne!(result.exit_code, Some(0));
            assert!(!workspace.0.join("must-not-exist").exists());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn workspace_write_sandbox_cannot_reach_a_host_sibling() {
        if sandbox_strength(false) != SandboxStrength::Strong {
            return;
        }
        let workspace = TestWorkspace::new();
        let outside = TestWorkspace::new();
        let outside_marker = outside.0.join("marker");
        fs::write(&outside_marker, "original").unwrap();
        let broker = ProcessBroker::default();
        let mut command = request(ProcessCommand::Shell {
            shell: ProcessShell::Sh,
            script: format!(
                "printf inside > inside.txt; printf escaped > {}",
                shell_quote(&outside_marker.to_string_lossy())
            ),
        });
        command.access = ProcessAccess::WorkspaceWrite;
        let (review, _) = broker
            .review(
                &workspace.0,
                command,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    workspace_write: true,
                    ..ProcessAuthority::default()
                },
            )
            .unwrap();
        let (result, _) = broker
            .execute(
                &review.review_token,
                &review.action_sha256,
                Arc::new(|_| {}),
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(workspace.0.join("inside.txt")).unwrap(),
            "inside"
        );
        assert_eq!(fs::read_to_string(outside_marker).unwrap(), "original");
        assert_ne!(result.exit_code, Some(0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn escalated_execution_keeps_arguments_structured_and_bounds_output() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let mut command = request(ProcessCommand::Argv {
            program: "printf".into(),
            args: vec!["%s".into(), "$(touch escaped)".into()],
        });
        command.access = ProcessAccess::Escalated;
        let (review, _) = broker
            .review(
                &workspace.0,
                command,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    escalated: true,
                    ..ProcessAuthority::default()
                },
            )
            .unwrap();
        let (result, audit) = broker
            .execute(
                &review.review_token,
                &review.action_sha256,
                Arc::new(|_| {}),
            )
            .unwrap();

        assert_eq!(result.outcome, ProcessOutcome::Exited);
        assert_eq!(result.stdout, "$(touch escaped)");
        assert!(!workspace.0.join("escaped").exists());
        assert_eq!(audit.len(), 2);
        let serialized_audit = serde_json::to_string(&audit).unwrap();
        assert!(!serialized_audit.contains("touch escaped"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn child_process_count_has_a_hard_limit() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let mut command = request(ProcessCommand::Shell {
            shell: ProcessShell::Bash,
            script: "ulimit -u".into(),
        });
        command.access = ProcessAccess::Escalated;
        let (review, _) = broker
            .review(
                &workspace.0,
                command,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    escalated: true,
                    ..ProcessAuthority::default()
                },
            )
            .unwrap();
        let (result, _) = broker
            .execute(
                &review.review_token,
                &review.action_sha256,
                Arc::new(|_| {}),
            )
            .unwrap();

        assert_eq!(result.stdout.trim(), MAX_PROCESSES_PER_USER.to_string());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn output_is_drained_after_the_capture_limit_without_blocking_the_process() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let mut command = request(ProcessCommand::Shell {
            shell: ProcessShell::Sh,
            script: "yes x | head -c 1100000".into(),
        });
        command.access = ProcessAccess::Escalated;
        let (review, _) = broker
            .review(
                &workspace.0,
                command,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    escalated: true,
                    ..ProcessAuthority::default()
                },
            )
            .unwrap();
        let (result, _) = broker
            .execute(
                &review.review_token,
                &review.action_sha256,
                Arc::new(|_| {}),
            )
            .unwrap();

        assert_eq!(result.outcome, ProcessOutcome::Exited);
        assert_eq!(result.stdout.len(), MAX_CAPTURE_BYTES_PER_STREAM);
        assert!(result.stdout_truncated);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn timeout_terminates_the_process_group() {
        let workspace = TestWorkspace::new();
        let broker = ProcessBroker::default();
        let mut command = request(ProcessCommand::Argv {
            program: "sleep".into(),
            args: vec!["30".into()],
        });
        command.access = ProcessAccess::Escalated;
        command.timeout_ms = 100;
        let (review, _) = broker
            .review(
                &workspace.0,
                command,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    escalated: true,
                    ..ProcessAuthority::default()
                },
            )
            .unwrap();
        let started = Instant::now();
        let (result, _) = broker
            .execute(
                &review.review_token,
                &review.action_sha256,
                Arc::new(|_| {}),
            )
            .unwrap();

        assert_eq!(result.outcome, ProcessOutcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cancellation_terminates_children_and_grandchildren() {
        let workspace = TestWorkspace::new();
        let broker = Arc::new(ProcessBroker::default());
        let script =
            "sh -c 'sleep 30 & echo $! > grandchild.pid; wait' & echo $! > child.pid; wait";
        let mut command = request(ProcessCommand::Shell {
            shell: ProcessShell::Sh,
            script: script.into(),
        });
        command.access = ProcessAccess::Escalated;
        command.timeout_ms = 10_000;
        let (review, _) = broker
            .review(
                &workspace.0,
                command,
                ProcessOrigin::LocalUser,
                ProcessAuthority {
                    escalated: true,
                    ..ProcessAuthority::default()
                },
            )
            .unwrap();
        let process_id = review.process_id.clone();
        let token = review.review_token;
        let action_sha256 = review.action_sha256;
        let execution_broker = Arc::clone(&broker);
        let execution = std::thread::spawn(move || {
            execution_broker.execute(&token, &action_sha256, Arc::new(|_| {}))
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && (!workspace.0.join("child.pid").exists()
                || !workspace.0.join("grandchild.pid").exists())
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let child: i32 = fs::read_to_string(workspace.0.join("child.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let grandchild: i32 = fs::read_to_string(workspace.0.join("grandchild.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        broker.cancel(&process_id).unwrap();
        let (result, _) = execution.join().unwrap().unwrap();

        assert_eq!(result.outcome, ProcessOutcome::Cancelled);
        assert!(wait_until_process_absent(child));
        assert!(wait_until_process_absent(grandchild));
    }

    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        // SAFETY: signal 0 performs no mutation and only probes this PID.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(unix)]
    fn wait_until_process_absent(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !process_exists(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}
