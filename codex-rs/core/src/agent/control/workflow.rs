//! Local child bridge for a future bounded workflow runner; no tool is registered.
//! Captures authority from a native invocation, never from workflow-provided IDs.
//! Supervisor-owned children return only validated values through this invocation.
//! Public activation still requires a bounded runner and a native recovery journal.

use super::LocalAgentControl;
use super::workflow_schema::EncodingError;
use super::workflow_schema::WorkflowSchema;
use super::workflow_schema::check_json_budget;
use crate::agent::api::AgentInput;
use crate::agent::api::SpawnRequest;
use crate::agent::child_config::SpawnConfigOptions;
use crate::agent::child_config::SpawnConfigVersion;
use crate::agent::child_config::prepare_agent_spawn_config;
use crate::agent::next_thread_spawn_depth;
use crate::agent::types::AgentMessage;
use crate::agent::types::MessageDeliveryMode;
use crate::agent::types::SpawnAgentOptions;
use crate::codex_thread::CodexThread;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::tools::context::ToolInvocation;
use crate::tools::handlers::multi_agents_common::thread_spawn_source;
use codex_features::Feature;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::user_input::UserInput;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const MAX_CHILDREN: usize = 4;
const MAX_PROMPT_BYTES: usize = 8192;

#[allow(dead_code)]
pub(crate) enum WorkflowInput {
    UserInput(String),
    AgentMessage(String),
}

#[allow(dead_code)]
pub(crate) struct WorkflowAgentCall {
    pub(crate) task_name: String,
    pub(crate) input: WorkflowInput,
    pub(crate) role_name: Option<String>,
    pub(crate) schema: Value,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<ReasoningEffort>,
    pub(crate) compatible: bool,
    pub(crate) label: Option<String>,
}

#[derive(Clone)]
pub(crate) struct WorkflowAgentObservation {
    pub(crate) label: String,
    pub(crate) thread_id: String,
    pub(crate) status: String,
    pub(crate) model: String,
    pub(crate) provider: String,
    pub(crate) effort: Option<String>,
}
pub(crate) type WorkflowObserver = Arc<dyn Fn(WorkflowAgentObservation) + Send + Sync>;

// The bridge deliberately has no production caller until runner/journal/reporting are ready.
#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct NativeWorkflowBridge {
    session: Arc<Session>,
    step: Arc<StepContext>,
    cancellation: CancellationToken,
    observer: Option<WorkflowObserver>,
}

#[allow(dead_code)]
impl NativeWorkflowBridge {
    pub(crate) fn capture(invocation: &ToolInvocation) -> Self {
        Self {
            session: Arc::clone(&invocation.session),
            step: Arc::clone(&invocation.step_context),
            cancellation: invocation.cancellation_token.child_token(),
            observer: None,
        }
    }

    pub(crate) fn new(
        session: Arc<Session>,
        step: Arc<StepContext>,
        cancellation: CancellationToken,
        observer: Option<WorkflowObserver>,
    ) -> Self {
        Self {
            session,
            step,
            cancellation,
            observer,
        }
    }
    pub(crate) fn model_slug(&self) -> &str {
        &self.step.settings.model_info.slug
    }

    /// Owns a supervisor independently of an abortable tool dispatch future. Dropping
    /// this waiter signals cancellation; the supervisor still awaits child shutdown.
    pub(crate) async fn run(self, calls: Vec<WorkflowAgentCall>) -> Result<Vec<Value>, String> {
        let cancellation = self.cancellation.clone();
        let _cancel_on_drop = cancellation.clone().drop_guard();
        tokio::spawn(async move { self.run_owned(calls, cancellation).await })
            .await
            .map_err(|_| "workflow supervisor failed")?
    }

    pub(crate) fn start_owned(
        self,
        calls: Vec<WorkflowAgentCall>,
        reply: tokio::sync::oneshot::Sender<Result<Vec<Value>, String>>,
    ) -> tokio::task::JoinHandle<()> {
        let cancellation = self.cancellation.clone();
        tokio::spawn(async move {
            let result = self.run_owned(calls, cancellation).await;
            if reply.send(result).is_err() {
                tracing::debug!("workflow wave waiter dropped after cleanup");
            }
        })
    }

    async fn run_owned(
        self,
        calls: Vec<WorkflowAgentCall>,
        cancellation: CancellationToken,
    ) -> Result<Vec<Value>, String> {
        let control = self
            .session
            .services
            .local_agent_runtime
            .control(self.session.session_id());
        let mut owned = OwnedChildren {
            control: control.clone(),
            threads: Vec::new(),
        };
        let result = self
            .execute(&control, &mut owned.threads, calls, cancellation.clone())
            .await;
        let mut cleanup_failed = false;
        while let Some(child) = owned.threads.last() {
            // Restricted fresh children cannot create descendants. Never close another
            // session ID supplied by the DSL, caller or a returned result.
            if shutdown_owned_child(&control, child).await.is_err() {
                cleanup_failed = true;
            }
            owned.threads.pop();
        }
        if cleanup_failed {
            return Err("workflow child shutdown failed".into());
        }
        if cancellation.is_cancelled() {
            return Err("workflow cancelled".into());
        }
        result
    }

