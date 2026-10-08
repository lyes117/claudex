//! Conversational entrypoint to the same typed native caller as CLI workflow/start.
use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::protocol::WorkflowRunRequest;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub(crate) struct WorkflowHandler;
impl ToolExecutor<ToolInvocation> for WorkflowHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("Workflow")
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name:"Workflow".into(),
            description:"When the user requests a repository workflow, execute its ORIGINAL .claude/workflows script using this Workflow tool. Do not recreate its agents manually or substitute a workflow-codex/Node adapter. Execute only in that project native conversation (CLI --cwd selects the project); an outer-directory conversation is refused to preserve project instructions and permissions. Uses real native subagents, original French prompts and observable run status. args must be an object (or null), passed unchanged. model opus/sonnet/haiku explicitly use the captured Codex model with high/medium/low effort; exact native model IDs and explicit effort are supported. They never select Claude or Z.ai. schema is optional; structured schemas are validated locally without making optional fields required. Dates must be explicit arguments. Import, Node/shell APIs and nested workflows are refused. Children inherit native permissions and approval requirements. Run IDs must be unique. Pause/resume controls apply between child waves; a previous crash is not automatically replayed. Do not launch workflows whose network/publication effects have not been authorized.".into(),
            strict:false,defer_loading:None,
            parameters:JsonSchema::object(BTreeMap::from([
                ("scriptPath".into(),JsonSchema::string(Some("Script path within the captured working directory.".into()))),
                ("args".into(),JsonSchema::object(BTreeMap::new(),None,Some(true.into()))),
                ("runId".into(),JsonSchema::string(Some("Unique ASCII letters/digits/underscore/hyphen, 1..128 characters.".into()))),
            ]),Some(vec!["scriptPath".into(),"args".into(),"runId".into()]),Some(false.into())),output_schema:None,
        })
    }
    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            let ToolPayload::Function { arguments } = &invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "Workflow requires function arguments".into(),
                ));
            };
            if arguments.len() > 16 * 1024 {
                return Err(FunctionCallError::RespondToModel(
                    "Workflow argument budget exceeded".into(),
                ));
            }
            let request: WorkflowRunRequest = serde_json::from_str(arguments).map_err(|error| {
                FunctionCallError::RespondToModel(format!("Invalid Workflow request: {error}"))
            })?;
            let result = crate::agent::control::workflow_runner::run(
                invocation.session,
                invocation.step_context,
                request,
                invocation.cancellation_token,
            )
            .await;
            let success = result.is_ok();
            let text = match result {
                Ok(result) => result.to_string(),
                Err(error) => format!("Workflow failed: {error}"),
            };
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                text,
                Some(success),
            )))
        })
    }
}
impl CoreToolRuntime for WorkflowHandler {
    fn is_builtin_control_tool(&self) -> bool {
        true
    }
}
