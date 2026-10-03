use codex_extension_api::ToolName;
use codex_extension_api::ToolPolicy;
use pretty_assertions::assert_eq;

#[test]
fn composition_never_expands_either_ceiling() {
    let inventories = [
        None,
        Some(vec![]),
        Some(vec![ToolName::plain("Read")]),
        Some(vec![
            ToolName::namespaced("functions", "Read"),
            ToolName::plain("exec_command"),
        ]),
        Some(vec![ToolName::namespaced("mcp", "Read")]),
    ];
    let universe = [
        ToolName::plain("Read"),
        ToolName::namespaced("functions", "Read"),
        ToolName::plain("exec_command"),
        ToolName::namespaced("mcp", "Read"),
        ToolName::namespaced("another", "Read"),
    ];
    for left in &inventories {
        for right in &inventories {
            for bits in 0..64 {
                let parent = ToolPolicy {
                    allowed_tools: left.clone(),
                    require_managed_sandbox: bits & 1 != 0,
                    require_unified_exec: bits & 2 != 0,
                    expose_additional_permissions: bits & 4 != 0,
                };
                let child = ToolPolicy {
                    allowed_tools: right.clone(),
                    require_managed_sandbox: bits & 8 != 0,
                    require_unified_exec: bits & 16 != 0,
                    expose_additional_permissions: bits & 32 != 0,
                };
                let composed = parent.intersect(&child);
                assert!(composed.is_subset_of(&parent));
                assert!(composed.is_subset_of(&child));
                assert_eq!(
                    composed.allowed_tools.is_none(),
                    left.is_none() && right.is_none()
                );
                for tool in &universe {
                    assert_eq!(
                        composed.allows(tool),
                        parent.allows(tool) && child.allows(tool)
                    );
                }
                // A warm runtime must satisfy the independently supplied restrictions.
                assert_eq!(parent.is_subset_of(&composed), parent.is_subset_of(&child));
            }
        }
    }
}
