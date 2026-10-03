use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::ToolCall;
use codex_extension_items::ExtensionItem;
use codex_tools::ConversationHistory;
use codex_tools::ToolCallSource;
use codex_tools::ToolName;
use codex_tools::ToolPayload;
use codex_tools::TurnItemEmissionFuture;
use codex_tools::TurnItemEmitter;
use pretty_assertions::assert_eq;
use tokio::sync::Semaphore;

use super::*;

struct Capture {
    items: Mutex<Vec<(&'static str, ExtensionTurnItem)>>,
    start_gate: Semaphore,
    finish_gate: Semaphore,
    finish_entered: Semaphore,
    panic_at: Option<&'static str>,
}

impl TurnItemEmitter for Capture {
    fn emit_started<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            assert_ne!(self.panic_at, Some("started"), "Synthetic emitter panic");
            self.start_gate.acquire().await.unwrap().forget();
            self.items.lock().unwrap().push(("started", item));
        })
    }

    fn emit_completed<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            assert_ne!(self.panic_at, Some("completed"), "Synthetic emitter panic");
            self.finish_entered.add_permits(1);
            self.finish_gate.acquire().await.unwrap().forget();
            self.items.lock().unwrap().push(("completed", item));
        })
    }
}

fn fixture() -> (
    Arc<Publications>,
    Arc<TurnCalls>,
    Arc<Capture>,
    ToolCall<'static>,
) {
    let registry = Arc::new(Publications::default());
    let turn = Arc::new(TurnCalls::default());
    registry.admit(&turn, "turn", "read");
    let capture = Arc::new(Capture {
        items: Mutex::new(Vec::new()),
        start_gate: Semaphore::new(1),
        finish_gate: Semaphore::new(1),
        finish_entered: Semaphore::new(0),
        panic_at: None,
    });
    let call = ToolCall {
        turn_id: "turn".into(),
        call_id: "read".into(),
        tool_name: ToolName::plain("Read"),
        model: "fixture".into(),
        codex_turn_metadata: None,
        truncation_policy: codex_protocol::protocol::TruncationPolicy::Bytes(
            crate::MAX_RESPONSE_BYTES,
        ),
        source: ToolCallSource::Direct,
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: capture.clone(),
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: "{}".into(),
        },
    };
    (registry, turn, capture, call)
}

fn completion(capture: &Capture) -> (bool, String) {
    let items = capture.items.lock().unwrap();
    assert_eq!(
        items.iter().map(|(stage, _)| *stage).collect::<Vec<_>>(),
        ["started", "completed"]
    );
    let ExtensionItem::FileTool(item) = &items[1].1.item else {
        panic!("file card")
    };
    (item.success.unwrap(), item.output.clone().unwrap())
}

#[tokio::test]
async fn dropped_dispatch_discards_staged_output_and_preserves_an_existing_decision() {
    use codex_extension_api::ExtensionData;
    use codex_extension_api::ToolDispatchDroppedInput;
    use codex_extension_api::ToolLifecycleContributor;

    for already_rejected in [false, true] {
        let (registry, turn, mut capture, mut call) = fixture();
        let thread = ExtensionData::new("thread");
        thread.insert(
            Arc::try_unwrap(registry)
                .ok()
                .expect("Unshared fixture registry"),
        );
        let registry = thread.get::<Publications>().unwrap();
        call.turn_item_emitter = Arc::new(codex_tools::NoopTurnItemEmitter);
        Arc::get_mut(&mut capture).unwrap().finish_gate = Semaphore::new(0);
        call.turn_item_emitter = capture.clone();
        registry.begin(&call, FileTool::Read, "{}").await.unwrap();
        registry.stage(
            "turn",
            "read",
            Completion::new(/*success*/ true, "RAW_FILTERED_MARKER"),
        );
        if already_rejected {
            // A decision already sent remains owned by the independent publisher.
            let _ = registry.decide(
                "turn",
                "read",
                ToolCallOutcome::Completed { success: true },
                ToolResultDisposition::Rejected("Filtered result"),
            );
            capture.finish_entered.acquire().await.unwrap().forget();
        }
        for _ in 0..2 {
            crate::FileTools.on_tool_dispatch_dropped(ToolDispatchDroppedInput {
                thread_store: &thread,
                turn_id: "turn",
                call_id: "read",
            });
        }
        if !already_rejected {
            capture.finish_entered.acquire().await.unwrap().forget();
        }
        capture.finish_gate.add_permits(1);
        registry
            .finish(
                "turn",
                "read",
                ToolCallOutcome::Aborted,
                ToolResultDisposition::Unchanged,
            )
            .await;
        assert_eq!(
            completion(&capture),
            (
                false,
                if already_rejected {
                    "Filtered result".into()
                } else {
                    "File tool interrupted".into()
                }
            )
        );
        assert!(registry.0.lock().unwrap().is_empty());
        assert!(turn.0.lock().unwrap().keys.is_empty());
    }
}

