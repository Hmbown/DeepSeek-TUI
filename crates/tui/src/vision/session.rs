//! Vision session management for independent vision model conversations.
//!
//! This module provides session management for vision models, allowing
//! independent conversation state with subagent-like behavior.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::vision::client::{VisionClient, VisionClientConfig, VisionRequest, VisionResponse};

/// A vision session represents an independent conversation with a vision model
#[derive(Debug, Clone)]
pub struct VisionSession {
    pub id: String,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub config: VisionClientConfig,
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
    /// Create a new vision session
    pub fn new(config: VisionClientConfig, description: Option<String>) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now,
            last_activity: now,
            config,
            conversation_history: Vec::new(),
            metadata: SessionMetadata {
                description,
                ..Default::default()
            },
        }
    }

    /// Create a new vision session with a specific ID
    pub fn with_id(id: impl Into<String>, config: VisionClientConfig, description: Option<String>) -> Self {
        let now = Instant::now();
        Self {
            id: id.into(),
            created_at: now,
            last_activity: now,
            config,
            conversation_history: Vec::new(),
            metadata: SessionMetadata {
                description,
                ..Default::default()
            },
        }
    }

    /// Add a system message to the session
    pub fn add_system_message(&mut self, content: impl Into<String>) {
        self.conversation_history.push(VisionMessage {
            role: MessageRole::System,
            content: content.into(),
            image_data: None,
            timestamp: chrono::Utc::now(),
        });
        self.last_activity = Instant::now();
    }

    /// Add a user message to the session
    pub fn add_user_message(&mut self, content: impl Into<String>, image_data: Option<ImageData>) {
        self.conversation_history.push(VisionMessage {
            role: MessageRole::User,
            content: content.into(),
            image_data,
            timestamp: chrono::Utc::now(),
        });
        self.last_activity = Instant::now();
    }

    /// Add an assistant message to the session
    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.conversation_history.push(VisionMessage {
            role: MessageRole::Assistant,
            content: content.into(),
            image_data: None,
            timestamp: chrono::Utc::now(),
        });
        self.last_activity = Instant::now();
    }

    /// Get the conversation history
    #[must_use]
    pub fn history(&self) -> &[VisionMessage] {
        &self.conversation_history
    }

    /// Get session duration
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get time since last activity
    #[must_use]
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Update metadata with usage from a response
    pub fn update_usage(&mut self, response: &VisionResponse) {
        self.metadata.total_requests += 1;
        if let Some(usage) = &response.usage {
            self.metadata.total_tokens_used += u64::from(usage.total_tokens);
        }
        self.last_activity = Instant::now();
    }

    /// Clear conversation history (keep system messages)
    pub fn clear_history(&mut self) {
        let system_messages: Vec<VisionMessage> = self
            .conversation_history
            .iter()
            .filter(|m| m.role == MessageRole::System)
            .cloned()
            .collect();
        self.conversation_history = system_messages;
        self.last_activity = Instant::now();
    }

    /// Get a summary of the session
    #[must_use]
    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            id: self.id.clone(),
            message_count: self.conversation_history.len(),
            duration_secs: self.duration().as_secs(),
            idle_secs: self.idle_time().as_secs(),
            total_requests: self.metadata.total_requests,
            total_tokens: self.metadata.total_tokens_used,
            description: self.metadata.description.clone(),
        }
    }
}

/// Session summary for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub message_count: usize,
    pub duration_secs: u64,
    pub idle_secs: u64,
    pub total_requests: u64,
    pub total_tokens: u64,
    pub description: Option<String>,
}

/// Handle to an active vision session
#[derive(Debug, Clone)]
pub struct VisionSessionHandle {
    session: Arc<Mutex<VisionSession>>,
    client: VisionClient,
}

