//! Asynchronous command preparation never takes ownership of the visible draft.
//! Edits cancel preparation. Completion is consumed once and acknowledged only after dispatch.
use super::*;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(crate) struct ClaudeCommandRequest {
    pub(crate) id: uuid::Uuid,
    pub(crate) name: String,
    pub(crate) path: codex_utils_absolute_path::AbsolutePathBuf,
    pub(crate) arguments: String,
    pub(crate) cancellation: CancellationToken,
}

impl PartialEq for ClaudeCommandRequest {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.path == other.path
            && self.arguments == other.arguments
    }
}

pub(super) struct PendingClaudeCommand {
    request: ClaudeCommandRequest,
    draft: ComposerDraft,
    should_queue: bool,
}

impl Drop for PendingClaudeCommand {
    fn drop(&mut self) {
        self.request.cancellation.cancel();
    }
}

pub(crate) struct ResolvedClaudeSubmission {
    pub(crate) snapshot: ComposerDraftSnapshot,
    pub(crate) text: String,
    pub(crate) should_queue: bool,
    draft: ComposerDraft,
}

impl ChatComposer {
    pub(crate) fn cancel_claude_command(&mut self) {
        self.pending_claude_command = None;
    }

    pub(super) fn try_prepare_claude_command(&mut self, should_queue: bool) -> Option<InputResult> {
        if !self.slash_commands_enabled() || self.draft.is_bash_mode {
            return None;
        }
        let text = self.current_text_with_pending();
        let (name, arguments, _) = parse_slash_name(&text)?;
        // Reserved built-ins and their aliases keep their native semantics even when hidden.
        if crate::slash_command::built_in_slash_commands()
            .iter()
            .any(|(builtin, _)| *builtin == name)
        {
            return None;
        }
        if self
            .service_tier_commands
            .iter()
            .any(|command| command.name == name)
        {
            return None;
        }
        let skills = self.skills.as_ref()?;
        let Some(command) = self
            .claude_commands
            .iter()
            .find(|command| command.name == name)
        else {
            if skills.iter().any(|skill| skill.name == name) {
                self.claude_command_error();
                return Some(InputResult::None);
            }
            return None;
        };
        let skill = skills.iter().find(|skill| skill.path == command.path)?;
        if self.pending_claude_command.is_some() {
            return Some(InputResult::None);
        }
        if !super::super::claude_slash_catalog::user_invocable(skill) {
            self.claude_command_error();
            return Some(InputResult::None);
        }
        let request = ClaudeCommandRequest {
            id: uuid::Uuid::new_v4(),
            name: skill.name.clone(),
            path: skill.path.clone(),
            arguments: arguments.to_owned(),
            cancellation: CancellationToken::new(),
        };
        self.pending_claude_command = Some(PendingClaudeCommand {
            request: request.clone(),
            draft: self.snapshot_draft(),
            should_queue,
        });
        Some(InputResult::ClaudeCommand(request))
    }

    pub(crate) fn resolve_claude_command(
        &mut self,
        id: uuid::Uuid,
        result: Result<String, String>,
    ) -> Option<ResolvedClaudeSubmission> {
        let pending = self
            .pending_claude_command
            .take_if(|pending| pending.request.id == id)?;
        if self.snapshot_draft() != pending.draft || self.blocks_direct_input {
            return None;
        }
        let current_name = parse_slash_name(&pending.draft.text).map(|(name, _, _)| name);
        let valid = self.claude_commands.iter().any(|command| {
            Some(command.name.as_str()) == current_name && command.path == pending.request.path
        }) && self.skills.as_ref().is_some_and(|skills| {
            skills.iter().any(|skill| {
                skill.name == pending.request.name
                    && skill.path == pending.request.path
                    && super::super::claude_slash_catalog::user_invocable(skill)
            })
        });
        if !valid {
            return None;
        }
        let text = match result {
            Ok(text) if !text.is_empty() && text.len() <= 8 * 1024 => text,
            Ok(_) | Err(_) => {
                self.claude_command_error();
                return None;
            }
        };
        Some(ResolvedClaudeSubmission {
            snapshot: self.draft_snapshot(),
            text,
            should_queue: pending.should_queue,
            draft: pending.draft.clone(),
        })
    }

    pub(crate) fn acknowledge_claude_command(
        &mut self,
        resolved: ResolvedClaudeSubmission,
        accepted: bool,
    ) {
        if !accepted {
            self.restore_draft(resolved.draft);
            self.claude_command_error();
            return;
        }
        if self.snapshot_draft() != resolved.draft {
            return;
        }
        self.history.record_local_submission(HistoryEntry {
            text: resolved.draft.text.clone(),
            text_elements: resolved.draft.text_elements.clone(),
            local_image_paths: resolved.draft.local_image_paths.clone(),
            remote_image_urls: resolved.draft.remote_image_urls.clone(),
            mention_bindings: resolved.draft.mention_bindings.clone(),
            pending_pastes: resolved.draft.pending_pastes,
        });
        self.set_text_content(String::new(), Vec::new(), Vec::new());
        self.attachments.clear_remote_image_urls();
        self.draft.pending_pastes.clear();
        self.sync_popups();
    }

    fn claude_command_error(&mut self) {
        self.show_footer_flash(
            Line::from(
                "Skill unavailable or ambiguous; choose its qualified name from /; draft preserved"
                    .red(),
            ),
            Duration::from_secs(5),
        );
    }
}

#[cfg(test)]
#[path = "claude_submission_tests.rs"]
mod tests;