#[tokio::test]
async fn hook_disposition_controls_display_without_original_result() {
    for (disposition, expected) in [
        (
            ToolResultDisposition::Feedback("HOOK_FEEDBACK"),
            (true, "HOOK_FEEDBACK"),
        ),
        (
            ToolResultDisposition::Rejected("HOOK_REJECTED"),
            (false, "HOOK_REJECTED"),
        ),
        (ToolResultDisposition::Unchanged, (true, "ORIGINAL_MARKER")),
    ] {
        let (registry, _, capture, call) = fixture();
        registry.begin(&call, FileTool::Read, "{}").await.unwrap();
        registry.stage(
            "turn",
            "read",
            Completion::new(/*success*/ true, "ORIGINAL_MARKER"),
        );
        registry
            .finish(
                "turn",
                "read",
                ToolCallOutcome::Completed { success: true },
                disposition,
            )
            .await;
        assert_eq!(completion(&capture), (expected.0, expected.1.to_owned()));
        assert!(registry.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn completion_survives_callback_forced_abort_after_100ms() {
    let (registry, turn, capture, call) = fixture();
    capture.finish_gate.acquire().await.unwrap().forget();
    registry.begin(&call, FileTool::Read, "{}").await.unwrap();
    registry.stage(
        "turn",
        "read",
        Completion::new(/*success*/ true, "ORIGINAL_MARKER"),
    );
    let finishing = {
        let registry = registry.clone();
        tokio::spawn(async move {
            registry
                .finish(
                    "turn",
                    "read",
                    ToolCallOutcome::Completed { success: true },
                    ToolResultDisposition::Unchanged,
                )
                .await;
        })
    };
    capture.finish_entered.acquire().await.unwrap().forget();
    tokio::time::sleep(Duration::from_millis(120)).await;
    finishing.abort();
    assert!(finishing.await.unwrap_err().is_cancelled());
    let aborting = {
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.close(&turn).await;
        })
    };
    capture.finish_gate.add_permits(1);
    aborting.await.unwrap();
    assert_eq!(completion(&capture), (true, "ORIGINAL_MARKER".into()));
    assert!(registry.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn abort_during_started_emission_publishes_in_order_and_rejects_late_execution() {
    let (registry, turn, capture, call) = fixture();
    capture.start_gate.acquire().await.unwrap().forget();
    let beginning = {
        let registry = registry.clone();
        tokio::spawn(async move { registry.begin(&call, FileTool::Read, "{}").await })
    };
    tokio::task::yield_now().await;
    let closing = {
        let registry = registry.clone();
        let turn = turn.clone();
        tokio::spawn(async move { registry.close(&turn).await })
    };
    while !turn.0.lock().unwrap().closed {
        tokio::task::yield_now().await;
    }
    capture.start_gate.add_permits(1);
    assert!(beginning.await.unwrap().is_err());
    closing.await.unwrap();
    assert_eq!(
        completion(&capture),
        (false, "File tool interrupted".into())
    );
    registry.admit(&turn, "turn", "late");
    registry.stage(
        "turn",
        "read",
        Completion::new(/*success*/ true, "LATE_MARKER"),
    );
    assert!(registry.0.lock().unwrap().is_empty());
    assert!(turn.0.lock().unwrap().keys.is_empty());
}

#[tokio::test]
async fn finish_and_abort_race_publishes_one_terminal_item() {
    let (registry, turn, capture, call) = fixture();
    registry.begin(&call, FileTool::Read, "{}").await.unwrap();
    registry.stage(
        "turn",
        "read",
        Completion::new(/*success*/ true, "ORIGINAL_MARKER"),
    );
    tokio::join!(
        registry.close(&turn),
        registry.finish(
            "turn",
            "read",
            ToolCallOutcome::Completed { success: true },
            ToolResultDisposition::Unchanged
        )
    );
    let result = completion(&capture);
    assert!(
        result == (false, "File tool interrupted".into())
            || result == (true, "ORIGINAL_MARKER".into())
    );
    assert!(registry.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn pending_call_limit_and_closure_before_admission_are_bounded() {
    let registry = Publications::default();
    let turn = Arc::new(TurnCalls::default());
    for index in 0..(MAX_PENDING + 10) {
        registry.admit(&turn, "turn", &index.to_string());
    }
    assert_eq!(registry.0.lock().unwrap().len(), MAX_PENDING);
    registry.close(&turn).await;
    registry.admit(&turn, "turn", "late");
    assert!(registry.0.lock().unwrap().is_empty());
    let unopened = Arc::new(TurnCalls::default());
    registry.close(&unopened).await;
    registry.admit(&unopened, "new-turn", "late");
    assert!(registry.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn emitter_panic_does_not_leak_pending_slots() {
    for panic_at in ["started", "completed"] {
        let (registry, turn, mut capture, mut call) = fixture();
        // Replace the only shared emitter before the task is launched.
        call.turn_item_emitter = Arc::new(codex_tools::NoopTurnItemEmitter);
        Arc::get_mut(&mut capture).unwrap().panic_at = Some(panic_at);
        call.turn_item_emitter = capture.clone();
        let begun = registry.begin(&call, FileTool::Read, "{}").await;
        assert_eq!(begun.is_ok(), panic_at == "completed");
        registry
            .finish(
                "turn",
                "read",
                ToolCallOutcome::Completed { success: true },
                ToolResultDisposition::Unchanged,
            )
            .await;
        registry.close(&turn).await;
        assert!(registry.0.lock().unwrap().is_empty());
        assert!(turn.0.lock().unwrap().keys.is_empty());
    }
}

#[test]
fn closed_runtime_destroys_publication_without_reentrant_mutex_deadlock() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let handle = runtime.handle().clone();
    drop(runtime);
    let _entered = handle.enter();
    let (registry, turn, _, call) = fixture();
    assert!(futures::executor::block_on(registry.begin(&call, FileTool::Read, "{}")).is_err());
    assert!(registry.0.lock().unwrap().is_empty());
    assert!(turn.0.lock().unwrap().keys.is_empty());
}

#[tokio::test]
async fn filesystem_panic_after_started_needs_no_host_completion_to_release_the_slot() {
    use codex_extension_api::ToolExecutor;
    use futures::FutureExt;
    for source in [
        ToolCallSource::Direct,
        ToolCallSource::CodeMode {
            cell_id: "cell".into(),
            runtime_tool_call_id: "nested".into(),
        },
    ] {
        let (registry, turn, capture, mut call) = fixture();
        let root = tempfile::tempdir().unwrap();
        let mut environment = crate::tests::environment(root.path());
        let mut fs = crate::test_file_system::CheckedFileSystem::new(
            environment.file_system_sandbox_context.clone(),
        );
        fs.metadata_panic = true;
        environment.file_system = Arc::new(fs);
        call.environments = vec![environment];
        call.payload = ToolPayload::Function {
            arguments: r#"{"file_path":"sample"}"#.into(),
        };
        call.source = source;
        let tool = crate::NativeFileTool {
            tool: FileTool::Read,
            publications: registry.clone(),
        };
        assert!(
            std::panic::AssertUnwindSafe(tool.handle(call))
                .catch_unwind()
                .await
                .is_err()
        );
        // No finish or close callback: the handler guard owns the fallback decision.
        capture.finish_entered.acquire().await.unwrap().forget();
        while !registry.0.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            completion(&capture),
            (false, "File tool produced no result".into())
        );
        assert!(turn.0.lock().unwrap().keys.is_empty());
    }
}
