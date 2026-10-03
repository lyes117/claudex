use crate::ToolName;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;

const MAX_JSON_BYTES: usize = 8192;
const MAX_TOOLS: usize = 128;
const MAX_COMPONENT_BYTES: usize = 256;

/// Policy data supplied by the native authority, never by a model tool argument.
/// `None` grants an unrestricted allowlist; `Some([])` grants no tools.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolPolicySnapshotFields {
    pub allowed_tools: Option<Vec<ToolName>>,
    pub require_managed_sandbox: bool,
    pub require_unified_exec: bool,
    pub expose_additional_permissions: bool,
}

/// Validated version-one creation ceiling. This format alone does not restore
/// authority: the owning history reader must validate its thread identity too.
/// Decode persisted data only through `from_json_slice`, which checks the raw
/// byte bound before allocating JSON structures. There is intentionally no
/// general-purpose `Deserialize` implementation bypassing that boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolPolicySnapshot {
    version: u32,
    #[serde(flatten)]
    fields: ToolPolicySnapshotFields,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolPolicySnapshotError {
    #[error("tool policy snapshot exceeds its byte limit")]
    TooLarge,
    #[error("invalid tool policy snapshot JSON")]
    InvalidJson(#[source] serde_json::Error),
    #[error("unsupported tool policy snapshot version")]
    UnsupportedVersion,
    #[error("tool policy snapshot exceeds its tool count limit")]
    TooManyTools,
    #[error("invalid tool policy snapshot tool identity")]
    InvalidIdentity,
    #[error("duplicate tool policy snapshot tool identity")]
    DuplicateIdentity,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    version: u32,
    #[serde(deserialize_with = "required_tools")]
    allowed_tools: Option<Vec<WireTool>>,
    require_managed_sandbox: bool,
    require_unified_exec: bool,
    expose_additional_permissions: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTool {
    name: String,
    #[serde(deserialize_with = "required_namespace")]
    namespace: Option<String>,
}

fn required_tools<'de, D>(deserializer: D) -> Result<Option<Vec<WireTool>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::deserialize(deserializer)
}

fn required_namespace<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::deserialize(deserializer)
}

impl ToolPolicySnapshot {
    pub fn try_new(fields: ToolPolicySnapshotFields) -> Result<Self, ToolPolicySnapshotError> {
        if let Some(tools) = &fields.allowed_tools {
            if tools.len() > MAX_TOOLS {
                return Err(ToolPolicySnapshotError::TooManyTools);
            }
            let mut identities = HashSet::with_capacity(tools.len());
            for tool in tools {
                let valid_component =
                    |s: &str| s.len() <= MAX_COMPONENT_BYTES && !s.chars().any(char::is_control);
                if tool.name.is_empty()
                    || !valid_component(&tool.name)
                    || tool
                        .namespace
                        .as_deref()
                        .is_some_and(|s| !valid_component(s))
                {
                    return Err(ToolPolicySnapshotError::InvalidIdentity);
                }
                let namespace = if tool.is_default_namespace() {
                    None
                } else {
                    tool.namespace.as_deref()
                };
                if !identities.insert((namespace, tool.name.as_str())) {
                    return Err(ToolPolicySnapshotError::DuplicateIdentity);
                }
            }
        }
        let snapshot = Self { version: 1, fields };
        if serde_json::to_vec(&snapshot)
            .map_err(ToolPolicySnapshotError::InvalidJson)?
            .len()
            > MAX_JSON_BYTES
        {
            return Err(ToolPolicySnapshotError::TooLarge);
        }
        Ok(snapshot)
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, ToolPolicySnapshotError> {
        if bytes.len() > MAX_JSON_BYTES {
            return Err(ToolPolicySnapshotError::TooLarge);
        }
        let wire: WireSnapshot =
            serde_json::from_slice(bytes).map_err(ToolPolicySnapshotError::InvalidJson)?;
        if wire.version != 1 {
            return Err(ToolPolicySnapshotError::UnsupportedVersion);
        }
        Self::try_new(ToolPolicySnapshotFields {
            allowed_tools: wire.allowed_tools.map(|tools| {
                tools
                    .into_iter()
                    .map(|tool| ToolName {
                        name: tool.name,
                        namespace: tool.namespace,
                    })
                    .collect()
            }),
            require_managed_sandbox: wire.require_managed_sandbox,
            require_unified_exec: wire.require_unified_exec,
            expose_additional_permissions: wire.expose_additional_permissions,
        })
    }

    pub fn fields(&self) -> &ToolPolicySnapshotFields {
        &self.fields
    }
}

#[cfg(test)]
#[path = "tool_policy_snapshot_tests.rs"]
mod tests;
