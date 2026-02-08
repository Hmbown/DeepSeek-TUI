//! HTTP client for the DeepSeek OpenAI-compatible APIs.
//!
//! Uses the OpenAI Responses API when available, falling back to Chat Completions
//! if the Responses endpoint is unsupported by the target base URL.

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::config::{Config, RetryPolicy};
use crate::llm_client::{LlmClient, StreamEventBox};
use crate::logging;
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, Message, MessageDelta, MessageRequest, MessageResponse,
    StreamEvent, SystemPrompt, Tool, Usage,
};

fn to_api_tool_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if ch == '-' {
            out.push_str("--");
        } else {
            out.push_str("-x");
            out.push_str(&format!("{:06X}", ch as u32));
            out.push('-');
        }
    }
    out
}

fn from_api_tool_name(name: &str) -> String {
    let mut out = String::new();
    let mut iter = name.chars().peekable();
    while let Some(ch) = iter.next() {
        if ch != '-' {
            out.push(ch);
            continue;
        }
        if let Some('-') = iter.peek().copied() {
            iter.next();
            out.push('-');
            continue;
        }
        if iter.peek().copied() == Some('x') {
            iter.next();
            let mut hex = String::new();
            for _ in 0..6 {
                if let Some(h) = iter.next() {
                    hex.push(h);
                } else {
                    break;
                }
            }
            if let Ok(code) = u32::from_str_radix(&hex, 16)
                && let Some(decoded) = std::char::from_u32(code)
            {
                if let Some('-') = iter.peek().copied() {
                    iter.next();
                }
                out.push(decoded);
                continue;
            }
            out.push('-');
            out.push('x');
            out.push_str(&hex);
            continue;
        }
        out.push('-');
    }
    out
}

// === Types ===

/// Client for DeepSeek's OpenAI-compatible APIs.
#[must_use]
pub struct DeepSeekClient {
    http_client: reqwest::Client,
    base_url: String,
    retry: RetryPolicy,
    default_model: String,
    use_chat_completions: AtomicBool,
    /// Counter of chat-completions requests since last Responses API probe.
    /// After RESPONSES_RECOVERY_INTERVAL requests, we retry the Responses API.
    chat_fallback_counter: AtomicU32,
}

/// After this many chat-completions requests, retry the Responses API to see
/// if it has recovered.
const RESPONSES_RECOVERY_INTERVAL: u32 = 20;

impl Clone for DeepSeekClient {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            base_url: self.base_url.clone(),
            retry: self.retry.clone(),
            default_model: self.default_model.clone(),
            use_chat_completions: AtomicBool::new(
                self.use_chat_completions.load(Ordering::Relaxed),
            ),
            chat_fallback_counter: AtomicU32::new(
                self.chat_fallback_counter.load(Ordering::Relaxed),
            ),
        }
    }
}

// === DeepSeekClient ===

impl DeepSeekClient {
    /// Create a DeepSeek client from CLI configuration.
    pub fn new(config: &Config) -> Result<Self> {
        let api_key = config.deepseek_api_key()?;
        let base_url = config.deepseek_base_url();
        let retry = config.retry_policy();
        let default_model = config
            .default_text_model
            .clone()
            .unwrap_or_else(|| "deepseek-v3.2".to_string());

        logging::info(format!("DeepSeek base URL: {base_url}"));
        logging::info(format!(
            "Retry policy: enabled={}, max_retries={}, initial_delay={}s, max_delay={}s",
            retry.enabled, retry.max_retries, retry.initial_delay, retry.max_delay
        ));

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {api_key}"))?,
        );

