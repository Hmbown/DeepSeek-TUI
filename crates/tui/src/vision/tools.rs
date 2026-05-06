//! Vision tools for processing images and visual content.
//!
//! This module provides tools that implement the `ToolSpec` trait to integrate
//! with the vision model for processing images, screenshots, and other visual
//! content.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

use crate::tools::spec::{
    ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str, optional_str,
};
use crate::vision::session::{ImageData, VisionSessionManager};

/// Tool for analyzing images using the vision model.
pub struct VisionAnalyzeTool {
    session_manager: Arc<VisionSessionManager>,
}

impl VisionAnalyzeTool {
    /// Create a new vision analyze tool.
    #[must_use]
    pub fn new(session_manager: Arc<VisionSessionManager>) -> Self {
        Self { session_manager }
    }

    /// Read an image file and return base64 encoded data with MIME type.
    async fn read_image_file(path: &Path) -> Result<(String, String), ToolError> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Failed to read image file: {e}")))?;

        let mime_type = Self::detect_mime_type(path)?;
        let base64_data = BASE64.encode(&bytes);

        Ok((base64_data, mime_type))
    }

    /// Detect MIME type from file extension.
    fn detect_mime_type(path: &Path) -> Result<String, ToolError> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mime_type = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            _ => {
                return Err(ToolError::execution_failed(format!(
                    "Unsupported image format: {extension}"
                )))
            }
        };

        Ok(mime_type.to_string())
    }
}

#[async_trait]
impl ToolSpec for VisionAnalyzeTool {
    fn name(&self) -> &str {
        "vision_analyze"
    }

    fn description(&self) -> &str {
        "Analyze an image using the configured vision model. \
         Supports image files, screenshots, and base64-encoded images. \
         Requires `[vision_model]` configuration in config.toml."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "image_path": {
                    "type": "string",
                    "description": "Path to the image file to analyze"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional prompt to guide the analysis. Defaults to 'Describe this image in detail.'"
                },
                "session_id": {
                    "type": "string",
                    "description": "Optional session ID for maintaining conversation context. If not provided, a new session will be created."
                }
            },
            "required": ["image_path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let image_path = required_str(&input, "image_path")?;
        let prompt = optional_str(&input, "prompt").unwrap_or("Describe this image in detail.");
        let session_id = optional_str(&input, "session_id");

        // Resolve the image path relative to workspace
        let resolved_path = context.workspace.join(image_path);

        // Read and encode the image
        let (image_data, mime_type) = Self::read_image_file(&resolved_path).await?;

        // Get or create a session
        let session = if let Some(id) = session_id {
            self.session_manager
                .get_session(id)
                .await
                .ok_or_else(|| {
                    ToolError::execution_failed(format!("Vision session not found: {id}"))
                })?
        } else {
            self.session_manager
                .create_session(None, Some(format!("Analysis of {}", image_path)))
                .await
                .map_err(|e| ToolError::execution_failed(format!("Failed to create vision session: {e}")))?
        };

        // Analyze the image
        let response = session
            .analyze_image(&image_data, &mime_type, prompt)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Failed to analyze image: {e}")))?;

        let result = json!({
            "analysis": response.content,
            "model": response.model,
            "session_id": session.id().await,
            "usage": response.usage.map(|u| json!({
                "prompt_tokens": u.prompt_tokens,
                "completion_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens,
            })),
        });

        ToolResult::json(&result)
            .map_err(|e| ToolError::execution_failed(format!("Failed to serialize result: {e}")))
    }
}

/// Tool for OCR (text extraction from images).
pub struct VisionOcrTool {
    session_manager: Arc<VisionSessionManager>,
}

impl VisionOcrTool {
    /// Create a new vision OCR tool.
    #[must_use]
    pub fn new(session_manager: Arc<VisionSessionManager>) -> Self {
        Self { session_manager }
    }
}

#[async_trait]
impl ToolSpec for VisionOcrTool {
    fn name(&self) -> &str {
        "vision_ocr"
    }

    fn description(&self) -> &str {
        "Extract text from an image using the vision model (OCR). \
         Supports PNG, JPEG, GIF, WebP, BMP, and SVG formats. \
         Requires `[vision_model]` configuration in config.toml."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "image_path": {
                    "type": "string",
                    "description": "Path to the image file to extract text from"
                },
                "language": {
                    "type": "string",
                    "description": "Optional hint about the language in the image (e.g. 'English', 'Chinese')"
                }
            },
            "required": ["image_path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let image_path = required_str(&input, "image_path")?;
        let language_hint = optional_str(&input, "language").unwrap_or("");

        let prompt = if language_hint.is_empty() {
            "Extract all text from this image. Preserve the formatting as much as possible."
        } else {
            "Extract all text from this image. The text is in {language}. Preserve the formatting as much as possible."
        };

        // Delegate to VisionAnalyzeTool with an OCR-specific prompt
        let analyze_tool = VisionAnalyzeTool::new(self.session_manager.clone());

        let mut modified_input = input.clone();
        modified_input["prompt"] = json!(prompt);

        analyze_tool.execute(modified_input, context).await
    }
}

/// Helper function to encode image bytes to base64.
pub fn encode_image_to_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

/// Helper function to create ImageData from file bytes.
pub fn create_image_data(bytes: Vec<u8>, mime_type: impl Into<String>) -> ImageData {
    ImageData {
        base64_data: encode_image_to_base64(&bytes),
        mime_type: mime_type.into(),
        description: None,
    }
}
