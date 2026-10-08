use super::*;
use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::claude::ClaudePluginMcpConfig;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn native_memory_authority_obeys_features_layers_and_thread_exclusions() {
    let home = tempfile::tempdir().unwrap();
    let root = home
        .path()
        .join("plugins/cache/claudex-memory/claude-mem/local");
    std::fs::create_dir_all(root.join(".codex-plugin")).unwrap();
    let manifest = root.join(".codex-plugin/plugin.json");
    std::fs::write(&manifest, r#"{"name":"claude-mem"}"#).unwrap();
    let id = codex_config::claude::NATIVE_MEMORY_PLUGIN_ID;
    let user_config =
        "[features]\nplugins=true\n[plugins.\"claude-mem@claudex-memory\"]\nenabled=true\n";
    for (case, user, overlay, malformed, expected) in [
        ("active", user_config, None, false, true),
        ("absent", "[features]\nplugins=true\n", None, false, false),
        (
            "project-disabled",
            user_config,
            Some((
                true,
                "[plugins.\"claude-mem@claudex-memory\"]\nenabled=false\n",
            )),
            false,
            false,
        ),
        (
            "cli-disabled",
            user_config,
            Some((
                false,
                "[plugins.\"claude-mem@claudex-memory\"]\nenabled=false\n",
            )),
            false,
            false,
        ),
        (
            "feature-disabled",
            user_config,
            Some((false, "[features]\nplugins=false\n")),
            false,
            false,
        ),
        ("invalid-installation", user_config, None, true, false),
    ] {
        std::fs::write(
            &manifest,
            if malformed {
                "{"
            } else {
                r#"{"name":"claude-mem"}"#
            },
        )
        .unwrap();
        let mut layers = vec![ConfigLayerEntry::new(
            ConfigLayerSource::User {
                file: AbsolutePathBuf::from_absolute_path(home.path().join("config.toml")).unwrap(),
                profile: None,
            },
            toml::from_str(user).unwrap(),
        )];
        if let Some((project, wire)) = overlay {
            let source = if project {
                ConfigLayerSource::Project {
                    dot_codex_folder: AbsolutePathBuf::from_absolute_path(
                        home.path().join("project/.codex"),
                    )
                    .unwrap(),
                }
            } else {
                ConfigLayerSource::SessionFlags
            };
            layers.push(ConfigLayerEntry::new(source, toml::from_str(wire).unwrap()));
        }
        let stack = ConfigLayerStack::new(layers, Default::default(), Default::default()).unwrap();
        let config = Config::load_config_with_layer_stack(
            codex_exec_server::LOCAL_FS.as_ref(),
            stack.effective_config().try_into().unwrap(),
            ConfigOverrides {
                cwd: Some(home.path().to_path_buf()),
                ..Default::default()
            },
            AbsolutePathBuf::from_absolute_path(home.path()).unwrap(),
            stack,
        )
        .await
        .unwrap();
        let manager = crate::plugins::plugins_manager_for_config(
            &config,
            codex_login::test_support::auth_manager_from_optional_auth(None),
        );
        let input = config.plugins_config_input();
        let outcome = manager.plugins_for_config(&input).await;
        assert_eq!(
            outcome
                .plugins()
                .iter()
                .any(|p| p.config_name == id && p.is_active()),
            expected,
            "{case}"
        );
        assert!(
            !outcome
                .clone()
                .without_plugins(&[id.to_owned()])
                .plugins()
                .iter()
                .any(|p| p.config_name == id && p.is_active()),
            "{case}: thread exclusion"
        );
        if expected {
            // The same manager must recompute authority after a config override, not retain it.
            let mut disabled = config.clone();
            disabled
                .features
                .disable(Feature::Plugins)
                .expect("disable plugins in fixture");
            assert!(
                !manager
                    .plugins_for_config(&disabled.plugins_config_input())
                    .await
                    .plugins()
                    .iter()
                    .any(|p| p.config_name == id && p.is_active())
            );
        }
    }
}

fn server(command: &str) -> McpServerConfig {
    toml::from_str(&format!("command='{command}'")).unwrap()
}

#[tokio::test]
async fn memory_projection_preserves_explicit_and_runtime_mcp_definitions() {
    let home = tempfile::tempdir().unwrap();
    let layer = ConfigLayerEntry::new(
        ConfigLayerSource::User {
            file: AbsolutePathBuf::from_absolute_path(home.path().join("config.toml")).unwrap(),
            profile: None,
        },
        toml::from_str("[mcp_servers.collision]\ncommand='explicit'\n[mcp_servers.removed]\ncommand='original'\n").unwrap(),
    ).with_claude_plugin_mcp_configs(vec![ClaudePluginMcpConfig {
        plugin_id: "claude-mem@legacy".into(),
        config: toml::from_str("[mcp_servers.memory]\ncommand='legacy'\n[mcp_servers.collision]\ncommand='legacy'\n").unwrap(),
    }]);
    let stack = ConfigLayerStack::new(vec![layer], Default::default(), Default::default()).unwrap();
    let mut config = Config::load_config_with_layer_stack(
        codex_exec_server::LOCAL_FS.as_ref(),
        stack.effective_config().try_into().unwrap(),
        ConfigOverrides {
            cwd: Some(home.path().to_path_buf()),
            ..Default::default()
        },
        AbsolutePathBuf::from_absolute_path(home.path()).unwrap(),
        stack,
    )
    .await
    .unwrap();
    let selected = config.mcp_servers_with_legacy_selection(LegacyPluginSelection::NativeMemory);
    assert_eq!(
        selected,
        HashMap::from([
            ("collision".into(), server("explicit")),
            ("removed".into(), server("original")),
        ])
    );
    let mut updated = config.mcp_servers.get().clone();
    updated.remove("removed");
    updated.insert("memory".into(), server("runtime-replacement"));
    updated.insert("new".into(), server("runtime-added"));
    config.mcp_servers.set(updated.clone()).unwrap();
    assert_eq!(
        config.mcp_servers_with_legacy_selection(LegacyPluginSelection::NativeMemory),
        updated
    );
    assert_eq!(
        config.mcp_servers_with_legacy_selection(LegacyPluginSelection::KeepAll),
        updated
    );
}