        let http_client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            http_client,
            base_url,
            retry,
            default_model,
            use_chat_completions: AtomicBool::new(false),
            chat_fallback_counter: AtomicU32::new(0),
        })
    }

    async fn create_message_responses(
        &self,
        request: &MessageRequest,
    ) -> Result<Result<MessageResponse, ResponsesFallback>> {
        let mut body = json!({
            "model": request.model,
            "input": build_responses_input(&request.messages),
            "store": false,
            "max_output_tokens": request.max_tokens,
        });

        if let Some(instructions) = system_to_instructions(request.system.clone()) {
            body["instructions"] = json!(instructions);
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(tools) = request.tools.as_ref() {
            body["tools"] = json!(tools.iter().map(tool_to_responses).collect::<Vec<_>>());
        }
        if let Some(choice) = request.tool_choice.as_ref() {
            body["tool_choice"] = choice.clone();
        }

        let url = format!("{}/v1/responses", self.base_url.trim_end_matches('/'));
        let response =
            send_with_retry(&self.retry, || self.http_client.post(&url).json(&body)).await?;

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();

        if status.as_u16() == 404 || status.as_u16() == 405 {
            return Ok(Err(ResponsesFallback {
                status: status.as_u16(),
                body: response_text,
            }));
        }

        if !status.is_success() {
            anyhow::bail!("Failed to call DeepSeek Responses API: HTTP {status}: {response_text}");
        }

        let value: Value =
            serde_json::from_str(&response_text).context("Failed to parse Responses API JSON")?;
        let message = parse_responses_message(&value)?;
        Ok(Ok(message))
    }

    async fn create_message_chat(&self, request: &MessageRequest) -> Result<MessageResponse> {
        let messages =
            build_chat_messages(request.system.as_ref(), &request.messages, &request.model);
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens,
        });

        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(tools) = request.tools.as_ref() {
            body["tools"] = json!(tools.iter().map(tool_to_chat).collect::<Vec<_>>());
        }
        if let Some(choice) = request.tool_choice.as_ref() {
            if let Some(mapped) = map_tool_choice_for_chat(choice) {
                body["tool_choice"] = mapped;
            }
        }

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let response =
            send_with_retry(&self.retry, || self.http_client.post(&url).json(&body)).await?;

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Failed to call DeepSeek Chat API: HTTP {status}: {response_text}");
        }

        let value: Value =
            serde_json::from_str(&response_text).context("Failed to parse Chat API JSON")?;
        parse_chat_message(&value)
    }
}

// === Trait Implementations ===

