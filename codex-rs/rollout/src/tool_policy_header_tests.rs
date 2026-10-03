use super::*;
use crate::recorder::RolloutRecorder;
use codex_protocol::ToolName;
use codex_protocol::ToolPolicySnapshotFields;
use codex_protocol::protocol::SessionMeta;
use pretty_assertions::assert_eq;
use serde_json::json;

fn header_line() -> serde_json::Value {
    json!({"timestamp":"2026-10-03T00:00:00Z","type":"session_meta",
        "payload":SessionMeta::default()})
}

fn policy() -> serde_json::Value {
    json!({"version":1,"allowed_tools":[],"require_managed_sandbox":false,
        "require_unified_exec":true,"expose_additional_permissions":false})
}

#[test]
fn canonical_identity_and_ceiling_are_validated_from_original_bytes() {
    let mut value = header_line();
    let thread_id: ThreadId = serde_json::from_value(value["payload"]["id"].clone()).unwrap();
    assert!(
        parse_session_policy_header(&value.to_string())
            .unwrap()
            .unwrap()
            .tool_policy
            .is_none()
    );
    value["payload"]["tool_policy_snapshot"] = policy();
    let header = parse_session_policy_header(&value.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(header.thread_id, thread_id);
    assert_eq!(
        header.tool_policy.unwrap().fields(),
        &ToolPolicySnapshotFields {
            allowed_tools: Some(vec![]),
            require_managed_sandbox: false,
            require_unified_exec: true,
            expose_additional_permissions: false,
        }
    );
    // Equal Display strings must remain different tuple identities.
    value["payload"]["tool_policy_snapshot"]["allowed_tools"] = json!([
        ToolName::namespaced("a", "bc"),
        ToolName::namespaced("ab", "c")
    ]);
    assert!(parse_session_policy_header(&value.to_string()).is_ok());
    let escaped = value
        .to_string()
        .replace("session_meta", "session_\\u006deta");
    assert!(parse_session_policy_header(&escaped).unwrap().is_some());
}

#[test]
fn explicit_null_invalid_identity_and_invalid_metadata_never_mean_legacy() {
    for field in ["tool_policy_snapshot", "id", "cwd", "history_mode"] {
        let mut value = header_line();
        value["payload"][field] = json!(null);
        assert_eq!(
            parse_session_policy_header(&value.to_string())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData,
            "{field}"
        );
    }
    let mut value = header_line();
    value["payload"]["tool_policy_snapshot"] = policy();
    value["payload"]
        .as_object_mut()
        .unwrap()
        .remove("originator");
    assert!(parse_session_policy_header(&value.to_string()).is_err());
}

#[test]
fn duplicate_authority_keys_and_raw_size_are_checked_before_value_conversion() {
    let mut value = header_line();
    value["payload"]["tool_policy_snapshot"] = policy();
    let line = value.to_string();
    let id = value["payload"]["id"].to_string();
    for invalid in [
        line.replacen(
            "\"type\":\"session_meta\"",
            "\"type\":\"session_meta\",\"type\":\"session_meta\"",
            /*count*/ 1,
        ),
        line.replacen(
            &format!("\"id\":{id}"),
            &format!("\"id\":{id},\"id\":{id}"),
            /*count*/ 1,
        ),
        line.replacen(
            "\"version\":1",
            "\"version\":1,\"version\":1",
            /*count*/ 1,
        ),
        line.replacen(
            "\"tool_policy_snapshot\":",
            &format!(
                "\"tool_policy_snapshot\":{},\"tool_policy_snapshot\":",
                policy()
            ),
            /*count*/ 1,
        ),
        line.replacen(
            "\"allowed_tools\":[]",
            &format!("\"allowed_tools\":[{}]", " ".repeat(8192)),
            /*count*/ 1,
        ),
        line.replacen(
            "\"payload\":",
            &format!("\"payload\":{},\"payload\":", value["payload"]),
            /*count*/ 1,
        ),
    ] {
        assert_eq!(
            parse_session_policy_header(&invalid).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[tokio::test]
async fn canonical_invalid_header_cannot_fall_back_to_valid_ancestor() {
    let ancestor = header_line();
    let mut canonical = header_line();
    canonical["payload"]["tool_policy_snapshot"] = policy();
    let mut invalid_headers = vec!["{broken json".into()];
    for kind in [json!(null), json!("event_msg"), json!("future_record")] {
        let mut wrong_envelope = canonical.clone();
        wrong_envelope["type"] = kind;
        invalid_headers.push(wrong_envelope.to_string());
    }
    let mut missing_kind = canonical.clone();
    missing_kind.as_object_mut().unwrap().remove("type");
    invalid_headers.push(missing_kind.to_string());
    for malformed in [
        json!(null),
        json!({}),
        json!({"version":2}),
        json!({"version":1,"allowed_tools":null,"require_managed_sandbox":false,
            "require_unified_exec":false,"expose_additional_permissions":true,"extra":true}),
    ] {
        canonical["payload"]["tool_policy_snapshot"] = malformed;
        invalid_headers.push(canonical.to_string());
    }
    canonical["payload"]
        .as_object_mut()
        .unwrap()
        .remove("originator");
    invalid_headers.push(canonical.to_string());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    for invalid in invalid_headers {
        tokio::fs::write(&path, format!("{invalid}\n{ancestor}\n"))
            .await
            .unwrap();
        assert_eq!(
            RolloutRecorder::load_rollout_items(&path)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            crate::list::read_session_meta_line(&path)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[tokio::test]
async fn legacy_header_and_later_invalid_ancestor_policy_remain_readable() {
    let canonical = header_line();
    let mut ancestor = header_line();
    ancestor["payload"]["tool_policy_snapshot"] = json!({"version":2});
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    tokio::fs::write(&path, format!("\n{canonical}\n{ancestor}\n{{broken late\n"))
        .await
        .unwrap();
    let (items, id, errors) = RolloutRecorder::load_rollout_items(&path).await.unwrap();
    assert_eq!(
        (items.len(), id, errors),
        (
            2,
            Some(serde_json::from_value(canonical["payload"]["id"].clone()).unwrap()),
            1
        )
    );
    assert_eq!(
        crate::list::read_session_meta_line(&path)
            .await
            .unwrap()
            .meta
            .id,
        id.unwrap()
    );
}
