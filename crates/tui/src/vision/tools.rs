//! Vision tools for processing images and visual content.
//!
//! This module provides tools that integrate with the vision model
//! to process images, screenshots, and other visual content.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

use crate::tools::spec::{ToolContext, ToolResult, ToolSpec, ApprovalRequirement};
use crate::vision::client::{VisionClient, VisionClientConfig, VisionRequest};
use crate::vision::session::{ImageData, VisionSessionHandle, VisionSessionManager};

/// Tool for analyzing images using the vision model
pub struct VisionAnalyzeTool {
    session_manager: Arc<VisionSessionManager>,
}

impl VisionAnalyzeTool {
    /// Create a new vision analyze tool
    #[must_use]
    pub fn new(session_manager: Arc<VisionSessionManager>) -> Self {
        Self { session_manager }
    }

    /// Get the tool specification
    #[must_use]
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "vision_analyze".to_string(),
            description: Some(
                "Analyze an image using the configured vision model. \
                 Supports image files, screenshots, and base64-encoded images."
                    .to_string(),
            ),
            parameters: vec![
                crate::tools::spec::ParamSpec {
                    name: "image_path".to_string(),
                    description: Some("Path to the image file to analyze".to_string()),
                    required: true,
                    param_type: crate::tools::spec::ParamType::String,
                },
                crate::tools::spec::ParamSpec {
                    name: "prompt".to_string(),
                    description: Some(
                        "Optional prompt to guide the analysis. \
                         Defaults to 'Describe this image in detail.'"
                            .to_string(),
                    ),
                    required: false,
                    param_type: crate::tools::spec::ParamType::String,
                },
                crate::tools::spec::ParamSpec {
                    name: "session_id".to_string(),
                    description: Some(
                        "Optional session ID for maintaining conversation context. \
                         If not provided, a new session will be created."
                            .to_string(),
                    ),
                    required: false,
                    param_type: crate::tools::spec::ParamType::String,
                },
            ],
            returns: crate::tools::spec::ReturnSpec {
                description: "Analysis result from the vision model".to_string(),
                return_type: crate::tools::spec::ParamType::Object,
            },
            approval: ApprovalRequirement::Auto,
            capability: crate::tools::spec::ToolCapability::Read,
        }
    }

    /// Execute the vision analyze tool
    pub async fn execute(
        &self,
        args: &Value,
        context: &ToolContext,
    ) -> Result<ToolResult> {
        let image_path = args
            .get("image_path")
            .and_then(|v| v.as_str())
            .context("Missing required parameter: image_path")?;

        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe this image in detail.");

        let session_id = args.get("session_id").and_then(|v| v.as_str());

        // Resolve the image path
        let resolved_path = context.workspace_root.join(image_path);

        // Read and encode the image
        let (image_data, mime_type) = Self::read_image_file(&resolved_path).await?;

        // Get or create a session
        let session = if let Some(id) = session_id {
            self.session_manager
                .get_session(id)
                .await
                .context("Session not found")?
        } else {
            // Create a new session with default config
            let config = self.get_default_config(context)?;
            self.session_manager
                .create_session(Some(config), Some(format!("Analysis of {}", image_path)))
                .await?
        };

        // Analyze the image
        let response = session
            .analyze_image(&image_data, &mime_type, prompt)
            .await
            .context("Failed to analyze image")?;

        // Build the result
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

        Ok(ToolResult {
            success: true,
            data: Some(result),
            error: None,
            metadata: None,
        })
    }

    /// Read an image file and return base64 encoded data with MIME type
    async fn read_image_file(path: &Path) -> Result<(String, String)> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("Failed to read image file: {}", path.display()))?;

        let mime_type = Self::detect_mime_type(path)?;
        let base64_data = BASE64.encode(&bytes);

        Ok((base64_data, mime_type))
    }

    /// Detect MIME type from file extension
    fn detect_mime_type(path: &Path) -> Result<String> {
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
            _ => anyhow::bail!("Unsupported image format: {}", extension),
        };

        Ok(mime_type.to_string())
    }

    /// Get default vision client configuration from context
    fn get_default_config(&self, context: &ToolContext) -> Result<VisionClientConfig> {
        // This would typically come from the global config
        // For now, we'll return an error if no config is available
        anyhow::bail!("Vision model not configured. Please set up vision_model in config.toml")
    }
}

