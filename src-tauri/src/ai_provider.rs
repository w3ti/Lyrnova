use crate::{
    app_server::AgentRuntimeError,
    plugin_manifest::{PluginCapability, PluginPermission},
    plugins::{AiProviderRuntime, AiProviderSummary, PluginError, PluginRegistry},
};

const CODEX_APP_SERVER_MODULE: &str = "ai.codex";
const INTERACTIVE_CAPABILITIES: &[PluginCapability] = &[
    PluginCapability::AccountAuth,
    PluginCapability::AiChat,
    PluginCapability::AiTools,
    PluginCapability::Approvals,
];
const INTERACTIVE_PERMISSIONS: &[PluginPermission] = &[
    PluginPermission::WorkspaceRead,
    PluginPermission::ProcessSpawn,
    PluginPermission::NetworkAccess,
    PluginPermission::RequestApproval,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AiProviderAdapter {
    CodexAppServer,
}

pub(crate) fn resolve_ai_provider(
    registry: &PluginRegistry,
    capabilities: &[PluginCapability],
    permissions: &[PluginPermission],
) -> Result<AiProviderAdapter, AgentRuntimeError> {
    let provider = registry
        .active_ai_provider(capabilities, permissions)
        .map_err(map_provider_error)?;
    adapter_for_runtime(provider.runtime)
}

pub(crate) fn current_ai_provider(
    registry: &PluginRegistry,
) -> Result<Option<AiProviderSummary>, AgentRuntimeError> {
    let provider =
        match registry.active_ai_provider(INTERACTIVE_CAPABILITIES, INTERACTIVE_PERMISSIONS) {
            Ok(provider) => provider,
            Err(PluginError::NoAiProvider) => return Ok(None),
            Err(error) => return Err(map_provider_error(error)),
        };
    adapter_for_runtime(provider.runtime)?;
    Ok(Some(AiProviderSummary {
        id: provider.id,
        name: provider.name,
        capabilities: provider.capabilities,
    }))
}

fn adapter_for_runtime(runtime: AiProviderRuntime) -> Result<AiProviderAdapter, AgentRuntimeError> {
    match runtime {
        AiProviderRuntime::Builtin { module } if module == CODEX_APP_SERVER_MODULE => {
            Ok(AiProviderAdapter::CodexAppServer)
        }
        AiProviderRuntime::Builtin { .. } | AiProviderRuntime::Process => {
            Err(AgentRuntimeError::ProviderUnsupported)
        }
    }
}

fn map_provider_error(error: PluginError) -> AgentRuntimeError {
    match error {
        PluginError::NoAiProvider | PluginError::MultipleAiProviders => {
            AgentRuntimeError::ProviderUnavailable
        }
        PluginError::CapabilityUnavailable => AgentRuntimeError::ProviderCapabilityUnavailable,
        PluginError::PermissionDenied | PluginError::PermissionApprovalRequired => {
            AgentRuntimeError::PluginPermissionDenied
        }
        _ => AgentRuntimeError::PluginDisabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_installation_has_no_resolvable_ai_provider() {
        assert_eq!(
            resolve_ai_provider(
                &PluginRegistry::default(),
                &[PluginCapability::AiChat],
                &[PluginPermission::NetworkAccess],
            ),
            Err(AgentRuntimeError::ProviderUnavailable)
        );
        assert_eq!(current_ai_provider(&PluginRegistry::default()), Ok(None));
    }

    #[test]
    fn unknown_builtin_and_process_providers_require_a_typed_adapter() {
        assert_eq!(
            adapter_for_runtime(AiProviderRuntime::Builtin {
                module: "ai.example".into(),
            }),
            Err(AgentRuntimeError::ProviderUnsupported)
        );
        assert_eq!(
            adapter_for_runtime(AiProviderRuntime::Process),
            Err(AgentRuntimeError::ProviderUnsupported)
        );
    }
}
