//! Tools for Duo mode: Player-Coach autocoding workflow.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::duo::{
    DuoPhase, SharedDuoSession, generate_coach_prompt, generate_player_prompt, session_summary,
};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, required_str,
};

/// Initialize an autocoding session with requirements.
pub struct DuoInitTool {
    session: SharedDuoSession,
}

impl DuoInitTool {
    #[must_use]
    pub fn new(session: SharedDuoSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for DuoInitTool {
    fn name(&self) -> &'static str {
        "duo_init"
    }

    fn description(&self) -> &'static str {
        "Initialize a Duo autocoding session with requirements. Returns session summary."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "requirements": {
                    "type": "string",
                    "description": "The requirements document (source of truth). Should be structured as a checklist."
                },
                "max_turns": {
                    "type": "integer",
                    "description": "Maximum turns before timeout (default: 10)"
                },
                "session_name": {
                    "type": "string",
                    "description": "Optional human-readable session name (e.g., 'auth-feature')"
                },
                "approval_threshold": {
                    "type": "number",
                    "description": "Minimum compliance score for approval (0-1, default: 0.9)"
                }
            },
            "required": ["requirements"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let requirements = required_str(&input, "requirements")?;
        let max_turns = input
            .get("max_turns")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let session_name = optional_str(&input, "session_name").map(str::to_string);
        let approval_threshold = input.get("approval_threshold").and_then(|v| v.as_f64());

        let mut session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock Duo session"))?;

        let state = session.start_session(
            requirements.to_string(),
            session_name,
            max_turns,
            approval_threshold,
        );

        let summary = state.summary();
        Ok(ToolResult::success(format!(
            "Duo session initialized. Ready for player phase.\n\n{}",
            summary
        )))
    }
}

/// Generate the player prompt for implementation.
pub struct DuoPlayerTool {
    session: SharedDuoSession,
}

impl DuoPlayerTool {
    #[must_use]
    pub fn new(session: SharedDuoSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for DuoPlayerTool {
    fn name(&self) -> &'static str {
        "duo_player"
    }

    fn description(&self) -> &'static str {
        "Generate the player prompt for implementation. Must be in Init or Player phase. Call after implementing to advance to Coach phase."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "implementation_summary": {
                    "type": "string",
                    "description": "Optional summary of implementation work done (recorded in history)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let implementation_summary = optional_str(&input, "implementation_summary")
            .map(str::to_string)
            .unwrap_or_else(|| "Implementation in progress".to_string());

        let mut session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock Duo session"))?;

        let state = session
            .get_active_mut()
            .ok_or_else(|| ToolError::invalid_input("No active session. Call duo_init first."))?;

        // Check we're in a valid phase for player
        match state.phase {
            DuoPhase::Init | DuoPhase::Player => {
                // Generate prompt first
                let prompt = generate_player_prompt(state);

                // Advance to Coach phase
                state
                    .advance_to_coach(implementation_summary)
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?;

                Ok(ToolResult::success(format!(
                    "=== PLAYER PROMPT ===\n\n{}\n\n---\nAdvanced to Coach phase. Use duo_coach for verification.",
                    prompt
                )))
            }
            DuoPhase::Coach => Err(ToolError::invalid_input(
                "Already in Coach phase. Use duo_coach to get verification prompt.",
            )),
            DuoPhase::Approved => Err(ToolError::invalid_input(
                "Session already approved. Start a new session with duo_init.",
            )),
            DuoPhase::Timeout => Err(ToolError::invalid_input(
                "Session timed out. Start a new session with duo_init.",
            )),
        }
    }
}

/// Generate the coach prompt for validation.
pub struct DuoCoachTool {
    session: SharedDuoSession,
}

impl DuoCoachTool {
    #[must_use]
    pub fn new(session: SharedDuoSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for DuoCoachTool {
    fn name(&self) -> &'static str {
        "duo_coach"
    }

    fn description(&self) -> &'static str {
        "Generate the coach prompt for validation. Must be in Coach phase. Does NOT advance state."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock Duo session"))?;

        let state = session
            .get_active()
            .ok_or_else(|| ToolError::invalid_input("No active session. Call duo_init first."))?;

        if state.phase != DuoPhase::Coach {
            return Err(ToolError::invalid_input(format!(
                "Expected Coach phase, but current phase is {}. Use duo_player first.",
                state.phase
            )));
        }

        let prompt = generate_coach_prompt(state);

        Ok(ToolResult::success(format!(
            "=== COACH PROMPT ===\n\n{}\n\n---\nAfter verification, use duo_advance with feedback and approval status.",
            prompt
        )))
    }
}

/// Advance the session after coach review.
pub struct DuoAdvanceTool {
    session: SharedDuoSession,
}

impl DuoAdvanceTool {
    #[must_use]
    pub fn new(session: SharedDuoSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for DuoAdvanceTool {
    fn name(&self) -> &'static str {
        "duo_advance"
    }

    fn description(&self) -> &'static str {
        "Advance the session after coach review. Updates turn count and records feedback. Returns new status."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "feedback": {
                    "type": "string",
                    "description": "The coach's feedback text (compliance checklist and actions needed)"
                },
                "approved": {
                    "type": "boolean",
                    "description": "Whether the coach approved the implementation (look for 'COACH APPROVED')"
                },
                "compliance_score": {
                    "type": "number",
                    "description": "Optional compliance score (0-1) based on checklist items satisfied"
                }
            },
            "required": ["feedback", "approved"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let feedback = required_str(&input, "feedback")?;
        let approved = input
            .get("approved")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| ToolError::missing_field("approved"))?;
        let compliance_score = input.get("compliance_score").and_then(|v| v.as_f64());

