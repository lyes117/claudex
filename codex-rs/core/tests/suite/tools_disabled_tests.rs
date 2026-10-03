//! Native registry enforcement with deliberately unconditional extension tools.
use super::*;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExecutorFuture;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use pretty_assertions::assert_eq;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use test_case::test_case;

struct FileSpies(Arc<AtomicUsize>);
struct FileSpy {
    name: &'static str,
    calls: Arc<AtomicUsize>,
}

impl ToolContributor for FileSpies {
    fn tools(
        &self,
        _: &ExtensionData,
        _: &ExtensionData,
    ) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
        ["Read", "Grep", "Glob"]
            .into_iter()
            .map(|name| {
                Arc::new(FileSpy {
                    name,
                    calls: Arc::clone(&self.0),
                }) as _
            })
            .collect()
    }
}
impl<'call> ToolExecutor<ToolCall<'call>> for FileSpy {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(self.name)
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(codex_tools::ResponsesApiTool {
            name: self.name.into(),
            description: "Synthetic file executor spy".into(),
            strict: false,
            parameters: codex_tools::JsonSchema::default(),
            output_schema: None,
            defer_loading: None,
        })
    }
    fn handle<'a>(&'a self, call: ToolCall<'call>) -> ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(async move {
            // A missing environment must not be the reason these calls are denied.
            assert_eq!(call.environments.len(), 1);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(JsonToolOutput::new(json!({ "spy": self.name }))) as Box<dyn ToolOutput>)
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[test_case(false, false; "ordinary_direct_thread")]
#[test_case(true, false; "disabled_direct_child")]
#[test_case(true, true; "disabled_code_only_child")]
async fn no_tools_ceiling_blocks_unadvertised_calls_before_extension_execution(
    disabled: bool,
    code_only: bool,
) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.tool_contributor(Arc::new(FileSpies(Arc::clone(&calls))));
    let fixture = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .with_model_info_override("test-gpt-5.1-codex", move |info| {
            if code_only {
                info.tool_mode = Some(codex_protocol::openai_models::ToolMode::CodeModeOnly);
            }
        })
        .with_config(move |config| {
            config.tools_enabled = !disabled;
            if code_only {
                config
                    .features
                    .enable(codex_features::Feature::CodeMode)
                    .expect("code mode");
            } else {
                config
                    .features
                    .disable(codex_features::Feature::CodeMode)
                    .expect("direct mode");
            }
        })
        .build_with_auto_env(&server)
        .await?;
    assert_eq!(fixture.codex.environment_selections().await.len(), 1);
    fixture
        .codex
        .thread_extension_data()
        .insert(ToolPolicy::default());
    let child = if disabled {
        let mut config = fixture.config.clone();
        config.tools_enabled = true;
        let mut options = StartThreadOptions::new(config);
        options.session_source = Some(SessionSource::Internal(
            InternalSessionSource::MemoryConsolidation,
        ));
        options.environments = Some(fixture.codex.environment_selections().await);
        Some(
            fixture
                .thread_manager
                .spawn_internal_session(fixture.session_configured.thread_id, options)
                .await?,
        )
    } else {
        None
    };
    let thread = child
        .as_ref()
        .map(|child| &child.thread)
        .unwrap_or(&fixture.codex);
    thread.thread_extension_data().insert(ToolPolicy::default());
    let mut events = vec![responses::ev_response_created("attempts")];
    for name in ["Read", "Grep", "Glob"] {
        events.push(responses::ev_function_call(name, name, "{}"));
    }
    if disabled {
        events.push(responses::ev_custom_tool_call(
            "cell",
            "exec",
            "text('should-not-run')",
        ));
    }
    events.push(responses::ev_completed("attempts"));
    let response = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(events),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Synthetic hostile calls".into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(thread, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    let requests = response.requests();
    assert_eq!(requests.len(), 2);
    let tools = requests[0].body_json()["tools"]
        .as_array()
        .expect("tools")
        .clone();
    if disabled {
        assert_eq!(tools, Vec::<serde_json::Value>::new());
    } else {
        for name in ["Read", "Grep", "Glob"] {
            assert!(tools.iter().any(|tool| tool["name"] == name));
        }
    }
    for name in ["Read", "Grep", "Glob"] {
        let output = requests[1].function_call_output(name)["output"]
            .as_str()
            .expect("output")
            .to_owned();
        if disabled {
            assert_eq!(output, format!("unsupported call: {name}"));
        } else {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&output)?,
                json!({ "spy": name })
            );
        }
    }
    if disabled {
        assert_eq!(
            requests[1].custom_tool_call_output("cell")["output"],
            "unsupported custom tool call: exec"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), if disabled { 0 } else { 3 });
    if let Some(child) = child {
        child.thread.shutdown_and_wait().await?;
    }
    fixture.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broker_ceiling_hides_and_rejects_nested_file_executors() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.tool_contributor(Arc::new(FileSpies(Arc::clone(&calls))));
    let fixture = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .with_model_info_override("test-gpt-5.1-codex", |info| {
            info.tool_mode = Some(codex_protocol::openai_models::ToolMode::CodeModeOnly);
        })
        .with_config(|config| {
            config
                .features
                .enable(codex_features::Feature::CodeMode)
                .expect("code mode");
        })
        .build_with_auto_env(&server)
        .await?;
    let mut options = StartThreadOptions::new(fixture.config.clone());
    options.environments = Some(fixture.codex.environment_selections().await);
    options
        .thread_extension_init
        .insert(SessionIsolation::Inherit);
    options.thread_extension_init.insert(ToolPolicy {
        allowed_tools: Some(vec![ToolName::plain("exec"), ToolName::plain("wait")]),
        ..Default::default()
    });
    let started = fixture.thread_manager.start_thread(options).await?;
    let response = responses::mount_sse_sequence(&server, vec![
        responses::sse(vec![responses::ev_response_created("broker"), responses::ev_custom_tool_call(
            "cell", "exec", r#"
const result = [];
for (const name of ['Read', 'Grep', 'Glob']) {
  let rejected = false;
  try { await tools[name]({}); } catch { rejected = true; }
  result.push({ name, rejected, callable: typeof tools[name] === 'function', listed: ALL_TOOLS.some(t => t.name === name) });
}
text(JSON.stringify(result));
"#), responses::ev_completed("broker")]),
        responses::sse(vec![responses::ev_completed("done")]),
    ]).await;
    started
        .thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Try hidden nested executors".into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(&started.thread, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let requests = response.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].body_json()["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect::<Vec<_>>(),
        vec!["exec", "wait"]
    );
    let output =
        super::super::code_mode::custom_tool_output_last_non_empty_text(&requests[1], "cell")
            .expect("broker result");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output)?,
        json!([
            { "name": "Read", "rejected": true, "callable": false, "listed": false },
            { "name": "Grep", "rejected": true, "callable": false, "listed": false },
            { "name": "Glob", "rejected": true, "callable": false, "listed": false },
        ])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    started.thread.shutdown_and_wait().await?;
    fixture.codex.shutdown_and_wait().await?;
    Ok(())
}
