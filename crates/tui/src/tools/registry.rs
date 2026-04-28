//! Tool registry for managing and executing tools.
//!
//! The registry provides:
//! - Dynamic tool registration
//! - Tool lookup by name
//! - Conversion to API Tool format
//! - Filtering by capability

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::client::DeepSeekClient;
use crate::models::Tool;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

// === Types ===

/// Registry that holds all available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolSpec>>,
    context: ToolContext,
}

impl ToolRegistry {
    /// Create a new empty registry with the given context.
    #[must_use]
    pub fn new(context: ToolContext) -> Self {
        Self {
            tools: HashMap::new(),
            context,
        }
    }

    /// Register a tool in the registry.
    pub fn register(&mut self, tool: Arc<dyn ToolSpec>) {
        let name = tool.name().to_string();
        if self.tools.insert(name.clone(), tool).is_some() {
            tracing::warn!("Overwriting existing tool: {}", name);
        }
    }

    /// Register multiple tools at once.
    pub fn register_all(&mut self, tools: Vec<Arc<dyn ToolSpec>>) {
        for tool in tools {
            self.register(tool);
        }
    }

    /// Get a tool by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolSpec>> {
        self.tools.get(name).cloned()
    }

    /// Check if a tool exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Get all registered tool names.
    #[must_use]
    #[allow(dead_code)]
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(std::string::String::as_str).collect()
    }

    /// Get the number of registered tools.
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Get all registered tools.
    #[must_use]
    pub fn all(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools.values().cloned().collect()
    }

    /// Execute a tool by name with the given input.
    pub async fn execute(&self, name: &str, input: Value) -> Result<String, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::not_available(format!("tool '{name}' is not registered")))?;

        let result = tool.execute(input, &self.context).await?;
        Ok(result.content)
    }

    /// Execute a tool by name, returning the full `ToolResult`.
    pub async fn execute_full(&self, name: &str, input: Value) -> Result<ToolResult, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::not_available(format!("tool '{name}' is not registered")))?;

        tool.execute(input, &self.context).await
    }

    /// Execute a tool with an optional context override.
    ///
    /// This is used for retrying tools with elevated sandbox policies.
    pub async fn execute_full_with_context(
        &self,
        name: &str,
        input: Value,
        context_override: Option<&ToolContext>,
    ) -> Result<ToolResult, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::not_available(format!("tool '{name}' is not registered")))?;

        let ctx = context_override.unwrap_or(&self.context);
        tool.execute(input, ctx).await
    }

    /// Get the current tool context.
    #[must_use]
    pub fn context(&self) -> &ToolContext {
        &self.context
    }

    /// Convert all tools to API Tool format for sending to the model.
    #[must_use]
    pub fn to_api_tools(&self) -> Vec<Tool> {
        self.tools
            .values()
            .map(|tool| Tool {
                tool_type: None,
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
                allowed_callers: Some(vec!["direct".to_string()]),
                defer_loading: Some(tool.defer_loading()),
                input_examples: None,
                strict: None,
                cache_control: None,
            })
            .collect()
    }

    /// Convert tools to API Tool format with optional cache control on the last tool.
    #[must_use]
    #[allow(dead_code)]
    pub fn to_api_tools_with_cache(&self, enable_cache: bool) -> Vec<Tool> {
        let mut tools = self.to_api_tools();
        if enable_cache && let Some(last) = tools.last_mut() {
            last.cache_control = Some(crate::models::CacheControl {
                cache_type: "ephemeral".to_string(),
            });
        }
        tools
    }

    /// Filter tools by capability.
    #[must_use]
    #[allow(dead_code)]
    pub fn filter_by_capability(&self, capability: ToolCapability) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| t.capabilities().contains(&capability))
            .cloned()
            .collect()
    }

    /// Get read-only tools.
    #[must_use]
    #[allow(dead_code)]
    pub fn read_only_tools(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| t.is_read_only())
            .cloned()
            .collect()
    }

    /// Get tools that require approval.
    #[must_use]
    #[allow(dead_code)]
    pub fn approval_required_tools(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| t.approval_requirement() == ApprovalRequirement::Required)
            .cloned()
            .collect()
    }

    /// Get tools that suggest approval.
    #[must_use]
    #[allow(dead_code)]
    pub fn approval_suggested_tools(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| {
                matches!(
                    t.approval_requirement(),
                    ApprovalRequirement::Suggest | ApprovalRequirement::Required
                )
            })
            .cloned()
            .collect()
    }

    /// Update the context (e.g., when workspace changes).
    #[allow(dead_code)]
    pub fn set_context(&mut self, context: ToolContext) {
        self.context = context;
    }

    /// Get a mutable reference to the current context.
    #[must_use]
    #[allow(dead_code)]
    pub fn context_mut(&mut self) -> &mut ToolContext {
        &mut self.context
    }

    /// Remove a tool by name.
    #[must_use]
    #[allow(dead_code)]
    pub fn remove(&mut self, name: &str) -> Option<Arc<dyn ToolSpec>> {
        self.tools.remove(name)
    }

    /// Clear all tools from the registry.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.tools.clear();
    }
}

