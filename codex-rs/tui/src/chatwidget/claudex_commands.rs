//! Local management surfaces that also work with the embedded Codex engine.

use super::*;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

impl ChatWidget {
    pub(super) fn open_claudex_help(&mut self) {
        let items =
            crate::bottom_pane::slash_commands::builtins_for_input(self.builtin_command_flags())
                .into_iter()
                .map(|(name, command)| SelectionItem {
                    name: format!("/{name}"),
                    description: Some(command.description().to_string()),
                    search_value: Some(format!("/{name} {}", command.description())),
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::PrefillClaudexCommand(command))
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                })
                .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Claudex help".to_string()),
            subtitle: Some("/ commands · @ files · $ skills · ! shell · ? shortcuts".to_string()),
            items,
            is_searchable: true,
            search_placeholder: Some("Find a command".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            ..SelectionViewParams::picker()
        });
    }

    pub(super) fn open_agent_roles(&mut self) {
        let mut items: Vec<_> = self
            .config
            .agent_roles
            .iter()
            .map(|(name, role)| {
                let origin = role
                    .config_file
                    .as_ref()
                    .map(|path| path.display().to_string());
                SelectionItem {
                    name: name.clone(),
                    description: role.description.clone(),
                    search_value: Some(format!(
                        "{name} {}",
                        role.description.as_deref().unwrap_or_default()
                    )),
                    selected_description: origin,
                    ..Default::default()
                }
            })
            .collect();
        if items.is_empty() {
            items.push(SelectionItem {
                name: "No custom roles loaded".to_string(),
                search_value: Some("No custom roles loaded".to_string()),
                description: Some(
                    "Add .claude/agents/*.md or .codex/agents/*.toml; restart to load them."
                        .to_string(),
                ),
                ..Default::default()
            });
        }
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Agent roles".to_string()),
            subtitle: Some(
                "Loaded configuration · /tasks shows agents in this session".to_string(),
            ),
            items,
            is_searchable: true,
            search_placeholder: Some("Find a role".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            ..SelectionViewParams::picker()
        });
    }
}
