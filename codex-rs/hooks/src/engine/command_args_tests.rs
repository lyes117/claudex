use super::*;

fn expand_args(program: &str, args: &[String], event: &str, cwd: &Path) -> Result<Vec<String>, ()> {
    super::expand_args(program, args, event, cwd, &HashMap::new())
}

#[test]
fn structured_plugin_root_is_bounded_and_never_reinterpreted() {
    let env = HashMap::from([(
        "CLAUDE_PLUGIN_ROOT".to_string(),
        "root/${tool_input.other}".to_string(),
    )]);
    assert_eq!(
        super::expand_args(
            "node",
            &["${CLAUDE_PLUGIN_ROOT}/hook.js".to_string()],
            r#"{"tool_input":{"other":"must-not-expand"}}"#,
            Path::new("."),
            &env
        ),
        Ok(vec!["root/${tool_input.other}/hook.js".to_string()])
    );
    let env = HashMap::from([("CLAUDE_PLUGIN_ROOT".to_string(), "x".repeat(900_000))]);
    assert_eq!(
        super::expand_args(
            "node",
            &["${CLAUDE_PLUGIN_ROOT}".repeat(3_000)],
            "{}",
            Path::new("."),
            &env
        ),
        Err(())
    );
    assert_eq!(
        expand_program("${CLAUDE_PLUGIN_ROOT}/hook.exe", &env),
        Err(())
    );
}

#[test]
fn structured_args_preserve_empty_unicode_and_shell_metacharacters() {
    let value = "C:\\résumé\\a & echo injected; $(whoami) `x` \"quoted\"";
    let event = serde_json::json!({"tool_input": {"file_path": value}}).to_string();
    let args = vec![
        "".to_string(),
        "${tool_input.file_path}".to_string(),
        "prefix:${tool_input.file_path}".to_string(),
    ];
    assert_eq!(
        expand_args("node", &args, &event, Path::new(".")),
        Ok(vec![
            String::new(),
            value.to_string(),
            format!("prefix:{value}")
        ])
    );
}

#[test]
fn structured_args_resolve_project_dir_without_reinterpreting_payload() {
    let value = "${tool_input.other}";
    let event = serde_json::json!({"tool_input": {"file_path": value, "other": "must-not-expand"}})
        .to_string();
    assert_eq!(
        expand_args(
            "node",
            &["${CLAUDE_PROJECT_DIR}/${tool_input.file_path}".to_string()],
            &event,
            Path::new("project")
        ),
        Ok(vec![format!("project/{value}")])
    );
}

#[test]
fn structured_args_fail_closed_for_missing_non_string_or_invalid_event() {
    for event in [
        "{}",
        r#"{"tool_input":{"file_path":null}}"#,
        r#"{"tool_input":{"file_path":9}}"#,
        "invalid",
    ] {
        assert_eq!(
            expand_args(
                "node",
                &["${tool_input.file_path}".to_string()],
                event,
                Path::new(".")
            ),
            Err(())
        );
    }
}

#[test]
fn structured_args_bound_source_and_expanded_values() {
    assert_eq!(
        expand_args(
            "node",
            &vec![String::new(); MAX_ARGS + 1],
            "{}",
            Path::new(".")
        ),
        Err(())
    );
    assert_eq!(
        expand_args(
            "node",
            &["x".repeat(MAX_ARG_BYTES + 1)],
            "{}",
            Path::new(".")
        ),
        Err(())
    );
    let event = serde_json::json!({"large": "x".repeat(MAX_ARG_BYTES + 1)}).to_string();
    assert_eq!(
        expand_args("node", &["${large}".to_string()], &event, Path::new(".")),
        Err(())
    );
    assert_eq!(
        expand_args(
            "node",
            &[],
            &" ".repeat(MAX_EVENT_BYTES + 1),
            Path::new(".")
        ),
        Err(())
    );
    assert_eq!(
        expand_args("node\0secret", &[], "{}", Path::new(".")),
        Err(())
    );
    assert_eq!(
        expand_args("node", &["a\0b".to_string()], "{}", Path::new(".")),
        Err(())
    );
    assert_eq!(
        expand_args(
            "node",
            &["${value}".to_string()],
            r#"{"value":"a\u0000b"}"#,
            Path::new(".")
        ),
        Err(())
    );
}

#[test]
fn structured_args_refuse_amplification_before_allocating_the_expanded_output() {
    let event = serde_json::json!({"large":"x".repeat(900_000)}).to_string();
    let template = "${large}".repeat(8_000);
    assert!(template.len() <= MAX_ARG_BYTES);
    assert!(event.len() <= MAX_EVENT_BYTES);
    // The first append is refused; no multi-gigabyte intermediate is constructed.
    assert_eq!(
        expand_args("node", &[template], &event, Path::new(".")),
        Err(())
    );
    let event = serde_json::json!({"part":"x".repeat(40_000)}).to_string();
    assert_eq!(
        expand_args(
            "node",
            &["${part}".to_string(), "${part}".to_string()],
            &event,
            Path::new(".")
        ),
        Err(())
    );
}

#[cfg(windows)]
#[test]
fn structured_args_refuse_implicit_windows_batch_shell() {
    for program in ["hook.cmd", "C:\\hooks\\HOOK.BAT"] {
        assert_eq!(
            expand_args(
                program,
                &["& echo injected".to_string()],
                "{}",
                Path::new(".")
            ),
            Err(())
        );
    }
}
