//! Choose the next launch's transcript renderer without changing this session.
//!
//! `/tui` opens the picker; `/tui classic` and `/tui fullscreen` select the same
//! persisted preferences directly. Classic uses native terminal scrollback, while
//! temporary transcript overlays can still use the configured alternate buffer.
//! This does not restart the CLI or modify the running session's terminal ownership.

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionDescriptionLayout;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::picker_hint_line_for_keymap;

impl ChatWidget {
    pub(super) fn select_tui_mode(&mut self, args: &str) {
        let enabled = match args.to_ascii_lowercase().as_str() {
            "classic" | "scrollback" => false,
            "fullscreen" => true,
            _ => {
                self.add_error_message("Usage: /tui [classic|fullscreen]".to_string());
                return;
            }
        };
        self.app_event_tx
            .send(AppEvent::FullscreenTranscriptSelected { enabled });
    }

    pub(crate) fn show_tui_mode_picker(&mut self) {
        let items = [
            (
                false,
                "Classic",
                "Native terminal copy, paste and scrollback",
            ),
            (
                true,
                "Fullscreen",
                "Scroll within Claudex's fullscreen view",
            ),
        ]
        .into_iter()
        .map(|(enabled, name, description)| SelectionItem {
            name: name.into(),
            description: Some(description.into()),
            is_current: enabled == self.local_settings.tui.fullscreen_transcript,
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::FullscreenTranscriptSelected { enabled });
            })],
            dismiss_on_select: true,
            require_explicit_confirmation: true,
            ..Default::default()
        })
        .collect();
        self.show_selection_view(SelectionViewParams {
            title: Some("TUI mode for next launch".into()),
            description_layout: SelectionDescriptionLayout::Columns,
            footer_note: Some("Restart to apply. Launch overrides still apply.".into()),
            footer_hint: Some(picker_hint_line_for_keymap(&self.bottom_pane.list_keymap())),
            items,
            ..SelectionViewParams::picker()
        });
    }
}