/// Tool for comparing multiple images
pub struct VisionCompareTool {
    session_manager: Arc<VisionSessionManager>,
}

impl VisionCompareTool {
    /// Create a new vision compare tool
    #[must_use]
    pub fn new(session_manager: Arc<VisionSessionManager>) -> Self {
        Self { session_manager }
    }

    /// Get the tool specification
    #[must_use]
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "vision_compare".to_string(),
            description: Some(
                "Compare multiple images using the vision model. \
                 Useful for finding differences or similarities between images."
                    .to_string(),
            ),
            parameters: vec![
                crate::tools::spec::ParamSpec {
                    name: "image_paths".to_string(),
                    description: Some(
                        "Array of image file paths to compare".to_string(),
                    ),
                    required: true,
                    param_type: crate::tools::spec::ParamType::Array,
                },
                crate::tools::spec::ParamSpec {
                    name: "comparison_prompt".to_string(),
                    description: Some(
                        "Optional prompt to guide the comparison. \
                         Defaults to 'Compare these images and describe the differences.'"
                            .to_string(),
                    ),
                    required: false,
                    param_type: crate::tools::spec::ParamType::String,
                },
            ],
            returns: crate::tools::spec::ReturnSpec {
                description: "Comparison result from the vision model".to_string(),
                return_type: crate::tools::spec::ParamType::Object,
            },
            approval: ApprovalRequirement::Auto,
            capability: crate::tools::spec::ToolCapability::Read,
        }
    }
}

/// Tool for OCR (text extraction from images)
pub struct VisionOcrTool {
    session_manager: Arc<VisionSessionManager>,
}

impl VisionOcrTool {
    /// Create a new vision OCR tool
    #[must_use]
    pub fn new(session_manager: Arc<VisionSessionManager>) -> Self {
        Self { session_manager }
    }

    /// Get the tool specification
    #[must_use]
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "vision_ocr".to_string(),
            description: Some(
                "Extract text from an image using the vision model (OCR).".to_string(),
            ),
            parameters: vec![
                crate::tools::spec::ParamSpec {
                    name: "image_path".to_string(),
                    description: Some("Path to the image file".to_string()),
                    required: true,
                    param_type: crate::tools::spec::ParamType::String,
                },
                crate::tools::spec::ParamSpec {
                    name: "language".to_string(),
                    description: Some(
                        "Optional hint about the language in the image".to_string(),
                    ),
                    required: false,
                    param_type: crate::tools::spec::ParamType::String,
                },
            ],
            returns: crate::tools::spec::ReturnSpec {
                description: "Extracted text from the image".to_string(),
                return_type: crate::tools::spec::ParamType::Object,
            },
            approval: ApprovalRequirement::Auto,
            capability: crate::tools::spec::ToolCapability::Read,
        }
    }

    /// Execute OCR on an image
    pub async fn execute(
        &self,
        args: &Value,
        context: &ToolContext,
    ) -> Result<ToolResult> {
        let image_path = args
            .get("image_path")
            .and_then(|v| v.as_str())
            .context("Missing required parameter: image_path")?;

        let language_hint = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let prompt = if language_hint.is_empty() {
            "Extract all text from this image. Preserve the formatting as much as possible."
        } else {
            &format!(
                "Extract all text from this image. The text is in {}. Preserve the formatting as much as possible.",
                language_hint
            )
        };

        // Use the analyze tool's logic
        let analyze_tool = VisionAnalyzeTool::new(self.session_manager.clone());

        // Modify args to include the OCR prompt
        let mut modified_args = args.clone();
        modified_args["prompt"] = json!(prompt);

        analyze_tool.execute(&modified_args, context).await
    }
}

