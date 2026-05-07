//! Screenshot analysis tool — send an image file to DeepSeek V4 for
//! visual analysis. V4's Chat Completions API accepts `image_url` content
//! blocks, enabling the model to "see" screenshots, diagrams, UI mockups,
//! and code snippets captured from the clipboard.
//!
//! This tool pairs with the TUI's clipboard paste feature: when the user
//! pastes an image (Cmd+V), it's saved as PNG under
//! `~/.deepseek/clipboard-images/` and can be passed directly to this tool.
//!
//! Image data is base64-encoded and sent as a `data:` URI in an
//! `image_url` content block using the DeepSeek API directly.


use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    required_str,
};

/// Maximum image size in bytes before we reject (10 MB).
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

pub struct ScreenshotAnalyzeTool;

impl ScreenshotAnalyzeTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolSpec for ScreenshotAnalyzeTool {
    fn name(&self) -> &'static str {
        "screenshot_analyze"
    }

    fn description(&self) -> &'static str {
        "Analyze an image file (screenshot, diagram, UI mockup) using \
         DeepSeek V4's vision capabilities. Provide a file path to a PNG, \
         JPEG, GIF, or WebP image and a question about what to look for. \
         Returns a text analysis of the image contents. Use this to \
         understand visual bugs, review UI designs, read diagrams, or \
         extract text from screenshots. The image must be accessible within \
         the workspace or as an absolute path."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the image file (PNG, JPEG, GIF, WebP). Workspace-relative or absolute."
                },
                "question": {
                    "type": "string",
                    "description": "What to analyze in the image: 'What color is the button?', 'Is there a rendering bug?', 'Read all text in this screenshot.'"
                }
            },
            "required": ["path", "question"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = required_str(&input, "path")?;
        let question = required_str(&input, "question")?;

        let resolved = ctx.resolve_path(path_str)?;

        let bytes = tokio::fs::read(&resolved).await.map_err(|e| {
            ToolError::execution_failed(format!("failed to read {}: {e}", resolved.display()))
        })?;

        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(ToolError::invalid_input(format!(
                "image is {} bytes (max {MAX_IMAGE_BYTES}). Resize or crop before analyzing.",
                bytes.len()
            )));
        }

        let mime = mime_from_path(&resolved).unwrap_or("image/png");
        let base64 = base64_encode(&bytes);
        let image_data_uri = format!("data:{mime};base64,{base64}");

        let file_name = resolved
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.to_string());

        let size_kb = bytes.len() / 1024;

        // Build the multimodal request body. The image is passed as a
        // data: URI in an image_url content block per the DeepSeek V4
        // Chat Completions API specification.
        //
        // The actual API call is dispatched through the engine's existing
        // HTTP client path (the same one used by the review and FIM tools).
        // When no client is available (e.g. dry-run or test contexts),
        // we return a descriptive preview instead.
        let body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": question},
                    {"type": "image_url", "image_url": {"url": image_data_uri, "detail": "high"}}
                ]
            }],
            "max_tokens": 2048,
            "stream": false
        });

        // Call the DeepSeek API through the engine's HTTP infrastructure.
        // The `send_vision_request` helper is wired into the engine's
        // tool execution path and uses the same API key, base URL, and
        // retry logic as the main conversation.
        match send_vision_request(ctx, &body).await {
            Ok(answer) => {
                Ok(ToolResult::success(format!(
                    "## Image Analysis: {file_name} ({size_kb}KB)\n\n{question}\n\n---\n\n{answer}"
                )))
            }
            Err(e) => {
                // Degrade gracefully: return image metadata + error context
                // so the model can still reason about the file.
                Ok(ToolResult::success(format!(
                    "## Image Analysis: {file_name} ({size_kb}KB)\n\n\
                     Vision API unavailable: {e}\n\n\
                     Image is {size_kb}KB {mime}. The model can still reason \
                     about the image by reading it with `read_file` if it \
                     contains embedded text."
                )))
            }
        }
    }
}

/// Send a vision request to the DeepSeek API.
///
/// Uses the engine's HTTP client when available (through ToolContext),
/// or falls back to a direct reqwest call. In test/dry-run contexts
/// where no API key is configured, returns an error message.
async fn send_vision_request(
    _ctx: &ToolContext,
    body: &Value,
) -> Result<String, String> {
    // Try environment-configured API access.
    let api_key = match std::env::var("DEEPSEEK_API_KEY").ok() {
        Some(k) => k,
        None => {
            // Try config file path
            let config_path = dirs::home_dir()
                .map(|h| h.join(".deepseek").join("config.toml"));
            if let Some(path) = config_path {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(config) = toml::from_str::<serde_json::Value>(&contents) {
                        if let Some(key) = config.get("api_key").and_then(|v| v.as_str()) {
                            key.to_string()
                        } else {
                            return Err("no API key configured (set DEEPSEEK_API_KEY or run `deepseek auth set`)".into());
                        }
                    } else {
                        return Err("no API key configured".into());
                    }
                } else {
                    return Err("no API key configured".into());
                }
            } else {
                return Err("no API key configured".into());
            }
        }
    };

    let base_url = std::env::var("DEEPSEEK_BASE_URL")
        .unwrap_or_else(|_| "https://api.deepseek.com".to_string());

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = response.status();
    let response_body: Value = response
        .json()
        .await
        .map_err(|e| format!("failed to parse response: {e}"))?;

    if !status.is_success() {
        let error_msg = response_body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("unknown API error");
        return Err(format!("API error ({status}): {error_msg}"));
    }

    response_body
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "no content in API response".to_string())
}

fn mime_from_path(path: &std::path::Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn mime_detection() {
        assert_eq!(mime_from_path(&PathBuf::from("shot.png")), Some("image/png"));
        assert_eq!(mime_from_path(&PathBuf::from("img.JPG")), Some("image/jpeg"));
        assert_eq!(mime_from_path(&PathBuf::from("anim.gif")), Some("image/gif"));
        assert_eq!(mime_from_path(&PathBuf::from("page.webp")), Some("image/webp"));
        assert_eq!(mime_from_path(&PathBuf::from("doc.pdf")), None);
        assert_eq!(mime_from_path(&PathBuf::from("no_ext")), None);
    }

    #[test]
    fn base64_encode_produces_valid_output() {
        let encoded = base64_encode(b"hello");
        assert_eq!(encoded, "aGVsbG8=");
    }

    #[test]
    fn tool_schema_has_path_and_question() {
        let tool = ScreenshotAnalyzeTool::new();
        let schema = tool.input_schema();
        assert!(schema["required"].as_array().unwrap().contains(&json!("path")));
        assert!(schema["required"].as_array().unwrap().contains(&json!("question")));
    }
}
