//! Approval + user-input handshake for the agent loop.
//!
//! Extracted from `core/engine.rs` (P1.3). The agent loop blocks on these
//! two futures whenever a tool requires explicit approval (`await_tool_approval`)
//! or whenever a tool requests live user input (`await_user_input`). Channels
//! and engine state stay private to the parent module.

use crate::core::events::Event;
use crate::tools::spec::ToolError;
use crate::tools::user_input::{UserInputRequest, UserInputResponse};

use super::Engine;

#[derive(Debug, Clone)]
pub(super) enum ApprovalDecision {
    Approved {
        id: String,
    },
    Denied {
        id: String,
    },
    /// Retry a tool with an elevated sandbox policy.
    RetryWithPolicy {
        id: String,
        policy: crate::sandbox::SandboxPolicy,
    },
}

#[derive(Debug, Clone)]
pub(super) enum UserInputDecision {
    Submitted {
        id: String,
        response: UserInputResponse,
    },
    Cancelled {
        id: String,
    },
}

/// Result of awaiting tool approval from the user.
#[derive(Debug)]
pub(super) enum ApprovalResult {
    /// User approved the tool execution.
    Approved,
    /// User denied the tool execution.
    Denied,
    /// User requested retry with an elevated sandbox policy.
    RetryWithPolicy(crate::sandbox::SandboxPolicy),
}

impl Engine {
    pub(super) async fn await_tool_approval(
        &mut self,
        tool_id: &str,
    ) -> Result<ApprovalResult, ToolError> {
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    return Err(ToolError::execution_failed(
                        "Request cancelled while awaiting approval".to_string(),
                    ));
                }
                decision = self.rx_approval.recv() => {
                    let Some(decision) = decision else {
                        return Err(ToolError::execution_failed(
                            "Approval channel closed".to_string(),
                        ));
                    };
                    match decision {
                        ApprovalDecision::Approved { id } if id == tool_id => {
                            return Ok(ApprovalResult::Approved);
                        }
                        ApprovalDecision::Denied { id } if id == tool_id => {
                            return Ok(ApprovalResult::Denied);
                        }
                        ApprovalDecision::RetryWithPolicy { id, policy } if id == tool_id => {
                            return Ok(ApprovalResult::RetryWithPolicy(policy));
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    pub(super) async fn await_user_input(
        &mut self,
        tool_id: &str,
        request: UserInputRequest,
    ) -> Result<UserInputResponse, ToolError> {
        let _ = self
            .tx_event
            .send(Event::UserInputRequired {
                id: tool_id.to_string(),
                request,
            })
            .await;

        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    return Err(ToolError::execution_failed(
                        "Request cancelled while awaiting user input".to_string(),
                    ));
                }
                decision = self.rx_user_input.recv() => {
                    let Some(decision) = decision else {
                        return Err(ToolError::execution_failed(
                            "User input channel closed".to_string(),
                        ));
                    };
                    match decision {
                        UserInputDecision::Submitted { id, response } if id == tool_id => {
                            return Ok(response);
                        }
                        UserInputDecision::Cancelled { id } if id == tool_id => {
                            return Err(ToolError::execution_failed(
                                "User input cancelled".to_string(),
                            ));
                        }
                        _ => continue,
                    }
                }
            }
        }
    }
}