/// Builder for constructing a `ToolRegistry` with common tools.
pub struct ToolRegistryBuilder {
    tools: Vec<Arc<dyn ToolSpec>>,
}

impl ToolRegistryBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Add a custom tool.
    #[must_use]
    pub fn with_tool(mut self, tool: Arc<dyn ToolSpec>) -> Self {
        self.tools.push(tool);
        self
    }

    /// Include file tools (read, write, edit, list).
    #[must_use]
    pub fn with_file_tools(self) -> Self {
        use super::file::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
        self.with_tool(Arc::new(ReadFileTool))
            .with_tool(Arc::new(WriteFileTool))
            .with_tool(Arc::new(EditFileTool))
            .with_tool(Arc::new(ListDirTool))
    }

    /// Include only read-only file tools (read, list).
    #[must_use]
    pub fn with_read_only_file_tools(self) -> Self {
        use super::file::{ListDirTool, ReadFileTool};
        self.with_tool(Arc::new(ReadFileTool))
            .with_tool(Arc::new(ListDirTool))
    }

    /// Include shell execution tool.
    #[must_use]
    pub fn with_shell_tools(self) -> Self {
        use super::shell::{ExecShellTool, ShellInteractTool, ShellWaitTool};
        self.with_tool(Arc::new(ExecShellTool))
            .with_tool(Arc::new(ShellWaitTool::new("exec_shell_wait")))
            .with_tool(Arc::new(ShellInteractTool::new("exec_shell_interact")))
            .with_tool(Arc::new(ShellWaitTool::new("exec_wait")))
            .with_tool(Arc::new(ShellInteractTool::new("exec_interact")))
    }

    /// Include search tools (`grep_files`).
    #[must_use]
    pub fn with_search_tools(self) -> Self {
        use super::file_search::FileSearchTool;
        use super::search::GrepFilesTool;
        self.with_tool(Arc::new(GrepFilesTool))
            .with_tool(Arc::new(FileSearchTool))
    }

    /// Include git inspection tools (`git_status`, `git_diff`).
    #[must_use]
    pub fn with_git_tools(self) -> Self {
        use super::git::{GitDiffTool, GitStatusTool};
        self.with_tool(Arc::new(GitStatusTool))
            .with_tool(Arc::new(GitDiffTool))
    }

    /// Include git history tools (`git_log`, `git_show`, `git_blame`).
    #[must_use]
    pub fn with_git_history_tools(self) -> Self {
        use super::git_history::{GitBlameTool, GitLogTool, GitShowTool};
        self.with_tool(Arc::new(GitLogTool))
            .with_tool(Arc::new(GitShowTool))
            .with_tool(Arc::new(GitBlameTool))
    }

    /// Include workspace diagnostics tool.
    #[must_use]
    pub fn with_diagnostics_tool(self) -> Self {
        use super::diagnostics::DiagnosticsTool;
        self.with_tool(Arc::new(DiagnosticsTool))
    }

    /// Include project mapping tools.
    #[must_use]
    pub fn with_project_tools(self) -> Self {
        use super::project::ProjectMapTool;
        self.with_tool(Arc::new(ProjectMapTool))
    }

    /// Include cargo test runner tool.
    #[must_use]
    pub fn with_test_runner_tool(self) -> Self {
        use super::test_runner::RunTestsTool;
        self.with_tool(Arc::new(RunTestsTool))
    }

    /// Include structured data validation tool (`validate_data`).
    #[must_use]
    pub fn with_validation_tools(self) -> Self {
        use super::validate_data::ValidateDataTool;
        self.with_tool(Arc::new(ValidateDataTool))
    }

    /// Include web search tools.
    #[must_use]
    pub fn with_web_tools(self) -> Self {
        use super::fetch_url::FetchUrlTool;
        use super::finance::FinanceTool;
        use super::web_run::WebRunTool;
        use super::web_search::WebSearchTool;
        self.with_tool(Arc::new(WebSearchTool))
            .with_tool(Arc::new(FetchUrlTool))
            .with_tool(Arc::new(FinanceTool::new()))
            .with_tool(Arc::new(WebRunTool))
    }

    /// Previously registered the OpenAI-style `multi_tool_use.parallel`
    /// meta-tool. DeepSeek-V4 has native parallel tool calls (multiple
    /// `tool_calls` entries in one assistant turn) and the meta-tool name
    /// triggered the model to hallucinate OpenAI-internal XML wrappers
    /// (`<multi_tool_use.parallel><tool_name>…</tool_name>…`) instead of
    /// emitting native calls. Kept as a no-op so existing callers compile;
    /// the engine's compatibility dispatcher still handles legacy emissions.
    #[must_use]
    pub fn with_parallel_tool(self) -> Self {
        self
    }

    /// Include request_user_input tool.
    #[must_use]
    pub fn with_user_input_tool(self) -> Self {
        use super::user_input::RequestUserInputTool;
        self.with_tool(Arc::new(RequestUserInputTool))
    }

    /// Include patch tools (`apply_patch`).
    #[must_use]
    pub fn with_patch_tools(self) -> Self {
        use super::apply_patch::ApplyPatchTool;
        self.with_tool(Arc::new(ApplyPatchTool))
    }

    /// Include the RLM tool (`rlm`). Runs the full recursive language-model
    /// loop on a long input (file or inline content); the long input never
    /// enters the calling model's context window. The Python REPL exposes
    /// `llm_query` / `llm_query_batched` / `rlm_query` / `rlm_query_batched`
    /// helpers for sub-LLM work — that's where parallel fan-out belongs.
    #[must_use]
    pub fn with_rlm_tool(self, client: Option<DeepSeekClient>, root_model: String) -> Self {
        use super::rlm::RlmTool;
        self.with_tool(Arc::new(RlmTool::new(client, root_model)))
    }

    /// Include the review tool.
    #[must_use]
    pub fn with_review_tool(self, client: Option<DeepSeekClient>, model: String) -> Self {
        use super::review::ReviewTool;
        self.with_tool(Arc::new(ReviewTool::new(client, model)))
    }

    /// Include note tool.
    #[must_use]
    pub fn with_note_tool(self) -> Self {
        use super::shell::NoteTool;
        self.with_tool(Arc::new(NoteTool))
    }

    /// Include MCP tools from a connected pool as first-class registry
    /// citizens. Each MCP tool is wrapped in a lightweight adapter that
    /// implements `ToolSpec`, so the unified `ToolRegistryBuilder` flow
    /// handles them alongside native tools.
    ///
    /// MCP tools are marked `defer_loading` by default (except discovery
    /// helpers) to keep the model-visible catalog compact.
    #[must_use]
    #[allow(dead_code)]
    pub fn with_mcp_tools(
        mut self,
        mcp_pool: std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpPool>>,
    ) -> Self {
        // Snapshot the current tool list from the pool (non-blocking).
        // The adapter lazily resolves at execution time via the pool.
        if let Ok(pool) = mcp_pool.try_lock() {
            for (name, tool) in pool.all_tools() {
                let adapter = Arc::new(McpToolAdapter {
                    name: name.clone(),
                    tool: tool.clone(),
                    pool: mcp_pool.clone(),
                });
                self.tools.push(adapter);
            }
        }
        self
    }

    /// Include all agent tools (file tools + shell + note + search + patch).
    #[must_use]
    pub fn with_agent_tools(self, allow_shell: bool) -> Self {
        let builder = self
            .with_file_tools()
            .with_note_tool()
            .with_search_tools()
            .with_web_tools()
            .with_user_input_tool()
            .with_parallel_tool()
            .with_patch_tools()
            .with_git_tools()
            .with_git_history_tools()
            .with_diagnostics_tool()
            .with_project_tools()
            .with_test_runner_tool()
            .with_validation_tools();

        if allow_shell {
            builder.with_shell_tools()
        } else {
            builder
        }
    }

    /// Include the full agent tool surface: every tool family the parent gets
    /// in Agent mode, including review, RLM, and the sub-agent management
    /// family (so children can recurse). Used by both the parent's Agent-mode
    /// registry build (`core/engine.rs`) and by every sub-agent
    /// (`subagent::SubAgentToolRegistry`) — keeping them in lockstep.
    ///
    /// `allow_shell` mirrors the session's shell permission. `manager` and
    /// `runtime` are the sub-agent runtime — children pass through their own
    /// runtime so grandchildren can spawn within the same depth/cancellation
    /// envelope.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn with_full_agent_surface(
        self,
        client: Option<DeepSeekClient>,
        model: String,
        manager: super::subagent::SharedSubAgentManager,
        runtime: super::subagent::SubAgentRuntime,
        allow_shell: bool,
        todo_list: super::todo::SharedTodoList,
        plan_state: super::plan::SharedPlanState,
    ) -> Self {
        self.with_agent_tools(allow_shell)
            .with_todo_tool(todo_list)
            .with_plan_tool(plan_state)
            .with_review_tool(client.clone(), model.clone())
            .with_rlm_tool(client, model)
            .with_subagent_tools(manager, runtime)
    }

    /// Include the todo tool with a shared `TodoList`.
    #[must_use]
    pub fn with_todo_tool(self, todo_list: super::todo::SharedTodoList) -> Self {
        use super::todo::{TodoAddTool, TodoListTool, TodoUpdateTool, TodoWriteTool};
        self.with_tool(Arc::new(TodoWriteTool::new(todo_list.clone())))
            .with_tool(Arc::new(TodoAddTool::new(todo_list.clone())))
            .with_tool(Arc::new(TodoUpdateTool::new(todo_list.clone())))
            .with_tool(Arc::new(TodoListTool::new(todo_list)))
    }

    /// Include the plan tool with a shared `PlanState`.
    #[must_use]
    pub fn with_plan_tool(self, plan_state: super::plan::SharedPlanState) -> Self {
        use super::plan::UpdatePlanTool;
        self.with_tool(Arc::new(UpdatePlanTool::new(plan_state)))
    }

    /// Include sub-agent management tools.
    #[must_use]
    pub fn with_subagent_tools(
        self,
        manager: super::subagent::SharedSubAgentManager,
        runtime: super::subagent::SubAgentRuntime,
    ) -> Self {
        use super::subagent::{
            AgentAssignTool, AgentCancelTool, AgentCloseTool, AgentListTool, AgentResultTool,
            AgentResumeTool, AgentSendInputTool, AgentSpawnTool, AgentWaitTool,
            DelegateToAgentTool, ReportAgentJobResultTool, SpawnAgentsOnCsvTool,
        };
        use super::swarm::{AgentSwarmTool, SwarmResultTool, SwarmStatusTool};

        self.with_tool(Arc::new(AgentSpawnTool::new(
            manager.clone(),
            runtime.clone(),
        )))
        .with_tool(Arc::new(AgentSpawnTool::with_name(
            manager.clone(),
            runtime.clone(),
            "spawn_agent",
        )))
        .with_tool(Arc::new(DelegateToAgentTool::new(
            manager.clone(),
            runtime.clone(),
        )))
        .with_tool(Arc::new(AgentSwarmTool::new(
            manager.clone(),
            runtime.clone(),
        )))
        .with_tool(Arc::new(SpawnAgentsOnCsvTool::new(
            manager.clone(),
            runtime.clone(),
        )))
        .with_tool(Arc::new(ReportAgentJobResultTool))
        .with_tool(Arc::new(SwarmStatusTool::new(
            runtime.context.workspace.clone(),
        )))
        .with_tool(Arc::new(SwarmResultTool::new(
            runtime.context.workspace.clone(),
        )))
        .with_tool(Arc::new(AgentResultTool::new(manager.clone())))
        .with_tool(Arc::new(AgentSendInputTool::new(
            manager.clone(),
            "send_input",
        )))
        .with_tool(Arc::new(AgentAssignTool::new(
            manager.clone(),
            "agent_assign",
        )))
        .with_tool(Arc::new(AgentAssignTool::new(
            manager.clone(),
            "assign_agent",
        )))
        .with_tool(Arc::new(AgentWaitTool::new(manager.clone(), "wait")))
        .with_tool(Arc::new(AgentSendInputTool::new(
            manager.clone(),
            "agent_send_input",
        )))
        .with_tool(Arc::new(AgentWaitTool::new(manager.clone(), "agent_wait")))
        .with_tool(Arc::new(AgentResumeTool::new(
            manager.clone(),
            runtime.clone(),
        )))
        .with_tool(Arc::new(AgentCloseTool::new(manager.clone())))
        .with_tool(Arc::new(AgentCancelTool::new(manager.clone())))
        .with_tool(Arc::new(AgentListTool::new(manager)))
    }

    /// Build the registry with the given context.
    #[must_use]
    pub fn build(self, context: ToolContext) -> ToolRegistry {
        let mut registry = ToolRegistry::new(context);
        registry.register_all(self.tools);
        registry
    }
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapter that wraps an MCP tool definition so it can live in the
/// unified `ToolRegistry` alongside native tools (§5.B).
#[allow(dead_code)]
struct McpToolAdapter {
    name: String,
    tool: crate::mcp::McpTool,
    pool: std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpPool>>,
}

