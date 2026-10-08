use super::codec;
use super::codec::Fault;
use super::codec::Result;

pub(super) fn to_json(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> Result<serde_json::Value> {
    let text = v8::json::stringify(scope, value).ok_or(Fault::Script)?;
    // Inspect V8's UTF-8 length before making any Rust-owned copy.
    if text.utf8_length(scope) > codec::JSON_BYTES {
        return Err(Fault::Limit);
    }
    codec::json(&text.to_rust_string_lossy(scope))
}

pub(super) fn to_agent_json(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> Result<serde_json::Value> {
    let text = v8::json::stringify(scope, value).ok_or(Fault::Script)?;
    if text.utf8_length(scope) > codec::PROMPT_BYTES + codec::JSON_BYTES + 2048 {
        return Err(Fault::Limit);
    }
    codec::json_envelope(&text.to_rust_string_lossy(scope))
}

pub(super) fn from_json<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: &serde_json::Value,
) -> Result<v8::Local<'s, v8::Value>> {
    validate_exact_input_numbers(value)?;
    let bytes = codec::encode(value, codec::JSON_BYTES)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::Protocol)?;
    let text = v8::String::new(scope, text).ok_or(Fault::Limit)?;
    v8::json::parse(scope, text).ok_or(Fault::Protocol)
}

pub(super) fn validate_exact_input_numbers(value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::Number(number) => {
            let integer = number
                .as_i64()
                .is_some_and(|integer| integer.unsigned_abs() <= 9_007_199_254_740_991)
                || number
                    .as_u64()
                    .is_some_and(|integer| integer <= 9_007_199_254_740_991);
            if !integer {
                return Err(Fault::Unsupported);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_exact_input_numbers(value)?;
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_exact_input_numbers(value)?;
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) => {}
    }
    Ok(())
}
