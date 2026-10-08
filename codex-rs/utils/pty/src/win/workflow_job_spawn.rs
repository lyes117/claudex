use super::WORKFLOW_HOST_MEMORY_BYTES;
use super::WorkflowHostProcess;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::ptr;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::net::windows::named_pipe::ServerOptions;
use winapi::um::handleapi::SetHandleInformation;
use winapi::um::jobapi2::AssignProcessToJobObject;
use winapi::um::jobapi2::CreateJobObjectW;
use winapi::um::jobapi2::SetInformationJobObject;
use winapi::um::processthreadsapi::CreateProcessW;
use winapi::um::processthreadsapi::DeleteProcThreadAttributeList;
use winapi::um::processthreadsapi::GetCurrentProcessId;
use winapi::um::processthreadsapi::InitializeProcThreadAttributeList;
use winapi::um::processthreadsapi::LPPROC_THREAD_ATTRIBUTE_LIST;
use winapi::um::processthreadsapi::PROCESS_INFORMATION;
use winapi::um::processthreadsapi::ResumeThread;
use winapi::um::processthreadsapi::TerminateProcess;
use winapi::um::processthreadsapi::UpdateProcThreadAttribute;
use winapi::um::winbase::CREATE_NO_WINDOW;
use winapi::um::winbase::CREATE_SUSPENDED;
use winapi::um::winbase::CREATE_UNICODE_ENVIRONMENT;
use winapi::um::winbase::EXTENDED_STARTUPINFO_PRESENT;
use winapi::um::winbase::GetNamedPipeClientProcessId;
use winapi::um::winbase::HANDLE_FLAG_INHERIT;
use winapi::um::winbase::STARTF_USESTDHANDLES;
use winapi::um::winbase::STARTUPINFOEXW;
use winapi::um::winnt::HANDLE;
use winapi::um::winnt::JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
use winapi::um::winnt::JOB_OBJECT_LIMIT_JOB_MEMORY;
use winapi::um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
use winapi::um::winnt::JOB_OBJECT_LIMIT_PROCESS_MEMORY;
use winapi::um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
use winapi::um::winnt::JobObjectExtendedLimitInformation;

// These stable OS exports avoid adding a dependency or broadening winapi features.
#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(algorithm: HANDLE, buffer: *mut u8, length: u32, flags: u32) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
}

pub(super) async fn launch(
    executable: &Path,
    cwd: &Path,
    args: &[OsString],
) -> io::Result<WorkflowHostProcess> {
    match launch_unfinished(executable, cwd, args).await? {
        LaunchOutcome::Started(process) => Ok(process),
        LaunchOutcome::Failed { error, process } => {
            super::driver::retain_until_exit(process)
                .await
                .map_err(|_| io::Error::other("workflow failed-launch cleanup task failed"))??;
            Err(error)
        }
    }
}

pub(super) enum LaunchOutcome {
    Started(WorkflowHostProcess),
    Failed {
        error: io::Error,
        process: WorkflowHostProcess,
    },
}

/// No await follows native process creation; either exact ownership is returned or
/// an error means no process was created. The caller owns every later cleanup await.
pub(super) async fn launch_unfinished(
    executable: &Path,
    cwd: &Path,
    args: &[OsString],
) -> io::Result<LaunchOutcome> {
    if !executable.is_absolute() || !cwd.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workflow executable and cwd must be absolute",
        ));
    }
    if !executable
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workflow host must be a native executable",
        ));
    }
    let job = strict_job()?;
    let (stdin, child_stdin) = pipe(PipeDirection::ToHost).await?;
    let (stdout, child_stdout) = pipe(PipeDirection::FromHost).await?;
    let stderr = OpenOptions::new().write(true).open("NUL")?;
    inherit(&child_stdin)?;
    inherit(&child_stdout)?;
    inherit(&stderr)?;
    let start = create_contained(
        executable,
        cwd,
        args,
        &job,
        [&child_stdin, &child_stdout, &stderr],
    );
    drop((child_stdin, child_stdout, stderr));
    let process = match start? {
        StartResult::Started(process) => process,
        StartResult::Failed { error, process } => {
            let owner = WorkflowHostProcess {
                process: process.into_handle(),
                job,
                stdin: Some(stdin),
                stdout: Some(stdout),
            };
            return Ok(LaunchOutcome::Failed {
                error,
                process: owner,
            });
        }
    };
    Ok(LaunchOutcome::Started(WorkflowHostProcess {
        process,
        job,
        stdin: Some(stdin),
        stdout: Some(stdout),
    }))
}

