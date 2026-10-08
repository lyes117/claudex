use super::*;
use pretty_assertions::assert_eq;

#[test]
fn invocation_metadata_retains_hidden_commands_and_argument_hints() {
    let (metadata, _) = codex_config::claude::markdown(
        "---\nuser-invocable: false\nargument-hint: '[branche avec espaces]'\n---\nBody",
    )
    .expect("valid Markdown");
    assert_eq!(
        parse(&metadata),
        Ok(ClaudeCommandMetadata {
            user_invocable: false,
            argument_hint: Some("[branche avec espaces]".to_string()),
        })
    );
    assert!(parse(&serde_json::json!({"user-invocable": "false"})).is_err());
    assert!(parse(&serde_json::json!({"argument-hint": "x".repeat(1025)})).is_err());
}
