#![cfg(windows)]
//! Real V8 inside the owned Windows Job; framed delegate replies are synthetic.
//! No authentication, model call, public tool or user workflow is invoked.
use codex_utils_pty::WorkflowHostCompletion;
use codex_utils_pty::WorkflowHostLaunchCompletion;
use codex_utils_pty::WorkflowHostMode;
use codex_utils_pty::WorkflowHostProcess;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::io;
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;

async fn read(output: &mut (impl AsyncRead + Unpin)) -> io::Result<Value> {
    let length = output.read_u32().await? as usize;
    if length == 0 || length > 1024 * 1024 {
        return Err(io::Error::other("fixture frame exceeds cap"));
    }
    let mut bytes = vec![0; length];
    output.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

async fn write(input: &mut (impl AsyncWrite + Unpin), value: Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other("fixture frame exceeds cap"));
    }
    input.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    input.write_all(&bytes).await?;
    input.flush().await
}

async fn spawn(mode: WorkflowHostMode, deadline: Instant) -> io::Result<WorkflowHostProcess> {
    let executable =
        codex_utils_cargo_bin::cargo_bin("codex-code-mode-host").map_err(io::Error::other)?;
    let cwd = executable
        .parent()
        .ok_or_else(|| io::Error::other("host binary parent"))?;
    match WorkflowHostProcess::spawn_until(&executable, cwd, mode, deadline).await? {
        WorkflowHostLaunchCompletion::Started(process) => Ok(process),
        WorkflowHostLaunchCompletion::Rejected(error) => Err(error),
        WorkflowHostLaunchCompletion::Pending { reason, receipt } => {
            // A fixture reports failure only after confirming no setup process remains.
            receipt.await.map_err(io::Error::other)??;
            Err(reason)
        }
    }
}

async fn exercise(
    script: &str,
    mode: WorkflowHostMode,
) -> io::Result<WorkflowHostCompletion<Value>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut process = spawn(mode, deadline).await?;
    let mut input = process.take_stdin()?;
    let mut output = process.take_stdout()?;
    let script = script.to_owned();
    process
        .supervise(
            deadline,
            async move {
                assert_eq!(
                    read(&mut output).await?,
                    json!({"kind":"ready","version":1})
                );
                write(
                    &mut input,
                    json!({"kind":"start","version":1,"script":script,"arguments":{}}),
                )
                .await?;
                let first = read(&mut output).await?;
                if first["kind"] != "group" {
                    return Ok(first);
                }
                assert_eq!(first["sequence"], 1);
                write(
                    &mut input,
                    json!({"kind":"group_result","version":1,"sequence":1,"values":[{}]}),
                )
                .await?;
                read(&mut output).await
            },
            std::future::pending(),
        )
        .await
}

fn confirmed<T>(result: WorkflowHostCompletion<T>) -> (io::Result<T>, u32) {
    match result {
        WorkflowHostCompletion::Confirmed { outcome, exit_code } => (outcome, exit_code),
        WorkflowHostCompletion::Pending { .. } => panic!("fixture exit must be confirmed"),
    }
}

#[tokio::test]
async fn supervised_preflight_never_evaluates_body_or_metadata() -> io::Result<()> {
    let script = "export const meta = agent('never',{schema:{type:'object'}}); while(true){}";
    let (outcome, exit) = confirmed(exercise(script, WorkflowHostMode::Preflight).await?);
    assert_eq!(exit, 0);
    let result = outcome?;
    assert_eq!(result["kind"], "done");
    assert_eq!(result["result"]["parseOnly"], true);
    assert_eq!(result["result"]["v8Compiled"], false);
    assert_eq!(result["result"]["agentsAdmitted"], 0);
    Ok(())
}

#[tokio::test]
async fn supervised_deadline_stops_cpu_and_post_await_microtasks() -> io::Result<()> {
    let gate = "await agent('gate',{schema:{type:'object',properties:{},required:[],additionalProperties:false}});";
    for script in [
        "while(true){}".to_owned(),
        format!("{gate}for(;;){{}}"),
        format!(
            "{gate}function spin(){{Promise.resolve().then(spin)}}spin();await new Promise(()=>{{}});"
        ),
    ] {
        let (outcome, exit) = confirmed(exercise(&script, WorkflowHostMode::Execute).await?);
        assert_eq!(outcome.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_ne!(exit, 0);
    }
    Ok(())
}

#[tokio::test]
async fn external_arraybuffer_cannot_complete_above_job_commit_budget() -> io::Result<()> {
    let script = "const b=new Uint8Array(384*1024*1024);for(let i=0;i<b.length;i+=4096)b[i]=1;return b.length;";
    let (outcome, exit) = confirmed(exercise(script, WorkflowHostMode::Execute).await?);
    assert_ne!(exit, 0);
    if let Ok(message) = outcome {
        assert_ne!(message["kind"], "done");
    }
    Ok(())
}

#[tokio::test]
async fn eof_during_delegate_wait_confirms_owned_host_exit() -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut process = spawn(WorkflowHostMode::Execute, deadline).await?;
    let mut input = process.take_stdin()?;
    let mut output = process.take_stdout()?;
    let result = process.supervise(deadline, async move {
        assert_eq!(read(&mut output).await?["kind"], "ready");
        write(&mut input, json!({"kind":"start","version":1,"arguments":{},"script":"return await agent('gate',{schema:{type:'object'}});"})).await?;
        assert_eq!(read(&mut output).await?["kind"], "group");
        drop(input);
        read(&mut output).await
    }, std::future::pending()).await?;
    let (outcome, exit) = confirmed(result);
    assert_eq!(outcome?["kind"], "failed");
    assert_ne!(exit, 0);
    Ok(())
}
