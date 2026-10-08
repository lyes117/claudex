//! Parent-side ownership and exact-handle cleanup; no workflow authority is admitted here.
use super::WorkflowHostProcess;
use std::future::Future;
use std::io;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

const CLEANUP_BUDGET: Duration = Duration::from_secs(2);

/// Result of supervision. Only Confirmed permits reporting that the host has stopped.
#[must_use = "verify the exit receipt before releasing run ownership or reporting success"]
pub enum WorkflowHostCompletion<T> {
    /// Exact process exit was observed before its process/job handles were released.
    Confirmed {
        outcome: io::Result<T>,
        exit_code: u32,
    },
    /// Cleanup exceeded its budget or failed; an independent task retains exact handles.
    /// The caller must quarantine the run/admission until this receipt confirms exit.
    Pending {
        outcome: io::Result<T>,
        cleanup_error: io::Error,
        receipt: JoinHandle<io::Result<u32>>,
    },
}

pub(super) async fn supervise<T, Operation, Cancellation>(
    process: WorkflowHostProcess,
    deadline: Instant,
    operation: Operation,
    cancellation: Cancellation,
) -> io::Result<WorkflowHostCompletion<T>>
where
    T: Send + 'static,
    Operation: Future<Output = io::Result<T>> + Send + 'static,
    Cancellation: Future<Output = ()> + Send + 'static,
{
    let (waiter_alive, waiter_dropped) = oneshot::channel::<()>();
    let owner = tokio::spawn(async move {
        // Lexical scope drops parent pipe I/O before requesting process termination.
        let outcome = {
            tokio::pin!(operation, cancellation);
            tokio::select! {
                biased;
                _ = waiter_dropped => Err(io::Error::new(io::ErrorKind::Interrupted, "workflow waiter dropped")),
                _ = cancellation => Err(io::Error::new(io::ErrorKind::Interrupted, "workflow cancelled")),
                _ = tokio::time::sleep_until(deadline) => Err(io::Error::new(io::ErrorKind::TimedOut, "workflow deadline")),
                outcome = &mut operation => outcome,
            }
        };
        let exited = if outcome.is_ok() {
            match process.wait(CLEANUP_BUDGET).await {
                Ok(code) => Ok(code),
                // Natural-exit wait failure does not authorize release; force termination
                // and obtain a fresh exact-handle observation instead.
                Err(_) => process.terminate_and_wait(CLEANUP_BUDGET).await,
            }
        } else {
            process.terminate_and_wait(CLEANUP_BUDGET).await
        };
        match exited {
            Ok(exit_code) => WorkflowHostCompletion::Confirmed { outcome, exit_code },
            Err(cleanup_error) => WorkflowHostCompletion::Pending {
                outcome,
                cleanup_error,
                receipt: retain_until_exit(process),
            },
        }
    });
    let result = owner
        .await
        .map_err(|_| io::Error::other("workflow owner task failed"));
    // Keeping this sender alive until after await prevents accidental self-cancellation.
    drop(waiter_alive);
    result
}

/// Ownership outlives cancelled waiters and cleanup budgets. It is released only on
/// a successful exact-handle wait. Runtime/process shutdown remains an explicit limit.
pub(super) fn retain_until_exit(process: WorkflowHostProcess) -> JoinHandle<io::Result<u32>> {
    tokio::spawn(async move {
        loop {
            match process.terminate_and_wait(CLEANUP_BUDGET).await {
                Ok(code) => return Ok(code),
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    })
}
