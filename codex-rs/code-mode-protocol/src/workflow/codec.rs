//! Fixed, length-prefixed private protocol; never registered as a public tool.
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::io::Write;

#[path = "strict_json.rs"]
mod strict_json;
pub const VERSION: u8 = 1;
pub const FRAME_BYTES: usize = 1024 * 1024;
pub const SCRIPT_BYTES: usize = 512 * 1024;
pub const JSON_BYTES: usize = 8192;
pub const GROUP_RESULT_BYTES: usize = 128 * 1024;
pub const PROMPT_BYTES: usize = 8192;
pub const GROUPS: u64 = 64;
pub const LOGICAL_GROUP_CALLS: usize = 64;
pub const TOTAL_CALLS: usize = 1000;
pub const HEAP_BYTES: usize = 64 * 1024 * 1024;
pub type Result<T> = std::result::Result<T, Fault>;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    Protocol,
    Limit,
    Script,
    NoProgress,
    Cancelled,
    Unsupported,
}

#[derive(Debug, Serialize)]
pub struct AgentCall {
    pub name: String,
    pub prompt: String,
    pub role: Option<String>,
    pub phase: String,
    pub schema: Value,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParentMessage {
    Start {
        version: u8,
        script: String,
        arguments: Value,
    },
    GroupResult {
        version: u8,
        sequence: u64,
        values: Vec<Value>,
    },
    Cancel {
        version: u8,
    },
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChildMessage {
    Log {
        version: u8,
        phase: String,
        message: String,
    },
    Ready {
        version: u8,
    },
    Group {
        version: u8,
        sequence: u64,
        phase: String,
        calls: Vec<AgentCall>,
    },
    Done {
        version: u8,
        result: Value,
    },
    Failed {
        version: u8,
        code: Fault,
    },
}

pub fn decode(bytes: &[u8]) -> Result<ParentMessage> {
    if bytes.is_empty() || bytes.len() > FRAME_BYTES {
        return Err(Fault::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::Protocol)?;
    let value = strict_json::parse_envelope(text)?;
    super::decoding::message(value)
}
pub fn json_envelope(text: &str) -> Result<Value> {
    if text.len() > FRAME_BYTES {
        return Err(Fault::Limit);
    }
    strict_json::parse_envelope(text)
}
pub fn json(text: &str) -> Result<Value> {
    if text.len() > JSON_BYTES {
        return Err(Fault::Limit);
    }
    strict_json::parse(text)
}
pub fn decode_agent_value(value: Value) -> Result<AgentCall> {
    super::decoding::agent(value)
}
pub fn decode_child(bytes: &[u8]) -> Result<ChildMessage> {
    if bytes.is_empty() || bytes.len() > FRAME_BYTES {
        return Err(Fault::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::Protocol)?;
    super::decoding::child(strict_json::parse_envelope(text)?)
}
pub fn check_json(value: &Value) -> Result<()> {
    let bytes = encode(value, JSON_BYTES)?;
    json(std::str::from_utf8(&bytes).map_err(|_| Fault::Protocol)?)?;
    Ok(())
}
pub fn validate_start(message: &ParentMessage) -> Result<()> {
    let ParentMessage::Start {
        version,
        script,
        arguments,
    } = message
    else {
        return Err(Fault::Protocol);
    };
    if *version != VERSION {
        return Err(Fault::Protocol);
    }
    if script.is_empty() || script.len() > SCRIPT_BYTES {
        return Err(Fault::Limit);
    }
    check_json(arguments)
}
pub fn validate_group(phase: &str, calls: &[AgentCall]) -> Result<()> {
    if phase.is_empty()
        || phase.len() > 256
        || calls.is_empty()
        || calls.len() > LOGICAL_GROUP_CALLS
    {
        return Err(Fault::Limit);
    }
    let mut names = std::collections::HashSet::new();
    for call in calls {
        if call.name.is_empty()
            || call.name.len() > 128
            || !names.insert(&call.name)
            || call.prompt.is_empty()
            || call.prompt.len() > PROMPT_BYTES
            || call.phase.is_empty()
            || call.phase.len() > 256
            || call
                .role
                .as_ref()
                .is_some_and(|role| role.is_empty() || role.len() > 128)
        {
            return Err(Fault::Limit);
        }
        check_json(&call.schema)?;
        if !call.schema.is_null() && !call.schema.is_object() {
            return Err(Fault::Protocol);
        }
        if call
            .model
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
            || call.effort.as_ref().is_some_and(|value| {
                !["none", "minimal", "low", "medium", "high", "xhigh"].contains(&value.as_str())
            })
        {
            return Err(Fault::Protocol);
        }
    }
    Ok(())
}

struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("bounded encoding"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn encode<T: Serialize + ?Sized>(value: &T, limit: usize) -> Result<Vec<u8>> {
    let mut output = BoundedBytes {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| Fault::Limit)?;
    Ok(output.bytes)
}
pub fn write_frame<T: Serialize>(output: &mut impl Write, value: &T) -> Result<()> {
    let bytes = encode(value, FRAME_BYTES)?;
    output
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(|_| Fault::Protocol)?;
    output.write_all(&bytes).map_err(|_| Fault::Protocol)?;
    output.flush().map_err(|_| Fault::Protocol)
}
pub fn read_frame(input: &mut impl Read) -> Result<ParentMessage> {
    let mut prefix = [0; 4];
    input.read_exact(&mut prefix).map_err(|_| Fault::Protocol)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > FRAME_BYTES {
        return Err(Fault::Limit);
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes).map_err(|_| Fault::Protocol)?;
    decode(&bytes)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
