use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn already_cancelled_invocation_never_admits_a_child() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    invocation.cancellation_token.cancel();
    assert!(
        NativeWorkflowBridge::capture(&invocation)
            .run(vec![call(
                "cancelled",
                WorkflowInput::UserInput("must-never-run".into())
            ),])
            .await
            .is_err()
    );
    assert_eq!(
        harness.manager.list_thread_ids().await,
        vec![parent.session.thread_id]
    );
    assert!(turn_requests(&server).await.is_empty());
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn dropping_the_waiter_cancels_the_supervised_child() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    child_response(
        &server,
        "drop-sentinel",
        r#"{"answer":"late"}"#,
        Duration::from_secs(/*secs*/ 10),
    )
    .await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    let task = tokio::spawn(NativeWorkflowBridge::capture(&invocation).run(vec![call(
        "dropped",
        WorkflowInput::UserInput("drop-sentinel".into()),
    )]));
    timeout(Duration::from_secs(/*secs*/ 10), async {
        while turn_requests(&server).await.is_empty() {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    timeout(Duration::from_secs(/*secs*/ 10), async {
        while harness.manager.list_thread_ids().await.len() != 1 {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    assert_eq!(
        harness.manager.list_thread_ids().await,
        vec![parent.session.thread_id]
    );
    // The bridge cancels its derived token; it cannot cancel the whole parent turn.
    assert!(!invocation.cancellation_token.is_cancelled());
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn individually_valid_results_cannot_exceed_the_group_budget() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let output = serde_json::to_string(&json!({"answer":"x".repeat(5000)}))?;
    child_response(&server, "budget-first", &output, Duration::ZERO).await;
    child_response(&server, "budget-second", &output, Duration::ZERO).await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    let calls = ["budget-first", "budget-second"]
        .into_iter()
        .enumerate()
        .map(|(index, prompt)| {
            call(
                &format!("budget_{index}"),
                WorkflowInput::AgentMessage(prompt.into()),
            )
        })
        .collect();
    let before = parent
        .session
        .clone_history()
        .await
        .raw_items()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        NativeWorkflowBridge::capture(&invocation).run(calls).await,
        Err("workflow group result exceeds 8192 bytes".to_string())
    );
    assert_eq!(
        harness.manager.list_thread_ids().await,
        vec![parent.session.thread_id]
    );
    assert!(
        !parent
            .session
            .input_queue
            .has_pending_input(&parent.session.active_turn)
            .await
    );
    assert_eq!(
        parent
            .session
            .clone_history()
            .await
            .raw_items()
            .cloned()
            .collect::<Vec<_>>(),
        before
    );
    parent.shutdown_and_wait().await?;
    Ok(())
}
