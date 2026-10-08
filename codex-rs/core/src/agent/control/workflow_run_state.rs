//! Private status/control files; none contains prompts, arguments, or credentials.
use super::workflow::WorkflowAgentObservation;
use super::workflow::WorkflowObserver;
use codex_code_mode::workflow::WorkflowAgentStatus;
use codex_code_mode::workflow::WorkflowControlAction;
use codex_code_mode::workflow::WorkflowControlRequest;
use codex_code_mode::workflow::WorkflowRunStatus;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::io::AsyncReadExt;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub(super) struct RunState {
    pub(super) directory: PathBuf,
    status: Mutex<WorkflowRunStatus>,
    paused: AtomicBool,
    wake: Notify,
}

impl RunState {
    pub(super) async fn create(run_id: &str, parent: String) -> Result<Arc<Self>, String> {
        if !codex_code_mode::workflow::valid_run_id(run_id) {
            return Err("invalid workflow run ID".into());
        }
        let claude_home = codex_config::claude::home().ok_or("workflow home unavailable")?;
        let root = claude_home
            .parent()
            .ok_or("workflow home parent unavailable")?
            .join(".claudex/workflow-runs");
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(|error| format!("workflow run directory: {error}"))?;
        let directory = root.join(run_id);
        // Atomic directory creation prevents concurrent invocations or replacement of
        // an existing run. Resume is an in-process control operation, never replay.
        tokio::fs::create_dir(&directory).await.map_err(|error| {
            format!("workflow run already exists or cannot be created: {error}")
        })?;
        let now = chrono::Utc::now().timestamp_millis();
        let state=Arc::new(Self {directory,status:Mutex::new(WorkflowRunStatus {
            run_id:run_id.into(),status:"running".into(),phase:"Workflow".into(),started_at:now,updated_at:now,
            parent_thread_id:parent,token:uuid::Uuid::new_v4().to_string(),control_revision:None,
            agents:Vec::new(),logs:vec!["Runtime natif Codex ; opus/sonnet/haiku désignent le modèle natif capturé avec effort high/medium/low, jamais Claude.".into()],
        }),paused:AtomicBool::new(false),wake:Notify::new()});
        state.persist().await?;
        Ok(state)
    }

    fn update(&self, operation: impl FnOnce(&mut WorkflowRunStatus)) -> Result<(), String> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| "workflow status lock poisoned")?;
        operation(&mut status);
        status.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    pub(super) fn set_status(&self, status: &str) -> Result<(), String> {
        self.update(|state| state.status = status.into())
    }
    pub(super) fn phase(&self, phase: &str) -> Result<(), String> {
        self.update(|state| state.phase = phase.into())
    }
    pub(super) fn log(&self, message: String) -> Result<(), String> {
        self.update(|state| {
            state.logs.push(message);
            // Bound encoded bytes as well as count, including JSON escape expansion.
            while state.logs.len() > 24
                || codex_code_mode::workflow::encode(&state.logs, 64 * 1024).is_err()
            {
                state.logs.remove(0);
            }
        })
    }
    pub(super) fn observer(self: &Arc<Self>, cancellation: CancellationToken) -> WorkflowObserver {
        let state = Arc::clone(self);
        Arc::new(move |event: WorkflowAgentObservation| {
            let result = state.update(|status| {
                let value = WorkflowAgentStatus {
                    label: event.label,
                    status: event.status,
                    thread_id: event.thread_id,
                    model: event.model,
                    provider: event.provider,
                    effort: event.effort,
                };
                if let Some(agent) = status
                    .agents
                    .iter_mut()
                    .find(|agent| agent.thread_id == value.thread_id)
                {
                    *agent = value;
                } else {
                    // Status retains recent exact IDs; the native agent histories
                    // remain available through the normal picker and rollout store.
                    if status.agents.len() >= 256
                        && let Some(index) = status
                            .agents
                            .iter()
                            .position(|agent| agent.status != "running")
                    {
                        status.agents.remove(index);
                    }
                    status.agents.push(value);
                }
            });
            if let Err(error) = result {
                tracing::warn!("{error}");
                cancellation.cancel();
            }
        })
    }

    pub(super) async fn persist(&self) -> Result<(), String> {
        let bytes = {
            let state = self
                .status
                .lock()
                .map_err(|_| "workflow status lock poisoned")?;
            codex_code_mode::workflow::encode(&*state, 512 * 1024)
                .map_err(|_| "workflow status exceeds budget")?
        };
        let temporary = self.directory.join("status.next.json");
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(|error| format!("workflow status write: {error}"))?;
        tokio::fs::rename(temporary, self.directory.join("status.json"))
            .await
            .map_err(|error| format!("workflow status commit: {error}"))
    }

    pub(super) async fn wait_running(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        loop {
            let notified = self.wake.notified();
            if !self.paused.load(Ordering::Acquire) {
                return Ok(());
            }
            tokio::select! {
                _=cancellation.cancelled()=>return Err("workflow stopped".into()),
                _=notified=>{},
            }
        }
    }

    pub(super) async fn watch(
        self: Arc<Self>,
        cancellation: CancellationToken,
        finished: CancellationToken,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                _=finished.cancelled()=>return Ok(()),
                _=tokio::time::sleep(std::time::Duration::from_millis(200))=>{},
            }
            let path = self.directory.join("control.json");
            match read_bounded(&path, 8192).await {
                Ok(bytes) => {
                    let command = serde_json::from_slice::<WorkflowControlRequest>(&bytes)
                        .map_err(|_| "invalid workflow control request")?;
                    if !codex_code_mode::workflow::valid_run_id(&command.revision)
                        || command.token.len() > 128
                    {
                        return Err("invalid workflow control identifier".into());
                    }
                    let should_apply = {
                        let status = self
                            .status
                            .lock()
                            .map_err(|_| "workflow status lock poisoned")?;
                        command.token == status.token
                            && status.control_revision.as_ref() != Some(&command.revision)
                    };
                    if should_apply {
                        match command.action {
                            WorkflowControlAction::Pause => {
                                self.paused.store(true, Ordering::Release);
                                self.set_status("pausing")?;
                            }
                            WorkflowControlAction::Resume => {
                                self.paused.store(false, Ordering::Release);
                                self.set_status("running")?;
                                self.wake.notify_waiters();
                            }
                            WorkflowControlAction::Stop => {
                                self.set_status("stopping")?;
                                cancellation.cancel();
                                self.wake.notify_waiters();
                            }
                        }
                        self.update(|status| status.control_revision = Some(command.revision))?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("workflow control read: {error}")),
            }
            self.persist().await?;
        }
    }

    pub(super) fn mark_paused(&self) -> Result<(), String> {
        if self.paused.load(Ordering::Acquire) {
            self.set_status("paused")?;
        }
        Ok(())
    }
}

pub(super) async fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    let input = tokio::fs::File::open(path).await?;
    let mut bytes = Vec::new();
    input
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(std::io::Error::other("workflow file exceeds budget"));
    }
    Ok(bytes)
}
