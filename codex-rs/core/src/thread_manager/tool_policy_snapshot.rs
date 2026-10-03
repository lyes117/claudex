use codex_extension_api::ToolPolicy;
use codex_protocol::ToolPolicySnapshot;
use codex_protocol::ToolPolicySnapshotFields;

use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;

pub(crate) fn snapshot_tool_policy(policy: &ToolPolicy) -> CodexResult<serde_json::Value> {
    let snapshot = ToolPolicySnapshot::try_new(ToolPolicySnapshotFields {
        allowed_tools: policy.allowed_tools.clone(),
        require_managed_sandbox: policy.require_managed_sandbox,
        require_unified_exec: policy.require_unified_exec,
        expose_additional_permissions: policy.expose_additional_permissions,
    })
    .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
    serde_json::to_value(snapshot)
        .map_err(|_| CodexErr::InvalidRequest("cannot serialize creation tool policy".to_string()))
}
