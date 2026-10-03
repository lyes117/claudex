use super::*;
use crate::tools::context::ToolCallSource;
use codex_extension_api::ToolDispatchDroppedInput;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use futures::FutureExt;
use pretty_assertions::assert_eq;
use std::sync::Mutex;
use tokio::sync::Semaphore;

#[derive(Clone, Copy)]
enum FinishMode {
    Complete,
    Panic,
    Wait,
}

struct PanickingPayload;

impl Drop for PanickingPayload {
    fn drop(&mut self) {
        panic!("Synthetic panic payload destructor");
    }
}

struct CleanupObserver {
    mode: FinishMode,
    entered: Arc<Semaphore>,
    dropped: Arc<Mutex<Vec<(String, String, String)>>>,
    panic_in_cleanup: bool,
}

impl ToolLifecycleContributor for CleanupObserver {
    fn on_tool_finish<'a>(&'a self, _input: ToolFinishInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            match self.mode {
                FinishMode::Complete => {}
                FinishMode::Panic => panic!("Synthetic finish contributor panic"),
                FinishMode::Wait => {
                    self.entered.add_permits(1);
                    std::future::pending::<()>().await;
                }
            }
        })
    }

    fn on_tool_dispatch_dropped(&self, input: ToolDispatchDroppedInput<'_>) {
        if self.panic_in_cleanup {
            std::panic::panic_any(PanickingPayload);
        }
        self.dropped.lock().unwrap().push((
            input.thread_store.level_id().to_owned(),
            input.turn_id.to_owned(),
            input.call_id.to_owned(),
        ));
    }
}

#[tokio::test]
async fn dispatch_cleanup_survives_finish_panic_cancellation_and_cleanup_panic() {
    for mode in [FinishMode::Complete, FinishMode::Panic, FinishMode::Wait] {
        for source in [
            ToolCallSource::Direct,
            ToolCallSource::CodeMode {
                cell_id: "cell".into(),
                runtime_tool_call_id: "nested".into(),
            },
        ] {
            let (mut session, turn) = crate::session::tests::make_session_and_context().await;
            let entered = Arc::new(Semaphore::new(0));
            let dropped = Arc::new(Mutex::new(Vec::new()));
            let expected = vec![(
                session.services.thread_extension_data.level_id().to_owned(),
                turn.sub_id.clone(),
                "scope-call".into(),
            )];
            let mut builder =
                codex_extension_api::ExtensionRegistryBuilder::<crate::config::Config>::new();
            builder.tool_lifecycle_contributor(Arc::new(CleanupObserver {
                mode,
                entered: Arc::clone(&entered),
                dropped: Arc::new(Mutex::new(Vec::new())),
                // Exercise a second caught panic during unwind as well as at normal return.
                panic_in_cleanup: true,
            }));
            builder.tool_lifecycle_contributor(Arc::new(CleanupObserver {
                mode: FinishMode::Complete,
                entered: Arc::clone(&entered),
                dropped: Arc::clone(&dropped),
                panic_in_cleanup: false,
            }));
            session.services.extensions = Arc::new(builder.build());
            let session = Arc::new(session);
            let turn = Arc::new(turn);
            let name = ToolName::plain("scope_tool");
            let registry = ToolRegistry::with_handler_for_test(Arc::new(TestHandler {
                tool_name: name.clone(),
            }));
            let mut invocation = test_invocation(session, turn, "scope-call", name);
            invocation.source = source;
            let state = Arc::new(ToolCallState::default());
            let mut dispatch =
                Box::pin(registry.dispatch_any_with_state(invocation, Some(Arc::clone(&state))));
            match mode {
                FinishMode::Complete => assert!(dispatch.await.is_ok()),
                FinishMode::Panic => assert!(
                    std::panic::AssertUnwindSafe(dispatch)
                        .catch_unwind()
                        .await
                        .is_err()
                ),
                FinishMode::Wait => {
                    tokio::select! {
                        permit = entered.acquire() => permit.unwrap().forget(),
                        _ = &mut dispatch => panic!("Dispatch completed before the finish gate"),
                    }
                    drop(dispatch);
                }
            }
            // The normal completion claim precedes the failing/blocked callback.
            assert!(state.terminal_outcome_reached.load(Ordering::Acquire));
            assert_eq!(*dropped.lock().unwrap(), expected);
        }
    }
}
