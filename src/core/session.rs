//! Session state management for the core engine.
//!
//! Tracks conversation history, token usage, and session metadata.

use crate::models::{Message, SystemPrompt, Usage};
use crate::project_context::{ProjectContext, load_project_context_with_parents};
use std::path::PathBuf;

/// Session state for the engine.
#[derive(Debug, Clone)]
pub struct Session {
    /// Model being used
    pub model: String,

    /// Workspace directory
    pub workspace: PathBuf,

    /// System prompt (optional)
    pub system_prompt: Option<SystemPrompt>,

    /// Conversation history (API format)
    pub messages: Vec<Message>,

    /// Total tokens used in this session
    pub total_usage: SessionUsage,

    /// Whether shell execution is allowed
    pub allow_shell: bool,

    /// Whether to trust paths outside workspace
    pub trust_mode: bool,

    /// Notes file path
    pub notes_path: PathBuf,

    /// MCP config path
    pub mcp_config_path: PathBuf,

    /// Session ID (for tracking)
    pub id: String,

    /// Project context loaded from AGENTS.md, etc.
    pub project_context: Option<ProjectContext>,
}

/// Cumulative usage statistics for a session.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_field_names)]
pub struct SessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl SessionUsage {
    /// Add usage from a turn
    pub fn add(&mut self, usage: &Usage) {
        self.input_tokens += u64::from(usage.input_tokens);
        self.output_tokens += u64::from(usage.output_tokens);
    }

    /// Total tokens used
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

impl Session {
    /// Create a new session
    pub fn new(
        model: String,
        workspace: PathBuf,
        allow_shell: bool,
        trust_mode: bool,
        notes_path: PathBuf,
        mcp_config_path: PathBuf,
    ) -> Self {
        // Load project context from AGENTS.md, CLAUDE.md, etc.
        let project_context = load_project_context_with_parents(&workspace);
        let has_context = project_context.has_instructions();

        Self {
            model,
            workspace,
            system_prompt: None,
            messages: Vec::new(),
            total_usage: SessionUsage::default(),
            allow_shell,
            trust_mode,
            notes_path,
            mcp_config_path,
            id: uuid::Uuid::new_v4().to_string(),
            project_context: if has_context {
                Some(project_context)
            } else {
                None
            },
        }
    }

    /// Get project instructions as a system prompt block (if available)
    pub fn get_project_instructions(&self) -> Option<String> {
        self.project_context
            .as_ref()
            .and_then(super::super::project_context::ProjectContext::as_system_block)
    }

    /// Add a message to the conversation
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Clear the conversation history
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Get the message count
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}
