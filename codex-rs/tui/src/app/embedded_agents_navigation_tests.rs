use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn embedded_agents_dashboard_discovers_its_native_session() -> Result<()> {
    let (mut app, mut events, _operations) =
        Box::pin(crate::app::tests::make_test_app_with_channels()).await;
    assert!(matches!(app.app_server_target, AppServerTarget::Embedded));
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let started = Box::pin(server.start_thread(&app.config)).await?;
    let id = started.session.thread_id;
    app.open_agents_overview(&server);
    assert!(
        app.chat_widget
            .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
            .is_some()
    );
    finish_overview_refresh(&mut app, &server, &mut events).await;
    assert!(
        matches!(app.agents_overview.threads.get(&id), Some(Some(thread)) if thread.id == id.to_string())
    );
    assert!(app.agents_overview.visible_thread_ids.contains(&id));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn embedded_agents_navigation_preserves_two_blank_sessions_and_drafts() -> Result<()> {
    let mut app = Box::pin(make_test_app()).await;
    assert!(matches!(app.app_server_target, AppServerTarget::Embedded));
    trust_fixture_folders(&mut app);
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let first = Box::pin(server.start_thread(&app.config)).await?;
    let first_id = first.session.thread_id;
    app.pending_startup_thread_start = true;
    Box::pin(app.handle_startup_thread_started(&mut server, Ok(first))).await?;
    assert!(app.agents_overview.blank_sessions.contains_key(&first_id));
    app.chat_widget.insert_str("First unsubmitted draft");
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.pause_events();
    Box::pin(app.start_fresh_session(
        &mut tui,
        &mut server,
        /*session_start_source*/ None,
        /*initial_user_message*/ None,
        /*new_thread_name*/ None,
    ))
    .await;
    let second_id = app.chat_widget.thread_id().expect("second blank session");
    assert_ne!(first_id, second_id);
    assert!(app.agents_overview.blank_sessions.contains_key(&second_id));
    app.chat_widget.insert_str("Second unsubmitted draft");
    Box::pin(app.select_agents_overview_thread(&mut tui, &mut server, first_id)).await?;
    assert_eq!(app.chat_widget.thread_id(), Some(first_id));
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "First unsubmitted draft"
    );
    Box::pin(app.select_agents_overview_thread(&mut tui, &mut server, second_id)).await?;
    assert_eq!(app.chat_widget.thread_id(), Some(second_id));
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "Second unsubmitted draft"
    );
    let loaded = server
        .thread_loaded_list(codex_app_server_protocol::ThreadLoadedListParams::default())
        .await?;
    assert_eq!(
        loaded.data.into_iter().collect::<HashSet<_>>(),
        HashSet::from([first_id.to_string(), second_id.to_string()])
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn embedded_agents_archive_and_delete_keep_the_other_runtime_loaded() -> Result<()> {
    for delete in [false, true] {
        let mut app = Box::pin(make_test_app()).await;
        assert!(matches!(app.app_server_target, AppServerTarget::Embedded));
        trust_fixture_folders(&mut app);
        let id = ThreadId::from_string(
            &app_test_support::create_fake_rollout(
                &app.config.codex_home,
                "2025-01-05T12-00-00",
                "2025-01-05T12:00:00Z",
                "Native lifecycle fixture",
                Some(&app.config.model_provider_id),
                /*git_info*/ None,
            )
            .expect("saved lifecycle fixture"),
        )?;
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let resumed = Box::pin(server.resume_thread(
            &app.local_settings,
            app.config.clone(),
            id,
            crate::app_server_session::ResumeModelSettings::PreserveExistingThread,
        ))
        .await?;
        app.enqueue_primary_thread_session(resumed.session, resumed.turns)
            .await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.pause_events();
        Box::pin(app.start_fresh_session(
            &mut tui,
            &mut server,
            /*session_start_source*/ None,
            /*initial_user_message*/ None,
            /*new_thread_name*/ None,
        ))
        .await;
        let other = app
            .chat_widget
            .thread_id()
            .expect("other TUI-owned session");
        assert_ne!(id, other);
        app.chat_widget.insert_str("Retained lifecycle draft");
        Box::pin(app.select_agents_overview_thread(&mut tui, &mut server, id)).await?;
        assert_eq!(app.chat_widget.thread_id(), Some(id));
        let result = if delete {
            Box::pin(app.delete_current_thread(&mut tui, &mut server)).await?
        } else {
            Box::pin(app.archive_current_thread(&mut tui, &mut server)).await?
        };
        assert!(matches!(result, AppRunControl::Continue));
        assert!(
            app.chat_widget
                .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
                .is_some()
        );
        let loaded = server
            .thread_loaded_list(codex_app_server_protocol::ThreadLoadedListParams::default())
            .await?;
        assert_eq!(loaded.data, vec![other.to_string()]);
        Box::pin(app.select_agents_overview_thread(&mut tui, &mut server, other)).await?;
        assert_eq!(app.chat_widget.thread_id(), Some(other));
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "Retained lifecycle draft"
        );
        server.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn embedded_agents_external_writer_stays_read_only_and_returns_to_dashboard() -> Result<()> {
    let mut app = Box::pin(make_test_app()).await;
    assert!(matches!(app.app_server_target, AppServerTarget::Embedded));
    trust_fixture_folders(&mut app);
    std::fs::write(
        app.config.codex_home.join("config.toml"),
        "[tui]\nresume_cwd = \"current\"\n",
    )?;
    let id = ThreadId::from_string(
        &app_test_support::create_fake_rollout(
            &app.config.codex_home,
            "2025-01-05T12-00-00",
            "2025-01-05T12:00:00Z",
            "External writer fixture",
            Some(&app.config.model_provider_id),
            /*git_info*/ None,
        )
        .expect("saved writer fixture"),
    )?;
    let mut owner = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    Box::pin(owner.resume_thread(
        &app.local_settings,
        app.config.clone(),
        id,
        crate::app_server_session::ResumeModelSettings::PreserveExistingThread,
    ))
    .await?;
    let mut viewer = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.pause_events();
    Box::pin(app.select_agents_overview_thread(&mut tui, &mut viewer, id)).await?;
    assert_eq!(app.chat_widget.thread_id(), Some(id));
    assert!(app.chat_widget.is_external_writer_view());
    for key in [KeyCode::Esc, KeyCode::Left] {
        Box::pin(app.handle_tui_event(&mut tui, &mut viewer, TuiEvent::Key(key.into()))).await?;
        assert!(
            app.chat_widget
                .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
                .is_some()
        );
        Box::pin(app.select_agents_overview_thread(&mut tui, &mut viewer, id)).await?;
        assert!(app.chat_widget.is_external_writer_view());
    }
    let loaded = viewer
        .thread_loaded_list(codex_app_server_protocol::ThreadLoadedListParams::default())
        .await?;
    assert_eq!(loaded.data, Vec::<String>::new());
    let owned = owner
        .thread_loaded_list(codex_app_server_protocol::ThreadLoadedListParams::default())
        .await?;
    assert_eq!(owned.data, vec![id.to_string()]);
    viewer.shutdown().await?;
    owner.shutdown().await?;
    Ok(())
}
