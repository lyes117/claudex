use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClaudeCommandExpandParams;
use codex_app_server_protocol::ClaudeCommandExpandResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SkillsConfigWriteParams;
use codex_app_server_protocol::SkillsConfigWriteResponse;
use codex_app_server_protocol::SkillsListParams;
use codex_app_server_protocol::SkillsListResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_core::config::set_project_trust_level;
use codex_protocol::config_types::TrustLevel;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[tokio::test]
async fn command_expansion_revalidates_catalog_source_cwd_path_and_live_enabled_state() -> Result<()>
{
    check_expansion_revalidation(SkillSource::ClaudeCommand).await
}

#[tokio::test]
async fn project_native_skill_expansion_revalidates_permissions_source_and_live_enabled_state()
-> Result<()> {
    check_expansion_revalidation(SkillSource::NativeProject).await
}

#[tokio::test]
async fn global_native_skill_expansion_revalidates_permissions_source_and_live_enabled_state()
-> Result<()> {
    check_expansion_revalidation(SkillSource::NativeUser).await
}

#[derive(Clone, Copy)]
enum SkillSource {
    ClaudeCommand,
    NativeProject,
    NativeUser,
}

async fn check_expansion_revalidation(source: SkillSource) -> Result<()> {
    let home = tempfile::tempdir()?;
    let cwd = tempfile::tempdir()?;
    std::fs::create_dir(cwd.path().join(".git"))?;
    let path = match source {
        SkillSource::ClaudeCommand => cwd.path().join(".claude/commands/deploy.md"),
        SkillSource::NativeProject => cwd.path().join(".agents/skills/deploy/SKILL.md"),
        SkillSource::NativeUser => home.path().join("skills/deploy/SKILL.md"),
    };
    std::fs::create_dir_all(path.parent().expect("skill directory"))?;
    let write = |metadata: &str, body: &str| {
        std::fs::write(
            &path,
            format!("---\nname: deploy\ndescription: fixture\n{metadata}---\n{body}"),
        )
    };
    write(
        "argument-hint: '[revision]'\n",
        "Ship $ARGUMENTS[0] with $ARGUMENTS",
    )?;
    let permissions = cwd.path().join(".claude/settings.json");
    std::fs::create_dir_all(permissions.parent().expect("settings directory"))?;
    std::fs::write(&permissions, "{}")?;
    set_project_trust_level(home.path(), cwd.path(), TrustLevel::Trusted)?;
    let fixture_home = home.path().to_string_lossy();
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_managed_config()
        .with_env_overrides(&[
            ("USERPROFILE", Some(&fixture_home)),
            ("HOME", Some(&fixture_home)),
        ])
        .build_initialized_with_timeout(Duration::from_secs(30))
        .await?;
    let thread = app
        .start_thread(ThreadStartParams {
            cwd: Some(cwd.path().to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await?
        .thread;
    let list = app
        .send_skills_list_request(SkillsListParams {
            cwds: vec![cwd.path().to_path_buf()],
            force_reload: true,
        })
        .await?;
    let catalog: SkillsListResponse = app.read_response(list).await?;
    let skill = catalog
        .data
        .iter()
        .flat_map(|entry| &entry.skills)
        .find(|skill| skill.name == "deploy" && skill.path.as_path() == path.as_path())
        .expect("catalog skill");
    match source {
        SkillSource::ClaudeCommand => assert!(
            skill
                .claude_command
                .as_ref()
                .is_some_and(|command| command.user_invocable)
        ),
        SkillSource::NativeProject | SkillSource::NativeUser => {
            assert_eq!(skill.claude_command, None);
        }
    }
    let mut params = ClaudeCommandExpandParams {
        thread_id: thread.id,
        cwd: cwd.path().to_path_buf(),
        name: skill.name.clone(),
        path: skill.path.clone(),
        arguments: "\"\u{00e9}quipe \u{7814}\u{7a76}\" main".into(),
    };
    let id = app
        .send_request(
            "skills/claudeCommand/expand",
            Some(serde_json::to_value(&params)?),
        )
        .await?;
    let response: ClaudeCommandExpandResponse = app.read_response(id).await?;
    assert_eq!(
        response,
        ClaudeCommandExpandResponse {
            text:
                "Ship \u{00e9}quipe \u{7814}\u{7a76} with \"\u{00e9}quipe \u{7814}\u{7a76}\" main"
                    .into()
        }
    );
    for invalid in 0..6 {
        let mut rejected = params.clone();
        match invalid {
            0 => rejected.cwd = home.path().to_path_buf(),
            1 => {
                rejected.path =
                    AbsolutePathBuf::from_absolute_path(home.path().join("not-a-command.md"))?
            }
            2 => {
                write("user-invocable: false\n", "hidden content")?;
            }
            3 => {
                write("", &"x".repeat(32 * 1024 + 1))?;
            }
            4 => {
                write("", &"x".repeat(8 * 1024 + 1))?;
            }
            5 => {
                write("context: fork\n", "unsupported content")?;
            }
            _ => unreachable!(),
        }
        assert_rejected(&mut app, &rejected).await?;
    }
    write("", "Available again")?;
    for policy in ["deny", "ask"] {
        std::fs::write(
            &permissions,
            serde_json::to_vec(&serde_json::json!({
                "permissions": {(policy): ["Skill"]}
            }))?,
        )?;
        assert_rejected(&mut app, &params).await?;
    }
    std::fs::write(&permissions, "{}")?;
    let id = app
        .send_request(
            "skills/config/write",
            Some(serde_json::to_value(SkillsConfigWriteParams {
                path: Some(params.path.clone()),
                name: None,
                enabled: false,
            })?),
        )
        .await?;
    let disabled: SkillsConfigWriteResponse = app.read_response(id).await?;
    assert!(!disabled.effective_enabled);
    assert_rejected(&mut app, &params).await?;
    params.thread_id = codex_protocol::ThreadId::new().to_string();
    assert_rejected(&mut app, &params).await?;
    app.shutdown_gracefully().await?;
    Ok(())
}

async fn assert_rejected(
    app: &mut TestAppServer,
    params: &ClaudeCommandExpandParams,
) -> Result<()> {
    let id = app
        .send_request(
            "skills/claudeCommand/expand",
            Some(serde_json::to_value(params)?),
        )
        .await?;
    let error = app
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(
        error.error.message,
        "Claude command is unavailable for this thread"
    );
    Ok(())
}
