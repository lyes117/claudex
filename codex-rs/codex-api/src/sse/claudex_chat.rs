//! Bounded Chat Completions decoding for the optional Claudex GLM route.
//! This decoder does not establish subscription eligibility or execute tools.
use crate::common::ResponseEvent;
use crate::error::ApiError;
use codex_protocol::ResponseItemId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

const MAX_FRAME_BYTES: usize = 65_536;
const MAX_STREAM_BYTES: usize = 262_144;
const MAX_FRAMES: usize = 2048;
const MAX_TOOL_CALLS: usize = 16;
const MAX_OUTPUT_BYTES: usize = 8192;
const MAX_ARGUMENT_BYTES: usize = 1024;

#[derive(Deserialize)]
struct Frame {
    id: String,
    model: String,
    choices: Vec<Choice>,
    usage: Option<Usage>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    index: u32,
    delta: Delta,
    finish_reason: Option<String>,
    logprobs: Option<serde_json::Value>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Delta {
    role: Option<String>,
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolDelta>>,
    refusal: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolDelta {
    index: usize,
    id: Option<String>,
    r#type: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    prompt_tokens_details: Option<PromptDetails>,
    completion_tokens_details: Option<CompletionDetails>,
}

#[derive(Deserialize)]
struct PromptDetails {
    cached_tokens: Option<i64>,
}

#[derive(Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<i64>,
}

#[derive(Default)]
struct PendingTool {
    id: String,
    name: String,
    arguments: String,
    function_type_seen: bool,
}

/// One response, one choice, bounded output, with terminal confirmation required.
/// The caller must bound raw SSE framing and apply its own network deadline.
pub struct ClaudexChatDecoder {
    allowed_tools: BTreeSet<String>,
    response_id: Option<String>,
    content: String,
    reasoning: String,
    reasoning_closed: bool,
    tools: BTreeMap<usize, PendingTool>,
    finish_reason: Option<String>,
    usage: Option<TokenUsage>,
    bytes: usize,
    frames: usize,
    terminated: bool,
    poisoned: bool,
}

impl ClaudexChatDecoder {
    /// Captures the exact advertised function names, never permissions from the model.
    pub fn new(allowed_tools: BTreeSet<String>) -> Result<Self, ApiError> {
        if allowed_tools.len() > 32 || allowed_tools.iter().any(|name| !valid_name(name)) {
            return Err(invalid_stream());
        }
        Ok(Self {
            allowed_tools,
            response_id: None,
            content: String::new(),
            reasoning: String::new(),
            reasoning_closed: false,
            tools: BTreeMap::new(),
            finish_reason: None,
            usage: None,
            bytes: 0,
            frames: 0,
            terminated: false,
            poisoned: false,
        })
    }

