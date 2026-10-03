use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn fields(allowed_tools: Option<Vec<ToolName>>) -> ToolPolicySnapshotFields {
    ToolPolicySnapshotFields {
        allowed_tools,
        require_managed_sandbox: true,
        require_unified_exec: false,
        expose_additional_permissions: false,
    }
}

fn valid_json() -> serde_json::Value {
    json!({"version":1,"allowed_tools":null,"require_managed_sandbox":true,
        "require_unified_exec":false,"expose_additional_permissions":false})
}

fn decode(value: &serde_json::Value) -> Result<ToolPolicySnapshot, ToolPolicySnapshotError> {
    ToolPolicySnapshot::from_json_slice(&serde_json::to_vec(value).unwrap())
}

#[test]
fn round_trip_preserves_empty_unrestricted_and_namespace_components() {
    for tools in [
        None,
        Some(vec![]),
        Some(vec![
            ToolName::plain("Read"),
            ToolName::namespaced("", "Grep"),
            ToolName::namespaced("functions", "Glob"),
            ToolName::namespaced("ab", "c"),
            ToolName::namespaced("a", "bc"),
        ]),
    ] {
        let input = fields(tools);
        let snapshot = ToolPolicySnapshot::try_new(input.clone()).unwrap();
        let restored =
            ToolPolicySnapshot::from_json_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert_eq!(restored.fields(), &input);
        assert_eq!(restored, snapshot);
    }
}

#[test]
fn every_field_is_required_including_nullable_allowlist_and_namespace() {
    let valid = valid_json();
    for key in valid.as_object().unwrap().keys() {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            matches!(
                decode(&missing),
                Err(ToolPolicySnapshotError::InvalidJson(_))
            ),
            "{key}"
        );
    }
    let mut missing = valid;
    missing["allowed_tools"] = json!([{"name":"Read"}]);
    assert!(matches!(
        decode(&missing),
        Err(ToolPolicySnapshotError::InvalidJson(_))
    ));
}

#[test]
fn rejects_unknown_fields_types_versions_duplicates_and_trailing_data() {
    let mut unknown = valid_json();
    unknown["extra"] = json!(true);
    let mut nested_unknown = valid_json();
    nested_unknown["allowed_tools"] = json!([{"name":"Read","namespace":null,"extra":true}]);
    let mut bad_bool = valid_json();
    bad_bool["require_unified_exec"] = json!(null);
    let mut bad_list = valid_json();
    bad_list["allowed_tools"] = json!({});
    for value in [unknown, nested_unknown, bad_bool, bad_list] {
        assert!(matches!(
            decode(&value),
            Err(ToolPolicySnapshotError::InvalidJson(_))
        ));
    }
    for version in [0, 2, u32::MAX] {
        let mut value = valid_json();
        value["version"] = json!(version);
        assert!(matches!(
            decode(&value),
            Err(ToolPolicySnapshotError::UnsupportedVersion)
        ));
    }
    let valid = serde_json::to_string(&valid_json()).unwrap();
    for invalid in [
        valid.replacen(
            "\"version\":1",
            "\"version\":1,\"version\":1",
            /*count*/ 1,
        ),
        valid.replacen(
            "\"allowed_tools\":null",
            "\"allowed_tools\":null,\"allowed_tools\":[]",
            /*count*/ 1,
        ),
        format!("{valid} null"),
        valid.replace("\"version\":1", "\"version\":4294967296"),
    ] {
        assert!(matches!(
            ToolPolicySnapshot::from_json_slice(invalid.as_bytes()),
            Err(ToolPolicySnapshotError::InvalidJson(_))
        ));
    }
    for duplicate in [
        r#"{"name":"Read","name":"Grep","namespace":null}"#,
        r#"{"name":"Read","namespace":null,"namespace":"mcp"}"#,
    ] {
        let invalid = valid.replace(
            "\"allowed_tools\":null",
            &format!("\"allowed_tools\":[{duplicate}]"),
        );
        assert!(matches!(
            ToolPolicySnapshot::from_json_slice(invalid.as_bytes()),
            Err(ToolPolicySnapshotError::InvalidJson(_))
        ));
    }
    assert!(matches!(
        ToolPolicySnapshot::from_json_slice(&[0xff]),
        Err(ToolPolicySnapshotError::InvalidJson(_))
    ));
}

