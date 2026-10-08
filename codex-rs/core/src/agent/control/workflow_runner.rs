//! Native caller for a credential-free V8 child and exact native agent admissions.
use super::workflow::NativeWorkflowBridge;
use super::workflow::WorkflowAgentCall;
use super::workflow::WorkflowInput;
use super::workflow_run_state::RunState;
use super::workflow_run_state::read_bounded;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use codex_code_mode::workflow::ChildMessage;
use codex_code_mode::workflow::ParentMessage;
use codex_code_mode::workflow::VERSION;
use codex_code_mode::workflow::{self as wire};
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::WorkflowRunRequest;
use codex_utils_pty::WorkflowHostCompletion;
use codex_utils_pty::WorkflowHostLaunchCompletion;
use codex_utils_pty::WorkflowHostMode;
use codex_utils_pty::WorkflowHostProcess;
use serde_json::Value;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::Semaphore;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

static ADMISSION: OnceLock<Arc<Semaphore>> = OnceLock::new();
#[cfg(test)]
#[path = "workflow_runner_tests.rs"]
mod tests;
type WaveReceipts = Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>;
struct Admission(Option<tokio::sync::OwnedSemaphorePermit>);
impl Drop for Admission {
    fn drop(&mut self) {
        if let Some(permit) = self.0.take() {
            permit.forget();
        }
    }
}

/// The independent owner retains admission, setup, host and native wave until cleanup.
/// Dropping the caller cancels work, without aborting the cleanup owner. Runtime loss
/// remains best-effort handle Drop and never constitutes a successful exit receipt.
pub(crate) async fn run(
    session: Arc<Session>,
    step: Arc<StepContext>,
    request: WorkflowRunRequest,
    cancellation: CancellationToken,
) -> Result<Value, String> {
    let permit = Arc::clone(ADMISSION.get_or_init(|| Arc::new(Semaphore::new(16))))
        .try_acquire_owned()
        .map_err(|_| "native workflow admission full or quarantined")?;
    let token = cancellation.child_token();
    let _cancel_on_drop = token.clone().drop_guard();
    tokio::spawn(async move {
        let mut admission = Admission(Some(permit));
        let mut release_allowed = true;
        let outcome = owned(
            session,
            step,
            request,
            token,
            &mut admission.0,
            &mut release_allowed,
        )
        .await;
        // Unknown cleanup keeps the admission reserved for the process lifetime.
        // No caller timeout or error can turn that state into successful release.
        if release_allowed && let Some(permit) = admission.0.take() {
            drop(permit);
        }
        outcome
    })
    .await
    .map_err(|_| "workflow owner failed; cleanup not proven")?
}

