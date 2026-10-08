use super::*;
use crate::ConfigLayerEntry;
use crate::ConfigLayerSource;
use crate::ConfigLayerStack;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

fn test_stack(home: &Path, user_enabled: bool) -> ConfigLayerStack {
    let mut user = ConfigLayerEntry::new(
        ConfigLayerSource::User {
            file: AbsolutePathBuf::from_absolute_path(home.join(".codex/config.toml")).unwrap(),
            profile: None,
        },
        TomlValue::Table(toml::map::Map::new()),
    );
    user.claude_config_enabled = user_enabled;
    ConfigLayerStack::new(vec![user], Default::default(), Default::default()).unwrap()
}

#[test]
fn replacement_requires_user_marker_and_current_active_canonical_plugin() {
    let home = tempfile::tempdir().unwrap();
    let stack = test_stack(home.path(), true);
    let active = [NATIVE_MEMORY_PLUGIN_ID];
    assert_eq!(
        LegacyPluginSelection::for_user_home(&stack, active.into_iter(), Some(home.path()))
            .unwrap(),
        LegacyPluginSelection::KeepAll
    );
    let directory = home.path().join(".claudex/memory");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("native-active.json"),
        r#"{"version":1,"active":true,"pluginId":"claude-mem@claudex-memory"}"#,
    )
    .unwrap();
    for ids in [vec![], vec!["claude-mem@other"]] {
        assert_eq!(
            LegacyPluginSelection::for_user_home(&stack, ids.into_iter(), Some(home.path()))
                .unwrap(),
            LegacyPluginSelection::KeepAll
        );
    }
    let selected =
        LegacyPluginSelection::for_user_home(&stack, active.into_iter(), Some(home.path()))
            .unwrap();
    assert_eq!(selected, LegacyPluginSelection::NativeMemory);
    assert!(!selected.retains("claude-mem@legacy"));
    assert!(selected.retains("claude-memory@legacy"));
    assert_eq!(
        LegacyPluginSelection::for_user_home(&stack, std::iter::empty(), Some(home.path()))
            .unwrap(),
        LegacyPluginSelection::KeepAll
    );
    let ignored = test_stack(home.path(), false);
    assert_eq!(
        LegacyPluginSelection::for_user_home(&ignored, active.into_iter(), Some(home.path()))
            .unwrap(),
        LegacyPluginSelection::KeepAll
    );
    fs::write(
        directory.join("native-active.json"),
        r#"{"version":1,"active":true,"pluginId":"wrong"}"#,
    )
    .unwrap();
    assert_eq!(
        LegacyPluginSelection::for_user_home(&stack, active.into_iter(), Some(home.path()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn selected_mcp_preserves_explicit_collisions_and_layer_precedence() {
    let home = tempfile::tempdir().unwrap();
    let mut stack = test_stack(home.path(), true);
    let mut layer = stack.layers_low_to_high().next().unwrap().clone();
    layer.claude_plugin_mcp_configs = vec![ClaudePluginMcpConfig {
        plugin_id: "claude-mem@legacy".into(),
        config: toml::from_str(
            "[mcp_servers.memory]\ncommand='legacy'\n[mcp_servers.collision]\ncommand='legacy'\n",
        )
        .unwrap(),
    }];
    layer.config = toml::from_str("[mcp_servers.collision]\ncommand='explicit'\n").unwrap();
    stack = ConfigLayerStack::new(vec![layer], Default::default(), Default::default()).unwrap();
    let all = selected_mcp_config(&stack, LegacyPluginSelection::KeepAll);
    assert_eq!(
        all.get("memory").unwrap().get("command").unwrap().as_str(),
        Some("legacy")
    );
    assert_eq!(
        all.get("collision")
            .unwrap()
            .get("command")
            .unwrap()
            .as_str(),
        Some("explicit")
    );
    let selected = selected_mcp_config(&stack, LegacyPluginSelection::NativeMemory);
    assert_eq!(
        selected,
        toml::from_str::<TomlValue>("[collision]\ncommand='explicit'\n").unwrap()
    );
    assert_eq!(stack.effective_config().get("mcp_servers"), Some(&all));
    let mut layers = stack.layers_low_to_high().cloned().collect::<Vec<_>>();
    layers.push(ConfigLayerEntry::new(
        ConfigLayerSource::SessionFlags,
        toml::from_str("[mcp_servers.collision]\ncommand='cli'\n").unwrap(),
    ));
    let overridden = ConfigLayerStack::new(layers, Default::default(), Default::default()).unwrap();
    assert_eq!(
        selected_mcp_config(&overridden, LegacyPluginSelection::NativeMemory),
        toml::from_str::<TomlValue>("[collision]\ncommand='cli'\n").unwrap()
    );
}
