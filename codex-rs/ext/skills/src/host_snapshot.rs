use std::io;
use std::sync::Arc;

use crate::SkillLoadOutcome;
use codex_skills::SkillMetadata;

/// Immutable snapshot of host-owned skills and their source filesystems.
#[derive(Debug, Clone)]
pub struct HostSkillsSnapshot {
    outcome: Arc<SkillLoadOutcome>,
}

impl HostSkillsSnapshot {
    pub fn new(outcome: Arc<SkillLoadOutcome>) -> Self {
        Self { outcome }
    }

    pub fn outcome(&self) -> &SkillLoadOutcome {
        self.outcome.as_ref()
    }

    /// Read a command only through its recorded authority, with a 32 KiB stream budget.
    /// A retargeted symlink is rejected before and after reading; no arbitrary path is accepted.
    pub async fn read_claude_command_text(&self, skill: &SkillMetadata) -> io::Result<String> {
        let fs = self
            .outcome
            .file_system_for_skill(skill)
            .ok_or_else(|| io::Error::other("Claude command filesystem unavailable"))?;
        let path = codex_utils_path_uri::PathUri::from_abs_path(&skill.path_to_skills_md);
        let before = fs.canonicalize(&path, /*sandbox*/ None).await?;
        if before != path {
            return Err(io::Error::other("Claude command identity changed"));
        }
        let text = crate::loader::read_command_text(fs.as_ref(), &path).await?;
        if fs.canonicalize(&path, /*sandbox*/ None).await? != before {
            return Err(io::Error::other("Claude command identity changed"));
        }
        Ok(text)
    }

    pub async fn read_skill_text(&self, skill: &SkillMetadata) -> io::Result<String> {
        self.outcome.read_skill_text(skill).await
    }
}