fn create_contained(
    executable: &Path,
    cwd: &Path,
    args: &[OsString],
    job: &OwnedHandle,
    files: [&File; 3],
) -> io::Result<StartResult> {
    let application = wide(executable.as_os_str())?;
    let directory = wide(cwd.as_os_str())?;
    let mut command = command_line(executable.as_os_str(), args)?;
    let mut environment = minimal_environment()?;
    let [child_stdin, child_stdout, stderr] = files;
    let mut handles = [
        child_stdin.as_raw_handle().cast(),
        child_stdout.as_raw_handle().cast(),
        stderr.as_raw_handle().cast(),
    ];
    let mut attributes = HandleList::new(&mut handles)?;
    // SAFETY: this Windows C record permits all-zero initialization before field setup.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[2];
    startup.lpAttributeList = attributes.as_mut_ptr();
    // SAFETY: this is an output-only Windows C record with valid all-zero initialization.
    let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: all strings, the environment, startup attributes and the three
    // explicitly allowlisted inheritable handles remain alive for CreateProcessW.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            /*bInheritHandles*/ 1,
            CREATE_NO_WINDOW
                | CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_mut_ptr().cast(),
            directory.as_ptr(),
            &mut startup.StartupInfo,
            &mut information,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreateProcessW transfers these newly created handles.
    let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess.cast()) };
    let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread.cast()) };
    let pending = SuspendedProcess(process);
    // SAFETY: the process has never run and both handles remain owned.
    if unsafe {
        AssignProcessToJobObject(job.as_raw_handle().cast(), pending.0.as_raw_handle().cast())
    } == 0
    {
        return Ok(StartResult::Failed {
            error: io::Error::last_os_error(),
            process: pending,
        });
    }
    // SAFETY: this is the exact initial suspended thread of our contained process.
    if unsafe { ResumeThread(thread.as_raw_handle().cast()) } == u32::MAX {
        return Ok(StartResult::Failed {
            error: io::Error::last_os_error(),
            process: pending,
        });
    }
    let process = pending.into_handle();
    // Child-facing handles are closed in this process before returning. No other
    // parent handle can enter this child through the explicit attribute allowlist.
    drop(thread);
    Ok(StartResult::Started(process))
}

enum StartResult {
    Started(OwnedHandle),
    Failed {
        error: io::Error,
        process: SuspendedProcess,
    },
}

