use serde_json::{Value, json};
use crate::tools::spec::{ToolContext, ToolError, ToolResult, ToolSpec};

pub struct FimEditTool;

#[async_trait::async_trait]
impl ToolSpec for FimEditTool {
    fn name(&self) -> &'static str { "fim_edit" }
    fn description(&self) -> &'static str { "Fill-in-the-middle edit via DeepSeek /beta FIM endpoint" }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"prefix_anchor":{"type":"string"},"suffix_anchor":{"type":"string"}},"required":["path","prefix_anchor","suffix_anchor"]})
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let path = input["path"].as_str().ok_or_else(|| ToolError::invalid_input("missing path"))?;
        let _ = (path, &input["prefix_anchor"], &input["suffix_anchor"]);
        Ok(ToolResult::text("FIM edit stub — wire to /beta endpoint in follow-up"))
    }
}