impl LlmClient for DeepSeekClient {
    fn provider_name(&self) -> &'static str {
        "deepseek"
    }

    fn model(&self) -> &str {
        &self.default_model
    }

    async fn create_message(&self, request: MessageRequest) -> Result<MessageResponse> {
        // Check if it's time to probe Responses API recovery
        if self.use_chat_completions.load(Ordering::Relaxed) {
            let count = self.chat_fallback_counter.fetch_add(1, Ordering::Relaxed);
            if count > 0 && count % RESPONSES_RECOVERY_INTERVAL == 0 {
                logging::info("Probing Responses API recovery...");
                let request_clone = request.clone();
                match self.create_message_responses(&request).await? {
                    Ok(message) => {
                        logging::info("Responses API recovered! Switching back.");
                        self.use_chat_completions.store(false, Ordering::Relaxed);
                        self.chat_fallback_counter.store(0, Ordering::Relaxed);
                        return Ok(message);
                    }
                    Err(_) => {
                        logging::info("Responses API still unavailable, continuing with chat.");
                    }
                }
                return self.create_message_chat(&request_clone).await;
            }
            return self.create_message_chat(&request).await;
        }

        let request_clone = request.clone();
        match self.create_message_responses(&request).await? {
            Ok(message) => Ok(message),
            Err(fallback) => {
                logging::warn(format!(
                    "Responses API unavailable (HTTP {}). Falling back to chat completions.",
                    fallback.status
                ));
                logging::info(format!(
                    "Responses fallback body: {}",
                    crate::utils::truncate_with_ellipsis(&fallback.body, 500, "...")
                ));
                self.use_chat_completions.store(true, Ordering::Relaxed);
                self.chat_fallback_counter.store(0, Ordering::Relaxed);
                self.create_message_chat(&request_clone).await
            }
        }
    }

    async fn create_message_stream(&self, request: MessageRequest) -> Result<StreamEventBox> {
        // Try true SSE streaming via chat completions (widely supported)
        let messages =
            build_chat_messages(request.system.as_ref(), &request.messages, &request.model);
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(tools) = request.tools.as_ref() {
            body["tools"] = json!(tools.iter().map(tool_to_chat).collect::<Vec<_>>());
        }
        if let Some(choice) = request.tool_choice.as_ref() {
            if let Some(mapped) = map_tool_choice_for_chat(choice) {
                body["tool_choice"] = mapped;
            }
        }

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let response =
            send_with_retry(&self.retry, || self.http_client.post(&url).json(&body)).await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("SSE stream request failed: HTTP {status}: {error_text}");
        }

        let model = request.model.clone();
        let byte_stream = response.bytes_stream();

        let stream = async_stream::stream! {
            use futures_util::StreamExt;

            // Emit a synthetic MessageStart
            yield Ok(StreamEvent::MessageStart {
                message: MessageResponse {
                    id: String::new(),
                    r#type: "message".to_string(),
                    role: "assistant".to_string(),
                    content: Vec::new(),
                    model: model.clone(),
                    stop_reason: None,
                    stop_sequence: None,
                    usage: Usage { input_tokens: 0, output_tokens: 0 },
                },
            });

            let mut line_buf = String::new();
            let mut byte_buf = Vec::new();
            let mut content_index: u32 = 0;
            let mut text_started = false;
            let mut thinking_started = false;
            let mut tool_indices: std::collections::HashMap<u32, bool> = std::collections::HashMap::new();
            let is_reasoning_model = requires_reasoning_content(&model);

            let mut byte_stream = std::pin::pin!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        yield Err(anyhow::anyhow!("Stream read error: {e}"));
                        break;
                    }
                };

                byte_buf.extend_from_slice(&chunk);

                // Process complete SSE lines from the buffer
                loop {
                    let buf_str = String::from_utf8_lossy(&byte_buf);
                    let Some(newline_pos) = buf_str.find('\n') else { break };
                    let line: String = buf_str[..newline_pos].trim_end_matches('\r').to_string();
                    let consumed = newline_pos + 1;
                    byte_buf = byte_buf[consumed..].to_vec();

                    if line.is_empty() {
                        // Empty line = event boundary, process accumulated data
                        if !line_buf.is_empty() {
                            let data = std::mem::take(&mut line_buf);
                            if data.trim() == "[DONE]" {
                                // Stream complete
                            } else if let Ok(chunk_json) = serde_json::from_str::<Value>(&data) {
                                // Parse the SSE chunk into stream events
                                for event in parse_sse_chunk(
                                    &chunk_json,
                                    &mut content_index,
                                    &mut text_started,
                                    &mut thinking_started,
                                    &mut tool_indices,
                                    is_reasoning_model,
                                ) {
                                    yield Ok(event);
                                }
                            }
                        }
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        line_buf.push_str(data);
                    }
                    // Ignore other SSE fields (event:, id:, retry:)
                }
            }

            // Close any open blocks
            if thinking_started {
                yield Ok(StreamEvent::ContentBlockStop { index: content_index.saturating_sub(1) });
            }
            if text_started {
                yield Ok(StreamEvent::ContentBlockStop { index: content_index.saturating_sub(1) });
            }

            yield Ok(StreamEvent::MessageStop);
        };

        Ok(Pin::from(Box::new(stream)
            as Box<
                dyn futures_util::Stream<Item = Result<StreamEvent>> + Send,
            >))
    }
}

// === Responses API Helpers ===

#[derive(Debug)]
struct ResponsesFallback {
    status: u16,
    body: String,
}

