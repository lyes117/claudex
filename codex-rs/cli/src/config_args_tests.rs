use crate::config_args;
use clap::error::ErrorKind;
use pretty_assertions::assert_eq;

#[test]
fn global_config_args_keep_overrides_from_three_command_levels() {
    let cli = config_args::try_parse_from([
        "claudex",
        "-c",
        "model=first",
        "exec",
        "-c",
        "openai_base_url=http://fixture.invalid",
        "resume",
        "--last",
        "-c",
        "model=last",
    ])
    .expect("parse configuration across command levels");
    assert_eq!(
        cli.config_overrides.raw_overrides,
        vec![
            "model=first",
            "openai_base_url=http://fixture.invalid",
            "model=last",
        ]
    );
}

#[test]
fn global_config_args_keep_attached_flags_and_alias_order() {
    let cli = config_args::try_parse_from([
        "claudex",
        "-cmodel=first",
        "e",
        "--config=note=a=b",
        "resume",
        "--last",
        "--config",
        "model=last",
    ])
    .expect("parse attached configuration and exec alias");
    assert_eq!(
        cli.config_overrides.raw_overrides,
        vec!["model=first", "note=a=b", "model=last"]
    );
}

#[test]
fn global_config_args_leave_literal_prompt_after_separator() {
    let cli = config_args::try_parse_from([
        "claudex",
        "-c",
        "model=first",
        "exec",
        "--",
        "-cmodel=literal",
    ])
    .expect("parse literal prompt");
    assert_eq!(cli.config_overrides.raw_overrides, vec!["model=first"]);
    let Some(crate::Subcommand::Exec(exec)) = cli.subcommand else {
        panic!("expected exec")
    };
    assert_eq!(exec.prompt.as_deref(), Some("-cmodel=literal"));
}

#[test]
fn global_config_args_leave_mcp_child_configuration_after_separator() {
    let cli = config_args::try_parse_from([
        "claudex",
        "-c",
        "model=first",
        "mcp",
        "add",
        "fixture",
        "--",
        "node",
        "-c",
        "child=true",
    ])
    .expect("parse MCP child arguments");
    assert_eq!(cli.config_overrides.raw_overrides, vec!["model=first"]);
    let Some(crate::Subcommand::Mcp(mcp)) = cli.subcommand else {
        panic!("expected MCP")
    };
    assert!(mcp.config_overrides.raw_overrides.is_empty());
    let crate::mcp_cmd::McpSubcommand::Add(add) = mcp.subcommand else {
        panic!("expected add")
    };
    assert_eq!(
        add.transport_args.stdio.expect("stdio transport").command,
        vec!["node", "-c", "child=true"]
    );
}

#[test]
fn global_config_args_clear_marketplace_descendants_without_losing_order() {
    let cli = config_args::try_parse_from([
        "claudex",
        "-c",
        "model=first",
        "plugin",
        "-c",
        "model=second",
        "marketplace",
        "-c",
        "model=third",
        "list",
        "-c",
        "model=last",
    ])
    .expect("parse nested marketplace configuration");
    assert_eq!(
        cli.config_overrides.raw_overrides,
        vec!["model=first", "model=second", "model=third", "model=last"]
    );
    assert!(cli.interactive.config_overrides.raw_overrides.is_empty());
    let Some(crate::Subcommand::Plugin(plugin)) = cli.subcommand else {
        panic!("expected plugin")
    };
    assert!(plugin.config_overrides.raw_overrides.is_empty());
    let crate::plugin_cmd::PluginSubcommand::Marketplace(marketplace) = plugin.subcommand else {
        panic!("expected marketplace")
    };
    assert!(marketplace.config_overrides.raw_overrides.is_empty());
}

#[test]
fn global_config_args_forward_resume_overrides_once_in_input_order() {
    let cli = config_args::try_parse_from([
        "claudex",
        "-c",
        "model=first",
        "resume",
        "--last",
        "-c",
        "model=last",
    ])
    .expect("parse interactive resume");
    let Some(crate::Subcommand::Resume(resume)) = cli.subcommand else {
        panic!("expected resume")
    };
    assert!(
        resume
            .config_overrides
            .0
            .config_overrides
            .raw_overrides
            .is_empty()
    );
    let interactive = crate::finalize_resume_interactive(
        cli.interactive,
        cli.config_overrides,
        resume.session_id,
        resume.last,
        resume.all,
        resume.include_non_interactive,
        resume.config_overrides.0,
    );
    assert_eq!(
        interactive.config_overrides.raw_overrides,
        vec!["model=first", "model=last"]
    );
}

#[test]
fn global_config_args_preserve_empty_configuration() {
    let cli = config_args::try_parse_from(["claudex", "exec", "resume", "--last"])
        .expect("parse without configuration");
    assert_eq!(cli.config_overrides.raw_overrides, Vec::<String>::new());
}

#[test]
fn global_config_args_preserve_help_version_and_missing_value_errors() {
    assert_eq!(
        config_args::try_parse_from(["claudex", "--help"])
            .unwrap_err()
            .kind(),
        ErrorKind::DisplayHelp
    );
    assert_eq!(
        config_args::try_parse_from(["claudex", "--version"])
            .unwrap_err()
            .kind(),
        ErrorKind::DisplayVersion
    );
    assert!(config_args::try_parse_from(["claudex", "exec", "resume", "-c"]).is_err());
}

#[test]
fn global_config_args_command_tree_remains_valid() {
    config_args::command().debug_assert();
}

#[test]
fn global_config_args_keep_descendant_sandbox_after_root_generated_defaults() {
    let interactive = crate::tests::finalize_resume_from_args(&[
        "claudex",
        "--approve-for-me",
        "resume",
        "--last",
        "-c",
        r#"sandbox_mode="read-only""#,
    ]);
    assert_eq!(
        interactive
            .config_overrides
            .raw_overrides
            .last()
            .map(String::as_str),
        Some(r#"sandbox_mode="read-only""#)
    );
}

#[test]
fn global_config_args_keep_descendant_feature_after_root_toggle() {
    let mut cli = config_args::try_parse_from([
        "claudex",
        "--enable",
        "multi_agent",
        "resume",
        "--last",
        "-c",
        "features.multi_agent=false",
    ])
    .expect("parse feature override");
    config_args::apply_root_overrides(&mut cli).expect("root configuration");
    assert_eq!(
        cli.config_overrides
            .raw_overrides
            .last()
            .map(String::as_str),
        Some("features.multi_agent=false")
    );
}