    /// Consumes an already framed SSE `data` field. Errors contain no provider body.
    pub fn push_data(&mut self, data: &str) -> Result<Vec<ResponseEvent>, ApiError> {
        if self.terminated || self.poisoned {
            return Err(invalid_stream());
        }
        self.bytes = self.bytes.saturating_add(data.len());
        self.frames = self.frames.saturating_add(1);
        let result = if data.len() > MAX_FRAME_BYTES
            || self.bytes > MAX_STREAM_BYTES
            || self.frames > MAX_FRAMES
        {
            Err(invalid_stream())
        } else if data == "[DONE]" {
            self.complete()
        } else {
            self.decode_frame(data)
        };
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    /// EOF alone never turns a truncated response into a successful response.
    pub fn finish_eof(&self) -> Result<(), ApiError> {
        if self.terminated && !self.poisoned {
            Ok(())
        } else {
            Err(invalid_stream())
        }
    }

    fn decode_frame(&mut self, data: &str) -> Result<Vec<ResponseEvent>, ApiError> {
        let frame: Frame = serde_json::from_str(data).map_err(|_| invalid_stream())?;
        if frame.error.is_some() || frame.model != "glm-5.3" || !valid_id(&frame.id) {
            return Err(invalid_stream());
        }
        let mut events = Vec::new();
        match self.response_id.as_ref() {
            Some(id) if *id != frame.id => return Err(invalid_stream()),
            Some(_) => {}
            None => {
                self.response_id = Some(frame.id.clone());
                events.push(ResponseEvent::Created {
                    response_id: Some(frame.id),
                });
            }
        }
        if let Some(usage) = frame.usage {
            if self.usage.is_some()
                || (!frame.choices.is_empty()
                    && frame
                        .choices
                        .iter()
                        .any(|choice| choice.finish_reason.is_none()))
            {
                return Err(invalid_stream());
            }
            self.usage = Some(validate_usage(usage)?);
        }
        if frame.choices.is_empty() {
            if self.finish_reason.is_none() || self.usage.is_none() {
                return Err(invalid_stream());
            }
            return Ok(events);
        }
        if frame.choices.len() != 1 || self.finish_reason.is_some() {
            return Err(invalid_stream());
        }
        let choice = frame
            .choices
            .into_iter()
            .next()
            .ok_or_else(invalid_stream)?;
        if choice.index != 0
            || choice.logprobs.is_some()
            || choice.delta.refusal.is_some()
            || choice
                .delta
                .role
                .as_deref()
                .is_some_and(|role| role != "assistant")
        {
            return Err(invalid_stream());
        }
        if let Some(delta) = choice.delta.reasoning_content.filter(|s| !s.is_empty()) {
            if self.reasoning_closed || !self.content.is_empty() {
                return Err(invalid_stream());
            }
            if self.reasoning.is_empty() {
                events.push(ResponseEvent::OutputItemAdded(reasoning_item(
                    self.response_id.as_deref().ok_or_else(invalid_stream)?,
                    String::new(),
                )));
            }
            self.reasoning.push_str(&delta);
            events.push(ResponseEvent::ReasoningContentDelta {
                delta,
                content_index: 0,
            });
        }
        if let Some(delta) = choice.delta.content.filter(|s| !s.is_empty()) {
            if self.content.is_empty() {
                if !self.reasoning.is_empty() {
                    events.push(ResponseEvent::OutputItemDone(reasoning_item(
                        self.response_id.as_deref().ok_or_else(invalid_stream)?,
                        self.reasoning.clone(),
                    )));
                    self.reasoning_closed = true;
                }
                events.push(ResponseEvent::OutputItemAdded(message_item(
                    self.response_id.as_deref().ok_or_else(invalid_stream)?,
                    String::new(),
                )));
            }
            self.content.push_str(&delta);
            events.push(ResponseEvent::OutputTextDelta(delta));
        }
        for delta in choice.delta.tool_calls.unwrap_or_default() {
            if delta.index >= MAX_TOOL_CALLS {
                return Err(invalid_stream());
            }
            let pending = self.tools.entry(delta.index).or_default();
            if let Some(id) = delta.id {
                if !pending.id.is_empty() || !valid_id(&id) {
                    return Err(invalid_stream());
                }
                pending.id = id;
            }
            if let Some(kind) = delta.r#type {
                if pending.function_type_seen || kind != "function" {
                    return Err(invalid_stream());
                }
                pending.function_type_seen = true;
            }
            if let Some(function) = delta.function {
                if let Some(name) = function.name {
                    pending.name.push_str(&name);
                    if pending.name.len() > 64 {
                        return Err(invalid_stream());
                    }
                }
                if let Some(arguments) = function.arguments {
                    pending.arguments.push_str(&arguments);
                    if pending.arguments.len() > MAX_ARGUMENT_BYTES {
                        return Err(invalid_stream());
                    }
                }
            }
        }
        let tool_bytes: usize = self.tools.values().map(|tool| tool.arguments.len()).sum();
        if self.content.len() + self.reasoning.len() + tool_bytes > MAX_OUTPUT_BYTES {
            return Err(invalid_stream());
        }
        if let Some(reason) = choice.finish_reason {
            if !matches!(reason.as_str(), "stop" | "tool_calls") {
                return Err(invalid_stream());
            }
            self.finish_reason = Some(reason);
        }
        Ok(events)
    }