fn system_to_instructions(system: Option<SystemPrompt>) -> Option<String> {
    match system {
        Some(SystemPrompt::Text(text)) => Some(text),
        Some(SystemPrompt::Blocks(blocks)) => {
            let joined = blocks
                .into_iter()
                .map(|b| b.text)
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            if joined.trim().is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        None => None,
    }
}

fn build_responses_input(messages: &[Message]) -> Vec<Value> {
    let mut items = Vec::new();

    for message in messages {
        let role = message.role.as_str();
        let text_type = if role == "user" {
            "input_text"
        } else {
            "output_text"
        };

        for block in &message.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    items.push(json!({
                        "type": "message",
                        "role": role,
                        "content": [{
                            "type": text_type,
                            "text": text,
                        }]
                    }));
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let args = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
                    items.push(json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": to_api_tool_name(name),
                        "arguments": args,
                    }));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    items.push(json!({
                        "type": "function_call_output",
                        "call_id": tool_use_id,
                        "output": content,
                    }));
                }
                ContentBlock::Thinking { .. } => {}
            }
        }
    }

    items
}

fn tool_to_responses(tool: &Tool) -> Value {
    json!({
        "type": "function",
        "name": to_api_tool_name(&tool.name),
        "description": tool.description,
        "parameters": tool.input_schema,
    })
}

