//! Slash discovery consumes server-owned metadata, never UI-local Markdown paths.

use std::collections::HashMap;

use super::slash_commands::ServiceTierCommand;
use crate::skills_helpers::skill_description;
use crate::slash_command::built_in_slash_commands;
use codex_app_server_protocol::SkillMetadata;
use codex_app_server_protocol::SkillScope;
use codex_utils_absolute_path::AbsolutePathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaudeSlashCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) argument_hint: Option<String>,
    pub(crate) path: AbsolutePathBuf,
}

pub(crate) fn commands(
    skills: &[SkillMetadata],
    service_tiers: &[ServiceTierCommand],
) -> Vec<ClaudeSlashCommand> {
    let reserved = built_in_slash_commands();
    let is_reserved = |name: &str| {
        reserved.iter().any(|(builtin, _)| *builtin == name)
            || service_tiers.iter().any(|command| command.name == name)
    };
    let candidates: Vec<_> = skills
        .iter()
        .filter(|skill| user_invocable(skill))
        .map(|skill| {
            let name = match &skill.plugin_id {
                Some(plugin) if !skill.name.contains(':') => format!("{plugin}:{}", skill.name),
                Some(_) | None => skill.name.clone(),
            };
            (skill, name)
        })
        .collect();
    let mut name_counts = HashMap::<&str, usize>::new();
    for (_, name) in &candidates {
        *name_counts.entry(name.as_str()).or_insert(0) += 1;
    }
    let commands: Vec<_> = candidates
        .iter()
        .map(|(skill, name)| {
            let name = if name_counts.get(name.as_str()).copied().unwrap_or_default() > 1 {
                let scope = match skill.scope {
                    SkillScope::Repo => "repo",
                    SkillScope::User => "user",
                    SkillScope::System => "system",
                    SkillScope::Admin => "admin",
                };
                format!("{scope}:{name}")
            } else if is_reserved(name) {
                format!("skill:{name}")
            } else {
                name.clone()
            };
            ClaudeSlashCommand {
                name,
                description: skill_description(skill).to_owned(),
                argument_hint: skill
                    .claude_command
                    .as_ref()
                    .and_then(|command| command.argument_hint.clone()),
                path: skill.path.clone(),
            }
        })
        .collect();
    let mut qualified_counts = HashMap::<&str, usize>::new();
    for command in &commands {
        *qualified_counts.entry(command.name.as_str()).or_insert(0) += 1;
    }
    // Never choose the first path when even qualified names collide.
    commands
        .iter()
        .filter(|command| {
            !is_reserved(&command.name) && qualified_counts.get(command.name.as_str()) == Some(&1)
        })
        .cloned()
        .collect()
}

pub(crate) fn user_invocable(skill: &SkillMetadata) -> bool {
    skill.enabled
        && skill
            .claude_command
            .as_ref()
            .is_none_or(|command| command.user_invocable)
}

#[cfg(test)]
#[path = "claude_slash_catalog_tests.rs"]
mod tests;
