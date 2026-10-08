//! Real failed-assignment cleanup, not a simulated Windows process.
use super::*;
use pretty_assertions::assert_eq;
use winapi::um::synchapi::CreateEventW;

#[tokio::test]
async fn empty_job_cleanup_still_stops_unassigned_suspended_process() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let cwd = executable
        .parent()
        .ok_or_else(|| io::Error::other("fixture parent"))?;
    // SAFETY: null name/security create a private noninheritable event; no pointers retained.
    let handle = unsafe { CreateEventW(ptr::null_mut(), 1, 0, ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreateEventW transfers this uniquely owned event handle.
    let invalid_job = unsafe { OwnedHandle::from_raw_handle(handle.cast()) };
    let (stdin, child_stdin) = pipe(PipeDirection::ToHost).await?;
    let (stdout, child_stdout) = pipe(PipeDirection::FromHost).await?;
    let stderr = OpenOptions::new().write(true).open("NUL")?;
    for file in [&child_stdin, &child_stdout, &stderr] {
        inherit(file)?;
    }
    let cleanup_job = strict_job()?;
    let start = create_contained(
        &executable,
        cwd,
        &[OsString::from("--help")],
        &invalid_job,
        [&child_stdin, &child_stdout, &stderr],
    )?;
    let StartResult::Failed { process, .. } = start else {
        panic!("event handle cannot be accepted as a Job Object");
    };
    let process = WorkflowHostProcess {
        process: process.into_handle(),
        job: cleanup_job,
        stdin: Some(stdin),
        stdout: Some(stdout),
    };
    let observer = process.process.try_clone()?;
    assert_eq!(super::super::poll_exit(&observer)?, None);
    let receipt = super::super::driver::retain_until_exit(process);
    let code = tokio::time::timeout(std::time::Duration::from_secs(3), receipt)
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)??;
    assert_eq!(super::super::poll_exit(&observer)?, Some(code));
    Ok(())
}
