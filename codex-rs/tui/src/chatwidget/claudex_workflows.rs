//! Bounded local workflow status and control. The panel never launches inference itself.

use super::*;
use serde::Deserialize;
use std::io::Read;

const VIEW_ID: &str = "claudex-workflows";
const MAX_STATUS_BYTES: u64 = 512 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowRun {
    pub(crate) run_id: String,
    status: String,
    #[serde(default)]
    phase: Option<String>,
    started_at: u64,
    #[serde(default)]
    agents: Vec<WorkflowAgent>,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkflowAgent {
    label: String,
    status: String,
}

fn label(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

fn workflow_root() -> Option<PathBuf> {
    codex_config::claude::home()?
        .parent()
        .map(|home| home.join(".claudex/workflow-runs"))
}

fn load_runs() -> Result<Vec<WorkflowRun>, String> {
    let root = workflow_root().ok_or("User home unavailable")?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Cannot read local workflow runs".to_string()),
    };
    let mut runs = Vec::new();
    for entry in entries.take(4096).flatten() {
        let Ok(file) = std::fs::File::open(entry.path().join("status.json")) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file
            .take(MAX_STATUS_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_STATUS_BYTES
        {
            continue;
        }
        let Ok(run) = serde_json::from_slice::<WorkflowRun>(&bytes) else {
            continue;
        };
        if run.agents.len() <= 1000
            && run.run_id.len() <= 128
            && !run.run_id.is_empty()
            && run
                .run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            && entry.file_name().to_str() == Some(run.run_id.as_str())
        {
            runs.push(run);
        }
    }
    runs.sort_by_key(|run| std::cmp::Reverse(run.started_at));
    runs.truncate(1000);
    Ok(runs)
}

impl ChatWidget {
    pub(crate) fn open_claudex_workflows(&mut self, selection: Option<String>) {
        self.claudex_workflow_generation = self.claudex_workflow_generation.wrapping_add(1);
        self.claudex_workflow_selection = selection;
        self.claudex_workflow_rows.clear();
        let loading = || SelectionViewParams {
            view_id: Some(VIEW_ID),
            title: Some("Workflows".to_string()),
            subtitle: Some("Local runs · loading status…".to_string()),
            ..SelectionViewParams::picker()
        };
        if !self
            .bottom_pane
            .replace_selection_view_if_present(VIEW_ID, loading())
        {
            self.bottom_pane.show_selection_view(loading());
        }
        self.refresh_claudex_workflows(Duration::ZERO);
    }

    fn refresh_claudex_workflows(&self, delay: Duration) {
        let tx = self.app_event_tx.clone();
        let selection = self.claudex_workflow_selection.clone();
        let generation = self.claudex_workflow_generation;
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let result = tokio::task::spawn_blocking(load_runs)
                .await
                .unwrap_or_else(|_| Err("Workflow refresh failed".to_string()));
            tx.send(AppEvent::ClaudexWorkflowsLoaded {
                generation,
                selection,
                result,
            });
        });
    }

    pub(crate) fn apply_claudex_workflows(
        &mut self,
        generation: u64,
        selection: Option<String>,
        result: Result<Vec<WorkflowRun>, String>,
    ) {
        if generation != self.claudex_workflow_generation
            || selection != self.claudex_workflow_selection
        {
            return;
        }
        let mut selected_index = self.bottom_pane.selected_index_for_present_view(VIEW_ID);
        let mut items = Vec::new();
        let subtitle = match result {
            Err(error) => error,
            Ok(runs) => {
                if let Some(id) = selection {
                    items.push(SelectionItem {
                        name: "Back to workflow runs".to_string(),
                        actions: vec![Box::new(|tx| tx.send(AppEvent::OpenClaudexWorkflow(None)))],
                        ..Default::default()
                    });
                    if let Some(run) = runs.iter().find(|run| run.run_id == id) {
                        for (name, action) in [
                            ("Pause after current agent", "pause"),
                            ("Resume queued agents", "resume"),
                            ("Stop run and retain checkpoint", "stop"),
                        ] {
                            let run_id = run.run_id.clone();
                            let action = action.to_string();
                            items.push(SelectionItem {
                                name: name.to_string(),
                                is_disabled: !matches!(
                                    run.status.as_str(),
                                    "running" | "pausing" | "paused"
                                ),
                                actions: vec![Box::new(move |tx| {
                                    tx.send(AppEvent::ControlClaudexWorkflow {
                                        run_id: run_id.clone(),
                                        action: action.clone(),
                                    })
                                })],
                                ..Default::default()
                            });
                        }
                        for agent in &run.agents {
                            items.push(SelectionItem {
                                name: label(&agent.label),
                                description: Some(label(&agent.status)),
                                ..Default::default()
                            });
                        }
                        format!(
                            "{} · {} · {}",
                            run.run_id,
                            label(&run.status),
                            label(run.phase.as_deref().unwrap_or("No phase"))
                        )
                    } else {
                        "Run no longer available".to_string()
                    }
                } else {
                    let selected_id =
                        selected_index.and_then(|index| self.claudex_workflow_rows.get(index));
                    selected_index =
                        selected_id.and_then(|id| runs.iter().position(|run| &run.run_id == id));
                    self.claudex_workflow_rows =
                        runs.iter().map(|run| run.run_id.clone()).collect();
                    for run in runs {
                        let run_id = run.run_id.clone();
                        let completed = run
                            .agents
                            .iter()
                            .filter(|agent| matches!(agent.status.as_str(), "completed" | "cached"))
                            .count();
                        items.push(SelectionItem {
                            name: run.run_id,
                            description: Some(format!(
                                "{} · {completed}/{} agents · {}",
                                label(&run.status),
                                run.agents.len(),
                                label(run.phase.as_deref().unwrap_or("No phase"))
                            )),
                            actions: vec![Box::new(move |tx| {
                                tx.send(AppEvent::OpenClaudexWorkflow(Some(run_id.clone())))
                            })],
                            ..Default::default()
                        });
                    }
                    "Local runs · enter to inspect · status refreshes every second".to_string()
                }
            }
        };
        if items.is_empty() {
            items.push(SelectionItem {
                name: "No workflow runs".to_string(),
                is_disabled: true,
                description: Some("Start one with claudex workflow <script.js>".to_string()),
                ..Default::default()
            });
        }
        if self.bottom_pane.replace_selection_view_if_present(
            VIEW_ID,
            SelectionViewParams {
                view_id: Some(VIEW_ID),
                title: Some("Workflows".to_string()),
                subtitle: Some(subtitle),
                items,
                initial_selected_idx: selected_index,
                ..SelectionViewParams::picker()
            },
        ) {
            self.refresh_claudex_workflows(Duration::from_secs(1));
        }
    }

    pub(crate) fn control_claudex_workflow(&self, run_id: String, action: String) {
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let executable = std::env::current_exe().map_err(|_| "Cannot find Claudex executable")?;
                let output = std::process::Command::new(executable).args(["workflow", &action, &run_id]).output().map_err(|_| "Cannot start workflow controller")?;
                if output.status.success() { Ok(()) } else { Err("Workflow control failed; refresh status or inspect claudex workflow status".to_string()) }
            }).await.unwrap_or_else(|_| Err("Workflow controller failed".to_string()));
            tx.send(AppEvent::ClaudexWorkflowControlled(result));
        });
    }
}
