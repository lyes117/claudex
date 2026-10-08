use super::PendingCall;
use super::State;
use super::codec;
use super::codec::Fault;
use super::value;

pub(super) fn agent(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut returned: v8::ReturnValue<v8::Value>,
) {
    let result = (|| {
        if scope.get_slot::<State>().ok_or(Fault::Protocol)?.finalizing {
            return Err(Fault::Script);
        }
        let value = value::to_agent_json(scope, args.get(0))?;
        let call = codec::decode_agent_value(value)?;
        codec::validate_group("agent", std::slice::from_ref(&call))?;
        let resolver = v8::PromiseResolver::new(scope).ok_or(Fault::Limit)?;
        let promise = resolver.get_promise(scope);
        let resolver = v8::Global::new(scope, resolver);
        let state = scope.get_slot_mut::<State>().ok_or(Fault::Protocol)?;
        if state.pending.len() >= codec::LOGICAL_GROUP_CALLS || state.calls >= codec::TOTAL_CALLS {
            return Err(Fault::Limit);
        }
        state.calls += 1;
        state.pending.push(PendingCall { call, resolver });
        returned.set(promise.into());
        Ok(())
    })();
    if let Err(code) = result {
        fail(scope, code);
    }
}

pub(super) fn phase(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut returned: v8::ReturnValue<v8::Value>,
) {
    let result = (|| {
        if scope.get_slot::<State>().ok_or(Fault::Protocol)?.finalizing {
            return Err(Fault::Script);
        }
        let value = args.get(0);
        if !value.is_string() {
            return Err(Fault::Protocol);
        }
        let text = value.to_string(scope).ok_or(Fault::Script)?;
        if text.utf8_length(scope) > 256 {
            return Err(Fault::Limit);
        }
        let text = text.to_rust_string_lossy(scope);
        if text.is_empty() {
            return Err(Fault::Limit);
        }
        scope.get_slot_mut::<State>().ok_or(Fault::Protocol)?.phase = text;
        returned.set(v8::undefined(scope).into());
        Ok(())
    })();
    if let Err(code) = result {
        fail(scope, code);
    }
}

pub(super) fn log(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut returned: v8::ReturnValue<v8::Value>,
) {
    let result = (|| {
        let value = args.get(0);
        if !value.is_string() {
            return Err(Fault::Protocol);
        }
        let text = value.to_string(scope).ok_or(Fault::Script)?;
        if text.utf8_length(scope) > codec::JSON_BYTES {
            return Err(Fault::Limit);
        }
        let message = text.to_rust_string_lossy(scope);
        let state = scope.get_slot_mut::<State>().ok_or(Fault::Protocol)?;
        if state.finalizing || state.logs.len() >= 64 || state.log_count >= 1000 {
            return Err(Fault::Limit);
        }
        state.log_count += 1;
        state.logs.push((state.phase.clone(), message));
        returned.set(v8::undefined(scope).into());
        Ok(())
    })();
    if let Err(code) = result {
        fail(scope, code);
    }
}

fn fail(scope: &mut v8::PinScope<'_, '_>, code: Fault) {
    if let Some(state) = scope.get_slot_mut::<State>() {
        state.fault = Some(code);
    }
    if let Some(message) = v8::String::new(scope, "workflow operation refused") {
        scope.throw_exception(message.into());
    }
}
