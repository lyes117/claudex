//! Deterministic CLI entrypoint: the TUI submits a typed request to its native session.
use clap::Args;
use codex_protocol::protocol::WorkflowRunRequest;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub(crate) struct WorkflowCli {
    /// Local script, or list/status/pause/resume/stop.
    pub(crate) target: String,
    /// Run ID for a control/status operation.
    pub(crate) id: Option<String>,
    #[arg(long, default_value = "{}")]
    pub(crate) args: String,
    #[arg(long)]
    pub(crate) run_id: Option<String>,
    #[arg(long)]
    pub(crate) cwd: Option<PathBuf>,
    #[arg(long)]
    pub(crate) execution_profile: Option<String>,
}

impl WorkflowCli {
    pub(crate) fn prepare(self) -> anyhow::Result<Option<(WorkflowRunRequest, Option<PathBuf>)>> {
        if matches!(
            self.target.as_str(),
            "list" | "status" | "pause" | "resume" | "stop"
        ) {
            self.control()?;
            return Ok(None);
        }
        if self.id.is_some() {
            anyhow::bail!("unexpected positional workflow argument");
        }
        if self
            .execution_profile
            .as_deref()
            .is_some_and(|value| value != "native")
        {
            anyhow::bail!(
                "Native Workflow uses the conversation's Codex model and permissions; execution-profile mappings are not activated."
            );
        }
        if self.args.len() > 8192 {
            anyhow::bail!("Workflow args exceed 8192 bytes");
        }
        let args = serde_json::from_str::<serde_json::Value>(&self.args)?;
        if !args.is_object() && !args.is_null() {
            anyhow::bail!("Workflow args must be a bounded JSON object or null");
        }
        let run_id = self
            .run_id
            .unwrap_or_else(|| codex_protocol::ThreadId::new().to_string());
        validate_id(&run_id)?;
        Ok(Some((
            WorkflowRunRequest {
                script_path: self.target.into(),
                args,
                run_id,
            },
            self.cwd,
        )))
    }

    fn control(&self) -> anyhow::Result<()> {
        let home = codex_config::claude::home()
            .ok_or_else(|| anyhow::anyhow!("Workflow home unavailable"))?;
        let root = home
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Workflow home parent unavailable"))?
            .join(".claudex/workflow-runs");
        if self.target == "list" {
            if !root.exists() {
                println!("[]");
                return Ok(());
            }
            let mut runs = Vec::new();
            for entry in std::fs::read_dir(root)?.take(1000) {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                if let Ok(mut status) = read_json(&entry.path().join("status.json")) {
                    if let Some(object) = status.as_object_mut() {
                        object.remove("token");
                    }
                    runs.push(status);
                }
            }
            println!("{}", serde_json::to_string_pretty(&runs)?);
            return Ok(());
        }
        let id = self
            .id
            .as_ref()
            .or(self.run_id.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Workflow run ID required"))?;
        validate_id(id)?;
        let directory = root.join(id);
        let status = read_json(&directory.join("status.json"))?;
        if self.target == "status" {
            let mut status = status;
            if let Some(object) = status.as_object_mut() {
                object.remove("token");
            }
            println!("{}", serde_json::to_string_pretty(&status)?);
            return Ok(());
        }
        let current = status
            .get("status")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Invalid workflow status"))?;
        if !matches!(current, "running" | "pausing" | "paused" | "stopping") {
            anyhow::bail!(
                "Workflow is not active; completed/crashed runs are not automatically replayed"
            );
        }
        let token = status
            .get("token")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Native workflow control token missing"))?;
        let command = serde_json::json!({"token":token,"revision":codex_protocol::ThreadId::new().to_string(),"action":self.target});
        let temporary = directory.join(format!("control.{}.tmp", codex_protocol::ThreadId::new()));
        std::fs::write(&temporary, serde_json::to_vec(&command)?)?;
        std::fs::rename(&temporary, directory.join("control.json"))?;
        println!(
            "{}",
            serde_json::json!({"runId":id,"requested":self.target,"revision":command["revision"],"acknowledged":false})
        );
        Ok(())
    }
}

fn validate_id(value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        anyhow::bail!("Invalid workflow run ID");
    }
    Ok(())
}
fn read_json(path: &Path) -> anyhow::Result<serde_json::Value> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 2 * 1024 * 1024 {
        anyhow::bail!("Workflow status exceeds budget");
    }
    Ok(serde_json::from_slice(&bytes)?)
}
