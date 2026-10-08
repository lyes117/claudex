use super::WORKFLOW_HOST_MEMORY_BYTES;
use super::WorkflowHostProcess;
use super::spawn;
use pretty_assertions::assert_eq;
use std::ffi::OsString;
use std::io::BufRead;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::handleapi::GetHandleInformation;
use winapi::um::handleapi::SetHandleInformation;
use winapi::um::jobapi2::QueryInformationJobObject;
use winapi::um::synchapi::CreateEventW;
use winapi::um::synchapi::SetEvent;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::HANDLE_FLAG_INHERIT;
use winapi::um::winnt::JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
use winapi::um::winnt::JOB_OBJECT_LIMIT_BREAKAWAY_OK;
use winapi::um::winnt::JOB_OBJECT_LIMIT_JOB_MEMORY;
use winapi::um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
use winapi::um::winnt::JOB_OBJECT_LIMIT_PROCESS_MEMORY;
use winapi::um::winnt::JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
use winapi::um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
use winapi::um::winnt::JobObjectExtendedLimitInformation;

pub(super) async fn fixture() -> std::io::Result<WorkflowHostProcess> {
    let args = [
        "--ignored",
        "--exact",
        "win::workflow_job::tests::host_fixture",
        "--nocapture",
        "--test-threads=1",
    ];
    let executable = std::env::current_exe()?;
    let cwd = executable
        .parent()
        .ok_or_else(|| std::io::Error::other("test executable has no directory"))?;
    spawn::launch(&executable, cwd, &args.map(OsString::from)).await
}

async fn instruction(process: &mut WorkflowHostProcess, value: &str) -> std::io::Result<String> {
    let mut stdin = process.take_stdin()?;
    let stdout = process.take_stdout()?;
    stdin.write_all(value.as_bytes()).await?;
    stdin.shutdown().await?;
    let mut output = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        stdout.take(4096).read_to_end(&mut output),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "fixture stdout timeout"))??;
    String::from_utf8(output).map_err(std::io::Error::other)
}

