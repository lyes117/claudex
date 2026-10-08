//! Background resolution uses the current app-server and drops canceled or retired preparations.
use super::*;
use codex_app_server_protocol::ClaudeCommandExpandParams;
use codex_app_server_protocol::ClaudeCommandExpandResponse;
use codex_app_server_protocol::RequestId;

pub(super) static CLAUDE_PREPARATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl App {
    pub(super) fn expand_claude_command(
        &mut self,
        app_server: &AppServerSession,
        thread_id: Option<ThreadId>,
        cwd: AbsolutePathBuf,
        request: crate::bottom_pane::ClaudeCommandRequest,
    ) {
        let Some(thread_id) = thread_id else {
            self.chat_widget
                .on_claude_command_expanded(request.id, Err("Thread not ready".to_string()));
            return;
        };
        if self.current_displayed_thread_id() != Some(thread_id)
            || self.chat_widget.config_ref().cwd != cwd
        {
            return;
        }
        let handle = app_server.request_handle();
        let events = self.app_event_tx.clone();
        tokio::spawn(async move {
            let cancellation = request.cancellation.clone();
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {}
                _ = async {
                    // One deadline covers both queueing and the RPC. Expiry retires client
                    // preparation; it does not claim to cancel work already running server-side.
                    let result = tokio::time::timeout(Duration::from_secs(/*secs*/ 30), async {
                        let _permit = CLAUDE_PREPARATION.lock().await;
                        handle.request_typed::<ClaudeCommandExpandResponse>(ClientRequest::ClaudeCommandExpand {
                            request_id: RequestId::String(format!("claude-command-{}", request.id)),
                            params: ClaudeCommandExpandParams { thread_id: thread_id.to_string(), cwd: cwd.to_path_buf(), name: request.name, path: request.path, arguments: request.arguments },
                        }).await
                    }).await;
                    let result = match result {
                        Ok(Ok(response)) => Ok(response.text),
                        Ok(Err(_)) | Err(_) => Err("Could not resolve Claude command".to_string()),
                    };
                    events.send(AppEvent::ClaudeCommandExpanded { thread_id, cwd, id: request.id, result });
                } => {}
            }
        });
    }
}
