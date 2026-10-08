use super::*;
use pretty_assertions::assert_eq;

#[test]
fn conditional_string_edit_only_replaces_matching_persisted_values() -> anyhow::Result<()> {
    for (initial, expected) in [("never", "auto"), ("auto", "auto"), ("always", "always")] {
        let home = tempfile::tempdir()?;
        let path = home.path().join(CONFIG_TOML_FILE);
        std::fs::write(
            &path,
            format!(
                "# preserved comment\n[tui]\nalternate_screen = \"{initial}\"\nanimations = false\n"
            ),
        )?;
        ConfigEditsBuilder::for_config_path(&path)
            .with_edits([
                ConfigEdit::SetPath {
                    segments: vec!["tui".into(), "fullscreen_transcript".into()],
                    value: value(true),
                },
                ConfigEdit::SetPathIfString {
                    segments: vec!["tui".into(), "alternate_screen".into()],
                    expected: "never".into(),
                    value: value("auto"),
                },
            ])
            .apply_blocking()?;
        let contents = std::fs::read_to_string(&path)?;
        let parsed: toml::Value = toml::from_str(&contents)?;
        assert_eq!(
            serde_json::to_value(parsed)?,
            serde_json::json!({"tui": {
                "alternate_screen": expected,
                "animations": false,
                "fullscreen_transcript": true,
            }}),
        );
        assert!(contents.starts_with("# preserved comment\n"));
    }
    Ok(())
}

#[test]
fn conditional_string_edit_preserves_absent_and_non_string_values() -> anyhow::Result<()> {
    for contents in ["# no preference\n", "[tui]\nalternate_screen = false\n"] {
        let home = tempfile::tempdir()?;
        let path = home.path().join(CONFIG_TOML_FILE);
        std::fs::write(&path, contents)?;
        ConfigEditsBuilder::for_config_path(&path)
            .with_edits([ConfigEdit::SetPathIfString {
                segments: vec!["tui".into(), "alternate_screen".into()],
                expected: "never".into(),
                value: value("auto"),
            }])
            .apply_blocking()?;
        assert_eq!(std::fs::read_to_string(&path)?, contents);
    }
    Ok(())
}

#[test]
fn conditional_string_edit_uses_current_contents_at_transaction_time() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let path = home.path().join(CONFIG_TOML_FILE);
    std::fs::write(&path, "[tui]\nalternate_screen = \"never\"\n")?;
    let builder =
        ConfigEditsBuilder::for_config_path(&path).with_edits([ConfigEdit::SetPathIfString {
            segments: vec!["tui".into(), "alternate_screen".into()],
            expected: "never".into(),
            value: value("auto"),
        }]);
    let later = "[tui]\nalternate_screen = \"always\"\n";
    std::fs::write(&path, later)?;
    builder.apply_blocking()?;
    assert_eq!(std::fs::read_to_string(&path)?, later);
    Ok(())
}

#[test]
fn conditional_string_edit_and_mode_write_do_not_modify_an_invalid_document() -> anyhow::Result<()>
{
    let home = tempfile::tempdir()?;
    let path = home.path().join(CONFIG_TOML_FILE);
    let invalid = "[tui\nalternate_screen = \"never\"\n";
    std::fs::write(&path, invalid)?;
    let result = ConfigEditsBuilder::for_config_path(&path)
        .with_edits([
            ConfigEdit::SetPath {
                segments: vec!["tui".into(), "fullscreen_transcript".into()],
                value: value(true),
            },
            ConfigEdit::SetPathIfString {
                segments: vec!["tui".into(), "alternate_screen".into()],
                expected: "never".into(),
                value: value("auto"),
            },
        ])
        .apply_blocking();
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&path)?, invalid);
    Ok(())
}