fn strict_job() -> io::Result<OwnedHandle> {
    // SAFETY: null security attributes/name create a private noninheritable handle.
    let handle = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreateJobObjectW transfers a unique newly created handle.
    let job = unsafe { OwnedHandle::from_raw_handle(handle.cast()) };
    // SAFETY: this Windows C record permits all-zero initialization before limit setup.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_JOB_MEMORY
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.ProcessMemoryLimit = WORKFLOW_HOST_MEMORY_BYTES;
    limits.JobMemoryLimit = WORKFLOW_HOST_MEMORY_BYTES;
    // SAFETY: limits has the Windows-defined layout and remains writable.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle().cast(),
            JobObjectExtendedLimitInformation,
            ptr::addr_of_mut!(limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

enum PipeDirection {
    ToHost,
    FromHost,
}

async fn pipe(direction: PipeDirection) -> io::Result<(NamedPipeServer, File)> {
    let parent_reads = matches!(direction, PipeDirection::FromHost);
    let mut random = [0_u8; 16];
    // SAFETY: the system-preferred RNG writes precisely this local array.
    if unsafe { BCryptGenRandom(ptr::null_mut(), random.as_mut_ptr(), random.len() as u32, 2) } < 0
    {
        return Err(io::Error::other(
            "workflow pipe random identifier unavailable",
        ));
    }
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let name = format!(r"\\.\pipe\claudex-workflow-{suffix}");
    let server = ServerOptions::new()
        .access_inbound(parent_reads)
        .access_outbound(!parent_reads)
        .first_pipe_instance(true)
        .max_instances(1)
        .reject_remote_clients(true)
        .in_buffer_size(65_536)
        .out_buffer_size(65_536)
        .create(&name)?;
    let client = OpenOptions::new()
        .read(!parent_reads)
        .write(parent_reads)
        .open(&name)?;
    server.connect().await?;
    let mut client_pid = 0;
    // Verify the preconnected child endpoint was opened by this parent, before
    // it is inherited by the host. A foreign local pipe connection fails closed.
    // SAFETY: server is a valid connected pipe and client_pid is writable.
    if unsafe { GetNamedPipeClientProcessId(server.as_raw_handle().cast(), &mut client_pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetCurrentProcessId has no pointer preconditions.
    if client_pid != unsafe { GetCurrentProcessId() } {
        return Err(io::Error::other(
            "workflow pipe endpoint ownership mismatch",
        ));
    }
    Ok((server, client))
}

fn inherit(file: &File) -> io::Result<()> {
    // SAFETY: file owns a valid handle. Only this flag is changed.
    if unsafe {
        SetHandleInformation(
            file.as_raw_handle().cast(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn minimal_environment() -> io::Result<Vec<u16>> {
    let mut root = vec![0_u16; 32_768];
    // SAFETY: root is a writable UTF-16 buffer with the specified capacity.
    let length =
        unsafe { GetSystemWindowsDirectoryW(root.as_mut_ptr(), root.len() as u32) } as usize;
    if length == 0 || length >= root.len() {
        return Err(io::Error::other("workflow system directory unavailable"));
    }
    let mut environment: Vec<u16> = "SystemRoot=".encode_utf16().collect();
    environment.extend_from_slice(&root[..length]);
    environment.extend_from_slice(&[0, 0]);
    Ok(environment)
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workflow argument contains NUL",
        ));
    }
    value.push(0);
    Ok(value)
}

fn command_line(executable: &OsStr, args: &[OsString]) -> io::Result<Vec<u16>> {
    let mut command = Vec::new();
    for argument in std::iter::once(executable).chain(args.iter().map(OsString::as_os_str)) {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        command.push(b'"' as u16);
        let mut slashes = 0;
        for unit in argument.encode_wide() {
            if unit == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "workflow argument contains NUL",
                ));
            }
            if unit == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            command.extend(std::iter::repeat_n(
                b'\\' as u16,
                if unit == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            slashes = 0;
            command.push(unit);
        }
        command.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        command.push(b'"' as u16);
    }
    if command.len() >= 32_767 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workflow command line too long",
        ));
    }
    command.push(0);
    Ok(command)
}

struct SuspendedProcess(OwnedHandle);
impl SuspendedProcess {
    fn into_handle(self) -> OwnedHandle {
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: this suppresses Drop and transfers the sole owned handle exactly once.
        unsafe { ptr::read(&this.0) }
    }
}
impl Drop for SuspendedProcess {
    fn drop(&mut self) {
        // Failures after creation must terminate the exact suspended/assigned host,
        // including when nested-job assignment is denied. There is no uncontained retry.
        // SAFETY: the exact process handle remains owned.
        unsafe { TerminateProcess(self.0.as_raw_handle().cast(), 1) };
    }
}

struct HandleList {
    storage: Vec<usize>,
}
impl HandleList {
    fn new(handles: &mut [HANDLE; 3]) -> io::Result<Self> {
        let mut bytes = 0;
        // SAFETY: the first call queries the required size; failure is expected.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut bytes) };
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0_usize; bytes.div_ceil(std::mem::size_of::<usize>())];
        // Unlike Vec<u8>, usize storage has the pointer alignment Windows requires.
        let pointer = storage.as_mut_ptr().cast();
        // SAFETY: storage is aligned, large enough and live until Delete is called.
        if unsafe { InitializeProcThreadAttributeList(pointer, 1, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut list = Self { storage };
        // SAFETY: handles remains alive and unchanged through CreateProcessW. The
        // HANDLE_LIST constant is the documented ProcThreadAttributeHandleList value.
        if unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                0x0002_0002,
                handles.as_mut_ptr().cast(),
                std::mem::size_of_val(handles),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }
    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}
impl Drop for HandleList {
    fn drop(&mut self) {
        // SAFETY: this list was successfully initialized and is deleted exactly once.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

#[cfg(test)]
#[path = "workflow_job_spawn_tests.rs"]
mod tests;