#[async_trait::async_trait]
impl ToolSpec for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        // McpTool.description is Option<String>; fall back to the
        // prefixed name when absent.
        self.tool.description.as_deref().unwrap_or(&self.name)
    }

    fn input_schema(&self) -> Value {
        self.tool.input_schema.clone()
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Conservatively treat MCP tools as requiring approval and
        // network access unless they're known discovery helpers.
        let name_lower = self.name.to_lowercase();
        if name_lower.contains("list_mcp")
            || name_lower.contains("read_mcp")
            || name_lower.contains("mcp_read")
            || name_lower.contains("mcp_get_prompt")
        {
            vec![ToolCapability::ReadOnly]
        } else {
            vec![ToolCapability::Network, ToolCapability::RequiresApproval]
        }
    }

    fn defer_loading(&self) -> bool {
        // Discovery helpers stay loaded; everything else is deferred.
        let keep_loaded = matches!(
            self.name.as_str(),
            "list_mcp_resources"
                | "list_mcp_resource_templates"
                | "mcp_read_resource"
                | "read_mcp_resource"
                | "mcp_get_prompt"
        );
        !keep_loaded
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let mut pool = self.pool.lock().await;
        let result = pool
            .call_tool(&self.name, input)
            .await
            .map_err(|e| ToolError::execution_failed(format!("MCP tool failed: {e}")))?;
        let content = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
        Ok(ToolResult::success(content))
    }
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use crate::tools::ToolRegistryBuilder;
    use crate::tools::spec::{
        ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
    };

    use super::ToolRegistry;

    /// A simple test tool for unit testing
    struct TestTool {
        name: String,
        description: String,
    }

    #[async_trait::async_trait]
    impl ToolSpec for TestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn input_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            })
        }

        fn capabilities(&self) -> Vec<ToolCapability> {
            vec![ToolCapability::ReadOnly]
        }

        async fn execute(
            &self,
            input: Value,
            _context: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            let message = required_str(&input, "message")?;
            Ok(ToolResult::success(format!("Echo: {message}")))
        }
    }

    fn make_test_tool(name: &str) -> Arc<TestTool> {
        Arc::new(TestTool {
            name: name.to_string(),
            description: "A test tool".to_string(),
        })
    }

    #[test]
    fn test_registry_register_and_get() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        let tool = make_test_tool("test_tool");
        registry.register(tool);

        assert!(registry.contains("test_tool"));
        assert!(!registry.contains("nonexistent"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_registry_names() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        registry.register(make_test_tool("tool_a"));
        registry.register(make_test_tool("tool_b"));

        let names = registry.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[test]
    fn test_registry_to_api_tools() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        registry.register(make_test_tool("my_tool"));

        let api_tools = registry.to_api_tools();
        assert_eq!(api_tools.len(), 1);
        assert_eq!(api_tools[0].name, "my_tool");
        assert_eq!(api_tools[0].description, "A test tool");
    }

    #[test]
    fn test_registry_remove() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        registry.register(make_test_tool("removable"));
        assert!(registry.contains("removable"));

        let _ = registry.remove("removable");
        assert!(!registry.contains("removable"));
    }

    #[test]
    fn test_registry_clear() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        registry.register(make_test_tool("tool1"));
        registry.register(make_test_tool("tool2"));
        assert_eq!(registry.len(), 2);

        registry.clear();
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn test_registry_execute() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        registry.register(make_test_tool("echo"));

        let result = registry
            .execute("echo", json!({"message": "hello"}))
            .await
            .expect("execute");

        assert_eq!(result, "Echo: hello");
    }

    #[tokio::test]
    async fn test_registry_execute_unknown_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let registry = ToolRegistry::new(ctx);

        let result = registry.execute("nonexistent", json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_builder_basic() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let registry = ToolRegistryBuilder::new()
            .with_tool(make_test_tool("custom"))
            .build(ctx);

        assert!(registry.contains("custom"));
    }

    #[test]
    fn test_filter_by_capability() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        registry.register(make_test_tool("readonly_tool"));

        let readonly = registry.filter_by_capability(ToolCapability::ReadOnly);
        assert_eq!(readonly.len(), 1);

        let writes = registry.filter_by_capability(ToolCapability::WritesFiles);
        assert_eq!(writes.len(), 0);
    }

    #[test]
    fn test_read_only_tools() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let mut registry = ToolRegistry::new(ctx);

        registry.register(make_test_tool("reader"));

        let readonly = registry.read_only_tools();
        assert_eq!(readonly.len(), 1);
        assert_eq!(readonly[0].name(), "reader");
    }

    #[test]
    fn test_builder_with_web_tools_includes_finance() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let registry = ToolRegistryBuilder::new().with_web_tools().build(ctx);

        assert!(registry.contains("finance"));
    }

    #[test]
    fn test_builder_with_agent_tools_includes_finance() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let registry = ToolRegistryBuilder::new()
            .with_agent_tools(false)
            .build(ctx);

        assert!(registry.contains("finance"));
    }
}
