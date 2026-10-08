//! Route validated command text through the native submission/queue machinery.
use super::user_messages::UserMessageHistoryOverride;
use super::*;

impl ChatWidget {
    pub(crate) fn cancel_claude_command(&mut self) {
        self.bottom_pane.cancel_claude_command();
    }

    pub(crate) fn on_claude_command_expanded(
        &mut self,
        id: uuid::Uuid,
        result: Result<String, String>,
    ) {
        let Some(submission) = self.bottom_pane.resolve_claude_command(id, result) else {
            return;
        };
        let snapshot = &submission.snapshot;
        let user_message = UserMessage {
            text: submission.text.clone(),
            text_elements: Vec::new(),
            local_images: snapshot.local_images.clone(),
            remote_image_urls: snapshot.remote_image_urls.clone(),
            mention_bindings: snapshot.mention_bindings.clone(),
        };
        let history = UserMessageHistoryRecord::Override(UserMessageHistoryOverride {
            text: snapshot.text.clone(),
            text_elements: snapshot.text_elements.clone(),
        });
        let submit_now = !submission.should_queue
            && self.is_session_configured()
            && !self.is_plan_streaming_in_tui()
            && !self.input_queue.suppress_queue_autosend
            && !self.input_queue.rate_limit_recovery_pending
            && (!self.input_queue.user_turn_pending_start
                || self.turn_lifecycle.agent_turn_running)
            && !self.only_user_shell_commands_running();
        let accepted = if submit_now {
            self.reasoning_buffer.clear();
            self.reasoning_header = None;
            self.reasoning_summary_parts.clear();
            self.set_status_header(String::from("Working"));
            self.submit_user_message_with_history_and_shell_escape_policy(
                user_message,
                history,
                ShellEscapePolicy::Disallow,
                UserMessageSource::Prompt,
            )
            .0
        } else {
            self.queue_user_message_with_history(
                user_message,
                QueuedInputAction::Literal,
                Vec::new(),
                UserMessageSource::Prompt,
                history,
            )
        };
        self.bottom_pane
            .acknowledge_claude_command(submission, accepted);
        if accepted {
            self.app_event_tx.send(AppEvent::FollowTranscript);
        }
        self.request_redraw();
    }
}
