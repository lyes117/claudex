//! Shared CLI parsing and completion tree for global configuration overrides.

use super::MultitoolCli;
use super::Subcommand;
use clap::CommandFactory;
use clap::FromArgMatches;
use std::ffi::OsString;

pub(super) fn command() -> clap::Command {
    let command = MultitoolCli::command();
    #[expect(
        clippy::expect_used,
        reason = "the derived root CLI always flattens CliConfigOverrides"
    )]
    let config_argument = command
        .get_arguments()
        .find(|argument| argument.get_id() == "raw_overrides")
        .expect("the root CLI defines configuration overrides")
        .clone();
    localize_config_argument(command, &config_argument)
}

pub(super) fn parse() -> MultitoolCli {
    try_parse_from(std::env::args_os()).unwrap_or_else(|error| error.exit())
}

pub(super) fn try_parse_from<I, T>(arguments: I) -> Result<MultitoolCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut command = command();
    let mut matches = command.try_get_matches_from_mut(arguments)?;
    let root_override_count = matches
        .get_many::<String>("raw_overrides")
        .map_or(/*default*/ 0, Iterator::count);
    let overrides = collect_config_overrides(&matches);
    let mut cli = MultitoolCli::from_arg_matches_mut(&mut matches)
        .map_err(|error| error.format(&mut command))?;
    clear_descendant_config_overrides(&mut cli);
    cli.config_overrides.raw_overrides = overrides;
    cli.root_config_override_count = root_override_count;
    Ok(cli)
}

pub(super) fn apply_root_overrides(cli: &mut MultitoolCli) -> anyhow::Result<()> {
    let mut generated = codex_utils_cli::CliConfigOverrides {
        raw_overrides: std::mem::take(&mut cli.feature_toggles).to_overrides()?,
    };
    cli.interactive
        .shared
        .take_auto_review_config_overrides(&mut generated);
    let insertion = cli.root_config_override_count;
    cli.root_config_override_count += generated.raw_overrides.len();
    cli.config_overrides
        .raw_overrides
        .splice(insertion..insertion, generated.raw_overrides);
    Ok(())
}

fn localize_config_argument(command: clap::Command, config: &clap::Arg) -> clap::Command {
    // Clap replaces a global Append vector with the descendant's whole vector.
    // Keep independent local vectors until parsing completes. Building the tree
    // first would already propagate global clones, so localize every node now.
    let defines_config = command
        .get_arguments()
        .any(|argument| argument.get_id() == "raw_overrides");
    let command = if defines_config {
        command.mut_arg("raw_overrides", |argument| {
            argument.global(/*yes*/ false)
        })
    } else {
        command.arg(config.clone().global(/*yes*/ false))
    };
    command.mut_subcommands(|subcommand| localize_config_argument(subcommand, config))
}

fn collect_config_overrides(matches: &clap::ArgMatches) -> Vec<String> {
    let mut overrides: Vec<String> = matches
        .get_many::<String>("raw_overrides")
        .map(|values| values.cloned().collect())
        .unwrap_or_default();
    if let Some((_, subcommand)) = matches.subcommand() {
        overrides.extend(collect_config_overrides(subcommand));
    }
    overrides
}

fn clear_descendant_config_overrides(cli: &mut MultitoolCli) {
    // Keep Clap's matches intact for derives, then move ownership of the
    // ordered configuration sequence to the root. Existing dispatchers prepend
    // that sequence once; descendant copies would otherwise duplicate options.
    cli.interactive.config_overrides.raw_overrides.clear();
    let Some(subcommand) = &mut cli.subcommand else {
        return;
    };
    match subcommand {
        Subcommand::Exec(command) => command.config_overrides.raw_overrides.clear(),
        Subcommand::Login(command) => command.config_overrides.raw_overrides.clear(),
        Subcommand::Logout(command) => command.config_overrides.raw_overrides.clear(),
        Subcommand::Mcp(command) => command.config_overrides.raw_overrides.clear(),
        Subcommand::Plugin(command) => {
            command.config_overrides.raw_overrides.clear();
            if let crate::plugin_cmd::PluginSubcommand::Marketplace(marketplace) =
                &mut command.subcommand
            {
                marketplace.config_overrides.raw_overrides.clear();
            }
        }
        Subcommand::Resume(command) => command
            .config_overrides
            .0
            .config_overrides
            .raw_overrides
            .clear(),
        Subcommand::Fork(command) => command
            .config_overrides
            .0
            .config_overrides
            .raw_overrides
            .clear(),
        Subcommand::Archive(command) | Subcommand::Unarchive(command) => command
            .config_overrides
            .config_overrides
            .raw_overrides
            .clear(),
        Subcommand::Delete(command) => command
            .session
            .config_overrides
            .config_overrides
            .raw_overrides
            .clear(),
        Subcommand::Queue(command) => command
            .config_overrides
            .config_overrides
            .raw_overrides
            .clear(),
        Subcommand::Cloud(command) => command.config_overrides.raw_overrides.clear(),
        Subcommand::Sandbox(command) => command.config_overrides.raw_overrides.clear(),
        Subcommand::Apply(command) => command.config_overrides.raw_overrides.clear(),
        Subcommand::Agents(_)
        | Subcommand::TcpTunnel(_)
        | Subcommand::Review(_)
        | Subcommand::AppServer(_)
        | Subcommand::RemoteControl(_)
        | Subcommand::Completion(_)
        | Subcommand::Update
        | Subcommand::Doctor(_)
        | Subcommand::Debug(_)
        | Subcommand::Execpolicy(_)
        | Subcommand::MigrateRollouts(_)
        | Subcommand::ResponsesApiProxy(_)
        | Subcommand::StdioToUds(_)
        | Subcommand::ExecServer(_)
        | Subcommand::Features(_) => {}
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        Subcommand::App(_) => {}
    }
}
