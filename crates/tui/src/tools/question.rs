//! `question` tool — the model asks the user a clarifying question and
//! waits for a freeform text response.
//!
//! In TUI mode the question is displayed in a highlighted modal; the user
//! types a response and presses Enter. In auto-approve mode (YOLO, `--auto`)
//! the question is logged at INFO level and the tool returns `"proceed"`
//! without blocking.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Payload for the `question` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionRequest {
    /// The clarifying question the model wants to ask the user.
    pub text: String,
}

impl QuestionRequest {
    /// Parse and validate a `question` tool input.
    pub fn from_value(value: &Value) -> Result<Self, ToolError> {
        let request: QuestionRequest =
            serde_json::from_value(value.clone()).map_err(|e| {
                ToolError::invalid_input(format!("Invalid question payload: {e}"))
            })?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ToolError> {
        if self.text.trim().is_empty() {
            return Err(ToolError::invalid_input(
                "question.text must be non-empty",
            ));
        }
        Ok(())
    }
}

/// Response type returned to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResponse {
    pub answer: String,
}

impl QuestionResponse {
    pub fn new(answer: impl Into<String>) -> Self {
        Self {
            answer: answer.into(),
        }
    }
}

pub struct QuestionTool;

#[async_trait]
impl ToolSpec for QuestionTool {
    fn name(&self) -> &'static str {
        "question"
    }

    fn description(&self) -> &'static str {
        "Ask the user a clarifying question and wait for a freeform text response. \
         Use this when you need additional context, clarification, or a decision \
         that cannot be derived from the available information."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The clarifying question to ask the user."
                }
            },
            "required": ["text"],
            "additionalProperties": false
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
            "question must be handled by the engine",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_question_request() {
        let req = QuestionRequest {
            text: "What port should I use?".to_string(),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn rejects_empty_text() {
        let req = QuestionRequest {
            text: "   ".to_string(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn deserializes_from_json() {
        let value = json!({ "text": "Which branch to target?" });
        let req = QuestionRequest::from_value(&value).unwrap();
        assert_eq!(req.text, "Which branch to target?");
    }

    #[test]
    fn rejects_missing_text() {
        let value = json!({});
        assert!(QuestionRequest::from_value(&value).is_err());
    }

    #[test]
    fn response_serializes() {
        let resp = QuestionResponse::new("use port 8080");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["answer"], "use port 8080");
    }
}