    async fn execute(
        &self,
        control: &LocalAgentControl,
        owned: &mut Vec<Arc<CodexThread>>,
        calls: Vec<WorkflowAgentCall>,
        cancellation: CancellationToken,
    ) -> Result<Vec<Value>, String> {
        if calls.is_empty() || calls.len() > MAX_CHILDREN {
            return Err("workflow requires 1..4 children".into());
        }
        if !self.step.turn.config.agents_enabled {
            return Err("native workflow agents are disabled in this conversation".into());
        }
        let manager = control
            .runtime
            .upgrade()
            .map_err(|_| "workflow requires a live local backend")?;
        let parent = manager
            .get_thread(self.session.thread_id)
            .await
            .map_err(|_| "workflow parent is unavailable")?;
        if !Arc::ptr_eq(&parent.session, &self.session) {
            return Err("workflow invocation does not belong to the local parent".into());
        }
        let deadline = Instant::now() + Duration::from_secs(/*secs*/ 300);
        let mut prepared = Vec::new();
        let mut names = HashSet::new();
        let compatible = calls.iter().all(|call| call.compatible);
        // Validate the entire wave before admitting any child, including late failures.
        for call in calls {
            let prompt = match &call.input {
                WorkflowInput::UserInput(text) | WorkflowInput::AgentMessage(text) => text,
            };
            if prompt.trim().is_empty()
                || prompt.len()
                    > if call.compatible {
                        MAX_PROMPT_BYTES
                    } else {
                        8192
                    }
                || call.task_name.len() > 64
                || call.role_name.as_ref().is_some_and(|role| role.len() > 64)
                || !names.insert(call.task_name.clone())
            {
                return Err("invalid or excessive workflow child input".into());
            }
            let schema = if call.schema.is_null() {
                None
            } else if call.compatible {
                Some(WorkflowSchema::compile_compatible(call.schema.clone())?)
            } else {
                Some(WorkflowSchema::compile_for_output(call.schema.clone())?)
            };
            if let Some(message)=codex_config::claude::permission_block(
                &self.step.turn.config.config_layer_stack,"spawn_agent",
                &serde_json::json!({"message":prompt,"task_name":call.task_name,"subagent_type":call.role_name}),
            ).map_err(|error|format!("workflow agent permission configuration: {error}"))? {return Err(message);}
            let config = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err("workflow cancelled".into()),
                _ = tokio::time::sleep_until(deadline) => return Err("workflow deadline exceeded".into()),
                result = prepare_agent_spawn_config(&self.session, &self.step, SpawnConfigOptions {
                    version: SpawnConfigVersion::V2, full_history_fork: false,
                    role_name: call.role_name.as_deref(), model: call.model.as_deref(), reasoning_effort: call.effort,
                }) => result?,
            };
            let source = thread_spawn_source(
                self.session.thread_id,
                &self.step.turn.session_source,
                next_thread_spawn_depth(&self.step.turn.session_source),
                config.role_name.as_deref(),
                Some(call.task_name.clone()),
            )
            .map_err(|_| "invalid workflow child task name")?;
            let mut config = config.config;
            if call.compatible
                && (config.model_provider_id != "openai"
                    || config.model.as_ref().is_some_and(|model| model.len() > 128))
            {
                return Err("workflow child configuration must preserve the native Codex provider and bounded model ID".into());
            }
            let observation = WorkflowAgentObservation {
                label: call.label.unwrap_or_else(|| call.task_name.clone()),
                thread_id: String::new(),
                status: "running".into(),
                model: config
                    .model
                    .clone()
                    .unwrap_or_else(|| self.step.settings.model_info.slug.clone()),
                provider: config.model_provider_id.clone(),
                effort: config
                    .model_reasoning_effort
                    .as_ref()
                    .map(ToString::to_string),
            };
            config.agents_enabled = false;
            let _ = config.features.disable(Feature::MultiAgentV2);
            let _ = config.features.disable(Feature::Collab);
            let schema_wire = if call.compatible {
                None
            } else {
                schema.as_ref().map(|schema| schema.wire().clone())
            };
            let input = match call.input {
                WorkflowInput::UserInput(text) => {
                    let text = if call.compatible && schema.is_some() {
                        format!(
                            "{text}\n\nReturn only a JSON value satisfying this schema, without Markdown fences:\n{}",
                            call.schema
                        )
                    } else {
                        text
                    };
                    if text.len() > MAX_PROMPT_BYTES {
                        return Err(
                            "workflow final prompt including schema exceeds 8192 bytes".into()
                        );
                    }
                    AgentInput::UserInput(vec![UserInput::Text {
                        text,
                        text_elements: Vec::new(),
                    }])
                }
                WorkflowInput::AgentMessage(text) => AgentInput::Message {
                    message: AgentMessage::Plaintext(text),
                    mode: MessageDeliveryMode::TriggerTurn,
                },
            };
            prepared.push((config, source, input, schema, schema_wire, observation));
        }
        let mut waits = FuturesUnordered::new();
        for (index, (config, source, input, schema, schema_wire, mut observation)) in
            prepared.into_iter().enumerate()
        {
            if cancellation.is_cancelled() || Instant::now() >= deadline {
                return Err("workflow cancelled or expired".into());
            }
            // Do not abandon an in-progress admission: capture its exact runtime first,
            // then observe cancellation. This deadline does not bound spawn latency.
            let admitted = control
                .spawn_retained_with_reporting(
                    SpawnRequest {
                        caller: self.session.thread_id,
                        config,
                        input,
                        source,
                        options: SpawnAgentOptions {
                            parent_thread_id: Some(self.session.thread_id),
                            parent_turn_id: Some(self.step.turn.sub_id.clone()),
                            root_turn_id: self.step.turn.turn_metadata_state.root_turn_id(),
                            turn_trigger: self.step.turn.turn_metadata_state.current_turn_trigger(),
                            environments: Some(self.step.environments.clone()),
                            final_output_json_schema: schema_wire,
                            cyber_access_program: self.step.turn.cyber_access_program,
                            ..Default::default()
                        },
                    },
                    super::CompletionReporting::SupervisorOwned,
                )
                .await
                .map_err(|_| "workflow child admission failed")?;
            let child = admitted.thread;
            observation.thread_id = child.session.thread_id.to_string();
            let mut subscription = child.subscribe_status();
            owned.push(child);
            let observer = self.observer.clone();
            if let Some(observer) = &observer {
                observer(observation.clone());
            }
            waits.push(async move {
                loop {
                    let terminal = {
                        let status = subscription.borrow_and_update();
                        match &*status {
                            AgentStatus::Completed(Some(text)) => {
                                observation.status = "completed".into();
                                if let Some(observer) = &observer {
                                    observer(observation.clone());
                                }
                                Some(match &schema {
                                    Some(schema) => {
                                        schema.parse_result(text).map(|result| (index, result))
                                    }
                                    None if text.len() <= 8192 => {
                                        Ok((index, Value::String(text.clone())))
                                    }
                                    None => Err("workflow text result exceeds 8192 bytes".into()),
                                })
                            }
                            AgentStatus::PendingInit | AgentStatus::Running => None,
                            AgentStatus::Completed(None)
                            | AgentStatus::Errored(_)
                            | AgentStatus::Interrupted
                            | AgentStatus::Shutdown
                            | AgentStatus::NotFound => {
                                observation.status = "failed".into();
                                if let Some(observer) = &observer {
                                    observer(observation.clone());
                                }
                                Some(Err("workflow child did not produce a result".into()))
                            }
                        }
                    };
                    if let Some(terminal) = terminal {
                        return terminal;
                    }
                    subscription
                        .changed()
                        .await
                        .map_err(|_| "workflow child status stream ended")?;
                }
            });
        }
        let mut results = vec![Value::Null; owned.len()];
        while !waits.is_empty() {
            let (index, result) = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err("workflow cancelled".into()),
                _ = tokio::time::sleep_until(deadline) => return Err("workflow deadline exceeded".into()),
                result = waits.next() => result.ok_or("workflow wait ended")??,
            };
            results[index] = result;
        }
        if cancellation.is_cancelled() {
            return Err("workflow cancelled".into());
        }
        if compatible {
            codex_code_mode::workflow::encode(
                &results,
                codex_code_mode::workflow::GROUP_RESULT_BYTES,
            )
            .map_err(|_| "workflow native wave result budget exceeded")?;
        } else {
            check_json_budget(&results).map_err(|error| match error {
                EncodingError::Limit => "workflow group result exceeds 8192 bytes",
                EncodingError::Invalid => "invalid workflow group result",
            })?;
        }
        Ok(results)
    }
}

