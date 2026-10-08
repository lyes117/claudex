//! Persist the next launch's renderer preference without touching terminal ownership.

use super::App;
use crate::config_update::format_config_error;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;

impl App {
    pub(super) async fn save_fullscreen_transcript(&mut self, enabled: bool) {
        // Change only a persisted Never preference, not an effective launch restriction
        // imposed by --no-alt-screen, SSH compatibility, or other configuration layers.
        // Evaluate the condition in the same document transaction as the mode write.
        let mut edits = vec![ConfigEdit::SetPath {
            segments: vec!["tui".into(), "fullscreen_transcript".into()],
            value: toml_edit::value(enabled),
        }];
        if enabled {
            edits.push(ConfigEdit::SetPathIfString {
                segments: vec!["tui".into(), "alternate_screen".into()],
                expected: "never".into(),
                value: toml_edit::value("auto"),
            });
        }
        let result =
            ConfigEditsBuilder::for_config_path(self.local_settings.user_config_path.as_path())
                .with_edits(edits)
                .apply()
                .await;
        match result {
            Ok(()) => {
                self.local_settings.tui.fullscreen_transcript = enabled;
                self.chat_widget.local_settings.tui.fullscreen_transcript = enabled;
                let mode = if enabled { "Fullscreen" } else { "Classic" };
                self.chat_widget.add_info_message(
                    format!("Saved TUI mode: {mode}. Restart Claudex to apply; launch overrides still apply."),
                    /*hint*/ None,
                );
            }
            Err(error) => self.chat_widget.add_error_message(format!(
                "Failed to save TUI mode: {}",
                format_config_error(&error),
            )),
        }
    }
}

#[cfg(test)]
#[path = "tui_mode_picker_tests.rs"]
mod tests;