impl VisionSessionHandle {
    /// Create a new session handle
    pub async fn new(config: VisionClientConfig, description: Option<String>) -> Result<Self> {
        let client = VisionClient::new(config.clone())?;
        let session = VisionSession::new(config, description);

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            client,
        })
    }

    /// Get the session ID
    pub async fn id(&self) -> String {
        self.session.lock().await.id.clone()
    }

    /// Send a message with an optional image to the session
    pub async fn send_message(
        &self,
        prompt: impl Into<String>,
        image_data: Option<ImageData>,
    ) -> Result<VisionResponse> {
        let prompt = prompt.into();

        // Add user message to history
        {
            let mut session = self.session.lock().await;
            session.add_user_message(&prompt, image_data.clone());
        }

        // Build the request
        let request = if let Some(img) = &image_data {
            VisionRequest::new(&prompt, &img.base64_data, &img.mime_type)
        } else {
            // For text-only requests, we still need to use the vision API
            // but with a placeholder or we can build a custom request
            VisionRequest::new(&prompt, "", "text/plain")
        };

        // Get system message from history if any
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

        // Send the request
        let response = self.client.process_image(request).await?;

        // Add assistant response to history
        {
            let mut session = self.session.lock().await;
            session.add_assistant_message(&response.content);
            session.update_usage(&response);
        }

        Ok(response)
    }

    /// Send an image for analysis
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

    /// Get session summary
    pub async fn summary(&self) -> SessionSummary {
        self.session.lock().await.summary()
    }

    /// Get conversation history
    pub async fn history(&self) -> Vec<VisionMessage> {
        self.session.lock().await.conversation_history.clone()
    }

    /// Clear conversation history
    pub async fn clear_history(&self) {
        self.session.lock().await.clear_history();
    }

    /// Add a system message
    pub async fn set_system_message(&self, message: impl Into<String>) {
        let mut session = self.session.lock().await;
        // Remove existing system messages
        session.conversation_history.retain(|m| m.role != MessageRole::System);
        session.add_system_message(message);
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

        self.sessions.write().await.insert(id.clone(), handle.clone());

        Ok(handle)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Option<VisionSessionHandle> {
        self.sessions.read().await.get(id).cloned()
    }

    /// List all active sessions
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let sessions = self.sessions.read().await;
        let mut summaries = Vec::new();

        for handle in sessions.values() {
            summaries.push(handle.summary().await);
        }

        summaries
    }

    /// Remove a session
    pub async fn remove_session(&self, id: &str) -> bool {
        self.sessions.write().await.remove(id).is_some()
    }

    /// Clean up idle sessions
    pub async fn cleanup_idle_sessions(&self, max_idle_duration: Duration) -> usize {
        let mut to_remove = Vec::new();
        {
            let sessions = self.sessions.read().await;
            for (id, handle) in sessions.iter() {
                let idle_time = handle.summary().await.idle_secs;
                if Duration::from_secs(idle_time) > max_idle_duration {
                    to_remove.push(id.clone());
                }
            }
        }

        let count = to_remove.len();
        if count > 0 {
            let mut sessions = self.sessions.write().await;
            for id in &to_remove {
                sessions.remove(id);
            }
        }

        count
    }

    /// Get the number of active sessions
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Clear all sessions
    pub async fn clear_all_sessions(&self) {
        self.sessions.write().await.clear();
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
            model: "gpt-4o".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            timeout_secs: 120,
        };

        let session = VisionSession::new(config, Some("Test session".to_string()));

        assert_eq!(session.conversation_history.len(), 0);
        assert_eq!(session.metadata.description, Some("Test session".to_string()));
    }

    #[test]
    fn test_vision_session_messages() {
        let config = VisionClientConfig {
            model: "gpt-4o".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            timeout_secs: 120,
        };

        let mut session = VisionSession::new(config, None);

        session.add_system_message("You are a helpful assistant");
        session.add_user_message("Hello", None);
        session.add_assistant_message("Hi there!");

        assert_eq!(session.conversation_history.len(), 3);
        assert_eq!(session.conversation_history[0].role, MessageRole::System);
        assert_eq!(session.conversation_history[1].role, MessageRole::User);
        assert_eq!(session.conversation_history[2].role, MessageRole::Assistant);
    }
}
