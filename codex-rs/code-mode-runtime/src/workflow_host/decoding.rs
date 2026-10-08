//! Extract typed primitives without re-deserializing validated Value subtrees.
//! serde_json's private Number/RawValue keys must remain ordinary user data.
use serde_json::Map;
use serde_json::Value;

use super::codec::AgentCall;
use super::codec::Fault;
use super::codec::ParentMessage;
use super::codec::Result;

pub(super) fn message(value: Value) -> Result<ParentMessage> {
    let mut fields = object(value)?;
    let kind = string(&mut fields, "kind")?;
    let version = u8::try_from(unsigned(&mut fields, "version")?).map_err(|_| Fault::Protocol)?;
    let message = match kind.as_str() {
        "start" => ParentMessage::Start {
            version,
            script: string(&mut fields, "script")?,
            arguments: take(&mut fields, "arguments")?,
        },
        "group_result" => {
            let sequence = unsigned(&mut fields, "sequence")?;
            let Value::Array(values) = take(&mut fields, "values")? else {
                return Err(Fault::Protocol);
            };
            ParentMessage::GroupResult {
                version,
                sequence,
                values,
            }
        }
        "cancel" => ParentMessage::Cancel { version },
        _ => return Err(Fault::Protocol),
    };
    finish(fields)?;
    Ok(message)
}

pub(super) fn agent(value: Value) -> Result<AgentCall> {
    let mut fields = object(value)?;
    let name = string(&mut fields, "name")?;
    let prompt = string(&mut fields, "prompt")?;
    let phase = string(&mut fields, "phase")?;
    let role = match fields.remove("role") {
        None | Some(Value::Null) => None,
        Some(Value::String(role)) => Some(role),
        Some(_) => return Err(Fault::Protocol),
    };
    let schema = take(&mut fields, "schema")?;
    finish(fields)?;
    Ok(AgentCall {
        name,
        prompt,
        role,
        phase,
        schema,
    })
}

fn object(value: Value) -> Result<Map<String, Value>> {
    match value {
        Value::Object(fields) => Ok(fields),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            Err(Fault::Protocol)
        }
    }
}

fn take(fields: &mut Map<String, Value>, key: &str) -> Result<Value> {
    fields.remove(key).ok_or(Fault::Protocol)
}

fn string(fields: &mut Map<String, Value>, key: &str) -> Result<String> {
    match take(fields, key)? {
        Value::String(value) => Ok(value),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            Err(Fault::Protocol)
        }
    }
}

fn unsigned(fields: &mut Map<String, Value>, key: &str) -> Result<u64> {
    match take(fields, key)? {
        Value::Number(value) => value.as_u64().ok_or(Fault::Protocol),
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => {
            Err(Fault::Protocol)
        }
    }
}

fn finish(fields: Map<String, Value>) -> Result<()> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(Fault::Protocol)
    }
}
