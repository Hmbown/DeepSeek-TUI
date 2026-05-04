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
        goal_objective: Option<String>,
        /// Reasoning-effort tier: `"off" | "low" | "medium" | "high" | "max"`.
        /// `None` lets the provider apply its default.
        reasoning_effort: Option<String>,
        allow_shell: bool,
        trust_mode: bool,
        auto_approve: bool,
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

    /// Run a Recursive Language Model (RLM) turn per Algorithm 1 of
    /// Zhang et al. (arXiv:2512.24601). The prompt is stored in the REPL
    /// as `context`; the root LLM only sees metadata.
    Rlm {
        /// The user's prompt — stored in REPL, NOT in the LLM context.
        content: String,
        /// The model to use for root LLM calls.
        model: String,
        /// The model to use for sub-LLM (llm_query) calls.
        child_model: String,
        /// Recursion budget for `sub_rlm()` calls. Paper experiments use
        /// depth=1; defaults set by the `/rlm` command.
        max_depth: u32,
    },

    /// Edit the last user message: remove the last user+assistant exchange
    /// from the session, then re-send with the new content.
    #[allow(dead_code)]
    EditLastTurn { new_message: String },

    /// Shutdown the engine
    Shutdown,
}

impl Op {
    /// Create a send message operation
    pub fn send(
        content: impl Into<String>,
        mode: AppMode,
        model: impl Into<String>,
        reasoning_effort: Option<String>,
        allow_shell: bool,
        trust_mode: bool,
        auto_approve: bool,
    ) -> Self {
        Op::SendMessage {
            content: content.into(),
            mode,
            model: model.into(),
            goal_objective: None,
            reasoning_effort,
            allow_shell,
            trust_mode,
            auto_approve,
        }
    }
}
