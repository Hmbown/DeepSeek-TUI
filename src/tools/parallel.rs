//! Tool wrapper for executing multiple tool calls in parallel.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct MultiToolUseParallelTool;

#[async_trait]
impl ToolSpec for MultiToolUseParallelTool {
    fn name(&self) -> &'static str {
        "multi_tool_use.parallel"
    }

    fn description(&self) -> &'static str {
        "Execute multiple tool calls in parallel and return their results."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool_uses": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "recipient_name": { "type": "string" },
                            "parameters": { "type": "object" }
                        },
                        "required": ["recipient_name", "parameters"]
                    }
                }
            },
            "required": ["tool_uses"]
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
        Err(ToolError::execution_failed(
            "multi_tool_use.parallel must be handled by the engine",
        ))
    }
}
