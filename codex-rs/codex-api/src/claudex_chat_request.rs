//! Fail-closed text/function mapping only; not a Responses-history migration or transport.
//! Historical reasoning is deliberately unsupported until native GLM replay is implemented.

use crate::ResponsesApiRequest;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeSeed;
use serde::de::MapAccess;
use serde::de::SeqAccess;
use serde::de::Visitor;
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::io::Write;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_FRAGMENT_BYTES: usize = 8 * 1024;
const MAX_TEXT_BYTES: usize = 48 * 1024;
const MAX_ITEMS: usize = 256;
const MAX_TOOLS: usize = 64;
const MAX_JSON_DEPTH: usize = 16;
const MAX_JSON_NODES: usize = 2048;

/// Fixed diagnostics never contain supplied instructions, arguments, or provider data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClaudexChatRequestError {
    #[error("unsupported Chat Completions request control")]
    UnsupportedControl,
    #[error("unsupported Chat Completions history item")]
    UnsupportedItem,
    #[error("unsupported Chat Completions function definition")]
    UnsupportedTool,
    #[error("invalid Chat Completions tool history")]
    InvalidHistory,
    #[error("invalid or unsupported bounded JSON")]
    InvalidJson,
    #[error("Chat Completions request budget exceeded")]
    LimitExceeded,
    #[error("Chat Completions output budget must be between 1 and 16384 tokens")]
    InvalidOutputBudget,
}

type Result<T> = std::result::Result<T, ClaudexChatRequestError>;

/// Immutable, validated GLM-5.3 wire request. No endpoint, authentication, or headers.
/// Function fixtures do not establish a working multi-turn loop: reasoning replay is refused.
#[derive(Debug, Serialize)]
pub struct ClaudexChatRequest {
    model: &'static str,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ChatTool>,
    tool_choice: &'static str,
    thinking: Thinking,
    reasoning_effort: &'static str,
    max_tokens: u32,
    stream: bool,
    tool_stream: bool,
}

