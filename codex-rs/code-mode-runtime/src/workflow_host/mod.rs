//! Private workflow process. Must run under an externally enforced OS resource budget.
//! This entry point does not authenticate, create agents, or expose ordinary tools.
mod bootstrap;
mod callbacks;
mod codec;
mod import_preflight;
mod value;

use std::io::Read;
use std::io::Write;

use serde_json::json;

use codec::ChildMessage;
use codec::Fault;
use codec::ParentMessage;
use codec::Result;
use codec::VERSION;

struct PendingCall {
    call: codec::AgentCall,
    resolver: v8::Global<v8::PromiseResolver>,
}

#[derive(Default)]
struct State {
    pending: Vec<PendingCall>,
    phase: String,
    calls: usize,
    fault: Option<Fault>,
    finalizing: bool,
    logs: Vec<(String, String)>,
    log_count: usize,
}

/// Runs one bounded framed exchange; external supervision must enforce memory and time.
pub fn run_stdio() -> std::result::Result<(), &'static str> {
    run_stdio_mode(Mode::Execute)
}

/// Parses local source without evaluating it or initializing V8/agent callbacks.
pub fn run_preflight_stdio() -> std::result::Result<(), &'static str> {
    run_stdio_mode(Mode::Preflight)
}

enum Mode {
    Execute,
    Preflight,
}

fn run_stdio_mode(mode: Mode) -> std::result::Result<(), &'static str> {
    let input = std::io::stdin();
    let output = std::io::stdout();
    let mut input = input.lock();
    let mut output = output.lock();
    let result = match mode {
        Mode::Execute => run(&mut input, &mut output),
        Mode::Preflight => run_preflight(&mut input, &mut output),
    };
    match result {
        Ok(()) => Ok(()),
        Err(code) => {
            codec::write_frame(
                &mut output,
                &ChildMessage::Failed {
                    version: VERSION,
                    code,
                },
            )
            .map_err(|_| "private workflow host transport failed")?;
            Err("private workflow host failed")
        }
    }
}

fn run_preflight(input: &mut impl Read, output: &mut impl Write) -> Result<()> {
    codec::write_frame(output, &ChildMessage::Ready { version: VERSION })?;
    let start = codec::read_frame(input)?;
    codec::validate_start(&start)?;
    let ParentMessage::Start { script, .. } = start else {
        return Err(Fault::Protocol);
    };
    let prepared = import_preflight::prepare(&script)?;
    codec::write_frame(
        output,
        &ChildMessage::Done {
            version: VERSION,
            result: json!({"parseOnly":true,"v8Compiled":false,"sourceBytes":script.len(),"metaTokenRemoved":prepared!=script,"agentsAdmitted":0}),
        },
    )
}

