use super::*;

fn discover(args: Option<Vec<String>>) -> (ConfiguredHandler, HookListEntry) {
    let path =
        AbsolutePathBuf::try_from(std::env::temp_dir().join("claudex-argv-hook-fixture.json"))
            .unwrap();
    let states = HashMap::new();
    let mut source = HookHandlerSource {
        path: &path,
        key_source: path.display().to_string(),
        source: HookSource::User,
        is_managed: false,
        requirement: HookRequirement::Optional,
        bypass_hook_trust: true,
        hook_states: &states,
        env: HashMap::from([("CLAUDE_PLUGIN_ROOT".to_string(), "plugin-root".to_string())]),
        plugin_id: None,
    };
    let mut handlers = Vec::new();
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut order = 0;
    append_matcher_groups(
        &mut handlers,
        &mut entries,
        &mut warnings,
        &mut order,
        &mut source,
        HookEventName::PostToolUse,
        vec![MatcherGroup {
            matcher: Some("Edit|Write".to_string()),
            hooks: vec![HookHandlerConfig::Command {
                command: "node".to_string(),
                args,
                command_windows: Some("windows-node.exe".to_string()),
                timeout_sec: Some(5),
                r#async: false,
                status_message: None,
                additional_context_limit: None,
            }],
        }],
    );
    assert!(warnings.is_empty());
    (handlers.remove(0), entries.remove(0))
}

#[test]
fn structured_args_flow_through_plugin_expansion_listing_and_trust() {
    let template = vec![
        "${CLAUDE_PLUGIN_ROOT}/hook.js".to_string(),
        "${tool_input.file_path}".to_string(),
    ];
    let (handler, entry) = discover(Some(template.clone()));
    let ConfiguredHandlerKind::Command { command, args, .. } = handler.kind else {
        panic!("command expected")
    };
    assert_eq!(
        command,
        if cfg!(windows) {
            "windows-node.exe"
        } else {
            "node"
        }
    );
    assert_eq!(args, Some(template));
    assert_eq!(
        entry.handler,
        HookListEntryHandler::Command {
            command,
            args,
            r#async: false
        }
    );
    let (_, legacy) = discover(None);
    let (_, empty) = discover(Some(Vec::new()));
    let (_, changed) = discover(Some(vec!["different.js".to_string()]));
    assert_ne!(legacy.current_hash, empty.current_hash);
    assert_ne!(empty.current_hash, entry.current_hash);
    assert_ne!(changed.current_hash, entry.current_hash);
}
