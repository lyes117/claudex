//! Native admission retains the exact runtime through initial input and ownership handoff.
use super::spawn::SpawnInitialInput;
use super::*;
use crate::codex_thread::CodexThread;
use crate::codex_thread::ThreadConfigSnapshot;

pub(super) struct AdmittedAgent {
    pub(super) agent: LiveAgent,
    pub(super) config: ThreadConfigSnapshot,
    pub(super) thread: Arc<CodexThread>,
}

impl LocalAgentControl {
    pub(super) async fn admit_input_to_thread(
        &self,
        thread: &Arc<CodexThread>,
        state: &Arc<ThreadManagerState>,
        input: SpawnInitialInput,
        start_options: TurnStartOptions,
    ) -> CodexResult<String> {
        match input {
            SpawnInitialInput::UserInput(input) => {
                self.send_input_to_thread(thread, state, input, start_options)
                    .await
            }
            SpawnInitialInput::InterAgentCommunication(communication, context) => {
                let communication_for_log =
                    crate::agent_communication::logging_enabled().then(|| communication.clone());
                let (parent_turn_id, root_turn_id) = if communication.trigger_turn {
                    (
                        start_options.parent_turn_id.clone(),
                        start_options.root_turn_id.clone(),
                    )
                } else {
                    (None, None)
                };
                let result = state
                    .send_op_to_thread(
                        thread,
                        Op::InterAgentCommunication {
                            communication,
                            start_options,
                        },
                        parent_turn_id,
                        root_turn_id,
                    )
                    .await;
                let result = self
                    .handle_exact_thread_request_result(thread, state, result)
                    .await;
                if let (Some(communication), Ok(communication_id)) =
                    (communication_for_log, &result)
                {
                    crate::agent_communication::emit_agent_communication_send(
                        communication_id,
                        &context,
                        &communication,
                        thread.session.thread_id,
                    );
                }
                result
            }
        }
    }

    pub(super) async fn handle_exact_thread_request_result(
        &self,
        thread: &Arc<CodexThread>,
        state: &Arc<ThreadManagerState>,
        result: CodexResult<String>,
    ) -> CodexResult<String> {
        if result
            .as_ref()
            .is_err_and(|err| matches!(err.details(), CodexErrorDetails::InternalAgentDied))
        {
            let id = thread.session.thread_id;
            let mut threads = state.threads.write().await;
            if threads
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, thread))
            {
                threads.remove(&id);
                self.forget_v2_residency(id);
                self.runtime.registry.release_spawned_thread(id);
            }
        }
        result
    }
}