/// Tool for managing vision sessions
pub struct VisionSessionTool {
    session_manager: Arc<VisionSessionManager>,
}

impl VisionSessionTool {
    /// Create a new vision session tool
    #[must_use]
    pub fn new(session_manager: Arc<VisionSessionManager>) -> Self {
        Self { session_manager }
    }

    /// Get the tool specification
    #[must_use]
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "vision_session".to_string(),
            description: Some(
                "Manage vision model sessions. \
                 Supports creating, listing, and closing sessions."
                    .to_string(),
            ),
            parameters: vec![
                crate::tools::spec::ParamSpec {
                    name: "action".to_string(),
                    description: Some(
                        "Action to perform: 'create', 'list', 'close', 'clear'".to_string(),
                    ),
                    required: true,
                    param_type: crate::tools::spec::ParamType::String,
                },
                crate::tools::spec::ParamSpec {
                    name: "session_id".to_string(),
                    description: Some("Session ID (required for 'close' action)".to_string()),
                    required: false,
                    param_type: crate::tools::spec::ParamType::String,
                },
                crate::tools::spec::ParamSpec {
                    name: "description".to_string(),
                    description: Some("Optional description for new session".to_string()),
                    required: false,
                    param_type: crate::tools::spec::ParamType::String,
                },
            ],
            returns: crate::tools::spec::ReturnSpec {
                description: "Session operation result".to_string(),
                return_type: crate::tools::spec::ParamType::Object,
            },
            approval: ApprovalRequirement::Auto,
            capability: crate::tools::spec::ToolCapability::Read,
        }
    }

    /// Execute the vision session tool
    pub async fn execute(&self, args: &Value) -> Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .context("Missing required parameter: action")?;

        match action {
            "create" => {
                let description = args.get("description").and_then(|v| v.as_str());
                // This would need config from somewhere - simplified for now
                let result = json!({
                    "success": true,
                    "message": "Session creation requires vision model configuration",
                });
                Ok(ToolResult {
                    success: true,
                    data: Some(result),
                    error: None,
                    metadata: None,
                })
            }
            "list" => {
                let sessions = self.session_manager.list_sessions().await;
                let result = json!({
                    "sessions": sessions.iter().map(|s| json!({
                        "id": s.id,
                        "message_count": s.message_count,
                        "duration_secs": s.duration_secs,
                        "total_requests": s.total_requests,
                        "total_tokens": s.total_tokens,
                        "description": s.description,
                    })).collect::<Vec<_>>(),
                });
                Ok(ToolResult {
                    success: true,
                    data: Some(result),
                    error: None,
                    metadata: None,
                })
            }
            "close" => {
                let session_id = args
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .context("Missing required parameter: session_id for 'close' action")?;

                let removed = self.session_manager.remove_session(session_id).await;
                let result = json!({
                    "success": removed,
                    "session_id": session_id,
                    "message": if removed { "Session closed" } else { "Session not found" },
                });
                Ok(ToolResult {
                    success: removed,
                    data: Some(result),
                    error: None,
                    metadata: None,
                })
            }
            "clear" => {
                self.session_manager.clear_all_sessions().await;
                let result = json!({
                    "success": true,
                    "message": "All sessions cleared",
                });
                Ok(ToolResult {
                    success: true,
                    data: Some(result),
                    error: None,
                    metadata: None,
                })
            }
            _ => anyhow::bail!("Unknown action: {}", action),
        }
    }
}

/// Helper function to encode image bytes to base64
pub fn encode_image_to_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

/// Helper function to create ImageData from file bytes
pub fn create_image_data(bytes: Vec<u8>, mime_type: impl Into<String>) -> ImageData {
    ImageData {
        base64_data: encode_image_to_base64(&bytes),
        mime_type: mime_type.into(),
        description: None,
    }
}
