//! JSONL event types matching the Codex CLI event stream format.
//!
//! Emitted to stdout when `--json` is set.

use serde::Serialize;

/// A single JSONL event, matching Codex CLI output format.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum CodexEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted {
        thread_id: String,
    },
    #[serde(rename = "turn.started")]
    TurnStarted {
        turn_id: String,
        thread_id: String,
    },
    #[serde(rename = "item.completed")]
    ItemCompleted {
        item: Item,
    },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        thread_id: String,
        usage: UsageInfo,
    },
    #[serde(rename = "thread.completed")]
    ThreadCompleted {
        thread_id: String,
    },
    #[serde(rename = "error")]
    Error {
        error: String,
    },
}

/// A completed item within a turn.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Item {
    #[serde(rename = "agent_message")]
    AgentMessage {
        thread_id: String,
        text: String,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        thread_id: String,
        text: String,
    },
    #[serde(rename = "command_execution")]
    CommandExecution {
        thread_id: String,
        command: String,
        output: String,
        exit_code: i32,
    },
}

/// Token usage info for turn completion.
#[derive(Debug, Clone, Serialize)]
pub struct UsageInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cached_input_tokens: u32,
    pub output_tokens_details: OutputTokensDetails,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputTokensDetails {
    pub reasoning_tokens: u32,
}

impl CodexEvent {
    pub fn thread_started(thread_id: &str) -> Self {
        Self::ThreadStarted {
            thread_id: thread_id.to_string(),
        }
    }

    pub fn turn_started(turn_id: &str, thread_id: &str) -> Self {
        Self::TurnStarted {
            turn_id: turn_id.to_string(),
            thread_id: thread_id.to_string(),
        }
    }

    pub fn agent_message(thread_id: &str, text: &str) -> Self {
        Self::ItemCompleted {
            item: Item::AgentMessage {
                thread_id: thread_id.to_string(),
                text: text.to_string(),
            },
        }
    }

    pub fn reasoning(thread_id: &str, text: &str) -> Self {
        Self::ItemCompleted {
            item: Item::Reasoning {
                thread_id: thread_id.to_string(),
                text: text.to_string(),
            },
        }
    }

    pub fn command_execution(thread_id: &str, command: &str, output: &str, exit_code: i32) -> Self {
        Self::ItemCompleted {
            item: Item::CommandExecution {
                thread_id: thread_id.to_string(),
                command: command.to_string(),
                output: output.to_string(),
                exit_code,
            },
        }
    }

    pub fn turn_completed(
        thread_id: &str,
        input_tokens: u32,
        output_tokens: u32,
        cached_input_tokens: u32,
        reasoning_tokens: u32,
    ) -> Self {
        Self::TurnCompleted {
            thread_id: thread_id.to_string(),
            usage: UsageInfo {
                input_tokens,
                output_tokens,
                cached_input_tokens,
                output_tokens_details: OutputTokensDetails { reasoning_tokens },
            },
        }
    }

    pub fn thread_completed(thread_id: &str) -> Self {
        Self::ThreadCompleted {
            thread_id: thread_id.to_string(),
        }
    }

    pub fn error(message: &str) -> Self {
        Self::Error {
            error: message.to_string(),
        }
    }
}