    fn complete(&mut self) -> Result<Vec<ResponseEvent>, ApiError> {
        let id = self.response_id.as_ref().ok_or_else(invalid_stream)?;
        let reason = self.finish_reason.as_deref().ok_or_else(invalid_stream)?;
        if (reason == "tool_calls") == self.tools.is_empty()
            || (self.content.is_empty() && self.tools.is_empty())
        {
            return Err(invalid_stream());
        }
        let mut seen_ids = BTreeSet::new();
        let mut events = Vec::new();
        if !self.reasoning.is_empty() && !self.reasoning_closed {
            events.push(ResponseEvent::OutputItemDone(reasoning_item(
                id,
                self.reasoning.clone(),
            )));
        }
        if !self.content.is_empty() {
            events.push(ResponseEvent::OutputItemDone(message_item(
                id,
                self.content.clone(),
            )));
        }
        for pending in self.tools.values() {
            let arguments: serde_json::Value =
                serde_json::from_str(&pending.arguments).map_err(|_| invalid_stream())?;
            if !valid_id(&pending.id)
                || !seen_ids.insert(pending.id.as_str())
                || !pending.function_type_seen
                || !self.allowed_tools.contains(&pending.name)
                || !arguments.is_object()
                || !bounded_json(&arguments, 0)
            {
                return Err(invalid_stream());
            }
            let item = ResponseItem::FunctionCall {
                id: Some(ResponseItemId::with_suffix("glm_call", &pending.id)),
                name: pending.name.clone(),
                namespace: None,
                arguments: pending.arguments.clone(),
                encrypted_function_args: None,
                call_id: pending.id.clone(),
                internal_chat_message_metadata_passthrough: None,
            };
            events.push(ResponseEvent::OutputItemAdded(item.clone()));
            events.push(ResponseEvent::OutputItemDone(item));
        }
        events.push(ResponseEvent::Completed {
            response_id: id.clone(),
            token_usage: self.usage.clone(),
            usage_metadata: None,
            end_turn: Some(reason == "stop"),
        });
        self.terminated = true;
        Ok(events)
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_graphic())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn bounded_json(value: &serde_json::Value, depth: usize) -> bool {
    depth <= 12
        && match value {
            serde_json::Value::Array(values) => {
                values.len() <= 256 && values.iter().all(|v| bounded_json(v, depth + 1))
            }
            serde_json::Value::Object(values) => {
                values.len() <= 256 && values.values().all(|v| bounded_json(v, depth + 1))
            }
            _ => true,
        }
}

fn validate_usage(usage: Usage) -> Result<TokenUsage, ApiError> {
    let cached = usage
        .prompt_tokens_details
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);
    let reasoning = usage
        .completion_tokens_details
        .and_then(|d| d.reasoning_tokens)
        .unwrap_or(0);
    if usage.prompt_tokens < 0
        || usage.completion_tokens < 0
        || cached < 0
        || reasoning < 0
        || cached > usage.prompt_tokens
        || reasoning > usage.completion_tokens
        || usage.prompt_tokens.checked_add(usage.completion_tokens) != Some(usage.total_tokens)
    {
        return Err(invalid_stream());
    }
    Ok(TokenUsage {
        input_tokens: usage.prompt_tokens,
        cached_input_tokens: cached,
        cache_write_input_tokens: 0,
        output_tokens: usage.completion_tokens,
        reasoning_output_tokens: reasoning,
        total_tokens: usage.total_tokens,
        codex_rollout_budget_units: None,
    })
}

fn message_item(id: &str, text: String) -> ResponseItem {
    ResponseItem::Message {
        id: Some(ResponseItemId::with_suffix("glm_msg", id)),
        role: "assistant".to_owned(),
        content: vec![ContentItem::OutputText { text }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn reasoning_item(id: &str, text: String) -> ResponseItem {
    ResponseItem::Reasoning {
        id: Some(ResponseItemId::with_suffix("glm_reasoning", id)),
        summary: Vec::new(),
        content: Some(vec![ReasoningItemContent::ReasoningText { text }]),
        encrypted_content: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn invalid_stream() -> ApiError {
    ApiError::Stream("invalid or incomplete GLM chat stream".to_owned())
}

#[cfg(test)]
#[path = "claudex_chat_tests.rs"]
mod tests;
