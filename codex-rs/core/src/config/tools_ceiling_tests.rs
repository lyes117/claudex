use crate::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn no_tools_ceiling_survives_managed_and_session_overrides() -> anyhow::Result<()> {
    for (base, managed, session, expected) in [
        (None, None, None, true),
        (Some(false), Some(true), Some(true), true),
        (Some(true), Some(true), Some(false), false),
        (Some(true), Some(false), Some(true), false),
        (Some(true), Some(true), Some(true), true),
    ] {
        let home = tempdir()?;
        if let Some(base) = base {
            std::fs::write(
                home.path().join("config.toml"),
                format!("[tools]\nenabled = {base}\n"),
            )?;
        }
        let managed_path = home.path().join("managed_config.toml");
        if let Some(managed) = managed {
            std::fs::write(&managed_path, format!("[tools]\nenabled = {managed}\n"))?;
        }
        let config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .loader_overrides(LoaderOverrides::with_managed_config_path_for_tests(
                managed_path,
            ))
            .cli_overrides(
                session
                    .into_iter()
                    .map(|enabled| ("tools.enabled".into(), enabled.into()))
                    .collect(),
            )
            .build()
            .await?;
        assert_eq!(config.tools_enabled, expected);
    }
    Ok(())
}

#[tokio::test]
async fn active_profile_keeps_normal_priority_but_runtime_false_remains_a_ceiling()
-> anyhow::Result<()> {
    let home = tempdir()?;
    std::fs::write(
        home.path().join("config.toml"),
        "[tools]\nenabled = false\n",
    )?;
    std::fs::write(
        home.path().join("work.config.toml"),
        "[tools]\nenabled = true\n",
    )?;
    for (session, expected) in [(None, true), (Some(false), false)] {
        let config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .loader_overrides(LoaderOverrides {
                user_config_profile: Some("work".parse()?),
                user_config_path: Some(home.path().join("work.config.toml").try_into()?),
                ..LoaderOverrides::without_managed_config_for_tests()
            })
            .cli_overrides(
                session
                    .into_iter()
                    .map(|enabled| ("tools.enabled".into(), enabled.into()))
                    .collect(),
            )
            .build()
            .await?;
        assert_eq!(config.tools_enabled, expected);
    }
    Ok(())
}
