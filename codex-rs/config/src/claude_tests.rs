use super::*;
use pretty_assertions::assert_eq;

#[test]
fn native_skills_and_ignored_user_plugins_keep_their_scope() {
    assert!(!is_markdown_source(Path::new(
        ".agents/skills/example/SKILL.md"
    )));
    assert!(is_markdown_source(Path::new(".claude/commands/example.md")));
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join(".git")).unwrap();
    let directory = project.path().join(".claude");
    fs::create_dir(&directory).unwrap();
    assert!(plugins_for_scope(&directory, false).unwrap().is_empty());
    if let Some(home) = home() {
        let no_git = home.parent().unwrap().join(".claudex-test-no-git/.claude");
        assert!(plugins_for_scope(&no_git, false).unwrap().is_empty());
    }
}

#[test]
fn markdown_agent_keeps_body_and_uses_codex_model() {
    let (name, description, config) = agent_config("---\r\nname: reviewer\r\ndescription: Review changes\r\nmodel: sonnet\r\n---\r\nInspect security.", Path::new("reviewer.md")).unwrap();
    assert_eq!(
        (name, description, config),
        (
            "reviewer".into(),
            Some("Review changes".into()),
            toml::toml! { developer_instructions = "Inspect security." }.into()
        )
    );
}

#[test]
fn malformed_or_unenforceable_metadata_is_rejected() {
    assert!(markdown("---\nscalar\n---\nbody").is_err());
    assert!(agent_config("---\ntools: Read, Edit\n---\nbody", Path::new("agent.md")).is_err());
}

#[test]
fn command_expansion_preserves_literals_and_merges_permission_lists() {
    let mut settings = serde_json::json!({"permissions":{"deny":["Bash(rm:*)"]}});
    merge_settings(
        &mut settings,
        serde_json::json!({"permissions":{"deny":["Bash(del:*)"]}}),
    );
    assert_eq!(
        settings,
        serde_json::json!({"permissions":{"deny":["Bash(rm:*)", "Bash(del:*)"]}})
    );
    let metadata = serde_json::json!({"arguments":["first"]});
    assert_eq!(
        expand_command(
            &metadata,
            "$ARGUMENTS|$ARGUMENTS[1]|$0|$first|\\$1",
            "\"hello world\" second",
            &["hello world".into(), "second".into()],
            Path::new("command.md")
        )
        .unwrap(),
        "\"hello world\" second|second|hello world|hello world|$1"
    );
    assert!(validate_skill(&serde_json::json!({"disallowed-tools":"Bash"})).is_err());
}

#[test]
fn absent_disabled_mcp_server_does_not_create_invalid_transport() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("settings.json"),
        r#"{"disabledMcpjsonServers":["missing"]}"#,
    )
    .unwrap();
    let config = native_config(temp.path(), &mut Vec::new()).unwrap();
    assert!(config.get("mcp_servers").is_none());
}

#[test]
fn local_settings_override_and_mcp_is_read_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join(".claude");
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("settings.json"),
        r#"{"env":{"CLAUDEX_FIXTURE":"original"},"permissions":{"deny":["Bash(rm:*)"]}}"#,
    )
    .unwrap();
    fs::write(
        directory.join("settings.local.json"),
        r#"{"env":{"CLAUDEX_FIXTURE":"local"}}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".mcp.json"),
        r#"{"mcpServers":{"fixture":{"command":"node","args":["server.mjs"],"type":"stdio"}}}"#,
    )
    .unwrap();
    let config = native_config(&directory, &mut Vec::new()).unwrap();
    assert_eq!(
        config
            .get("shell_environment_policy")
            .unwrap()
            .get("set")
            .unwrap()
            .get("CLAUDEX_FIXTURE")
            .unwrap()
            .as_str(),
        Some("local")
    );
    assert_eq!(
        config
            .get("mcp_servers")
            .unwrap()
            .get("fixture")
            .unwrap()
            .get("command")
            .unwrap()
            .as_str(),
        Some("node")
    );
    assert!(
        permission_block_in_directories(
            &[directory],
            "Bash",
            &serde_json::json!({"command":"echo safe && rm dangerous"})
        )
        .unwrap()
        .is_some()
    );
    assert!(temp.path().join(".mcp.json").is_file());
    assert!(!temp.path().join(".codex").exists());
}

#[test]
fn hook_events_are_filtered_without_losing_supported_events() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("settings.json"), r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo hello"},{"type":"http","url":"https://example.invalid"}]}],"Notification":[]}}"#).unwrap();
    let mut warnings = Vec::new();
    let sources = hook_sources_for_scope(temp.path(), None, false, &mut warnings).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].1.session_start.len(), 1);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("Notification"))
    );
}