#[test]
fn namespace_aliases_cannot_duplicate_a_permission() {
    for namespace in [None, Some("".into()), Some("functions".into())] {
        let tools = vec![ToolName::plain("Read"), ToolName::new(namespace, "Read")];
        assert!(matches!(
            ToolPolicySnapshot::try_new(fields(Some(tools))),
            Err(ToolPolicySnapshotError::DuplicateIdentity)
        ));
    }
}

#[test]
fn rejects_invalid_names_and_counts_at_decode_and_creation() {
    for tool in [
        ToolName::plain(""),
        ToolName::plain("Read\n"),
        ToolName::plain("x".repeat(MAX_COMPONENT_BYTES + 1)),
        ToolName::namespaced("\u{7f}", "Read"),
        ToolName::namespaced("x".repeat(MAX_COMPONENT_BYTES + 1), "Read"),
    ] {
        let mut value = valid_json();
        value["allowed_tools"] = json!([tool.clone()]);
        assert!(matches!(
            decode(&value),
            Err(ToolPolicySnapshotError::InvalidIdentity)
        ));
        assert!(matches!(
            ToolPolicySnapshot::try_new(fields(Some(vec![tool]))),
            Err(ToolPolicySnapshotError::InvalidIdentity)
        ));
    }
    let tools: Vec<_> = (0..=MAX_TOOLS)
        .map(|i| ToolName::plain(format!("t{i}")))
        .collect();
    let mut value = valid_json();
    value["allowed_tools"] = json!(tools);
    assert!(matches!(
        decode(&value),
        Err(ToolPolicySnapshotError::TooManyTools)
    ));
    assert!(matches!(
        ToolPolicySnapshot::try_new(fields(Some(tools.clone()))),
        Err(ToolPolicySnapshotError::TooManyTools)
    ));
    assert!(ToolPolicySnapshot::try_new(fields(Some(tools[..MAX_TOOLS].to_vec()))).is_ok());
    assert!(
        ToolPolicySnapshot::try_new(fields(Some(vec![ToolName::plain(
            "x".repeat(MAX_COMPONENT_BYTES)
        )])))
        .is_ok()
    );
}

#[test]
fn raw_and_canonical_byte_limits_cannot_be_bypassed_by_whitespace_or_escapes() {
    let snapshot = ToolPolicySnapshot::try_new(fields(/*allowed_tools*/ None)).unwrap();
    let mut bytes = serde_json::to_vec(&snapshot).unwrap();
    bytes.resize(MAX_JSON_BYTES, b' ');
    assert_eq!(
        ToolPolicySnapshot::from_json_slice(&bytes).unwrap(),
        snapshot
    );
    bytes.push(b' ');
    assert!(matches!(
        ToolPolicySnapshot::from_json_slice(&bytes),
        Err(ToolPolicySnapshotError::TooLarge)
    ));
    let tools = (0..MAX_TOOLS)
        .map(|i| ToolName::plain(format!("{i}{}", "x".repeat(64))))
        .collect();
    assert!(matches!(
        ToolPolicySnapshot::try_new(fields(Some(tools))),
        Err(ToolPolicySnapshotError::TooLarge)
    ));
    let escaped = serde_json::to_string(&valid_json()).unwrap().replace(
        "\"allowed_tools\":null",
        &format!(
            "\"allowed_tools\":[{{\"name\":\"{}\",\"namespace\":null}}]",
            "\\u0061".repeat(1400)
        ),
    );
    assert!(matches!(
        ToolPolicySnapshot::from_json_slice(escaped.as_bytes()),
        Err(ToolPolicySnapshotError::TooLarge)
    ));
}
