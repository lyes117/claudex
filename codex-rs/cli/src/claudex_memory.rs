//! Global memory utilities with native CLI help and completion.
use clap::Args;
use codex_utils_cli::CliConfigOverrides;
use codex_utils_cli::SharedCliOptions;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub(crate) struct MemoryCli {
    /// Use a dedicated Claudex memory installation.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: MemoryCommand,
}

#[derive(Debug, clap::Subcommand)]
enum MemoryCommand {
    /// Install a built memory package with the official Codex observer.
    Install {
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        bun: PathBuf,
        #[arg(long)]
        binary: Option<PathBuf>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        codex_home: Option<PathBuf>,
    },
    /// Show installation and owned worker status.
    Status,
    /// Start the owned memory worker.
    Start,
    /// Stop the owned memory worker.
    Stop,
    /// Search a project's memory through the worker.
    Search {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        query: String,
    },
    /// Inspect the context prepared for a project's next session.
    Context {
        #[arg(long)]
        project: Option<String>,
    },
}

impl MemoryCli {
    fn helper_arguments(self) -> Vec<OsString> {
        let mut arguments = Vec::new();
        match self.command {
            MemoryCommand::Install {
                package,
                bun,
                binary,
                port,
                codex_home,
            } => {
                arguments.extend([
                    "install".into(),
                    "--package".into(),
                    package.into_os_string(),
                    "--bun".into(),
                    bun.into_os_string(),
                ]);
                if let Some(binary) = binary {
                    arguments.extend(["--binary".into(), binary.into_os_string()]);
                }
                if let Some(port) = port {
                    arguments.extend(["--port".into(), port.to_string().into()]);
                }
                if let Some(codex_home) = codex_home {
                    arguments.extend(["--codex-home".into(), codex_home.into_os_string()]);
                }
            }
            MemoryCommand::Status => arguments.push("status".into()),
            MemoryCommand::Start => arguments.push("start".into()),
            MemoryCommand::Stop => arguments.push("stop".into()),
            MemoryCommand::Search { project, query } => {
                arguments.push("search".into());
                if let Some(project) = project {
                    arguments.extend(["--project".into(), project.into()]);
                }
                arguments.extend(["--query".into(), query.into()]);
            }
            MemoryCommand::Context { project } => {
                arguments.push("context".into());
                if let Some(project) = project {
                    arguments.extend(["--project".into(), project.into()]);
                }
            }
        }
        if let Some(root) = self.root {
            arguments.extend(["--root".into(), root.into_os_string()]);
        }
        arguments
    }
}

pub(crate) fn validate_runtime_options(
    cli: &codex_tui::Cli,
    overrides: &CliConfigOverrides,
) -> anyhow::Result<()> {
    let SharedCliOptions {
        images,
        model,
        oss,
        oss_provider,
        config_profile_v2,
        sandbox_mode,
        auto_review,
        dangerously_bypass_approvals_and_sandbox,
        bypass_hook_trust,
        cwd: _,
        worktree,
        add_dir,
    } = cli.shared.clone().into_inner();
    if !overrides.raw_overrides.is_empty()
        || !images.is_empty()
        || model.is_some()
        || oss
        || oss_provider.is_some()
        || config_profile_v2.is_some()
        || sandbox_mode.is_some()
        || auto_review
        || dangerously_bypass_approvals_and_sandbox
        || bypass_hook_trust
        || worktree
        || !add_dir.is_empty()
        || cli.prompt.is_some()
        || cli.approval_policy.is_some()
        || cli.web_search
        || cli.no_alt_screen
        || cli.no_daemon
    {
        anyhow::bail!(
            "`claudex memory` does not apply model, permission or TUI options; use its dedicated memory options"
        );
    }
    Ok(())
}

pub(crate) async fn run(command: MemoryCli, cwd: Option<PathBuf>) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || run_helper(command.helper_arguments(), cwd)).await??;
    Ok(())
}

fn run_helper(arguments: Vec<OsString>, cwd: Option<PathBuf>) -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    let script = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Executable directory unavailable"))?
        .join("claude-memory.mjs");
    if !script.is_file() {
        anyhow::bail!("Memory helper missing; run scripts/install-claudex.ps1");
    }
    let mut child = std::process::Command::new("node");
    child
        .arg(script)
        .args(arguments)
        .env("CLAUDEX_BIN", executable);
    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }
    let status = child.status()?;
    if !status.success() {
        anyhow::bail!("Memory command failed with {status}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "claudex_memory_tests.rs"]
mod tests;
