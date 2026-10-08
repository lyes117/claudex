use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

struct PendingRead(Arc<AtomicBool>);
impl Drop for PendingRead {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn claude_command_server_deadline_drops_pending_resolution() {
    let dropped = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&dropped);
    let work = async move {
        let _pending_read = PendingRead(observed);
        std::future::pending::<()>().await;
        Ok(None)
    };
    let result = resolve(work).await;
    assert_eq!(
        result.unwrap_err().message,
        "Claude command expansion timed out"
    );
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn claude_command_server_deadline_preserves_completed_result_and_error() {
    assert!(matches!(resolve(async { Ok(None) }).await, Ok(None)));
    let expected = super::super::invalid_request("unavailable".to_string());
    let actual = resolve(async { Err(expected.clone()) }).await.unwrap_err();
    assert_eq!(actual, expected);
}
