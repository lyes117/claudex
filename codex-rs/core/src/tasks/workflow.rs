//! A direct native submission uses the same turn lifecycle as other session tasks.
use super::SessionTask;
use super::SessionTaskResult;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use codex_protocol::ResponseItemId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
#[cfg(windows)]
use codex_protocol::protocol::EventMsg;
#[cfg(windows)]
use codex_protocol::protocol::WarningEvent;
use codex_protocol::protocol::WorkflowRunRequest;
use codex_thread_store::PersistContext;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) struct WorkflowTask(pub(crate) WorkflowRunRequest);
impl SessionTask for WorkflowTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "session_task.workflow"
    }
    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation: CancellationToken,
    ) -> SessionTaskResult {
        session.emit_turn_started(&turn).await;
        #[cfg(windows)]
        let output = {
            let step = session
                .capture_step_context(Arc::clone(&turn), &cancellation)
                .await?;
            match crate::agent::control::workflow_runner::run(
                Arc::clone(&session),
                step,
                self.0.clone(),
                cancellation,
            )
            .await
            {
                Ok(result) => result.to_string(),
                Err(message) => {
                    session
                        .send_event(
                            &turn,
                            EventMsg::Warning(WarningEvent {
                                message: message.clone(),
                            }),
                        )
                        .await;
                    format!("Workflow failed: {message}")
                }
            }
        };
        #[cfg(not(windows))]
        let output = {
            let _ = (cancellation, &self.0);
            "Native Workflow currently requires Windows containment.".to_string()
        };
        session
            .record_response_item_and_emit_turn_item(
                &turn,
                turn.model_info(),
                ResponseItem::Message {
                    id: Some(ResponseItemId::new("msg")),
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText {
                        text: output.clone(),
                    }],
                    phase: Some(MessagePhase::FinalAnswer),
                    internal_chat_message_metadata_passthrough: None,
                },
            )
            .await;
        session
            .ensure_rollout_materialized(PersistContext::Standard)
            .await;
        Ok(Some(output))
    }
}
