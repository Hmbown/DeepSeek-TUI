//! Calculator tool for evaluating arithmetic expressions.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, required_str,
};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
struct CalculatorResponse {
    value: String,
    result: String,
}

pub struct CalculatorTool;

#[async_trait]
impl ToolSpec for CalculatorTool {
    fn name(&self) -> &'static str {
        "calculator"
    }

    fn description(&self) -> &'static str {
        "Evaluate a basic arithmetic expression."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "expression": { "type": "string" },
                "prefix": { "type": "string" },
                "suffix": { "type": "string" }
            },
            "required": ["expression"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let expression = required_str(&input, "expression")?;
        let prefix = optional_str(&input, "prefix").unwrap_or("");
        let suffix = optional_str(&input, "suffix").unwrap_or("");

        let value = meval::eval_str(expression)
            .map_err(|e| ToolError::invalid_input(format!("Invalid expression: {e}")))?;

        let rendered = format_value(value);
        let result = format!("{prefix}{rendered}{suffix}");

        ToolResult::json(&CalculatorResponse {
            value: rendered,
            result,
        })
        .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

fn format_value(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{:.0}", value)
    } else {
        let rendered = format!("{value}");
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_expression() {
        let value = meval::eval_str("2 + 2").unwrap();
        assert_eq!(format_value(value), "4");
    }
}
