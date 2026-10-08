//! Typed workflow dispatch; the native session owns execution and child lifetimes.
use super::thread_processor::ThreadRequestProcessor;
use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::WorkflowStartParams;
use codex_app_server_protocol::WorkflowStartResponse;
use codex_protocol::ThreadId;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::WorkflowRunRequest;

impl ThreadRequestProcessor {
    pub(crate) async fn workflow_start(
        &self,
        params: WorkflowStartParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        if params.run_id.is_empty()
            || params.run_id.len() > 128
            || !params
                .run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            || (!params.args.is_object() && !params.args.is_null())
            || params.script_path.as_os_str().is_empty()
            || params.script_path.as_os_str().len() > 4096
            || serde_json::to_vec(&params.args).map_or(true, |bytes| bytes.len() > 65536)
        {
            return Err(invalid_params(
                "Invalid or excessive native workflow request",
            ));
        }
        let thread_id = ThreadId::from_string(&params.thread_id)
            .map_err(|_| invalid_params("Invalid workflow thread ID"))?;
        let thread = self
            .thread_manager
            .get_thread(thread_id)
            .await
            .map_err(|_| invalid_params("Workflow thread is not loaded"))?;
        super::thread_input::ensure_direct_input_allowed(thread.as_ref()).await?;
        let run_id = params.run_id.clone();
        let submission_id = thread
            .submit(Op::RunWorkflow {
                request: WorkflowRunRequest {
                    script_path: params.script_path,
                    args: params.args,
                    run_id: params.run_id,
                },
            })
            .await
            .map_err(|_| internal_error("Could not submit native workflow"))?;
        Ok(Some(
            WorkflowStartResponse {
                run_id,
                submission_id,
            }
            .into(),
        ))
    }
}