fn run(input: &mut impl Read, output: &mut impl Write) -> Result<()> {
    codec::write_frame(output, &ChildMessage::Ready { version: VERSION })?;
    let start: ParentMessage = codec::read_frame(input)?;
    codec::validate_start(&start)?;
    let ParentMessage::Start {
        script, arguments, ..
    } = start
    else {
        return Err(Fault::Protocol);
    };
    let script = import_preflight::prepare(&script)?;
    crate::initialize_v8(crate::V8JitMode::Disabled).map_err(|_| Fault::Unsupported)?;
    let params = v8::CreateParams::default()
        .heap_limits(/*initial*/ 4 * 1024 * 1024, codec::HEAP_BYTES);
    let isolate = &mut v8::Isolate::new(params);
    isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
    isolate.set_allow_wasm_code_generation_callback(deny_wasm);
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    context.set_allow_generation_from_strings(false);
    let scope = &mut v8::ContextScope::new(scope, context);
    scope.set_slot(State {
        phase: "Workflow".to_string(),
        ..State::default()
    });
    let arguments = value::from_json(scope, &arguments)?;
    let key = v8::String::new(scope, "__workflowArguments").ok_or(Fault::Limit)?;
    if context.global(scope).set(scope, key.into(), arguments) != Some(true) {
        return Err(Fault::Script);
    }
    let agent = v8::Function::new(scope, callbacks::agent).ok_or(Fault::Limit)?;
    let phase = v8::Function::new(scope, callbacks::phase).ok_or(Fault::Limit)?;
    let log = v8::Function::new(scope, callbacks::log).ok_or(Fault::Limit)?;
    for (name, function) in [
        ("__workflowAgent", agent),
        ("__workflowPhase", phase),
        ("__workflowLog", log),
    ] {
        let key = v8::String::new(scope, name).ok_or(Fault::Limit)?;
        if context
            .global(scope)
            .set(scope, key.into(), function.into())
            != Some(true)
        {
            return Err(Fault::Script);
        }
    }
    // Compile the complete AST-preflighted source before any callback executes.
    // No dynamic import loader is installed; V8 remains the final syntax authority.
    let source = format!("{}\n(async () => {{\n{script}\n}})()", bootstrap::SOURCE);
    let text = v8::String::new(scope, &source).ok_or(Fault::Limit)?;
    let compiled = v8::Script::compile(scope, text, /*origin*/ None).ok_or(Fault::Script)?;
    let result = compiled.run(scope).ok_or(Fault::Script)?;
    let promise = v8::Local::<v8::Promise>::try_from(result).map_err(|_| Fault::Script)?;
    let promise = v8::Global::new(scope, promise);
    let mut sequence = 0;
    loop {
        scope.perform_microtask_checkpoint();
        if let Some(code) = scope.get_slot::<State>().and_then(|state| state.fault) {
            return Err(code);
        }
        // A script failure revokes its queued requests before any group is emitted.
        if v8::Local::new(scope, &promise).state() == v8::PromiseState::Rejected {
            return Err(Fault::Script);
        }
        let logs = std::mem::take(&mut scope.get_slot_mut::<State>().ok_or(Fault::Protocol)?.logs);
        for (phase, message) in logs {
            codec::write_frame(
                output,
                &ChildMessage::Log {
                    version: VERSION,
                    phase,
                    message,
                },
            )?;
        }
        let pending = std::mem::take(
            &mut scope
                .get_slot_mut::<State>()
                .ok_or(Fault::Protocol)?
                .pending,
        );
        if pending.is_empty() {
            let promise = v8::Local::new(scope, &promise);
            match promise.state() {
                v8::PromiseState::Fulfilled => {
                    scope
                        .get_slot_mut::<State>()
                        .ok_or(Fault::Protocol)?
                        .finalizing = true;
                    let result = promise.result(scope);
                    let result = if result.is_undefined() {
                        json!(null)
                    } else {
                        value::to_json(scope, result)?
                    };
                    // JSON.stringify invokes user getters/toJSON; no callback can
                    // create work after the root's result is being committed.
                    scope.perform_microtask_checkpoint();
                    let state = scope.get_slot::<State>().ok_or(Fault::Protocol)?;
                    if let Some(code) = state.fault {
                        return Err(code);
                    }
                    if !state.pending.is_empty() {
                        return Err(Fault::Script);
                    }
                    return codec::write_frame(
                        output,
                        &ChildMessage::Done {
                            version: VERSION,
                            result,
                        },
                    );
                }
                v8::PromiseState::Rejected => return Err(Fault::Script),
                v8::PromiseState::Pending => return Err(Fault::NoProgress),
            }
        }
        sequence += 1;
        if sequence > codec::GROUPS {
            return Err(Fault::Limit);
        }
        let phase = scope
            .get_slot::<State>()
            .ok_or(Fault::Protocol)?
            .phase
            .clone();
        let (calls, resolvers): (Vec<_>, Vec<_>) = pending
            .into_iter()
            .map(|pending| (pending.call, pending.resolver))
            .unzip();
        codec::validate_group(&phase, &calls)?;
        let expected = calls.len();
        codec::write_frame(
            output,
            &ChildMessage::Group {
                version: VERSION,
                sequence,
                phase,
                calls,
            },
        )?;
        let response: ParentMessage = codec::read_frame(input)?;
        let values = match response {
            ParentMessage::GroupResult {
                version: VERSION,
                sequence: received,
                values,
            } if received == sequence && values.len() == expected => values,
            ParentMessage::Cancel { version: VERSION } => return Err(Fault::Cancelled),
            ParentMessage::Start { .. }
            | ParentMessage::GroupResult { .. }
            | ParentMessage::Cancel { .. } => return Err(Fault::Protocol),
        };
        // Validate the entire group before any promise can observe a partial result.
        codec::encode(&values, codec::GROUP_RESULT_BYTES)?;
        for result in &values {
            codec::check_json(result)?;
            value::validate_exact_input_numbers(result)?;
        }
        for (resolver, result) in resolvers.into_iter().zip(values) {
            let result = value::from_json(scope, &result)?;
            let resolver = v8::Local::new(scope, resolver);
            if resolver.resolve(scope, result) != Some(true) {
                return Err(Fault::Script);
            }
        }
    }
}

// SAFETY: V8 calls this with its local context/string handles; neither is accessed
// or retained. Returning false unconditionally refuses code generation.
unsafe extern "C" fn deny_wasm(_: v8::Local<v8::Context>, _: v8::Local<v8::String>) -> bool {
    false
}
