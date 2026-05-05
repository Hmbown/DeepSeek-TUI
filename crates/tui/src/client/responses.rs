//! Responses API helpers for the experimental DeepSeek endpoint.
//!
//! Gated behind `DEEPSEEK_EXPERIMENTAL_RESPONSES_API`. Normal traffic uses
//! chat completions via `crate::client::chat`.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::models::{ContentBlock, Message, MessageRequest, MessageResponse, Tool, ToolCaller};

use super::{
    DeepSeekClient, ERROR_BODY_MAX_BYTES, api_url, apply_reasoning_effort, bounded_error_text,
    from_api_tool_name, parse_usage, system_to_instructions, to_api_tool_name,
};

#[derive(Debug)]
pub(super) struct ResponsesFallback {
    pub(super) status: u16,
    pub(super) body: String,
}

impl DeepSeekClient {
    pub(super) async fn create_message_responses(
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
        apply_reasoning_effort(
            &mut body,
            request.reasoning_effort.as_deref(),
            self.api_provider,
        );

        let url = api_url(&self.base_url, "responses");
        let response = self
            .send_with_retry(|| self.http_client.post(&url).json(&body))
            .await?;

        let status = response.status();

        if status.as_u16() == 404 || status.as_u16() == 405 {
            let body = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            return Ok(Err(ResponsesFallback {
                status: status.as_u16(),
                body,
            }));
        }

        if !status.is_success() {
            let error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            anyhow::bail!("Failed to call DeepSeek Responses API: HTTP {status}: {error_text}");
        }

        let response_text = response.text().await.unwrap_or_default();
        let value: Value =
            serde_json::from_str(&response_text).context("Failed to parse Responses API JSON")?;
        let message = parse_responses_message(&value)?;
        Ok(Ok(message))
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
                ContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    caller,
                } => {
                    let args = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
                    let mut item = json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": to_api_tool_name(name),
                        "arguments": args,
                    });
                    if let Some(caller) = caller {
                        item["caller"] = json!({
                            "type": caller.caller_type,
                            "tool_id": caller.tool_id,
                        });
                    }
                    items.push(item);
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                } => {
                    let mut item = json!({
                        "type": "function_call_output",
                        "call_id": tool_use_id,
                        "output": content,
                    });
                    if let Some(is_error) = is_error {
                        item["is_error"] = json!(is_error);
                    }
                    items.push(item);
                }
                ContentBlock::Thinking { .. } => {}
                ContentBlock::ServerToolUse { id, name, input } => {
                    items.push(json!({
                        "type": "server_tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                ContentBlock::ToolSearchToolResult {
                    tool_use_id,
                    content,
                } => {
                    items.push(json!({
                        "type": "tool_search_tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    }));
                }
                ContentBlock::CodeExecutionToolResult {
                    tool_use_id,
                    content,
                } => {
                    items.push(json!({
                        "type": "code_execution_tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    }));
                }
                        ContentBlock::WebSearchToolResult { .. } => {}
            }
        }
    }

    items
}

fn tool_to_responses(tool: &Tool) -> Value {
    let tool_type = tool.tool_type.as_deref().unwrap_or("function");
    let mut value = if tool_type == "function" {
        json!({
            "type": "function",
            "name": to_api_tool_name(&tool.name),
            "description": tool.description,
            "parameters": tool.input_schema,
        })
    } else if tool_type == "code_execution_20250825" {
        json!({
            "type": tool_type,
            "name": to_api_tool_name(&tool.name),
        })
    } else {
        json!({
            "type": tool_type,
            "name": to_api_tool_name(&tool.name),
            "description": tool.description,
            "input_schema": tool.input_schema,
        })
    };

    if let Some(allowed_callers) = &tool.allowed_callers {
        value["allowed_callers"] = json!(allowed_callers);
    }
    if let Some(defer_loading) = tool.defer_loading {
        value["defer_loading"] = json!(defer_loading);
    }
    if let Some(input_examples) = &tool.input_examples {
        value["input_examples"] = json!(input_examples);
    }
    if let Some(strict) = tool.strict {
        value["strict"] = json!(strict);
    }
    value
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
                            if let Some(text) = content_item.get("text").and_then(Value::as_str)
                                && !text.trim().is_empty()
                            {
                                content.push(ContentBlock::Text {
                                    text: text.to_string(),
                                    cache_control: None,
                                });
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
                    let caller = item.get("caller").and_then(|v| {
                        v.get("type")
                            .and_then(Value::as_str)
                            .map(|caller_type| ToolCaller {
                                caller_type: caller_type.to_string(),
                                tool_id: v
                                    .get("tool_id")
                                    .and_then(Value::as_str)
                                    .map(std::string::ToString::to_string),
                            })
                    });
                    content.push(ContentBlock::ToolUse {
                        id: call_id,
                        name: from_api_tool_name(&name),
                        input,
                        caller,
                    });
                }
                "function_call_output" => {
                    let tool_use_id = item
                        .get("call_id")
                        .or_else(|| item.get("tool_use_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string();
                    let content_text = item
                        .get("output")
                        .or_else(|| item.get("content"))
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else {
                                v.to_string()
                            }
                        })
                        .unwrap_or_default();
                    let is_error = item.get("is_error").and_then(Value::as_bool);
                    content.push(ContentBlock::ToolResult {
                        tool_use_id,
                        content: content_text,
                        is_error,
                        content_blocks: None,
                    });
                }
                "server_tool_use" => {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("server_tool")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("server_tool")
                        .to_string();
                    let input = item.get("input").cloned().unwrap_or(Value::Null);
                    content.push(ContentBlock::ServerToolUse { id, name, input });
                }
                "tool_search_tool_result" => {
                    let tool_use_id = item
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or("tool_search")
                        .to_string();
                    let content_value = item.get("content").cloned().unwrap_or(Value::Null);
                    content.push(ContentBlock::ToolSearchToolResult {
                        tool_use_id,
                        content: content_value,
                    });
                }
                "code_execution_tool_result" => {
                    let tool_use_id = item
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or("code_execution")
                        .to_string();
                    let content_value = item.get("content").cloned().unwrap_or(Value::Null);
                    content.push(ContentBlock::CodeExecutionToolResult {
                        tool_use_id,
                        content: content_value,
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

    if content.is_empty()
        && let Some(text) = payload.get("output_text").and_then(Value::as_str)
        && !text.trim().is_empty()
    {
        content.push(ContentBlock::Text {
            text: text.to_string(),
            cache_control: None,
        });
    }

    Ok(MessageResponse {
        id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model,
        stop_reason: None,
        stop_sequence: None,
        container: payload
            .get("container")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok()),
        usage,
    })
}
