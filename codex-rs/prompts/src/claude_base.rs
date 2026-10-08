//! Claudex base prompts: the extracted Claude Code system prompt plus the shared
//! ponytail contract.
//!
//! Provenance: the verbatim extraction lives in
//! `.build-tools/claude-prompts/` (see its `provenance.json`; Claude Code 2.1.289).
//! This module carries the cleaned, tool-neutral assembly — extraction scaffolding
//! and runtime placeholders removed, instruction text kept verbatim.
//!
//! The previous default (the Codex prompt bundled as `codex-models-manager/prompt.md`
//! and served through `render_model_instructions`) stays intact as the documented
//! fallback: set `base_instructions` or `model_instructions_file` in config to
//! restore it for a session.

/// The extracted Claude Code main system prompt (cleaned, tool-neutral).
pub const CLAUDE_BASE_INSTRUCTIONS: &str = include_str!("../templates/claude_main.md");

/// The ponytail contract, shared by every entry point — main base instructions,
/// agent-role developer instructions, subagent/workflow hints. Single source of truth.
pub const PONYTAIL_CONTRACT: &str = include_str!("../templates/ponytail.md");

/// Default base instructions for every session: the Claude Code prompt, ending
/// with the ponytail contract.
pub fn claude_base_instructions() -> String {
    format!("{CLAUDE_BASE_INSTRUCTIONS}\n\n{PONYTAIL_CONTRACT}")
}

/// Appends the shared ponytail contract to a sub-agent/workflow prompt.
pub fn with_ponytail(base: &str) -> String {
    format!("{base}\n\n{PONYTAIL_CONTRACT}")
}

#[cfg(test)]
#[path = "claude_base_tests.rs"]
mod tests;
