use super::*;

#[test]
fn old_catalog_without_metadata_deserializes_and_new_metadata_roundtrips() {
    let old = serde_json::json!({
        "name": "legacy", "description": "legacy command", "path": std::env::temp_dir().join("legacy.md"),
        "scope": "repo", "enabled": true, "pluginId": null,
    });
    let mut skill: super::super::SkillMetadata = serde_json::from_value(old).expect("old catalog");
    assert_eq!(skill.claude_command, None);
    skill.claude_command = Some(ClaudeCommandMetadata {
        user_invocable: false,
        argument_hint: Some("[日本語]".to_string()),
    });
    let serialized = serde_json::to_value(&skill).expect("catalog serialization");
    assert_eq!(
        serde_json::from_value::<super::super::SkillMetadata>(serialized)
            .expect("catalog roundtrip"),
        skill
    );
}