fn parse_responses_message(payload: &Value) -> Result<MessageResponse> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("response")
        .to_string();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let usage = parse_usage(payload.get("usage"));
    let mut content = Vec::new();

    if let Some(output) = payload.get("output").and_then(Value::as_array) {
        for item in output {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(role) = item.get("role").and_then(Value::as_str)
                        && role != "assistant"
                    {
                        continue;
                    }
                    if let Some(content_items) = item.get("content").and_then(Value::as_array) {
                        for content_item in content_items {
                            let content_type = content_item
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("output_text");
                            if content_type != "output_text" && content_type != "text" {
                                continue;
                            }
                            if let Some(text) = content_item.get("text").and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    content.push(ContentBlock::Text {
                                        text: text.to_string(),
                                        cache_control: None,
                                    });
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let call_id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string();
                    let input = match item.get("arguments") {
                        Some(Value::String(raw)) => {
                            serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()))
                        }
                        Some(other) => other.clone(),
                        None => Value::Null,
                    };
                    content.push(ContentBlock::ToolUse {
                        id: call_id,
                        name: from_api_tool_name(&name),
                        input,
                    });
                }
                "reasoning" => {
                    if let Some(summary) = item.get("summary").and_then(Value::as_array) {
                        let summary_text = summary
                            .iter()
                            .filter_map(|s| s.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !summary_text.trim().is_empty() {
                            content.push(ContentBlock::Thinking {
                                thinking: summary_text,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if content.is_empty() {
        if let Some(text) = payload.get("output_text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                content.push(ContentBlock::Text {
                    text: text.to_string(),
                    cache_control: None,
                });
            }
        }
    }

    Ok(MessageResponse {
        id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model,
        stop_reason: None,
        stop_sequence: None,
        usage,
    })
}

// === Chat Completions Helpers ===

fn build_chat_messages(
    system: Option<&SystemPrompt>,
    messages: &[Message],
    model: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    let include_reasoning = requires_reasoning_content(model);
    let mut pending_tool_calls: HashSet<String> = HashSet::new();

    if let Some(instructions) = system_to_instructions(system.cloned()) {
        if !instructions.trim().is_empty() {
            out.push(json!({
                "role": "system",
                "content": instructions,
            }));
        }
    }

    for message in messages {
        let role = message.role.as_str();
        let mut text_parts = Vec::new();
        let mut thinking_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_ids = Vec::new();
        let mut tool_results: Vec<(String, Value)> = Vec::new();

        for block in &message.content {
            match block {
                ContentBlock::Text { text, .. } => text_parts.push(text.clone()),
                ContentBlock::Thinking { thinking } => thinking_parts.push(thinking.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    let args = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": to_api_tool_name(name),
                            "arguments": args,
                        }
                    }));
                    tool_call_ids.push(id.clone());
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    tool_results.push((
                        tool_use_id.clone(),
                        json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content,
                        }),
                    ));
                }
            }
        }

        if role == "assistant" {
            let content = text_parts.join("\n");
            let mut msg = json!({
                "role": "assistant",
                "content": if content.is_empty() { Value::Null } else { json!(content) },
            });
            if include_reasoning {
                msg["reasoning_content"] = json!(thinking_parts.join("\n"));
            }
            if !tool_calls.is_empty() {
                msg["tool_calls"] = json!(tool_calls);
                pending_tool_calls = tool_call_ids.into_iter().collect();
            } else {
                pending_tool_calls.clear();
            }
            out.push(msg);
        } else if role == "user" {
            let content = text_parts.join("\n");
            if !content.trim().is_empty() {
                out.push(json!({
                    "role": "user",
                    "content": content,
                }));
            }
        }

        if !tool_results.is_empty() {
            if pending_tool_calls.is_empty() {
                logging::warn("Dropping tool results without matching tool_calls");
            } else {
                for (tool_id, tool_msg) in tool_results {
                    if pending_tool_calls.remove(&tool_id) {
                        out.push(tool_msg);
                    } else {
                        logging::warn(format!(
                            "Dropping tool result for unknown tool_call_id: {tool_id}"
                        ));
                    }
                }
            }
        } else if role != "assistant" {
            pending_tool_calls.clear();
        }
    }

    // Safety net: after compaction, an assistant message may have tool_calls
    // whose results were summarized away. The API rejects these, so strip
    // the tool_calls (downgrading to a plain assistant message) and remove
    // the now-orphaned tool result messages.
    let mut i = 0;
    while i < out.len() {
        let is_assistant_with_tools = out[i].get("role").and_then(Value::as_str)
            == Some("assistant")
            && out[i].get("tool_calls").is_some();

        if is_assistant_with_tools {
            let expected_ids: HashSet<String> = out[i]
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|c| c.get("id").and_then(Value::as_str).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            // Collect tool result IDs immediately following this assistant message.
            let mut found_ids: HashSet<String> = HashSet::new();
            let mut tool_result_end = i + 1;
            while tool_result_end < out.len() {
                if out[tool_result_end].get("role").and_then(Value::as_str) == Some("tool") {
                    if let Some(id) = out[tool_result_end]
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                    {
                        found_ids.insert(id.to_string());
                    }
                    tool_result_end += 1;
                } else {
                    break;
                }
            }

            // Also scan non-contiguous tool results up to the next assistant message
            // in case compaction left gaps.
            let mut scan = tool_result_end;
            while scan < out.len() {
                if out[scan].get("role").and_then(Value::as_str) == Some("assistant") {
                    break;
                }
                if out[scan].get("role").and_then(Value::as_str) == Some("tool") {
                    if let Some(id) = out[scan].get("tool_call_id").and_then(Value::as_str) {
                        found_ids.insert(id.to_string());
                    }
                }
                scan += 1;
            }

            if !expected_ids.is_subset(&found_ids) {
                let missing: Vec<_> = expected_ids.difference(&found_ids).collect();
                logging::warn(format!(
                    "Stripping orphaned tool_calls from assistant message \
                     (expected {} tool results, found {}, missing: {:?})",
                    expected_ids.len(),
                    found_ids.len(),
                    missing
                ));
                if let Some(obj) = out[i].as_object_mut() {
                    obj.remove("tool_calls");
                }
                // Remove contiguous tool results first
                if tool_result_end > i + 1 {
                    out.drain((i + 1)..tool_result_end);
                }
                // Remove any remaining non-contiguous tool results referencing expected_ids
                // (scan backward to avoid index shifting issues)
                let mut j = out.len();
                while j > i + 1 {
                    j -= 1;
                    if out[j].get("role").and_then(Value::as_str) == Some("tool") {
                        if let Some(id) = out[j].get("tool_call_id").and_then(Value::as_str) {
                            if expected_ids.contains(id) {
                                out.remove(j);
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }

    out
}

fn tool_to_chat(tool: &Tool) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": to_api_tool_name(&tool.name),
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

fn map_tool_choice_for_chat(choice: &Value) -> Option<Value> {
    if let Some(choice_str) = choice.as_str() {
        return Some(json!(choice_str));
    }
    let Some(choice_type) = choice.get("type").and_then(Value::as_str) else {
        return Some(choice.clone());
    };

    match choice_type {
        "auto" | "none" => Some(json!(choice_type)),
        "any" => Some(json!("auto")),
        "tool" => choice.get("name").and_then(Value::as_str).map(|name| {
            json!({
                "type": "function",
                "function": { "name": to_api_tool_name(name) }
            })
        }),
        _ => Some(choice.clone()),
    }
}

fn requires_reasoning_content(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.contains("deepseek-reasoner")
        || lower.contains("deepseek-r1")
        || lower.contains("deepseek-v3.2")
        || lower.contains("reasoner")
}

fn parse_chat_message(payload: &Value) -> Result<MessageResponse> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl")
        .to_string();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let choices = payload
        .get("choices")
        .and_then(Value::as_array)
        .context("Chat API response missing choices")?;
    let choice = choices
        .get(0)
        .context("Chat API response missing first choice")?;
    let message = choice
        .get("message")
        .context("Chat API response missing message")?;

    let mut content_blocks = Vec::new();
    if let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str) {
        if !reasoning.trim().is_empty() {
            content_blocks.push(ContentBlock::Thinking {
                thinking: reasoning.to_string(),
            });
        }
    }
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.trim().is_empty() {
            content_blocks.push(ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            });
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string();
            let function = call.get("function");
            let name = function
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let arguments = function
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .map(|raw| serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string())))
                .unwrap_or(Value::Null);

            content_blocks.push(ContentBlock::ToolUse {
                id,
                name: from_api_tool_name(&name),
                input: arguments,
            });
        }
    }

    let usage = parse_usage(payload.get("usage"));

    Ok(MessageResponse {
        id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content: content_blocks,
        model,
        stop_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        stop_sequence: None,
        usage,
    })
}