        let mut session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock Duo session"))?;

        let state = session
            .get_active_mut()
            .ok_or_else(|| ToolError::invalid_input("No active session. Call duo_init first."))?;

        if state.phase != DuoPhase::Coach {
            return Err(ToolError::invalid_input(format!(
                "Expected Coach phase, but current phase is {}",
                state.phase
            )));
        }

        // Advance the turn
        state
            .advance_turn(feedback.to_string(), approved, compliance_score)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Determine status message based on new phase
        let status_msg = match state.phase {
            DuoPhase::Approved => "🎉 APPROVED! All requirements verified.",
            DuoPhase::Timeout => "⏰ TIMEOUT. Max turns reached without approval.",
            DuoPhase::Player => "🔄 Continuing to next player turn...",
            _ => "Session updated.",
        };

        let summary = state.summary();
        let mut result = ToolResult::success(format!("{}\n\n{}", status_msg, summary));
        result.metadata = Some(json!({
            "phase": state.phase.to_string(),
            "status": state.status.to_string(),
            "turn": state.current_turn,
            "max_turns": state.max_turns,
            "approved": approved,
            "compliance_score": compliance_score,
            "is_complete": state.is_complete(),
        }));

        Ok(result)
    }
}

/// Show the current session status.
pub struct DuoStatusTool {
    session: SharedDuoSession,
}

impl DuoStatusTool {
    #[must_use]
    pub fn new(session: SharedDuoSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for DuoStatusTool {
    fn name(&self) -> &'static str {
        "duo_status"
    }

    fn description(&self) -> &'static str {
        "Show the current Duo session status including phase, turn count, and requirements."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock Duo session"))?;

        Ok(ToolResult::success(session_summary(&session)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duo::new_shared_duo_session;

    #[test]
    fn test_duo_init_tool_schema() {
        let session = new_shared_duo_session();
        let tool = DuoInitTool::new(session);

        assert_eq!(tool.name(), "duo_init");
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);

        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("requirements"))
        );
    }

    #[test]
    fn test_duo_player_tool_schema() {
        let session = new_shared_duo_session();
        let tool = DuoPlayerTool::new(session);

        assert_eq!(tool.name(), "duo_player");
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn test_duo_coach_tool_schema() {
        let session = new_shared_duo_session();
        let tool = DuoCoachTool::new(session);

        assert_eq!(tool.name(), "duo_coach");
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn test_duo_advance_tool_schema() {
        let session = new_shared_duo_session();
        let tool = DuoAdvanceTool::new(session);

        assert_eq!(tool.name(), "duo_advance");
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);

        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("feedback")));
        assert!(required.contains(&json!("approved")));
    }

    #[test]
    fn test_duo_status_tool_schema() {
        let session = new_shared_duo_session();
        let tool = DuoStatusTool::new(session);

        assert_eq!(tool.name(), "duo_status");
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }
}
