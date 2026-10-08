use super::*;
use crate::Subcommand;
use clap::error::ErrorKind;
use pretty_assertions::assert_eq;

#[test]
fn native_memory_parse_preserves_unicode_and_literal_query_arguments() {
    let cli = crate::config_args::try_parse_from([
        "claudex",
        "memory",
        "--root",
        "C:/fixture mémoire",
        "search",
        "--project",
        "fixture 研究",
        "--query",
        "literal $(unchanged); value",
    ])
    .expect("native command parses without executing the query");
    let Some(Subcommand::Memory(memory)) = cli.subcommand else {
        panic!("memory dispatcher selected");
    };
    assert_eq!(
        memory.helper_arguments(),
        vec![
            OsString::from("search"),
            "--project".into(),
            "fixture 研究".into(),
            "--query".into(),
            "literal $(unchanged); value".into(),
            "--root".into(),
            "C:/fixture mémoire".into(),
        ]
    );
}

#[test]
fn native_memory_install_keeps_all_paths_as_distinct_arguments() {
    let cli = crate::config_args::try_parse_from([
        "claudex",
        "memory",
        "install",
        "--package",
        "fixture package",
        "--bun",
        "fixture bun",
        "--binary",
        "fixture binary",
        "--port",
        "37781",
        "--codex-home",
        "fixture home",
        "--root",
        "fixture root",
    ])
    .expect("installation syntax");
    let Some(Subcommand::Memory(memory)) = cli.subcommand else {
        panic!("memory selected");
    };
    assert_eq!(
        memory.helper_arguments(),
        vec![
            OsString::from("install"),
            "--package".into(),
            "fixture package".into(),
            "--bun".into(),
            "fixture bun".into(),
            "--binary".into(),
            "fixture binary".into(),
            "--port".into(),
            "37781".into(),
            "--codex-home".into(),
            "fixture home".into(),
            "--root".into(),
            "fixture root".into(),
        ]
    );
}

#[test]
fn native_memory_help_and_invalid_options_are_handled_before_dispatch() {
    for arguments in [
        vec!["claudex", "memory", "--help"],
        vec!["claudex", "memory", "install", "--help"],
        vec!["claudex", "memory", "search", "--help"],
    ] {
        assert_eq!(
            crate::config_args::try_parse_from(arguments)
                .expect_err("help returns to CLI")
                .kind(),
            ErrorKind::DisplayHelp
        );
    }
    assert_eq!(
        crate::config_args::try_parse_from([
            "claudex",
            "memory",
            "install",
            "--package",
            "fixture",
            "--bun",
            "fixture",
            "--port",
            "70000",
        ])
        .expect_err("port rejected by native parser")
        .kind(),
        ErrorKind::ValueValidation
    );
    assert_eq!(
        crate::config_args::try_parse_from(
            ["claudex", "memory", "search", "--project", "fixture",]
        )
        .expect_err("missing query rejected")
        .kind(),
        ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn root_options_do_not_hide_memory_or_bypass_its_strict_config_guard() {
    let cli = crate::config_args::try_parse_from([
        "claudex",
        "--strict-config",
        "-c",
        "model=\"fixture\"",
        "memory",
        "status",
    ])
    .expect("root and native memory options parsed together");
    assert_eq!(
        cli.config_overrides.raw_overrides,
        vec!["model=\"fixture\"".to_owned()]
    );
    assert_eq!(
        crate::unsupported_subcommand_name_for_strict_config(&cli.subcommand),
        Some("memory")
    );
    assert!(
        crate::reject_root_strict_config_for_subcommand(
            cli.interactive.strict_config,
            &cli.subcommand
        )
        .is_err()
    );
    let Some(Subcommand::Memory(memory)) = cli.subcommand else {
        panic!("memory selected");
    };
    assert_eq!(memory.helper_arguments(), vec![OsString::from("status")]);
}

#[test]
fn memory_refuses_runtime_overrides_and_preserves_the_requested_working_directory() {
    for flags in [
        vec!["-m", "fixture"],
        vec!["--oss"],
        vec!["--local-provider", "ollama"],
        vec!["--sandbox", "read-only"],
        vec!["--ask-for-approval", "never"],
        vec!["--search"],
        vec!["--no-alt-screen"],
        vec!["--no-daemon"],
        vec!["--dangerously-bypass-hook-trust"],
        vec!["--add-dir", "fixture"],
        vec!["-c", "model=\"fixture\""],
        vec!["--enable", "plugins"],
    ] {
        let args = std::iter::once("claudex")
            .chain(flags)
            .chain(["memory", "status"]);
        let mut cli =
            crate::config_args::try_parse_from(args).expect("known runtime option syntax");
        crate::config_args::apply_root_overrides(&mut cli).expect("root overrides collected");
        assert!(validate_runtime_options(&cli.interactive, &cli.config_overrides).is_err());
    }
    let cli =
        crate::config_args::try_parse_from(["claudex", "-C", "fixture cwd", "memory", "status"])
            .expect("memory working-directory option");
    validate_runtime_options(&cli.interactive, &cli.config_overrides).expect("cwd is supported");
    assert_eq!(cli.interactive.cwd, Some(PathBuf::from("fixture cwd")));
}
