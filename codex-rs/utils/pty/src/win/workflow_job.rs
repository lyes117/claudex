//! Private Windows process containment for the credential-free Workflow JavaScript host.
//! Resource containment is not an OS filesystem/network security sandbox.

use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::time::Duration;
use tokio::net::windows::named_pipe::NamedPipeServer;
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::jobapi2::TerminateJobObject;
use winapi::um::processthreadsapi::GetExitCodeProcess;
use winapi::um::processthreadsapi::TerminateProcess;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::WAIT_FAILED;
use winapi::um::winbase::WAIT_OBJECT_0;

#[path = "workflow_job_driver.rs"]
mod driver;
pub use driver::WorkflowHostCompletion;

#[path = "workflow_job_launch.rs"]
mod launch;
pub use launch::WorkflowHostLaunchCompletion;

#[path = "workflow_job_spawn.rs"]
mod spawn;

/// Combined process/job committed-memory limit, including V8 external allocations.
pub const WORKFLOW_HOST_MEMORY_BYTES: usize = 256 * 1024 * 1024;

/// Fixed private host entrypoints; neither script arguments nor environment overrides.
#[derive(Debug, Clone, Copy)]
pub enum WorkflowHostMode {
    /// Execute the private bounded V8 host.
    Execute,
    /// Parse the script without executing V8 or metadata.
    Preflight,
}

/// Owns the exact host process, strict Job Object and overlapped parent pipe ends.
///
/// The constructor launches only a native executable's fixed private entrypoint.
/// entrypoint. It never runs a shell, loads user configuration or inherits the parent's
/// environment. Creation, containment and thread resumption fail closed.
pub struct WorkflowHostProcess {
    process: OwnedHandle,
    job: OwnedHandle,
    stdin: Option<NamedPipeServer>,
    stdout: Option<NamedPipeServer>,
}

impl WorkflowHostProcess {
    /// Launches a trusted native executable in a caller-authorized working directory.
    /// Both paths must be absolute. The workflow script never chooses these values.
    /// The caller must enforce its overall deadline around this operation and all I/O.
    pub async fn spawn(executable: &Path, cwd: &Path) -> io::Result<Self> {
        spawn::launch(executable, cwd, &[OsString::from("--workflow-host")]).await
    }

    /// Bounds async setup and exposes retained ownership when setup has not settled.
    /// Pending's receipt must confirm no owned process remains before admission release.
    pub async fn spawn_until(
        executable: &Path,
        cwd: &Path,
        mode: WorkflowHostMode,
        deadline: tokio::time::Instant,
    ) -> io::Result<WorkflowHostLaunchCompletion> {
        launch::spawn_until(executable, cwd, mode, deadline).await
    }

    /// Runs a bounded parent operation under an independently owned shutdown task.
    /// Only Confirmed proves host exit; Pending retains ownership until exact-handle wait.
    /// Dropping this waiter signals cancellation without aborting its shutdown task.
    pub async fn supervise<T, Operation, Cancellation>(
        self,
        deadline: tokio::time::Instant,
        operation: Operation,
        cancellation: Cancellation,
    ) -> io::Result<WorkflowHostCompletion<T>>
    where
        T: Send + 'static,
        Operation: Future<Output = io::Result<T>> + Send + 'static,
        Cancellation: Future<Output = ()> + Send + 'static,
    {
        driver::supervise(self, deadline, operation, cancellation).await
    }

    /// Transfers the overlapped writing endpoint. Only one owner may claim it.
    pub fn take_stdin(&mut self) -> io::Result<NamedPipeServer> {
        self.stdin
            .take()
            .ok_or_else(|| io::Error::other("workflow stdin already taken"))
    }

    /// Transfers the overlapped reading endpoint. Only one owner may claim it.
    pub fn take_stdout(&mut self) -> io::Result<NamedPipeServer> {
        self.stdout
            .take()
            .ok_or_else(|| io::Error::other("workflow stdout already taken"))
    }

    /// Waits for the exact process handle within the provided cleanup budget.
    /// A timeout is an explicit error; numeric process identifiers are never reopened.
    pub async fn wait(&self, budget: Duration) -> io::Result<u32> {
        wait_process(&self.process, budget).await
    }

    /// Terminates this owned Job Object and confirms its sole process has exited.
    pub async fn terminate_and_wait(&self, budget: Duration) -> io::Result<u32> {
        if let Some(code) = poll_exit(&self.process)? {
            return Ok(code);
        }
        // SAFETY: the job and exact process remain owned throughout these calls.
        let job_error = if unsafe { TerminateJobObject(self.job.as_raw_handle().cast(), 1) } == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        // A failed AssignProcessToJobObject leaves a suspended process outside the job.
        // Always stop that exact process as well; never retry an uncontained launch.
        // SAFETY: process is the exact owned creation handle, never a reopened PID.
        let process_error =
            if unsafe { TerminateProcess(self.process.as_raw_handle().cast(), 1) } == 0 {
                Some(io::Error::last_os_error())
            } else {
                None
            };
        // Exit observation is authoritative even when a termination raced natural exit.
        self.wait(budget).await.map_err(|wait_error| {
            io::Error::new(wait_error.kind(), format!(
                "workflow exit unconfirmed: wait={wait_error}; job={job_error:?}; process={process_error:?}"
            ))
        })
    }
}

async fn wait_process(process: &OwnedHandle, budget: Duration) -> io::Result<u32> {
    tokio::time::timeout(budget, async {
        loop {
            match poll_exit(process)? {
                Some(code) => return Ok(code),
                None => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "workflow host cleanup deadline exceeded",
        )
    })?
}

fn poll_exit(process: &OwnedHandle) -> io::Result<Option<u32>> {
    // SAFETY: process is owned; the zero-timeout wait never blocks.
    match unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) } {
        WAIT_OBJECT_0 => {
            let mut code = 0;
            // SAFETY: code is writable and the owned process handle survives this call.
            if unsafe { GetExitCodeProcess(process.as_raw_handle().cast(), &mut code) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Some(code))
        }
        WAIT_TIMEOUT => Ok(None),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other("unexpected workflow wait result")),
    }
}

impl Drop for WorkflowHostProcess {
    fn drop(&mut self) {
        // Closing the job also enforces KILL_ON_JOB_CLOSE. Explicit waiting is left to
        // terminate_and_wait, since Drop must never block an executor indefinitely.
        // SAFETY: job remains owned until after this destructor returns.
        unsafe { TerminateJobObject(self.job.as_raw_handle().cast(), 1) };
        // Failed assignment may leave the host outside the job. This exact-handle
        // fallback is best effort only; Drop never constitutes an exit receipt.
        // SAFETY: process is still the owned creation handle and remains live here.
        unsafe { TerminateProcess(self.process.as_raw_handle().cast(), 1) };
    }
}

#[cfg(test)]
#[path = "workflow_job_tests.rs"]
mod tests;
