//! Claudex-specific local utilities. Inference and authentication stay in Codex.
use std::path::PathBuf;

pub(crate) fn dispatch() -> anyhow::Result<Option<()>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("compat") => {
            let cwd = args
                .next()
                .map(PathBuf::from)
                .unwrap_or(std::env::current_dir()?);
            let mut directories = Vec::new();
            if let Some(home) = codex_config::claude::home() {
                directories.push(home);
            }
            for ancestor in cwd.ancestors() {
                directories.push(ancestor.join(".claude"));
                if ancestor.join(".git").exists() {
                    break;
                }
            }
            let mut warnings = Vec::new();
            let mut scopes = Vec::new();
            for directory in directories {
                if !directory.is_dir() {
                    continue;
                }
                let config = codex_config::claude::native_config(&directory, &mut warnings)?;
                let hooks = codex_config::claude::hook_sources(&directory, &mut warnings)?;
                let plugins = codex_config::claude::plugins(&directory)?;
                scopes.push(serde_json::json!({
                    "directory": directory,
                    "mcp_servers": config.get("mcp_servers").and_then(toml::Value::as_table).map(|table| table.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
                    "hook_handlers": hooks.iter().map(|(_, events, _)| events.handler_count()).sum::<usize>(),
                    "plugins": plugins.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                    "skills": directory.join("skills").is_dir(), "commands": directory.join("commands").is_dir(), "agents": directory.join("agents").is_dir(),
                }));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"engine": "openai/codex rust-v0.160.0", "configuration": "read-in-place", "scopes": scopes, "warnings": warnings, "limits": ["Claude-specific models are not mapped", "Prompt, agent, HTTP hooks and unsupported events are not executed", "Hooks retain native Codex trust review", "Agents and skills with unsupported execution restrictions are rejected", "Path-specific file permission rules block the entire applicable tool conservatively", "Scoped rules are conditional model instructions", "Workflow UI, pause and replay are not Claude runtime equivalents"]})
                )?
            );
            Ok(Some(()))
        }
        Some("workflow") => {
            let executable = std::env::current_exe()?;
            let script = executable
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Executable has no directory"))?
                .join("workflows.mjs");
            if !script.is_file() {
                anyhow::bail!(
                    "Workflow runtime missing: {}. Run scripts/install-claudex.ps1.",
                    script.display()
                );
            }
            let args = args.collect::<Vec<_>>();
            let run_id = args
                .windows(2)
                .find(|pair| pair[0] == "--run-id")
                .map(|pair| pair[1].as_str())
                .unwrap_or("new-run");
            if !run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                anyhow::bail!("Invalid workflow run ID");
            }
            let directory = codex_config::claude::home()
                .and_then(|home| {
                    home.parent()
                        .map(|path| path.join(".claudex/workflow-locks"))
                })
                .ok_or_else(|| anyhow::anyhow!("User home unavailable"))?;
            std::fs::create_dir_all(&directory)?;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(directory.join(format!("{run_id}.lock")))?;
            let mut lock = fd_lock::RwLock::new(file);
            let _guard = lock.try_write().map_err(|_| {
                anyhow::anyhow!("Workflow run is already active; exclusive OS lock unavailable")
            })?;
            let status = std::process::Command::new("node")
                .arg(script)
                .args(args)
                .env("CLAUDEX_BIN", executable)
                .env("CLAUDEX_WORKFLOW_LOCK_HELD", "1")
                .status()?;
            if !status.success() {
                anyhow::bail!("Workflow failed with {status}");
            }
            Ok(Some(()))
        }
        _ => Ok(None),
    }
}
