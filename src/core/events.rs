//! Events emitted by the core engine to the UI.
//!
//! These events flow from the engine to the TUI via a channel,
//! enabling non-blocking, real-time updates.

use serde_json::Value;

use crate::models::Usage;
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::subagent::SubAgentResult;

/// Events emitted by the engine to update the UI.
#[derive(Debug, Clone)]
pub enum Event {
    // === Streaming Events ===
    /// A new message block has started
    MessageStarted { index: usize },

    /// Incremental text content delta
    MessageDelta { index: usize, content: String },

    /// Message block completed
    MessageComplete { index: usize },

    /// Thinking block started
    ThinkingStarted { index: usize },

    /// Incremental thinking content delta
    ThinkingDelta { index: usize, content: String },

    /// Thinking block completed
    ThinkingComplete { index: usize },

    // === Tool Events ===
    /// Tool call initiated
    ToolCallStarted {
        id: String,
        name: String,
        input: Value,
    },

    /// Tool execution progress (for long-running tools)
    ToolCallProgress { id: String, output: String },

    /// Tool call completed
    ToolCallComplete {
        id: String,
        name: String,
        result: Result<ToolResult, ToolError>,
    },

    // === Turn Lifecycle ===
    /// A new turn has started (user sent a message)
    TurnStarted,

    /// The turn is complete (no more tool calls)
    TurnComplete { usage: Usage },

    // === Sub-Agent Events (for RLM mode) ===
    /// A sub-agent has been spawned
    AgentSpawned { id: String, prompt: String },

    /// Sub-agent progress update
    AgentProgress { id: String, status: String },

    /// Sub-agent completed
    AgentComplete { id: String, result: String },

    /// Sub-agent listing
    AgentList { agents: Vec<SubAgentResult> },

    // === System Events ===
    /// An error occurred
    Error { message: String, recoverable: bool },

    /// Status message for UI display
    Status { message: String },

    /// Pause terminal input events (for interactive subprocesses)
    PauseEvents,

    /// Resume terminal input events after subprocess completion
    ResumeEvents,

    /// Request user approval for a tool call
    ApprovalRequired {
        id: String,
        tool_name: String,
        description: String,
    },

    /// Request user decision after sandbox denial
    ElevationRequired {
        tool_id: String,
        tool_name: String,
        command: Option<String>,
        denial_reason: String,
        blocked_network: bool,
        blocked_write: bool,
    },
}

impl Event {
    /// Create a new error event
    pub fn error(message: impl Into<String>, recoverable: bool) -> Self {
        Event::Error {
            message: message.into(),
            recoverable,
        }
    }

    /// Create a new status event
    pub fn status(message: impl Into<String>) -> Self {
        Event::Status {
            message: message.into(),
        }
    }
}
