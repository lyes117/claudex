//! Observable setup ownership. A timeout never drops a live process from a channel.
use super::WorkflowHostMode;
use super::WorkflowHostProcess;
use super::driver;
use super::spawn;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

type Reply = io::Result<WorkflowHostProcess>;
type ReplySlot = Arc<Mutex<Option<Reply>>>;

/// Setup outcome; Pending requires caller quarantine until its exact cleanup receipt.
#[must_use = "retain run ownership until Started handoff or a successful cleanup receipt"]
pub enum WorkflowHostLaunchCompletion {
    /// Caller now owns the contained host; later shutdown still requires supervision.
    Started(WorkflowHostProcess),
    /// Setup was refused before creation or every created process was observed exited.
    Rejected(io::Error),
    /// Deadline won before handoff; the task retains all ownership until no child remains.
    Pending {
        reason: io::Error,
        receipt: JoinHandle<io::Result<()>>,
    },
}

pub(super) async fn spawn_until(
    executable: &Path,
    cwd: &Path,
    mode: WorkflowHostMode,
    deadline: Instant,
) -> io::Result<WorkflowHostLaunchCompletion> {
    if Instant::now() >= deadline {
        return Ok(WorkflowHostLaunchCompletion::Rejected(io::Error::new(
            io::ErrorKind::TimedOut,
            "workflow host setup deadline",
        )));
    }
    let executable = executable.to_owned();
    let cwd = cwd.to_owned();
    let slot: ReplySlot = Arc::new(Mutex::new(None));
    let owned_slot = Arc::clone(&slot);
    let (ready, ready_rx) = oneshot::channel();
    let (decision, mut decision_rx) = oneshot::channel();
    let argument = match mode {
        WorkflowHostMode::Execute => "--workflow-host",
        WorkflowHostMode::Preflight => "--workflow-preflight",
    };
    let arguments = [OsString::from(argument)];
    // The owner and observable receipt are created before the caller's first await.
    let owner = tokio::spawn(async move {
        let unfinished = tokio::select! {
            biased;
            _ = &mut decision_rx => return Ok(()),
            result = spawn::launch_unfinished(&executable, &cwd, &arguments) => result,
        };
        let reply = match unfinished {
            Err(error) => Err(error), // Native creation never happened on this error path.
            Ok(spawn::LaunchOutcome::Started(process)) => Ok(process),
            Ok(spawn::LaunchOutcome::Failed { error, process }) => {
                // Caller timeout cannot detach an unobservable inner cleanup: the outer
                // receipt completes only after this exact-process reaper confirms exit.
                driver::retain_until_exit(process)
                    .await
                    .map_err(io::Error::other)??;
                Err(error)
            }
        };
        publish(reply, owned_slot, ready, decision_rx).await
    });
    receive(deadline, slot, ready_rx, decision, owner).await
}

async fn publish(
    reply: Reply,
    slot: ReplySlot,
    ready: oneshot::Sender<()>,
    decision: oneshot::Receiver<()>,
) -> io::Result<()> {
    {
        let mut value = slot
            .lock()
            .map_err(|_| io::Error::other("workflow setup slot poisoned"))?;
        if value.is_some() {
            return Err(io::Error::other("workflow setup duplicate reply"));
        }
        *value = Some(reply);
    }
    if ready.send(()).is_ok() {
        // Accepted means the caller has synchronously taken the slot; sender drop
        // means timeout/cancellation. In both cases drain verifies remaining ownership.
        match decision.await {
            Ok(()) | Err(_) => {}
        }
    }
    match take_reply(&slot)? {
        Some(Ok(process)) => {
            driver::retain_until_exit(process)
                .await
                .map_err(io::Error::other)??;
            Ok(())
        }
        Some(Err(_)) | None => Ok(()),
    }
}

async fn receive(
    deadline: Instant,
    slot: ReplySlot,
    mut ready: oneshot::Receiver<()>,
    decision: oneshot::Sender<()>,
    owner: JoinHandle<io::Result<()>>,
) -> io::Result<WorkflowHostLaunchCompletion> {
    tokio::select! {
        biased;
        _ = tokio::time::sleep_until(deadline) => {
            drop(ready);
            drop(decision);
            return Ok(WorkflowHostLaunchCompletion::Pending {
                reason: io::Error::new(io::ErrorKind::TimedOut, "workflow host setup deadline"),
                receipt: owner,
            });
        }
        notified = &mut ready => notified.map_err(|_| io::Error::other("workflow setup owner failed"))?,
    }
    // A rounded or delayed timer may still be pending after the exact deadline.
    // Keep the reply in its owner slot until an on-time handoff is confirmed.
    if Instant::now() >= deadline {
        drop(ready);
        drop(decision);
        return Ok(WorkflowHostLaunchCompletion::Pending {
            reason: io::Error::new(io::ErrorKind::TimedOut, "workflow host setup deadline"),
            receipt: owner,
        });
    }
    let reply =
        take_reply(&slot)?.ok_or_else(|| io::Error::other("workflow setup reply missing"))?;
    decision
        .send(())
        .map_err(|_| io::Error::other("workflow setup handoff failed"))?;
    match reply {
        Ok(process) => Ok(WorkflowHostLaunchCompletion::Started(process)),
        Err(error) => Ok(WorkflowHostLaunchCompletion::Rejected(error)),
    }
}

fn take_reply(slot: &ReplySlot) -> io::Result<Option<Reply>> {
    Ok(slot
        .lock()
        .map_err(|_| io::Error::other("workflow setup slot poisoned"))?
        .take())
}

#[cfg(test)]
#[path = "workflow_job_launch_tests.rs"]
mod tests;