#[derive(Debug, Serialize)]
struct Thinking {
    r#type: &'static str,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ChatCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl ChatMessage {
    fn text(role: &'static str, content: String) -> Self {
        Self {
            role,
            content,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct ChatCall {
    id: String,
    r#type: &'static str,
    function: ChatArguments,
}

#[derive(Debug, Serialize)]
struct ChatArguments {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct ChatTool {
    r#type: &'static str,
    function: ChatFunction,
}

#[derive(Debug, Serialize)]
struct ChatFunction {
    name: String,
    description: String,
    parameters: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseFunction {
    r#type: String,
    name: String,
    description: String,
    parameters: Box<RawValue>,
    #[serde(default)]
    strict: bool,
}

impl ClaudexChatRequest {
    /// Maps only explicitly supported controls and text/function items, preserving text bytes.
    /// Missing effort selects `low`; unsupported efforts are never coerced. Local message IDs
    /// and tool-output `success` metadata are not wire content in either protocol.
    /// All supplied fragments are bounded before copying; final escaped JSON is capped too.
    pub fn from_responses(request: &ResponsesApiRequest, max_output_tokens: u32) -> Result<Self> {
        if !(1..=16384).contains(&max_output_tokens) {
            return Err(ClaudexChatRequestError::InvalidOutputBudget);
        }
        validate_controls(request)?;
        let reasoning_effort = match request.reasoning.as_ref().and_then(|r| r.effort.as_ref()) {
            None | Some(ReasoningEffort::Low) => "low",
            Some(ReasoningEffort::High) => "high",
            Some(ReasoningEffort::Max) => "max",
            Some(
                ReasoningEffort::None
                | ReasoningEffort::Minimal
                | ReasoningEffort::Medium
                | ReasoningEffort::XHigh
                | ReasoningEffort::Ultra
                | ReasoningEffort::Persistent
                | ReasoningEffort::Custom(_),
            ) => return Err(ClaudexChatRequestError::UnsupportedControl),
        };
        let mut text_bytes = 0;
        let tools = map_tools(request, &mut text_bytes)?;
        let messages = map_messages(request, &tools, &mut text_bytes)?;
        let mapped = Self {
            model: "glm-5.3",
            messages,
            tools,
            tool_choice: "auto",
            thinking: Thinking { r#type: "enabled" },
            reasoning_effort,
            max_tokens: max_output_tokens,
            stream: true,
            tool_stream: true,
        };
        mapped.to_json_bytes()?;
        Ok(mapped)
    }

    /// Serializes through a fixed-capacity writer; no unbounded intermediate JSON string.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        let mut writer = BoundedWriter(Vec::with_capacity(MAX_REQUEST_BYTES));
        serde_json::to_writer(&mut writer, self)
            .map_err(|_| ClaudexChatRequestError::LimitExceeded)?;
        Ok(writer.0)
    }
}

fn validate_controls(request: &ResponsesApiRequest) -> Result<()> {
    if request.model != "glm-5.3"
        || request.tool_choice != "auto"
        || request.parallel_tool_calls
        || request.store
        || !request.stream
        || request.stream_options.is_some()
        || !request.include.is_empty()
        || request.service_tier.is_some()
        || request.prompt_cache_key.is_some()
        || request.text.is_some()
        || request.client_metadata.is_some()
        || request.access_programs.is_some()
        || request
            .reasoning
            .as_ref()
            .is_some_and(|r| r.summary.is_some() || r.context.is_some())
    {
        return Err(ClaudexChatRequestError::UnsupportedControl);
    }
    if request.input.len() > MAX_ITEMS {
        return Err(ClaudexChatRequestError::LimitExceeded);
    }
    Ok(())
}

fn add_text(target: &mut String, text: &str, total: &mut usize) -> Result<()> {
    if text.len() > MAX_FRAGMENT_BYTES
        || target.len() + text.len() > MAX_FRAGMENT_BYTES
        || *total + text.len() > MAX_TEXT_BYTES
    {
        return Err(ClaudexChatRequestError::LimitExceeded);
    }
    *total += text.len();
    target.push_str(text);
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn map_tools(request: &ResponsesApiRequest, total: &mut usize) -> Result<Vec<ChatTool>> {
    let Some(raw) = request
        .tools
        .as_ref()
        .map(|tools| tools.as_raw_value().get())
    else {
        return Ok(Vec::new());
    };
    if raw.len() > MAX_REQUEST_BYTES {
        return Err(ClaudexChatRequestError::LimitExceeded);
    }
    let functions: Vec<ResponseFunction> =
        serde_json::from_str(raw).map_err(|_| ClaudexChatRequestError::UnsupportedTool)?;
    if functions.len() > MAX_TOOLS {
        return Err(ClaudexChatRequestError::LimitExceeded);
    }
    let mut names = BTreeSet::new();
    let mut tools = Vec::with_capacity(functions.len());
    for function in functions {
        if function.r#type != "function"
            || function.strict
            || !valid_name(&function.name)
            || !names.insert(function.name.clone())
        {
            return Err(ClaudexChatRequestError::UnsupportedTool);
        }
        let parameters = bounded_json(function.parameters.get())?;
        if !parameters.is_object() {
            return Err(ClaudexChatRequestError::UnsupportedTool);
        }
        let mut description = String::new();
        add_text(&mut description, &function.description, total)?;
        tools.push(ChatTool {
            r#type: "function",
            function: ChatFunction {
                name: function.name,
                description,
                parameters: function.parameters,
            },
        });
    }
    Ok(tools)
}

fn map_messages(
    request: &ResponsesApiRequest,
    tools: &[ChatTool],
    total: &mut usize,
) -> Result<Vec<ChatMessage>> {
    let mut messages = Vec::with_capacity(request.input.len() + 1);
    if !request.instructions.is_empty() {
        let mut text = String::new();
        add_text(&mut text, &request.instructions, total)?;
        messages.push(ChatMessage::text("system", text));
    }
    let mut seen = BTreeSet::new();
    let mut pending = BTreeMap::new();
    let mut call_group = false;
    let mut call_group_bytes = 0;
    let mut has_user = false;
    for item in &request.input {
        match item {
            ResponseItem::Message {
                role,
                content,
                phase,
                internal_chat_message_metadata_passthrough,
                ..
            } => {
                if !pending.is_empty() {
                    return Err(ClaudexChatRequestError::InvalidHistory);
                }
                if phase.is_some() || internal_chat_message_metadata_passthrough.is_some() {
                    return Err(ClaudexChatRequestError::UnsupportedItem);
                }
                let role = match role.as_str() {
                    "system" => "system",
                    "user" => {
                        has_user = true;
                        "user"
                    }
                    "assistant" => "assistant",
                    _ => return Err(ClaudexChatRequestError::UnsupportedItem),
                };
                if content.len() > MAX_ITEMS {
                    return Err(ClaudexChatRequestError::LimitExceeded);
                }
                let mut text = String::new();
                for part in content {
                    match part {
                        ContentItem::InputText { text: part }
                        | ContentItem::OutputText { text: part } => {
                            add_text(&mut text, part, total)?
                        }
                        ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {
                            return Err(ClaudexChatRequestError::UnsupportedItem);
                        }
                    }
                }
                messages.push(ChatMessage::text(role, text));
                call_group = false;
            }
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                encrypted_function_args,
                call_id,
                internal_chat_message_metadata_passthrough,
                ..
            } => {
                if namespace.is_some()
                    || encrypted_function_args.is_some()
                    || internal_chat_message_metadata_passthrough.is_some()
                {
                    return Err(ClaudexChatRequestError::UnsupportedItem);
                }
                if !valid_name(name)
                    || call_id.is_empty()
                    || call_id.len() > 256
                    || call_id.chars().any(char::is_control)
                    || !seen.insert(call_id.clone())
                    || (!pending.is_empty() && !call_group)
                    || !tools.iter().any(|tool| tool.function.name == *name)
                {
                    return Err(ClaudexChatRequestError::InvalidHistory);
                }
                if !bounded_json(arguments)?.is_object() {
                    return Err(ClaudexChatRequestError::InvalidJson);
                }
                if !call_group {
                    call_group_bytes = 0;
                    messages.push(ChatMessage::text("assistant", String::new()));
                }
                call_group_bytes += arguments.len() + name.len() + call_id.len();
                if call_group_bytes > MAX_FRAGMENT_BYTES {
                    return Err(ClaudexChatRequestError::LimitExceeded);
                }
                let mut bounded_arguments = String::new();
                add_text(&mut bounded_arguments, arguments, total)?;
                let message = messages
                    .last_mut()
                    .ok_or(ClaudexChatRequestError::InvalidHistory)?;
                message.tool_calls.push(ChatCall {
                    id: call_id.clone(),
                    r#type: "function",
                    function: ChatArguments {
                        name: name.clone(),
                        arguments: bounded_arguments,
                    },
                });
                pending.insert(call_id.clone(), name.clone());
                call_group = true;
            }
            ResponseItem::FunctionCallOutput {
                call_id,
                name,
                namespace,
                output,
                internal_chat_message_metadata_passthrough,
                ..
            } => {
                if namespace.is_some() || internal_chat_message_metadata_passthrough.is_some() {
                    return Err(ClaudexChatRequestError::UnsupportedItem);
                }
                let id = call_id
                    .as_ref()
                    .ok_or(ClaudexChatRequestError::InvalidHistory)?;
                let expected = pending
                    .remove(id)
                    .ok_or(ClaudexChatRequestError::InvalidHistory)?;
                if name.as_ref().is_some_and(|name| *name != expected) {
                    return Err(ClaudexChatRequestError::InvalidHistory);
                }
                let mut text = String::new();
                match &output.body {
                    FunctionCallOutputBody::Text(part) => add_text(&mut text, part, total)?,
                    FunctionCallOutputBody::ContentItems(parts) => {
                        if parts.len() > MAX_ITEMS {
                            return Err(ClaudexChatRequestError::LimitExceeded);
                        }
                        for part in parts {
                            match part {
                                FunctionCallOutputContentItem::InputText { text: part } => {
                                    add_text(&mut text, part, total)?
                                }
                                FunctionCallOutputContentItem::InputImage { .. }
                                | FunctionCallOutputContentItem::InputAudio { .. }
                                | FunctionCallOutputContentItem::EncryptedContent { .. } => {
                                    return Err(ClaudexChatRequestError::UnsupportedItem);
                                }
                            }
                        }
                    }
                }
                let mut message = ChatMessage::text("tool", text);
                message.tool_call_id = Some(id.clone());
                messages.push(message);
                call_group = false;
            }
            ResponseItem::AdditionalTools { .. }
            | ResponseItem::AgentMessage { .. }
            | ResponseItem::Reasoning { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::CustomToolCall { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::ConfigurationUpdate { .. }
            | ResponseItem::CompactionTrigger { .. }
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::Other => return Err(ClaudexChatRequestError::UnsupportedItem),
        }
    }
    if !has_user || !pending.is_empty() {
        return Err(ClaudexChatRequestError::InvalidHistory);
    }
    Ok(messages)
}

fn bounded_json(raw: &str) -> Result<Value> {
    if raw.len() > MAX_FRAGMENT_BYTES {
        return Err(ClaudexChatRequestError::LimitExceeded);
    }
    let mut nodes = 0;
    let mut decoder = serde_json::Deserializer::from_str(raw);
    JsonGuard {
        depth: 0,
        nodes: &mut nodes,
    }
    .deserialize(&mut decoder)
    .map_err(|_| ClaudexChatRequestError::InvalidJson)?;
    decoder
        .end()
        .map_err(|_| ClaudexChatRequestError::InvalidJson)?;
    serde_json::from_str(raw).map_err(|_| ClaudexChatRequestError::InvalidJson)
}

// Validate depth, node count and duplicate keys before allocating a serde_json::Value tree.
struct JsonGuard<'a> {
    depth: usize,
    nodes: &'a mut usize,
}
impl<'de> DeserializeSeed<'de> for JsonGuard<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<(), D::Error> {
        *self.nodes += 1;
        if self.depth > MAX_JSON_DEPTH || *self.nodes > MAX_JSON_NODES {
            return Err(serde::de::Error::custom("bounded JSON limit"));
        }
        decoder.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for JsonGuard<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bounded JSON")
    }
    fn visit_bool<E>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E>(self, _: f64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_str<E>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<(), A::Error> {
        while sequence
            .next_element_seed(JsonGuard {
                depth: self.depth + 1,
                nodes: &mut *self.nodes,
            })?
            .is_some()
        {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(serde::de::Error::custom("duplicate JSON key"));
            }
            map.next_value_seed(JsonGuard {
                depth: self.depth + 1,
                nodes: &mut *self.nodes,
            })?;
        }
        Ok(())
    }
}

struct BoundedWriter(Vec<u8>);
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_REQUEST_BYTES - self.0.len() {
            return Err(io::Error::other("request size limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "claudex_chat_request_tests.rs"]
mod tests;
