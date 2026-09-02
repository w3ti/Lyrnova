use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    plugin_manifest::PluginPermission,
    process_broker::{
        ProcessAccess, ProcessAuditEvent, ProcessAuthority, ProcessBroker, ProcessBrokerError,
        ProcessOrigin, ProcessOutputEvent, ProcessRequest, ProcessResult, ProcessReview,
        ProcessSandboxDiagnostic,
    },
};

const MAX_TASKS_PER_PROVIDER: usize = 128;
const MAX_TASK_LABEL_BYTES: usize = 96;
const MAX_TASK_DETAIL_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskProvider {
    pub id: String,
    pub name: String,
    pub permissions: BTreeSet<PluginPermission>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginTaskCatalog {
    items: Vec<PluginTaskDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginTaskDefinition {
    id: String,
    label: String,
    #[serde(default)]
    detail: Option<String>,
    execution: ProcessRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub plugin_id: String,
    pub plugin_name: String,
    pub task_id: String,
    pub label: String,
    pub detail: Option<String>,
    pub access: ProcessAccess,
    pub network: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskList {
    pub items: Vec<TaskSummary>,
    pub sandbox: ProcessSandboxDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReview {
    pub plugin_id: String,
    pub plugin_name: String,
    pub task_id: String,
    pub label: String,
    pub detail: Option<String>,
    pub process: ProcessReview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "domain", content = "error", rename_all = "snake_case")]
pub enum TaskError {
    InvalidCatalog,
    UnknownTask,
    PermissionDenied,
    ReviewNotFound,
    AuthorizationChanged,
    StateUnavailable,
    Process(ProcessBrokerError),
}

impl From<ProcessBrokerError> for TaskError {
    fn from(error: ProcessBrokerError) -> Self {
        Self::Process(error)
    }
}

#[derive(Clone)]
struct PendingTask {
    plugin_id: String,
    process_id: String,
    permissions: BTreeSet<PluginPermission>,
}

#[derive(Clone, Default)]
pub struct TaskBroker {
    process: Arc<ProcessBroker>,
    pending: Arc<Mutex<BTreeMap<String, PendingTask>>>,
    running: Arc<Mutex<BTreeMap<String, String>>>,
}

impl TaskBroker {
    pub fn sandbox_diagnostic(&self) -> ProcessSandboxDiagnostic {
        ProcessBroker::sandbox_diagnostic()
    }

    pub fn list(
        &self,
        provider: &TaskProvider,
        payload: Value,
    ) -> Result<Vec<TaskSummary>, TaskError> {
        parse_catalog(payload)?
            .into_iter()
            .map(|task| {
                authorize_execution(&task.execution, &provider.permissions)?;
                Ok(TaskSummary {
                    plugin_id: provider.id.clone(),
                    plugin_name: provider.name.clone(),
                    task_id: task.id,
                    label: task.label,
                    detail: task.detail,
                    access: task.execution.access,
                    network: task.execution.network,
                })
            })
            .collect()
    }

    pub fn review(
        &self,
        workspace: &Path,
        provider: &TaskProvider,
        task_id: &str,
        payload: Value,
    ) -> Result<(TaskReview, ProcessAuditEvent), TaskError> {
        let task = parse_catalog(payload)?
            .into_iter()
            .find(|task| task.id == task_id)
            .ok_or(TaskError::UnknownTask)?;
        let authority = authorize_execution(&task.execution, &provider.permissions)?;
        let (process, audit) = self.process.review(
            workspace,
            task.execution,
            ProcessOrigin::Plugin {
                plugin_id: provider.id.clone(),
            },
            authority,
        )?;
        let mut pending = match self.pending.lock() {
            Ok(pending) => pending,
            Err(_) => {
                let _ = self.process.discard_review(&process.review_token);
                return Err(TaskError::StateUnavailable);
            }
        };
        pending.insert(
            process.review_token.clone(),
            PendingTask {
                plugin_id: provider.id.clone(),
                process_id: process.process_id.clone(),
                permissions: provider.permissions.clone(),
            },
        );
        Ok((
            TaskReview {
                plugin_id: provider.id.clone(),
                plugin_name: provider.name.clone(),
                task_id: task.id,
                label: task.label,
                detail: task.detail,
                process,
            },
            audit,
        ))
    }

    pub fn pending_plugin_id(&self, review_token: &str) -> Result<String, TaskError> {
        self.pending
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .get(review_token)
            .map(|pending| pending.plugin_id.clone())
            .ok_or(TaskError::ReviewNotFound)
    }

    pub fn execute(
        &self,
        review_token: &str,
        current_permissions: &BTreeSet<PluginPermission>,
        emit: Arc<dyn Fn(ProcessOutputEvent) + Send + Sync>,
    ) -> Result<(ProcessResult, Vec<ProcessAuditEvent>), TaskError> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .remove(review_token)
            .ok_or(TaskError::ReviewNotFound)?;
        if &pending.permissions != current_permissions {
            let _ = self.process.discard_review(review_token);
            return Err(TaskError::AuthorizationChanged);
        }
        self.running
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .insert(pending.process_id.clone(), pending.plugin_id);
        let result = self
            .process
            .execute(review_token, emit)
            .map_err(TaskError::from);
        self.running
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .remove(&pending.process_id);
        result
    }

    pub fn discard(&self, review_token: &str) -> Result<(), TaskError> {
        self.pending
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .remove(review_token)
            .ok_or(TaskError::ReviewNotFound)?;
        self.process.discard_review(review_token)?;
        Ok(())
    }

    pub fn cancel(&self, process_id: &str) -> Result<(), TaskError> {
        self.process.cancel(process_id).map_err(TaskError::from)
    }

    pub fn invalidate_plugin(&self, plugin_id: &str) -> Result<(), TaskError> {
        let tokens: Vec<_> = self
            .pending
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .iter()
            .filter(|(_, pending)| pending.plugin_id == plugin_id)
            .map(|(token, _)| token.clone())
            .collect();
        for token in tokens {
            let _ = self.discard(&token);
        }
        let process_ids: Vec<_> = self
            .running
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .iter()
            .filter(|(_, owner)| owner.as_str() == plugin_id)
            .map(|(process_id, _)| process_id.clone())
            .collect();
        for process_id in process_ids {
            let _ = self.cancel(&process_id);
        }
        Ok(())
    }

    pub fn invalidate_all(&self) -> Result<(), TaskError> {
        let tokens: Vec<_> = self
            .pending
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .keys()
            .cloned()
            .collect();
        for token in tokens {
            let _ = self.discard(&token);
        }
        let process_ids: Vec<_> = self
            .running
            .lock()
            .map_err(|_| TaskError::StateUnavailable)?
            .keys()
            .cloned()
            .collect();
        for process_id in process_ids {
            let _ = self.cancel(&process_id);
        }
        Ok(())
    }
}

fn parse_catalog(payload: Value) -> Result<Vec<PluginTaskDefinition>, TaskError> {
    let catalog: PluginTaskCatalog =
        serde_json::from_value(payload).map_err(|_| TaskError::InvalidCatalog)?;
    if catalog.items.len() > MAX_TASKS_PER_PROVIDER {
        return Err(TaskError::InvalidCatalog);
    }
    let mut ids = BTreeSet::new();
    for task in &catalog.items {
        if !valid_task_id(&task.id)
            || !valid_text(&task.label, MAX_TASK_LABEL_BYTES)
            || task
                .detail
                .as_ref()
                .is_some_and(|detail| !valid_text(detail, MAX_TASK_DETAIL_BYTES))
            || !ids.insert(task.id.clone())
            || task.execution.access == ProcessAccess::Escalated
        {
            return Err(TaskError::InvalidCatalog);
        }
    }
    Ok(catalog.items)
}

fn authorize_execution(
    execution: &ProcessRequest,
    permissions: &BTreeSet<PluginPermission>,
) -> Result<ProcessAuthority, TaskError> {
    if !permissions.contains(&PluginPermission::ProcessSpawn)
        || !permissions.contains(&PluginPermission::WorkspaceRead)
        || execution.access == ProcessAccess::Escalated
        || (execution.access == ProcessAccess::WorkspaceWrite
            && !permissions.contains(&PluginPermission::WorkspaceWrite))
        || (execution.network && !permissions.contains(&PluginPermission::NetworkAccess))
    {
        return Err(TaskError::PermissionDenied);
    }
    Ok(ProcessAuthority {
        workspace_write: permissions.contains(&PluginPermission::WorkspaceWrite),
        network: permissions.contains(&PluginPermission::NetworkAccess),
        escalated: false,
    })
}

fn valid_task_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= maximum
        && !value.contains('\0')
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use serde_json::json;

    use super::*;
    use crate::process_broker::{ProcessCommand, ProcessShell};

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lyrnova-task-broker-test-{}",
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

    fn provider(permissions: &[PluginPermission]) -> TaskProvider {
        TaskProvider {
            id: "io.github.example.tasks".into(),
            name: "Example Tasks".into(),
            permissions: permissions.iter().copied().collect(),
        }
    }

    fn catalog(execution: ProcessRequest) -> Value {
        json!({
            "items": [{
                "id": "check",
                "label": "Check project",
                "detail": "Runs the safe checker",
                "execution": execution,
            }]
        })
    }

    fn request(access: ProcessAccess, network: bool) -> ProcessRequest {
        ProcessRequest {
            command: ProcessCommand::Argv {
                program: "true".into(),
                args: Vec::new(),
            },
            cwd: None,
            environment: BTreeMap::new(),
            access,
            network,
            timeout_ms: 5_000,
        }
    }

    #[test]
    fn catalogs_are_typed_bounded_and_reject_duplicate_ids() {
        let broker = TaskBroker::default();
        let provider = provider(&[
            PluginPermission::ProcessSpawn,
            PluginPermission::WorkspaceRead,
        ]);
        let items = broker
            .list(&provider, catalog(request(ProcessAccess::ReadOnly, false)))
            .unwrap();
        assert_eq!(items[0].task_id, "check");
        assert_eq!(items[0].plugin_id, provider.id);

        let duplicate = json!({
            "items": [
                { "id": "same", "label": "One", "execution": request(ProcessAccess::ReadOnly, false) },
                { "id": "same", "label": "Two", "execution": request(ProcessAccess::ReadOnly, false) }
            ]
        });
        assert_eq!(
            broker.list(&provider, duplicate),
            Err(TaskError::InvalidCatalog)
        );
        assert_eq!(
            broker.list(
                &provider,
                json!({ "items": [], "frontendAuthority": { "escalated": true } }),
            ),
            Err(TaskError::InvalidCatalog)
        );
    }

    #[test]
    fn grants_independently_control_write_and_network_and_never_allow_escalated() {
        let read = provider(&[
            PluginPermission::ProcessSpawn,
            PluginPermission::WorkspaceRead,
        ]);
        assert!(
            authorize_execution(&request(ProcessAccess::ReadOnly, false), &read.permissions)
                .is_ok()
        );
        assert_eq!(
            authorize_execution(
                &request(ProcessAccess::WorkspaceWrite, false),
                &read.permissions
            ),
            Err(TaskError::PermissionDenied)
        );
        assert_eq!(
            authorize_execution(&request(ProcessAccess::ReadOnly, true), &read.permissions),
            Err(TaskError::PermissionDenied)
        );
        assert_eq!(
            authorize_execution(&request(ProcessAccess::Escalated, false), &read.permissions),
            Err(TaskError::PermissionDenied)
        );
    }

    #[test]
    fn review_uses_the_plugin_definition_and_grant_changes_invalidate_its_token() {
        let workspace = TestWorkspace::new();
        let broker = TaskBroker::default();
        let provider = provider(&[
            PluginPermission::ProcessSpawn,
            PluginPermission::WorkspaceRead,
        ]);
        let payload = catalog(ProcessRequest {
            command: ProcessCommand::Shell {
                shell: ProcessShell::Sh,
                script: "printf reviewed".into(),
            },
            ..request(ProcessAccess::ReadOnly, false)
        });
        let (review, _) = broker
            .review(&workspace.0, &provider, "check", payload)
            .unwrap();
        assert_eq!(review.process.command, "printf reviewed");
        assert_eq!(review.process.origin, "plugin:io.github.example.tasks");

        let changed = [
            PluginPermission::ProcessSpawn,
            PluginPermission::WorkspaceRead,
            PluginPermission::NetworkAccess,
        ]
        .into_iter()
        .collect();
        assert_eq!(
            broker.execute(&review.process.review_token, &changed, Arc::new(|_| {})),
            Err(TaskError::AuthorizationChanged)
        );
    }

    #[test]
    fn an_unchanged_grant_consumes_the_review_exactly_once() {
        let workspace = TestWorkspace::new();
        let broker = TaskBroker::default();
        let provider = provider(&[
            PluginPermission::ProcessSpawn,
            PluginPermission::WorkspaceRead,
        ]);
        let (review, _) = broker
            .review(
                &workspace.0,
                &provider,
                "check",
                catalog(request(ProcessAccess::ReadOnly, false)),
            )
            .unwrap();
        let token = review.process.review_token;
        let result = broker.execute(&token, &provider.permissions, Arc::new(|_| {}));
        if review.process.sandbox == crate::process_broker::SandboxStrength::Strong {
            assert_eq!(result.unwrap().0.exit_code, Some(0));
        } else {
            assert_eq!(
                result,
                Err(TaskError::Process(ProcessBrokerError::SandboxUnavailable))
            );
        }
        assert_eq!(
            broker.execute(&token, &provider.permissions, Arc::new(|_| {})),
            Err(TaskError::ReviewNotFound)
        );
    }
}
