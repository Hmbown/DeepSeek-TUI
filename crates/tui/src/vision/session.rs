//! Vision session management for independent vision model conversations.
//!
//! This module provides session management for vision models, allowing
//! independent conversation state with subagent-like behavior.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::vision::client::{VisionClient, VisionClientConfig, VisionRequest, VisionResponse};

/// A vision session represents an independent conversation with a vision model
#[derive(Debug, Clone)]
pub struct VisionSession {
    pub id: String,
    pub conversation_history: Vec<VisionMessage>,
    pub metadata: SessionMetadata,
}

/// Message in a vision session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_data: Option<ImageData>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Image data attached to a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub base64_data: String,
    pub mime_type: String,
    pub description: Option<String>,
}

/// Message role
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// Session metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub total_requests: u64,
    pub total_tokens_used: u64,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
}

impl VisionSession {
    pub fn new(config: VisionClientConfig, description: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_history: Vec::new(),
            metadata: SessionMetadata {
                description,
                ..Default::default()
            },
        }
    }

    pub fn add_user_message(&mut self, content: impl Into<String>, image_data: Option<ImageData>) {
        self.conversation_history.push(VisionMessage {
            role: MessageRole::User,
            content: content.into(),
            image_data,
            timestamp: chrono::Utc::now(),
        });
    }

    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.conversation_history.push(VisionMessage {
            role: MessageRole::Assistant,
            content: content.into(),
            image_data: None,
            timestamp: chrono::Utc::now(),
        });
    }

    pub fn update_usage(&mut self, response: &VisionResponse) {
        self.metadata.total_requests += 1;
        if let Some(usage) = &response.usage {
            self.metadata.total_tokens_used += u64::from(usage.total_tokens);
        }
    }
}

/// Handle to an active vision session
#[derive(Debug, Clone)]
pub struct VisionSessionHandle {
    session: Arc<Mutex<VisionSession>>,
    client: VisionClient,
}

impl VisionSessionHandle {
    pub async fn new(config: VisionClientConfig, description: Option<String>) -> Result<Self> {
        let client = VisionClient::new(config.clone())?;
        let session = VisionSession::new(config, description);

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            client,
        })
    }

    pub async fn id(&self) -> String {
        self.session.lock().await.id.clone()
    }

    pub async fn send_message(
        &self,
        prompt: impl Into<String>,
        image_data: Option<ImageData>,
    ) -> Result<VisionResponse> {
        let prompt = prompt.into();

        {
            let mut session = self.session.lock().await;
            session.add_user_message(&prompt, image_data.clone());
        }

        let request = if let Some(img) = &image_data {
            VisionRequest::new(&prompt, &img.base64_data, &img.mime_type)
        } else {
            VisionRequest::new(&prompt, "", "text/plain")
        };

        let system_message: Option<String> = {
            let session = self.session.lock().await;
            session
                .conversation_history
                .iter()
                .find(|m| m.role == MessageRole::System)
                .map(|m| m.content.clone())
        };

        let request = if let Some(system) = system_message {
            request.with_system_message(system)
        } else {
            request
        };

        let response = self.client.process_image(request).await?;

        {
            let mut session = self.session.lock().await;
            session.add_assistant_message(&response.content);
            session.update_usage(&response);
        }

        Ok(response)
    }

    pub async fn analyze_image(
        &self,
        image_base64: impl Into<String>,
        mime_type: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Result<VisionResponse> {
        let image_data = ImageData {
            base64_data: image_base64.into(),
            mime_type: mime_type.into(),
            description: None,
        };

        self.send_message(prompt, Some(image_data)).await
    }
}

/// Manager for vision sessions
#[derive(Debug, Clone)]
pub struct VisionSessionManager {
    sessions: Arc<RwLock<HashMap<String, VisionSessionHandle>>>,
    default_config: Option<VisionClientConfig>,
}

impl VisionSessionManager {
    /// Create a new session manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_config: None,
        }
    }

    /// Create a new session manager with default configuration
    #[must_use]
    pub fn with_config(config: VisionClientConfig) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_config: Some(config),
        }
    }

    /// Create a new session
    pub async fn create_session(
        &self,
        config: Option<VisionClientConfig>,
        description: Option<String>,
    ) -> Result<VisionSessionHandle> {
        let config = config
            .or_else(|| self.default_config.clone())
            .context("No vision client configuration provided")?;

        let handle = VisionSessionHandle::new(config, description).await?;
        let id = handle.id().await;

        self.sessions
            .write()
            .await
            .insert(id.clone(), handle.clone());

        Ok(handle)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Option<VisionSessionHandle> {
        self.sessions.read().await.get(id).cloned()
    }
}

impl Default for VisionSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vision_session_creation() {
        let config = VisionClientConfig {
            model: "gemini-3.1-flash-lite-preview".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            timeout_secs: 120,
        };

        let session = VisionSession::new(config, Some("Test session".to_string()));

        assert_eq!(session.conversation_history.len(), 0);
        assert_eq!(
            session.metadata.description,
            Some("Test session".to_string())
        );
    }

    #[test]
    fn test_vision_session_messages() {
        let config = VisionClientConfig {
            model: "gemini-3.1-flash-lite-preview".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            timeout_secs: 120,
        };

        let mut session = VisionSession::new(config, None);

        session.conversation_history.push(VisionMessage {
            role: MessageRole::System,
            content: "You are a helpful assistant".to_string(),
            image_data: None,
            timestamp: chrono::Utc::now(),
        });
        session.add_user_message("Hello", None);
        session.add_assistant_message("Hi there!");

        assert_eq!(session.conversation_history.len(), 3);
        assert_eq!(session.conversation_history[0].role, MessageRole::System);
        assert_eq!(session.conversation_history[1].role, MessageRole::User);
        assert_eq!(session.conversation_history[2].role, MessageRole::Assistant);
    }
}
