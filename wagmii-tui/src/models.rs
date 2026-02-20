//! API request/response models for `Wagmii` and OpenAI-compatible endpoints.

use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 128_000;
pub const DEFAULT_COMPACTION_TOKEN_THRESHOLD: usize = 50_000;
pub const DEFAULT_COMPACTION_MESSAGE_THRESHOLD: usize = 50;
const COMPACTION_THRESHOLD_PERCENT: u32 = 80;
const COMPACTION_MESSAGE_DIVISOR: u32 = 1200;
const MAX_COMPACTION_MESSAGE_THRESHOLD: usize = 150;

// === Core Message Types ===

/// Request payload for sending a message to the API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
}

/// System prompt representation (plain text or structured blocks).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum SystemPrompt {
    Text(String),
    Blocks(Vec<SystemBlock>),
}

/// A structured system prompt block.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SystemBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// A chat message with role and content blocks.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

/// A single content block inside a message.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<ToolCaller>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        content_blocks: Option<Vec<serde_json::Value>>,
    },
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_search_tool_result")]
    ToolSearchToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
    #[serde(rename = "code_execution_tool_result")]
    CodeExecutionToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
}

/// Cache control metadata for tool definitions and blocks.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub cache_type: String,
}

/// Metadata describing who invoked a tool call.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCaller {
    #[serde(rename = "type")]
    pub caller_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
}

/// Tool definition exposed to the model.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tool {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<String>,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_callers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_examples: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// Container metadata for code-execution style server tools.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContainerInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Server-side tool usage counters.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ServerToolUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_execution_requests: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_search_requests: Option<u32>,
}

/// Response payload for a message request.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageResponse {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerInfo>,
    pub usage: Usage,
}

/// Token usage metadata for a response.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_tool_use: Option<ServerToolUsage>,
}

/// Map known models to their approximate context window sizes.
#[must_use]
pub fn context_window_for_model(model: &str) -> Option<u32> {
    let lower = model.to_lowercase();
    if lower.contains("wagmii-v3.2") {
        return Some(DEFAULT_CONTEXT_WINDOW_TOKENS);
    }
    if lower.contains("wagmii-chat")
        || lower.contains("wagmii-reasoner")
        || lower.contains("wagmii-r1")
    {
        return Some(DEFAULT_CONTEXT_WINDOW_TOKENS);
    }
    if lower.contains("wagmii") {
        return Some(DEFAULT_CONTEXT_WINDOW_TOKENS);
    }
    if lower.contains("claude") {
        return Some(200_000);
    }
    None
}

/// Derive a compaction token threshold from model context window.
///
/// Keeps headroom for tool outputs and assistant completion by defaulting to 80%
/// of known context windows.
#[must_use]
pub fn compaction_threshold_for_model(model: &str) -> usize {
    let Some(window) = context_window_for_model(model) else {
        return DEFAULT_COMPACTION_TOKEN_THRESHOLD;
    };

    let threshold = (u64::from(window) * u64::from(COMPACTION_THRESHOLD_PERCENT)) / 100;
    usize::try_from(threshold).unwrap_or(DEFAULT_COMPACTION_TOKEN_THRESHOLD)
}

/// Derive a compaction message-count threshold from model context window.
#[must_use]
pub fn compaction_message_threshold_for_model(model: &str) -> usize {
    let Some(window) = context_window_for_model(model) else {
        return DEFAULT_COMPACTION_MESSAGE_THRESHOLD;
    };

    let scaled = usize::try_from(window / COMPACTION_MESSAGE_DIVISOR)
        .unwrap_or(DEFAULT_COMPACTION_MESSAGE_THRESHOLD);
    scaled.clamp(
        DEFAULT_COMPACTION_MESSAGE_THRESHOLD,
        MAX_COMPACTION_MESSAGE_THRESHOLD,
    )
}

// === Streaming Structures ===

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
/// Streaming event types for SSE responses.
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageResponse },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: ContentBlockStart,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: Delta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDelta,
        usage: Option<Usage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
/// Content block types used in streaming starts.
pub enum ContentBlockStart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value, // usually empty or partial
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<ToolCaller>,
    },
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

// Variant names match legacy streaming spec, suppressing style warning
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
/// Delta events emitted during streaming responses.
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
/// Delta payload for message-level updates.
pub struct MessageDelta {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wagmii_models_map_to_128k_context_window() {
        assert_eq!(
            context_window_for_model("wagmii-reasoner"),
            Some(DEFAULT_CONTEXT_WINDOW_TOKENS)
        );
        assert_eq!(
            context_window_for_model("wagmii-v3.2"),
            Some(DEFAULT_CONTEXT_WINDOW_TOKENS)
        );
        assert_eq!(
            context_window_for_model("wagmii-v3.2-0324"),
            Some(DEFAULT_CONTEXT_WINDOW_TOKENS)
        );
    }

    #[test]
    fn compaction_threshold_scales_with_context_window() {
        assert_eq!(compaction_threshold_for_model("wagmii-reasoner"), 102_400);
        assert_eq!(compaction_threshold_for_model("unknown-model"), 50_000);
    }

    #[test]
    fn compaction_message_threshold_scales_with_context_window() {
        assert_eq!(
            compaction_message_threshold_for_model("wagmii-reasoner"),
            106
        );
        assert_eq!(compaction_message_threshold_for_model("unknown-model"), 50);
        assert_eq!(compaction_message_threshold_for_model("claude-3"), 150);
    }
}
