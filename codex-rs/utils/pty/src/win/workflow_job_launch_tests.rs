//! Actual contained process, deterministic handoff/timeout race; no simulated OS child.
use super::*;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn timeout_before_handoff_has_observable_exact_cleanup_receipt() -> io::Result<()> {
    let mut process = super::super::tests::fixture().await?;
    let observer = process.process.try_clone()?;
    process.take_stdin()?.write_all(b"spin\n").await?;
    let slot: ReplySlot = Arc::new(Mutex::new(None));
    let (ready, ready_rx) = oneshot::channel();
    let (decision, decision_rx) = oneshot::channel();
    let owner = tokio::spawn(publish(Ok(process), Arc::clone(&slot), ready, decision_rx));
    let result = receive(Instant::now(), slot, ready_rx, decision, owner).await?;
    let WorkflowHostLaunchCompletion::Pending { reason, receipt } = result else {
        panic!("expired handoff must preserve an observable receipt");
    };
    assert_eq!(reason.kind(), io::ErrorKind::TimedOut);
    tokio::time::timeout(Duration::from_secs(3), receipt)
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)??;
    assert!(super::super::poll_exit(&observer)?.is_some());
    Ok(())
}

#[tokio::test]
async fn dropping_setup_waiter_keeps_exact_cleanup_owner() -> io::Result<()> {
    let mut process = super::super::tests::fixture().await?;
    let observer = process.process.try_clone()?;
    process.take_stdin()?.write_all(b"spin\n").await?;
    let slot: ReplySlot = Arc::new(Mutex::new(None));
    let (ready, ready_rx) = oneshot::channel();
    let (decision, decision_rx) = oneshot::channel();
    let owner = tokio::spawn(publish(Ok(process), Arc::clone(&slot), ready, decision_rx));
    // Even an unpolled receive future owns the cancellation sender and task receipt.
    drop(receive(
        Instant::now() + Duration::from_secs(5),
        slot,
        ready_rx,
        decision,
        owner,
    ));
    super::super::wait_process(&observer, Duration::from_secs(3)).await?;
    assert!(super::super::poll_exit(&observer)?.is_some());
    Ok(())
}
