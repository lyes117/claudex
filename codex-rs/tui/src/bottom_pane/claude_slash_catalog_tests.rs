use super::super::command_popup::CommandItem;
use super::super::command_popup::CommandPopup;
use super::super::command_popup::CommandPopupFlags;
use super::*;
use codex_app_server_protocol::ClaudeCommandMetadata;
use codex_app_server_protocol::SkillScope;
use pretty_assertions::assert_eq;

fn skill(name: &str, enabled: bool, user_invocable: bool) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("Run {name}"),
        short_description: None,
        interface: None,
        dependencies: None,
        scope: SkillScope::Repo,
        enabled,
        plugin_id: None,
        path: AbsolutePathBuf::from_absolute_path(std::env::temp_dir().join(format!("{name}.md")))
            .expect("path"),
        claude_command: Some(ClaudeCommandMetadata {
            user_invocable,
            argument_hint: Some("[révision]".into()),
        }),
    }
}

#[test]
fn menu_tracks_enabled_policy_and_preserves_native_alias_precedence() {
    let mut skills = vec![
        skill("deploy", true, true),
        skill("hidden", true, false),
        skill("disabled", false, true),
        skill("model", true, true),
        skill("quit", true, true),
    ];
    assert_eq!(
        commands(&skills, &[]),
        vec![
            ClaudeSlashCommand {
                name: "deploy".into(),
                description: "Run deploy".into(),
                argument_hint: Some("[révision]".into()),
                path: skills[0].path.clone(),
            },
            ClaudeSlashCommand {
                name: "skill:model".into(),
                description: "Run model".into(),
                argument_hint: Some("[révision]".into()),
                path: skills[3].path.clone(),
            },
            ClaudeSlashCommand {
                name: "skill:quit".into(),
                description: "Run quit".into(),
                argument_hint: Some("[révision]".into()),
                path: skills[4].path.clone(),
            },
        ]
    );
    let expected_enabled = commands(&skills[1..], &[]);
    skills[0].enabled = false;
    assert_eq!(commands(&skills, &[]), expected_enabled);
}

#[test]
fn qualified_plugin_command_matches_bare_name_but_completes_qualified_name() {
    let skills = vec![skill("film:publish", true, true)];
    let mut popup = CommandPopup::new(CommandPopupFlags::default(), Vec::new())
        .with_claude_commands(commands(&skills, &[]));
    popup.on_composer_text_change("/pub".to_string());
    assert_eq!(
        popup.selected_item(),
        Some(CommandItem::ClaudeSkill(commands(&skills, &[]).remove(0)))
    );
    popup.on_composer_text_change("/film:pub".to_string());
    assert_eq!(
        popup.selected_item().expect("qualified command").command(),
        "film:publish"
    );
}

#[test]
fn native_skills_follow_commands_and_disambiguate_scope_plugin_and_tier_collisions() {
    let mut project = skill("deploy", true, true);
    project.claude_command = None;
    let mut user = project.clone();
    user.scope = SkillScope::User;
    user.path = AbsolutePathBuf::from_absolute_path(std::env::temp_dir().join("user/deploy.md"))
        .expect("path");
    let mut plugin = skill("deploy", true, true);
    plugin.plugin_id = Some("film".into());
    plugin.path = AbsolutePathBuf::from_absolute_path(std::env::temp_dir().join("film/deploy.md"))
        .expect("path");
    let native_tiers = vec![ServiceTierCommand {
        id: "priority".into(),
        name: "fast".into(),
        description: "Native tier".into(),
    }];
    let skills = vec![project, user, plugin, skill("fast", true, true)];
    let catalogue = commands(&skills, &native_tiers);
    assert_eq!(
        catalogue
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>(),
        vec!["repo:deploy", "user:deploy", "film:deploy", "skill:fast"]
    );
    let mut popup = CommandPopup::new(CommandPopupFlags::default(), native_tiers)
        .with_claude_commands(catalogue.clone());
    popup.on_composer_text_change("/".into());
    popup.move_up();
    for command in catalogue.iter().rev() {
        assert_eq!(
            popup.selected_item(),
            Some(CommandItem::ClaudeSkill(command.clone()))
        );
        popup.move_up();
    }
    assert!(!matches!(
        popup.selected_item(),
        Some(CommandItem::ClaudeSkill(_))
    ));

    let mut ambiguous = skills;
    let mut duplicate = ambiguous[0].clone();
    duplicate.path =
        AbsolutePathBuf::from_absolute_path(std::env::temp_dir().join("other/deploy.md"))
            .expect("path");
    ambiguous.push(duplicate);
    assert!(
        commands(&ambiguous, &[])
            .iter()
            .all(|command| command.name != "repo:deploy")
    );
}
