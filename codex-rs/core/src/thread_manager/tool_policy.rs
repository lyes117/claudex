//! Captures live authority before startup can unload its source. Durable ceilings
//! are not restored here: callers still must supply policies for cold resumes.

use super::*;
use codex_extension_api::ToolPolicy;

pub(crate) const LIVE_THREAD_TOOL_POLICY_MISMATCH: &str = "cannot resume a live thread whose tool policy exceeds the requested ceiling; stop it before resuming with narrower restrictions";

#[derive(Clone)]
pub(crate) struct CapturedToolPolicy {
    pub(super) thread_id: ThreadId,
    pub(crate) policy: Arc<ToolPolicy>,
}

impl CapturedToolPolicy {
    pub(crate) fn from_session(session: &Session) -> Self {
        Self {
            thread_id: session.thread_id(),
            policy: Arc::clone(&session.tool_policy),
        }
    }
}

pub(crate) fn resolve_local_tool_policy(
    init: &ExtensionDataInit,
    source: &SessionSource,
    config: &Config,
) -> Arc<ToolPolicy> {
    let local = init.get::<ToolPolicy>().unwrap_or_else(|| {
        // Preserve the reviewer fallback even when a parent ceiling is supplied.
        if crate::guardian::is_basic_session_source(source) {
            Arc::new(codex_guardian_reviewer::reviewer_tool_policy())
        } else {
            Arc::default()
        }
    });
    if config.tools_enabled {
        local
    } else {
        Arc::new(local.intersect(&ToolPolicy {
            allowed_tools: Some(Vec::new()),
            expose_additional_permissions: false,
            ..Default::default()
        }))
    }
}

impl ThreadManagerState {
    pub(crate) async fn capture_tool_policy(
        &self,
        thread_id: Option<ThreadId>,
    ) -> Option<CapturedToolPolicy> {
        let thread_id = thread_id?;
        self.threads
            .read()
            .await
            .get(&thread_id)
            .map(|thread| CapturedToolPolicy::from_session(&thread.session))
    }

    pub(super) async fn compose_tool_policy(
        &self,
        init: &mut ExtensionDataInit,
        source: &SessionSource,
        config: &Config,
        captured: Option<CapturedToolPolicy>,
        authority_id: Option<ThreadId>,
    ) -> Arc<ToolPolicy> {
        let local = resolve_local_tool_policy(init, source, config);
        let captured = match captured {
            Some(captured) => Some(captured),
            None => self.capture_tool_policy(authority_id).await,
        };
        let effective = match captured {
            Some(captured) => {
                // A new resident incarnation may impose a stricter ceiling. It
                // must refer to this authority, not an ancestor in fork history.
                let current = self.capture_tool_policy(Some(captured.thread_id)).await;
                let parent = match current {
                    Some(current) => captured.policy.intersect(&current.policy),
                    None => captured.policy.as_ref().clone(),
                };
                Arc::new(local.intersect(&parent))
            }
            None => local,
        };
        init.insert(effective.as_ref().clone());
        effective
    }
}

#[cfg(test)]
#[path = "tools_disabled_warm_tests.rs"]
mod tools_disabled_warm_tests;