fn parse_usage(usage: Option<&Value>) -> Usage {
    let input_tokens = usage
        .and_then(|u| u.get("input_tokens").or_else(|| u.get("prompt_tokens")))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| {
            u.get("output_tokens")
                .or_else(|| u.get("completion_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Usage {
        input_tokens: input_tokens as u32,
        output_tokens: output_tokens as u32,
    }
}

// === Streaming Helpers ===

/// Build synthetic stream events from a non-streaming response (used as fallback).
#[allow(dead_code)]
fn build_stream_events(response: &MessageResponse) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    let mut index = 0u32;

    events.push(StreamEvent::MessageStart {
        message: response.clone(),
    });

    for block in &response.content {
        match block {
            ContentBlock::Text { text, .. } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::Text {
                        text: String::new(),
                    },
                });
                if !text.is_empty() {
                    events.push(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::TextDelta { text: text.clone() },
                    });
                }
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::Thinking { thinking } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::Thinking {
                        thinking: String::new(),
                    },
                });
                if !thinking.is_empty() {
                    events.push(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::ThinkingDelta {
                            thinking: thinking.clone(),
                        },
                    });
                }
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::ToolUse { id, name, input } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                });
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::ToolResult { .. } => {}
        }
        index = index.saturating_add(1);
    }

    events.push(StreamEvent::MessageDelta {
        delta: MessageDelta {
            stop_reason: response.stop_reason.clone(),
            stop_sequence: response.stop_sequence.clone(),
        },
        usage: Some(response.usage.clone()),
    });
    events.push(StreamEvent::MessageStop);

    events
}

// === SSE Chunk Parser ===