// Fallback for a panic or abort of the supervisor itself. Normal completion awaits
// shutdown above; destructor recovery cannot promise completion during runtime exit.
struct OwnedChildren {
    control: LocalAgentControl,
    threads: Vec<Arc<CodexThread>>,
}
impl Drop for OwnedChildren {
    fn drop(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let threads = std::mem::take(&mut self.threads);
        let control = self.control.clone();
        drop(tokio::spawn(async move {
            for child in threads {
                if let Err(error) = shutdown_owned_child(&control, &child).await {
                    tracing::warn!("workflow fallback child shutdown failed: {error}");
                }
            }
        }));
    }
}

pub(super) async fn shutdown_owned_child(
    control: &LocalAgentControl,
    child: &Arc<CodexThread>,
) -> Result<(), String> {
    // Persistence failure must never prevent stopping work. The session shutdown
    // path itself records persistence errors after terminating runtime resources.
    let shutdown = child.shutdown_and_wait().await;
    child.wait_until_terminated().await;
    let id = child.session.thread_id;
    if let Ok(manager) = control.runtime.upgrade() {
        let mut threads = manager.threads.write().await;
        if threads
            .get(&id)
            .is_some_and(|current| Arc::ptr_eq(current, child))
        {
            threads.remove(&id);
            control.forget_v2_residency(id);
            control.runtime.registry.release_spawned_thread(id);
        }
    }
    shutdown.map_err(|_| "workflow child shutdown submission failed".into())
}