async fn owned(
    session: Arc<Session>,
    step: Arc<StepContext>,
    request: WorkflowRunRequest,
    cancellation: CancellationToken,
    permit: &mut Option<tokio::sync::OwnedSemaphorePermit>,
    release_allowed: &mut bool,
) -> Result<Value, String> {
    if !wire::valid_run_id(&request.run_id) {
        return Err("invalid workflow run ID".into());
    }
    if let Some(message)=codex_config::claude::permission_block(&step.turn.config.config_layer_stack,"Workflow",&serde_json::json!({"scriptPath":request.script_path,"args":request.args,"runId":request.run_id}))
        .map_err(|error|format!("workflow permission configuration: {error}"))? {return Err(message);}
    wire::check_json(&request.args).map_err(|_| "invalid workflow arguments")?;
    if !request.args.is_object() && !request.args.is_null() {
        return Err("workflow args must be an object".into());
    }
    if step.turn.mode() == codex_protocol::config_types::ModeKind::Plan {
        return Err("Workflow execution is not available in Plan mode".into());
    }
    if step.turn.config.model_provider_id != "openai" {
        return Err("native workflow currently requires the captured Codex provider; Z.ai routing is not activated".into());
    }
    let cwd = step
        .environments
        .single_local_environment_cwd()
        .ok_or("workflow requires exactly one local environment")?;
    let cwd = tokio::fs::canonicalize(cwd.as_path())
        .await
        .map_err(|error| format!("workflow cwd: {error}"))?;
    let supplied = if request.script_path.is_absolute() {
        request.script_path.clone()
    } else {
        cwd.join(&request.script_path)
    };
    let script_path = tokio::fs::canonicalize(&supplied)
        .await
        .map_err(|error| format!("workflow script: {error}"))?;
    if !script_path.starts_with(&cwd) {
        return Err("workflow script must belong to the captured working directory".into());
    }
    if let Some(workflows) = script_path.parent()
        && workflows
            .file_name()
            .is_some_and(|name| name == "workflows")
        && let Some(namespace) = workflows.parent()
        && namespace
            .file_name()
            .is_some_and(|name| name == ".claude" || name == ".codex")
        && namespace.parent() != Some(cwd.as_path())
    {
        return Err("Open the workflow project's native conversation first, or use CLI workflow --cwd for that project; its instructions and permissions must be captured there.".into());
    }
    let policy = step
        .turn
        .permission_profile_for_environments(&step.environments)
        .file_system_sandbox_policy();
    if !policy.can_read_local_path_with_cwd(&supplied, &cwd)
        || !policy.can_read_local_path_with_cwd(&script_path, &cwd)
    {
        return Err("workflow script read denied by native permissions".into());
    }
    let script = read_bounded(&script_path, wire::SCRIPT_BYTES)
        .await
        .map_err(|error| format!("workflow source read: {error}"))?;
    let script = String::from_utf8(script).map_err(|_| "workflow source must be UTF-8")?;
    let executable = codex_install_context::InstallContext::current().code_mode_host_program();
    let state = RunState::create(&request.run_id, session.thread_id.to_string()).await?;
    let finished = CancellationToken::new();
    let watcher = {
        let state = Arc::clone(&state);
        let token = cancellation.clone();
        let finished = finished.clone();
        tokio::spawn(async move {
            let result = state.watch(token.clone(), finished).await;
            if result.is_err() {
                token.cancel();
            }
            result
        })
    };
    let deadline = Instant::now() + Duration::from_secs(60 * 60);
    let waves: WaveReceipts = Arc::new(Mutex::new(Vec::new()));
    *release_allowed = false;
    let mut host_confirmed = false;
    let operation = async {
        let launch = WorkflowHostProcess::spawn_until(
            &executable,
            &cwd,
            WorkflowHostMode::Execute,
            Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(|error| format!("workflow host launch: {error}"))?;
        let mut process = match launch {
            WorkflowHostLaunchCompletion::Started(process) => process,
            WorkflowHostLaunchCompletion::Rejected(error) => {
                host_confirmed = true;
                return Err(format!("workflow host rejected: {error}"));
            }
            WorkflowHostLaunchCompletion::Pending { reason, receipt } => {
                state.set_status("quarantined")?;
                match receipt.await {
                    Ok(Ok(())) => {
                        host_confirmed = true;
                        return Err(format!("workflow setup expired, exit confirmed: {reason}"));
                    }
                    _ => {
                        if let Some(permit) = permit.take() {
                            permit.forget();
                        }
                        return Err("workflow setup exit unconfirmed; admission quarantined".into());
                    }
                }
            }
        };
        let (stdin, stdout) = match (process.take_stdin(), process.take_stdout()) {
            (Ok(stdin), Ok(stdout)) => (stdin, stdout),
            _ => {
                host_confirmed = process
                    .terminate_and_wait(Duration::from_secs(2))
                    .await
                    .is_ok();
                if !host_confirmed {
                    if let Some(permit) = permit.take() {
                        permit.forget();
                    }
                    state.set_status("quarantined")?;
                }
                return Err("workflow transport handoff failed".into());
            }
        };
        let bridge = NativeWorkflowBridge::new(
            session,
            step,
            cancellation.clone(),
            Some(state.observer(cancellation.clone())),
        );
        let driver = exchange(
            stdin,
            stdout,
            script,
            request.args,
            bridge,
            Arc::clone(&state),
            cancellation.clone(),
            Arc::clone(&waves),
        );
        let completion = process
            .supervise(deadline, driver, cancellation.clone().cancelled_owned())
            .await;
        match completion {
            Ok(WorkflowHostCompletion::Confirmed { outcome, exit_code }) => {
                host_confirmed = true;
                if exit_code != 0 {
                    Err(format!("workflow host exited unsuccessfully: {exit_code}"))
                } else {
                    outcome.map_err(|error| format!("workflow run: {error}"))
                }
            }
            Ok(WorkflowHostCompletion::Pending {
                outcome, receipt, ..
            }) => {
                state.set_status("quarantined")?;
                match receipt.await {
                    Ok(Ok(0)) => {
                        host_confirmed = true;
                        outcome.map_err(|error| {
                            format!("workflow run (exit confirmed after quarantine): {error}")
                        })
                    }
                    Ok(Ok(code)) => {
                        host_confirmed = true;
                        Err(format!(
                            "workflow host exited unsuccessfully after quarantine: {code}"
                        ))
                    }
                    _ => {
                        if let Some(permit) = permit.take() {
                            permit.forget();
                        }
                        Err("workflow host exit unconfirmed; admission quarantined".into())
                    }
                }
            }
            Err(error) => {
                if let Some(permit) = permit.take() {
                    permit.forget();
                }
                Err(format!(
                    "workflow supervision failed; admission quarantined: {error}"
                ))
            }
        }
    }
    .await;
    let stopped = cancellation.is_cancelled();
    if operation.is_err() {
        cancellation.cancel();
    }
    let receipts = std::mem::take(
        &mut *waves
            .lock()
            .map_err(|_| "workflow wave receipt lock poisoned")?,
    );
    for receipt in receipts {
        if receipt.await.is_err() {
            if let Some(permit) = permit.take() {
                permit.forget();
            }
            state.set_status("quarantined")?;
            state.log("Native wave owner failed; child exit not proven.".into())?;
        }
    }
    if !host_confirmed {
        if let Some(permit) = permit.take() {
            permit.forget();
        }
        state.set_status("quarantined")?;
    }
    *release_allowed = host_confirmed && permit.is_some();
    finished.cancel();
    let watcher = watcher.await.map_err(|_| "workflow status owner failed")?;
    if permit.is_some() {
        state.set_status(if stopped {
            "stopped"
        } else if operation.is_ok() {
            "completed"
        } else {
            "failed"
        })?;
    }
    if let Err(error) = &operation {
        state.log(error.clone())?;
    }
    state.persist().await?;
    watcher?;
    if permit.is_none() {
        return Err("workflow cleanup unconfirmed; admission quarantined".into());
    }
    if stopped {
        return Err("workflow stopped".into());
    }
    operation.map(
        |result| serde_json::json!({"runId":request.run_id,"status":"completed","result":result}),
    )
}

async fn exchange(
    mut input: NamedPipeServer,
    mut output: NamedPipeServer,
    script: String,
    args: Value,
    bridge: NativeWorkflowBridge,
    state: Arc<RunState>,
    cancellation: CancellationToken,
    waves: WaveReceipts,
) -> io::Result<Value> {
    if !matches!(
        read(&mut output).await?,
        ChildMessage::Ready { version: VERSION }
    ) {
        return Err(io::Error::other("workflow ready handshake refused"));
    }
    write(
        &mut input,
        &ParentMessage::Start {
            version: VERSION,
            script,
            arguments: args,
        },
    )
    .await?;
    let mut sequence = 0;
    let mut count = 0;
    loop {
        match read(&mut output).await? {
            ChildMessage::Log { phase, message, .. } => {
                state.phase(&phase).map_err(io::Error::other)?;
                state.log(message).map_err(io::Error::other)?;
            }
            ChildMessage::Group {
                sequence: received,
                phase,
                calls,
                ..
            } => {
                sequence += 1;
                count += calls.len();
                if received != sequence || sequence > wire::GROUPS || count > wire::TOTAL_CALLS {
                    return Err(io::Error::other("workflow group bounds refused"));
                }
                state.phase(&phase).map_err(io::Error::other)?;
                let mut values = Vec::with_capacity(calls.len());
                for (wave, calls) in calls.chunks(4).enumerate() {
                    state.mark_paused().map_err(io::Error::other)?;
                    state
                        .wait_running(&cancellation)
                        .await
                        .map_err(io::Error::other)?;
                    if cancellation.is_cancelled() {
                        return Err(io::Error::other("workflow stopped"));
                    }
                    let calls = calls
                        .iter()
                        .enumerate()
                        .map(|(index, call)| {
                            native_call(call, sequence, wave * 4 + index, bridge.model_slug())
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(io::Error::other)?;
                    let (reply, response) = tokio::sync::oneshot::channel();
                    // Capture ownership synchronously before the first result wait.
                    {
                        let mut receipts = waves
                            .lock()
                            .map_err(|_| io::Error::other("workflow wave receipt lock poisoned"))?;
                        receipts.push(bridge.clone().start_owned(calls, reply));
                    }
                    values.extend(
                        response
                            .await
                            .map_err(|_| io::Error::other("workflow wave owner failed"))?
                            .map_err(io::Error::other)?,
                    );
                }
                write(
                    &mut input,
                    &ParentMessage::GroupResult {
                        version: VERSION,
                        sequence,
                        values,
                    },
                )
                .await?;
            }
            ChildMessage::Done { result, .. } => {
                let mut trailing = [0; 1];
                if output.read(&mut trailing).await? != 0 {
                    return Err(io::Error::other("workflow host emitted data after Done"));
                }
                return Ok(result);
            }
            ChildMessage::Failed { code, .. } => {
                return Err(io::Error::other(format!("workflow host refused: {code:?}")));
            }
            ChildMessage::Ready { .. } => {
                return Err(io::Error::other("unexpected workflow handshake"));
            }
        }
    }
}

fn native_call(
    call: &wire::AgentCall,
    sequence: u64,
    index: usize,
    parent_model: &str,
) -> Result<WorkflowAgentCall, String> {
    let (model, default_effort) = match call.model.as_deref() {
        Some("opus") => (Some(parent_model.into()), Some(ReasoningEffort::High)),
        Some("sonnet") => (Some(parent_model.into()), Some(ReasoningEffort::Medium)),
        Some("haiku") => (Some(parent_model.into()), Some(ReasoningEffort::Low)),
        other => (other.map(str::to_owned), None),
    };
    let effort = call
        .effort
        .as_deref()
        .map(|value| {
            serde_json::from_value::<ReasoningEffort>(Value::String(value.into()))
                .map_err(|_| "invalid workflow effort".to_string())
                .and_then(|effort| match effort {
                    ReasoningEffort::Custom(_) => Err("invalid workflow effort".to_string()),
                    effort => Ok(effort),
                })
        })
        .transpose()?
        .or(default_effort);
    Ok(WorkflowAgentCall {
        task_name: format!("wf_{sequence}_{index}"),
        input: WorkflowInput::UserInput(call.prompt.clone()),
        role_name: call.role.clone(),
        schema: call.schema.clone(),
        model,
        effort,
        compatible: true,
        label: Some(call.name.clone()),
    })
}

async fn read(output: &mut NamedPipeServer) -> io::Result<ChildMessage> {
    let mut prefix = [0; 4];
    output.read_exact(&mut prefix).await?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > wire::FRAME_BYTES {
        return Err(io::Error::other("workflow frame limit"));
    }
    let mut bytes = vec![0; length];
    output.read_exact(&mut bytes).await?;
    wire::decode_child(&bytes)
        .map_err(|code| io::Error::other(format!("workflow frame refused: {code:?}")))
}
async fn write(input: &mut NamedPipeServer, message: &ParentMessage) -> io::Result<()> {
    let bytes = wire::encode(message, wire::FRAME_BYTES)
        .map_err(|_| io::Error::other("workflow frame encoding limit"))?;
    input.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    input.write_all(&bytes).await?;
    input.flush().await
}
