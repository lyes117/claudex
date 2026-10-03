use codex_protocol::ThreadId;
use codex_protocol::ToolPolicySnapshot;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::io;

/// Canonical header identity and validated creation ceiling. Callers restoring
/// authority must compare `thread_id` with the immediate resumed/forked thread.
#[derive(Debug)]
pub struct SessionPolicyHeader {
    pub thread_id: ThreadId,
    pub tool_policy: Option<ToolPolicySnapshot>,
}

#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(rename = "type")]
    kind: String,
    #[serde(borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct Header<'a> {
    id: ThreadId,
    // Missing means legacy; explicit null must be validated, never downgraded.
    #[serde(default, borrow, deserialize_with = "present_policy")]
    tool_policy_snapshot: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct PolicyMarker<'a> {
    #[serde(default, borrow, deserialize_with = "present_policy")]
    tool_policy_snapshot: Option<&'a RawValue>,
}

fn present_policy<'de, D>(deserializer: D) -> Result<Option<&'de RawValue>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    <&RawValue>::deserialize(deserializer).map(Some)
}

/// Validate the original JSON before tolerant rollout readers can skip it.
/// Unknown ordinary records are allowed, but malformed JSON before the first
/// canonical header and invalid canonical metadata fail closed. Do not run this
/// against later inherited SessionMeta records: they do not own this rollout.
pub fn parse_session_policy_header(line: &str) -> io::Result<Option<SessionPolicyHeader>> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid canonical session header",
        )
    };
    let envelope: Envelope<'_> = serde_json::from_str(line).map_err(|_| invalid())?;
    if envelope.kind != "session_meta" {
        if let Some(payload) = envelope.payload
            && payload.get().starts_with('{')
        {
            let marker: PolicyMarker<'_> =
                serde_json::from_str(payload.get()).map_err(|_| invalid())?;
            if marker.tool_policy_snapshot.is_some() {
                return Err(invalid());
            }
        }
        return Ok(None);
    }
    let payload = envelope.payload.ok_or_else(invalid)?;
    let header: Header<'_> = serde_json::from_str(payload.get()).map_err(|_| invalid())?;
    let tool_policy = header
        .tool_policy_snapshot
        .map(|raw| ToolPolicySnapshot::from_json_slice(raw.get().as_bytes()))
        .transpose()
        .map_err(|_| invalid())?;
    // A valid policy in otherwise broken metadata must not permit ancestor
    // fallback. Reuse the normal decoder for every existing metadata field.
    let value = serde_json::from_str(line).map_err(|_| invalid())?;
    crate::recorder::reject_unknown_thread_history_mode(&value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let decoded = crate::decode_rollout_line(value).map_err(|_| invalid())?;
    let crate::RolloutItem::SessionMeta(meta) = decoded.item else {
        return Err(invalid());
    };
    if meta.meta.id != header.id {
        return Err(invalid());
    }
    Ok(Some(SessionPolicyHeader {
        thread_id: header.id,
        tool_policy,
    }))
}

#[cfg(test)]
#[path = "tool_policy_header_tests.rs"]
mod tests;