#[tokio::test]
async fn native_host_job_enforces_memory_process_and_no_breakaway_limits() -> std::io::Result<()> {
    let mut process = fixture().await?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let queried = unsafe {
        QueryInformationJobObject(
            process.job.as_raw_handle().cast(),
            JobObjectExtendedLimitInformation,
            std::ptr::addr_of_mut!(limits).cast(),
            std::mem::size_of_val(&limits) as u32,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(queried, 0);
    assert_eq!(
        (
            limits.BasicLimitInformation.LimitFlags,
            limits.BasicLimitInformation.ActiveProcessLimit,
            limits.ProcessMemoryLimit,
            limits.JobMemoryLimit
        ),
        (
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_JOB_MEMORY
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            1,
            WORKFLOW_HOST_MEMORY_BYTES,
            WORKFLOW_HOST_MEMORY_BYTES
        )
    );
    assert_eq!(
        limits.BasicLimitInformation.LimitFlags
            & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK),
        0
    );
    assert!(
        instruction(&mut process, "memory\n")
            .await?
            .contains("WF:memory:denied")
    );
    assert_eq!(process.wait(Duration::from_secs(2)).await?, 0);
    Ok(())
}

#[tokio::test]
async fn host_environment_is_os_minimal_and_cannot_spawn_descendants() -> std::io::Result<()> {
    let mut process = fixture().await?;
    assert!(
        instruction(&mut process, "env\n")
            .await?
            .contains("WF:env:SystemRoot")
    );
    assert_eq!(process.wait(Duration::from_secs(2)).await?, 0);
    let mut process = fixture().await?;
    assert!(
        instruction(&mut process, "spawn\n")
            .await?
            .contains("WF:spawn:denied")
    );
    assert_eq!(process.wait(Duration::from_secs(2)).await?, 0);
    Ok(())
}

#[tokio::test]
async fn explicit_handle_list_excludes_unrelated_inheritable_parent_handle() -> std::io::Result<()>
{
    let event = unsafe {
        CreateEventW(
            std::ptr::null_mut(),
            /*bManualReset*/ 1,
            /*bInitialState*/ 0,
            std::ptr::null(),
        )
    };
    if event.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let event = unsafe { OwnedHandle::from_raw_handle(event.cast()) };
    let changed = unsafe {
        SetHandleInformation(
            event.as_raw_handle().cast(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    };
    assert_ne!(changed, 0);
    let mut process = fixture().await?;
    let handle = event.as_raw_handle() as usize;
    assert!(
        instruction(&mut process, &format!("handle:{handle}\n"))
            .await?
            .contains("WF:handle:attempted")
    );
    assert_eq!(process.wait(Duration::from_secs(2)).await?, 0);
    // Object identity is demonstrated by the parent event remaining unsignaled;
    // a numeric handle value may be independently reused by the child's runtime.
    assert_eq!(
        unsafe { WaitForSingleObject(event.as_raw_handle().cast(), 0) },
        WAIT_TIMEOUT
    );
    // The owned parent canary survives the host and remains queryable.
    let mut flags = 0;
    assert_ne!(
        unsafe { GetHandleInformation(event.as_raw_handle().cast(), &mut flags) },
        0
    );
    Ok(())
}

#[tokio::test]
async fn timeout_then_owned_job_termination_confirms_exit() -> std::io::Result<()> {
    let mut process = fixture().await?;
    process.take_stdin()?.write_all(b"spin\n").await?;
    assert_eq!(
        process
            .wait(Duration::from_millis(30))
            .await
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::TimedOut
    );
    assert_ne!(process.terminate_and_wait(Duration::from_secs(2)).await?, 0);
    Ok(())
}

#[tokio::test]
async fn dropping_supervisor_kills_only_its_owned_process() -> std::io::Result<()> {
    let mut process = fixture().await?;
    let exact_process = process.process.try_clone()?;
    process.take_stdin()?.write_all(b"spin\n").await?;
    drop(process);
    assert_ne!(
        super::wait_process(&exact_process, Duration::from_secs(2)).await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn native_host_rejects_shell_and_relative_path_without_launching() {
    assert_eq!(
        WorkflowHostProcess::spawn(Path::new("relative.exe"), Path::new("C:\\"))
            .await
            .err()
            .expect("relative executable must fail")
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(
        WorkflowHostProcess::spawn(Path::new("C:\\fixture.cmd"), Path::new("C:\\"))
            .await
            .err()
            .expect("shell script must fail")
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
}

#[tokio::test]
async fn successful_operation_is_returned_only_after_exact_process_exit() -> std::io::Result<()> {
    let mut process = fixture().await?;
    let observer = process.process.try_clone()?;
    process.take_stdin()?.write_all(b"spin\n").await?;
    let result = process
        .supervise(
            tokio::time::Instant::now() + Duration::from_secs(5),
            async { Ok(42) },
            std::future::pending(),
        )
        .await?;
    let super::WorkflowHostCompletion::Confirmed { outcome, exit_code } = result else {
        panic!("fixture cleanup must be confirmed");
    };
    assert_eq!(outcome?, 42);
    assert_eq!(super::poll_exit(&observer)?, Some(exit_code));
    Ok(())
}

#[tokio::test]
async fn cancelling_owner_operation_confirms_exact_exit_before_return() -> std::io::Result<()> {
    let mut process = fixture().await?;
    let observer = process.process.try_clone()?;
    process.take_stdin()?.write_all(b"spin\n").await?;
    let result = process
        .supervise::<(), _, _>(
            tokio::time::Instant::now() + Duration::from_secs(5),
            std::future::pending(),
            std::future::ready(()),
        )
        .await?;
    let super::WorkflowHostCompletion::Confirmed { outcome, exit_code } = result else {
        panic!("fixture cleanup must be confirmed");
    };
    assert_eq!(outcome.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
    assert_eq!(super::poll_exit(&observer)?, Some(exit_code));
    Ok(())
}

#[tokio::test]
async fn dropping_waiter_keeps_shutdown_owner_until_exact_exit() -> std::io::Result<()> {
    let mut process = fixture().await?;
    let observer = process.process.try_clone()?;
    process.take_stdin()?.write_all(b"spin\n").await?;
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(process.supervise::<(), _, _>(
        tokio::time::Instant::now() + Duration::from_secs(5),
        async move {
            started
                .send(())
                .map_err(|_| std::io::Error::other("fixture start receiver dropped"))?;
            std::future::pending().await
        },
        std::future::pending(),
    ));
    started_rx.await.map_err(std::io::Error::other)?;
    waiter.abort();
    assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
    super::wait_process(&observer, Duration::from_secs(3)).await?;
    assert!(super::poll_exit(&observer)?.is_some());
    Ok(())
}

/// Child fixture invoked directly by the test executable, never by production launch.
#[test]
#[ignore = "invoked only by owned Job Object integration fixtures"]
fn host_fixture() {
    let mut input = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut input)
        .expect("fixture input");
    let status = match input.trim() {
        "env" => {
            let mut names: Vec<String> = std::env::vars().map(|(name, _)| name).collect();
            names.sort();
            assert_eq!(names, vec!["SystemRoot"]);
            "WF:env:SystemRoot".to_owned()
        }
        "memory" => {
            let mut allocation = Vec::<u8>::new();
            assert!(allocation.try_reserve_exact(384 * 1024 * 1024).is_err());
            "WF:memory:denied".to_owned()
        }
        "spawn" => {
            match std::process::Command::new(std::env::current_exe().expect("fixture executable"))
                .arg("--list")
                .spawn()
            {
                Err(_) => {}
                Ok(mut child) => assert!(!child.wait().expect("child exit").success()),
            }
            "WF:spawn:denied".to_owned()
        }
        "spin" => loop {
            std::thread::sleep(Duration::from_millis(50));
        },
        value if value.starts_with("handle:") => {
            let handle: usize = value[7..].parse().expect("fixture handle");
            unsafe { SetEvent(handle as _) };
            "WF:handle:attempted".to_owned()
        }
        _ => panic!("unsupported fixture instruction"),
    };
    println!("{status}");
    std::io::stdout().flush().expect("fixture output");
}
