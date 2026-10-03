//! Synchronous resource cleanup for every dispatch scope, including nested code mode.

use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Arc;

use codex_extension_api::ToolDispatchDroppedInput;

use crate::session::session::Session;

pub(super) struct DispatchCleanup {
    pub(super) session: Arc<Session>,
    pub(super) turn_id: String,
    pub(super) call_id: String,
}

impl Drop for DispatchCleanup {
    fn drop(&mut self) {
        for contributor in self
            .session
            .services
            .extensions
            .tool_lifecycle_contributors()
        {
            // One broken contributor cannot skip other cleanup or cause a second
            // unwind to escape this destructor. Never log its panic payload.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                contributor.on_tool_dispatch_dropped(ToolDispatchDroppedInput {
                    thread_store: &self.session.services.thread_extension_data,
                    turn_id: &self.turn_id,
                    call_id: &self.call_id,
                });
            })) {
                // Panic payloads may themselves panic when destroyed. Dispose of
                // the first payload inside another catch; deliberately retain a
                // secondary payload rather than let another unwind escape Drop.
                if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                    std::mem::forget(secondary);
                }
                tracing::warn!("Tool dispatch resource cleanup contributor panicked");
            }
        }
    }
}
