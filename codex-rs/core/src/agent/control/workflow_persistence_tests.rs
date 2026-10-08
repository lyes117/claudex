use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn persistence_failure_cannot_prevent_workflow_child_shutdown() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    child_response(
        &server,
        "persist-sentinel",
        r#"{"answer":"late"}"#,
        Duration::from_secs(/*secs*/ 10),
    )
    .await;
    let (harness, parent, invocation) = bridge_harness(&server).await;
    let task = tokio::spawn(NativeWorkflowBridge::capture(&invocation).run(vec![call(
        "persist",
        WorkflowInput::AgentMessage("persist-sentinel".into()),
    )]));
    timeout(Duration::from_secs(/*secs*/ 10), async {
        while turn_requests(&server).await.is_empty() {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    let child_id = harness
        .manager
        .list_thread_ids()
        .await
        .into_iter()
        .find(|id| *id != parent.session.thread_id)
        .expect("admitted child");
    let child = harness.manager.get_thread(child_id).await?;
    assert_eq!(child.agent_status().await, AgentStatus::Running);
    // This deliberately discards only the fixture-owned writer, leaving the native
    // session Running. Its next persistence barrier now has a real storage error.
    child
        .session
        .live_thread()
        .expect("local persistence")
        .discard()
        .await?;
    assert!(child.session.flush_rollout().await.is_err());
    assert!(harness.control.shutdown_live_agent(child_id).await.is_err());
    assert_eq!(child.agent_status().await, AgentStatus::Running);
    invocation.cancellation_token.cancel();
    assert!(
        timeout(Duration::from_secs(/*secs*/ 10), task)
            .await??
            .is_err()
    );
    timeout(
        Duration::from_secs(/*secs*/ 10),
        child.wait_until_terminated(),
    )
    .await?;
    assert_eq!(
        harness.manager.list_thread_ids().await,
        vec![parent.session.thread_id]
    );
    parent.shutdown_and_wait().await?;
    Ok(())
}
