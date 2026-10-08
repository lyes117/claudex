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
        // `claudex zcode <PROMPT>` — route the prompt through the ZCode bridge.
        Some("zcode") => {
            // ponytail: one positional prompt, extra args ignored — no options on purpose.
            let prompt = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("usage : claudex zcode <PROMPT>"))?;
            run_zcode(&prompt)?;
            Ok(Some(()))
        }
        _ => Ok(None),
    }
}

/// Route one prompt to ZCode: spawn the CLI, create a session in the current
/// directory, send the turn, print the final reply on stdout. Exit 0 on a
/// completed turn, exit 1 with a clear message otherwise (anyhow error through
/// `main`).
fn run_zcode(prompt: &str) -> anyhow::Result<()> {
    let mut bridge = codex_api::ZcodeBridge::spawn().map_err(|error| {
        anyhow::anyhow!(
            "impossible de lancer le CLI ZCode ({error}) — vérifie que l'app ZCode est installée"
        )
    })?;
    let workspace = std::env::current_dir()?;
    let session = bridge
        .session_create(&workspace.to_string_lossy())
        .map_err(|error| anyhow::anyhow!("{error} — lance zcode et connecte-toi"))?;
    let turn = bridge.session_send(&session.session_id, prompt)?;
    match turn.outcome {
        codex_api::Outcome::Ok(result) => {
            let (drained, terminal) = bridge.wait_terminal(std::time::Duration::from_secs(120))?;
            let mut notifications = turn.notifications;
            notifications.extend(drained);
            let texte = texte_parcouru(&result)
                .or_else(|| notifications.iter().rev().find_map(texte_parcouru));
            match (terminal, texte) {
                // Measured real failure: the terminal event names the cause (e.g.
                // CONFIGURATION_ERROR "Select a model before continuing") — surface
                // it instead of the polite accepted ack.
                (Some(terminal), _) if terminal.status == "failed" => anyhow::bail!(
                    "tour ZCode échoué : {} — le CLI ZCode autonome ne résout pas de \
                     modèle : le catalogue vient de l'app ZCode (lancez la commande \
                     depuis une session de l'app, ou attendez la tranche catalog)",
                    terminal.error_message
                ),
                (_, Some(texte)) => {
                    println!("{texte}");
                    Ok(())
                }
                (Some(terminal), None) => anyhow::bail!(
                    "tour ZCode terminé ({}) sans réponse textuelle — format d'événement \
                     à calibrer sur le premier tour réussi",
                    terminal.status
                ),
                (None, None) => {
                    anyhow::bail!("tour ZCode sans événement terminal ni réponse")
                }
            }
        }
        codex_api::Outcome::Err { code, message, .. } => {
            anyhow::bail!("tour ZCode échoué ({code} {message}) — lance zcode et connecte-toi")
        }
    }
}

/// First string found under a reply-ish key, depth-first.
pub(crate) fn texte_parcouru(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .find(|(key, _)| matches!(key.as_str(), "content" | "text" | "message"))
            .and_then(|(_, value)| value.as_str().map(str::to_owned))
            .or_else(|| map.values().find_map(texte_parcouru)),
        serde_json::Value::Array(items) => items.iter().find_map(texte_parcouru),
        _ => None,
    }
}