/// Parse a single SSE chunk from the Chat Completions streaming API into
/// our internal `StreamEvent` representation.
fn parse_sse_chunk(
    chunk: &Value,
    content_index: &mut u32,
    text_started: &mut bool,
    thinking_started: &mut bool,
    tool_indices: &mut std::collections::HashMap<u32, bool>,
    is_reasoning_model: bool,
) -> Vec<StreamEvent> {
    let mut events = Vec::new();

    let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
        // Usage-only chunk (sent at end with stream_options)
        if let Some(usage_val) = chunk.get("usage") {
            let usage = parse_usage(Some(usage_val));
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: None,
                    stop_sequence: None,
                },
                usage: Some(usage),
            });
        }
        return events;
    };

    for choice in choices {
        let delta = choice.get("delta");
        let finish_reason = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string);

        if let Some(delta) = delta {
            // Handle reasoning_content (DeepSeek-Reasoner thinking)
            if is_reasoning_model {
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
                    if !reasoning.is_empty() {
                        if !*thinking_started {
                            events.push(StreamEvent::ContentBlockStart {
                                index: *content_index,
                                content_block: ContentBlockStart::Thinking {
                                    thinking: String::new(),
                                },
                            });
                            *thinking_started = true;
                        }
                        events.push(StreamEvent::ContentBlockDelta {
                            index: *content_index,
                            delta: Delta::ThinkingDelta {
                                thinking: reasoning.to_string(),
                            },
                        });
                    }
                }
            }

            // Handle regular content
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                if !content.is_empty() {
                    // Close thinking block if transitioning to text
                    if *thinking_started {
                        events.push(StreamEvent::ContentBlockStop {
                            index: *content_index,
                        });
                        *content_index += 1;
                        *thinking_started = false;
                    }
                    if !*text_started {
                        events.push(StreamEvent::ContentBlockStart {
                            index: *content_index,
                            content_block: ContentBlockStart::Text {
                                text: String::new(),
                            },
                        });
                        *text_started = true;
                    }
                    events.push(StreamEvent::ContentBlockDelta {
                        index: *content_index,
                        delta: Delta::TextDelta {
                            text: content.to_string(),
                        },
                    });
                }
            }

            // Handle tool calls
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    let tc_index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;

                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        tool_indices.entry(tc_index)
                    {
                        // Close text block if transitioning to tool use
                        if *text_started {
                            events.push(StreamEvent::ContentBlockStop {
                                index: *content_index,
                            });
                            *content_index += 1;
                            *text_started = false;
                        }
                        if *thinking_started {
                            events.push(StreamEvent::ContentBlockStop {
                                index: *content_index,
                            });
                            *content_index += 1;
                            *thinking_started = false;
                        }

                        let id = tc
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("tool_call")
                            .to_string();
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();

                        entry.insert(true);
                        events.push(StreamEvent::ContentBlockStart {
                            index: *content_index,
                            content_block: ContentBlockStart::ToolUse {
                                id,
                                name: from_api_tool_name(&name),
                                input: json!({}),
                            },
                        });
                    }

                    // Stream tool call arguments
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                    {
                        if !args.is_empty() {
                            events.push(StreamEvent::ContentBlockDelta {
                                index: *content_index,
                                delta: Delta::InputJsonDelta {
                                    partial_json: args.to_string(),
                                },
                            });
                        }
                    }
                }
            }
        }

        // Handle finish reason
        if let Some(reason) = finish_reason {
            // Close any open blocks
            if *text_started {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
                *text_started = false;
            }
            if *thinking_started {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
                *thinking_started = false;
            }
            // Close tool blocks
            for _ in tool_indices.drain() {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
            }

            // Emit usage from the chunk if available
            let chunk_usage = chunk.get("usage").map(|u| parse_usage(Some(u)));
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some(reason),
                    stop_sequence: None,
                },
                usage: chunk_usage,
            });
        }
    }

    events
}

// === Retry Helpers ===

