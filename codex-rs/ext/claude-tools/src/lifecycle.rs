use std::sync::Arc;

use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::ToolStartInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnLifecycleContributor;

use crate::FileTools;
use crate::publication::Publications;
use crate::publication::TurnCalls;

pub(crate) fn publications(thread: &ExtensionData) -> Arc<Publications> {
    thread.get_or_init(Publications::default)
}

impl ToolLifecycleContributor for FileTools {
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            if matches!(input.tool_name.name.as_str(), "Read" | "Glob" | "Grep")
                && input.tool_name.is_default_namespace()
            {
                let turn = input.turn_store.get_or_init(TurnCalls::default);
                publications(input.thread_store).admit(&turn, input.turn_id, input.call_id);
            }
        })
    }

    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            if let Some(registry) = input.thread_store.get::<Publications>() {
                registry
                    .finish(
                        input.turn_id,
                        input.call_id,
                        input.outcome,
                        input.result_disposition,
                    )
                    .await;
            }
        })
    }
}

impl TurnLifecycleContributor for FileTools {
    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let turn = input.turn_store.get_or_init(TurnCalls::default);
            publications(input.thread_store).close(&turn).await;
        })
    }
}
