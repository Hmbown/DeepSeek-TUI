//! HTTP client for vision-capable models (GPT-4o, Claude 3, Gemini, etc.)
//!
//! This client handles image processing tasks using vision-capable models
//! with support for base64-encoded images and multi-modal conversations.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::VisionModelConfig;
use crate::llm_client::{LlmError, RetryConfig, with_retry};

/// Configuration for the vision client
#[derive(Debug, Clone)]
pub struct VisionClientConfig {
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub timeout_secs: u64,
}

impl From<VisionModelConfig> for VisionClientConfig {
    fn from(config: VisionModelConfig) -> Self {
        Self {
            model: config.model,
            api_key: config.api_key.unwrap_or_default(),
            base_url: config.base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            timeout_secs: config.timeout_secs,
        }
    }
}

/// A vision request with image content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionRequest {
    /// Text prompt to accompany the image
    pub prompt: String,
    /// Base64-encoded image data
    pub image_base64: String,
    /// Image MIME type (e.g., "image/png", "image/jpeg")
    pub image_mime_type: String,
    /// Optional system message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    /// Maximum tokens for the response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Temperature for sampling (0.0 - 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

impl VisionRequest {
    /// Create a new vision request
    #[must_use]
    pub fn new(prompt: impl Into<String>, image_base64: impl Into<String>, image_mime_type: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            image_base64: image_base64.into(),
            image_mime_type: image_mime_type.into(),
            system_message: None,
            max_tokens: None,
            temperature: None,
        }
    }

    /// Set the system message
    #[must_use]
    pub fn with_system_message(mut self, message: impl Into<String>) -> Self {
        self.system_message = Some(message.into());
        self
    }

}

/// Response from a vision model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionResponse {
    /// The generated text response
    pub content: String,
    /// Token usage information
    pub usage: Option<VisionUsage>,
    /// Model that generated the response
    pub model: String,
    /// Raw response for debugging
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_response: Option<Value>,
}

/// Token usage for vision requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// HTTP client for vision-capable models
#[derive(Debug, Clone)]
pub struct VisionClient {
    http_client: reqwest::Client,
    config: VisionClientConfig,
}

impl VisionClient {
    /// Create a new vision client from configuration
    pub fn new(config: VisionClientConfig) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            http_client,
            config,
        })
    }

    /// Send a vision request to the model
    pub async fn process_image(&self, request: VisionRequest) -> Result<VisionResponse> {
        let max_tokens = request.max_tokens.unwrap_or(self.config.max_tokens);
        let temperature = request.temperature.unwrap_or(self.config.temperature);

        let payload = self.build_payload(&request, max_tokens, temperature);
        let response = self.send_request_with_retry(payload).await?;
        self.parse_response(response).await
    }

    /// Build the API request payload
    fn build_payload(&self, request: &VisionRequest, max_tokens: u32, temperature: f32) -> Value {
        let mut messages = Vec::new();

        // Add system message if provided
        if let Some(system) = &request.system_message {
            messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        // Add user message with image
        messages.push(json!({
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": request.prompt
                },
                {
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", request.image_mime_type, request.image_base64)
                    }
                }
            ]
        }));

        json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature
        })
    }

    /// Send request with retry logic
    async fn send_request_with_retry(&self, payload: Value) -> Result<reqwest::Response> {
        let client = self.http_client.clone();
        let url = format!("{}/chat/completions", self.config.base_url);
        let api_key = self.config.api_key.clone();

        let retry_config = RetryConfig {
            max_retries: 3,
            initial_delay: 1.0,
            max_delay: 30.0,
            ..RetryConfig::default()
        };

        with_retry(&retry_config, || {
            let client = client.clone();
            let url = url.clone();
            let api_key = api_key.clone();
            let payload = payload.clone();
            async move {
                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {}", api_key))
                        .map_err(|e| LlmError::NetworkError(format!("Invalid header: {e}")))?,
                );

                let response = client
                    .post(&url)
                    .headers(headers)
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| LlmError::from_reqwest(&e))?;

                let status = response.status();
                if !status.is_success() {
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    return Err(LlmError::from_http_response(status.as_u16(), &error_text));
                }

                Ok(response)
            }
        }, None)
        .await
        .map_err(|e| anyhow::anyhow!("Vision request failed: {}", e))
    }

    /// Parse the API response
    async fn parse_response(&self, response: reqwest::Response) -> Result<VisionResponse> {
        let json: Value = response
            .json()
            .await
            .context("Failed to parse response JSON")?;

        // Extract content from the response
        let content = json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        // Extract usage information
        let usage = json.get("usage").map(|u| VisionUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        });

        // Extract model information
        let model = json
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&self.config.model)
            .to_string();

        Ok(VisionResponse {
            content,
            usage,
            model,
            raw_response: Some(json),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vision_request_builder() {
        let request = VisionRequest {
            prompt: "Describe this image".to_string(),
            image_base64: "base64encodeddata".to_string(),
            image_mime_type: "image/png".to_string(),
            system_message: Some("You are a helpful assistant".to_string()),
            max_tokens: Some(1000),
            temperature: Some(0.5),
        };

        assert_eq!(request.prompt, "Describe this image");
        assert_eq!(request.image_base64, "base64encodeddata");
        assert_eq!(request.image_mime_type, "image/png");
    }
}