async fn send_with_retry<F>(policy: &RetryPolicy, mut build: F) -> Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let mut attempt: u32 = 0;

    loop {
        let result = build().send().await;

        match result {
            Ok(response) => {
                let status = response.status();

                // Return successful responses immediately
                if status.is_success() {
                    return Ok(response);
                }

                // Return non-retryable errors to let caller handle (e.g., 404 for fallback)
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if !retryable {
                    return Ok(response);
                }

                // Retry if policy allows and we haven't exceeded max retries
                if !policy.enabled || attempt >= policy.max_retries {
                    return Ok(response);
                }

                logging::warn(format!(
                    "Retryable HTTP {} (attempt {} of {})",
                    status.as_u16(),
                    attempt + 1,
                    policy.max_retries + 1
                ));
            }
            Err(err) => {
                if !policy.enabled || attempt >= policy.max_retries {
                    return Err(err.into());
                }
                logging::warn(format!(
                    "Request error: {} (attempt {} of {})",
                    err,
                    attempt + 1,
                    policy.max_retries + 1
                ));
            }
        }

        let delay = policy.delay_for_attempt(attempt);
        attempt += 1;
        logging::info(format!("Retrying after {:.2}s", delay.as_secs_f64()));
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_messages_include_reasoning_content_for_reasoner() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "plan".to_string(),
                },
                ContentBlock::Text {
                    text: "done".to_string(),
                    cache_control: None,
                },
            ],
        };
        let out = build_chat_messages(None, &[message], "deepseek-reasoner");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("plan")
        );
    }

    #[test]
    fn chat_messages_skip_reasoning_content_for_chat_model() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "deepseek-chat");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert!(assistant.get("reasoning_content").is_none());
    }

    #[test]
    fn chat_messages_drop_orphan_tool_results() {
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: "ok".to_string(),
            }],
        }];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        assert!(
            !out.iter()
                .any(|value| { value.get("role").and_then(Value::as_str) == Some("tool") })
        );
    }

    #[test]
    fn chat_messages_include_tool_results_when_call_present() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "list_dir".to_string(),
                    input: json!({}),
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "ok".to_string(),
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        assert!(
            out.iter()
                .any(|value| { value.get("role").and_then(Value::as_str) == Some("tool") })
        );
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert!(assistant.get("tool_calls").is_some());
    }

    #[test]
    fn chat_messages_encode_tool_call_names() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "web.run".to_string(),
                    input: json!({}),
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "ok".to_string(),
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        let tool_calls = assistant
            .get("tool_calls")
            .and_then(Value::as_array)
            .expect("tool_calls array");
        let function_name = tool_calls
            .first()
            .and_then(|call| call.get("function"))
            .and_then(|func| func.get("name"))
            .and_then(Value::as_str)
            .expect("tool call function name");

        assert_eq!(function_name, to_api_tool_name("web.run"));
    }

    #[test]
    fn chat_messages_strips_orphaned_tool_calls_after_compaction() {
        // Simulates post-compaction state: assistant has tool_calls but the
        // tool result messages were summarized away.
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-orphan".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path": "src/main.rs"}),
                }],
            },
            // No tool result follows — it was removed by compaction.
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "continue".to_string(),
                    cache_control: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        // The safety net should have stripped tool_calls.
        assert!(
            assistant.get("tool_calls").is_none(),
            "orphaned tool_calls should be stripped by safety net"
        );
    }

    #[test]
    fn chat_messages_keeps_valid_tool_calls_intact() {
        // Complete call+result pair should NOT be stripped.
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-ok".to_string(),
                    name: "list_dir".to_string(),
                    input: json!({}),
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-ok".to_string(),
                    content: "files".to_string(),
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert!(
            assistant.get("tool_calls").is_some(),
            "valid tool_calls should remain intact"
        );
        assert!(
            out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("tool")),
            "tool result should remain"
        );
    }

    #[test]
    fn chat_messages_strips_partial_tool_results() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: "t1".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "a.rs"}),
                    },
                    ContentBlock::ToolUse {
                        id: "t2".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "b.rs"}),
                    },
                    ContentBlock::ToolUse {
                        id: "t3".to_string(),
                        name: "shell".to_string(),
                        input: json!({"cmd": "ls"}),
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "content a".to_string(),
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t2".to_string(),
                    content: "content b".to_string(),
                }],
            },
            // No result for t3
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "continue".to_string(),
                    cache_control: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|v| v.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert!(
            assistant.get("tool_calls").is_none(),
            "partial tool_calls should be stripped"
        );
        assert!(
            !out.iter()
                .any(|v| v.get("role").and_then(Value::as_str) == Some("tool")),
            "all orphaned tool results should be removed"
        );
    }
}
