//! Operations submitted by the UI to the core engine.
//!
//! These operations flow from the TUI to the engine via a channel,
//! allowing the UI to remain responsive while the engine processes requests.

use crate::compaction::CompactionConfig;
use crate::models::{Message, SystemPrompt};
use crate::tui::app::AppMode;
use std::path::PathBuf;

/// Operations that can be submitted to the engine.
#[derive(Debug, Clone)]
pub enum Op {
    /// Send a message to the AI
    SendMessage {
        content: String,
        mode: AppMode,
        model: String,
        allow_shell: bool,
        trust_mode: bool,
    },

    /// Cancel the current request
    #[allow(dead_code)]
    CancelRequest,

    /// Approve a tool call that requires permission
    #[allow(dead_code)]
    ApproveToolCall { id: String },

    /// Deny a tool call that requires permission
    #[allow(dead_code)]
    DenyToolCall { id: String },

    /// Spawn a sub-agent
    #[allow(dead_code)]
    SpawnSubAgent { prompt: String },

    /// List current sub-agents and their status
    ListSubAgents,

    /// Change the operating mode
    #[allow(dead_code)]
    ChangeMode { mode: AppMode },

    /// Update the model being used
    #[allow(dead_code)]
    SetModel { model: String },

    /// Update auto-compaction settings
    SetCompaction { config: CompactionConfig },

    /// Sync engine session state (used for resume/load)
    SyncSession {
        messages: Vec<Message>,
        system_prompt: Option<SystemPrompt>,
        model: String,
        workspace: PathBuf,
    },

    /// Run context compaction immediately.
    CompactContext,

    /// Shutdown the engine
    Shutdown,
}

impl Op {
    /// Create a send message operation
    pub fn send(
        content: impl Into<String>,
        mode: AppMode,
        model: impl Into<String>,
        allow_shell: bool,
        trust_mode: bool,
    ) -> Self {
        Op::SendMessage {
            content: content.into(),
            mode,
            model: model.into(),
            allow_shell,
            trust_mode,
        }
    }
}
