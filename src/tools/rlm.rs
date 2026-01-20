//! Tools for RLM mode: evaluating expressions and issuing sub-queries.
//!
//! Implements recursive sub-queries per the RLM paper, with depth limiting
//! to prevent infinite recursion.

use async_trait::async_trait;
use regex::Regex;
use serde_json::{Value, json};

use crate::client::DeepSeekClient;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt, Tool, Usage};
use crate::rlm::{
    RlmContext, SharedRlmSession, context_id_from_path, eval_expr_mut, session_summary,
    unique_context_id,
};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolError, ToolResult, ToolSpec, optional_str,
    optional_u64, required_str,
};

const DEFAULT_QUERY_MAX_TOKENS: u32 = 2048;
const MAX_QUERY_MAX_TOKENS: u32 = 8192;
const MAX_EXEC_OUTPUT_CHARS: usize = 12_000;
const MAX_QUERY_CHARS: usize = 400_000;
const DEFAULT_AUTO_CHUNK_MAX_CHARS: usize = 20_000;

/// Maximum recursion depth for RLM sub-calls (per RLM paper)
const MAX_RECURSION_DEPTH: u32 = 3;
/// Maximum tool iterations within a single sub-call
const MAX_TOOL_ITERATIONS: u32 = 10;

fn normalize_load_path(raw: &str) -> Result<String, ToolError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ToolError::invalid_input("Path is required"));
    }

    if let Some(stripped) = trimmed.strip_prefix('@') {
        let stripped = stripped.trim();
        if stripped.is_empty() {
            return Err(ToolError::invalid_input(
                "Path is required after '@' prefix",
            ));
        }
        let stripped = stripped.trim_start_matches(['/', '\\']);
        if stripped.is_empty() {
            return Err(ToolError::invalid_input(
                "Path is required after '@' prefix",
            ));
        }
        return Ok(stripped.to_string());
    }

    Ok(trimmed.to_string())
}

/// Execute an RLM expression against the current context.
pub struct RlmExecTool {
    session: SharedRlmSession,
}

impl RlmExecTool {
    #[must_use]
    pub fn new(session: SharedRlmSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for RlmExecTool {
    fn name(&self) -> &'static str {
        "rlm_exec"
    }

    fn description(&self) -> &'static str {
        "Execute an RLM expression against the current context. Supports: len, line_count, lines(), search(), chunk(), chunk_sections(), chunk_lines(), chunk_auto(), vars/get/set/append/del."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "RLM expression(s) to evaluate"
                },
                "context_id": {
                    "type": "string",
                    "description": "Optional context id (defaults to active context)"
                }
            },
            "required": ["code"]
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
        input: Value,
        _context: &crate::tools::spec::ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let code = required_str(&input, "code")?;
        let context_id = optional_str(&input, "context_id").map(str::to_string);

        let mut session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock RLM session"))?;

        let ctx_id = context_id.unwrap_or_else(|| session.active_context.clone());
        let ctx = session
            .get_context_mut(&ctx_id)
            .ok_or_else(|| ToolError::invalid_input(format!("Context '{ctx_id}' not loaded")))?;

        let output =
            eval_script_mut(ctx, code).map_err(|e| ToolError::execution_failed(e.to_string()))?;

        let truncated = if output.len() > MAX_EXEC_OUTPUT_CHARS {
            let snippet = truncate_to_boundary(&output, MAX_EXEC_OUTPUT_CHARS);
            format!(
                "{}\n\n[output truncated to {} chars]",
                snippet, MAX_EXEC_OUTPUT_CHARS
            )
        } else {
            output
        };

        Ok(ToolResult::success(truncated))
    }
}

/// Load a file into the shared RLM session.
pub struct RlmLoadTool {
    session: SharedRlmSession,
}

impl RlmLoadTool {
    #[must_use]
    pub fn new(session: SharedRlmSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for RlmLoadTool {
    fn name(&self) -> &'static str {
        "rlm_load"
    }

    fn description(&self) -> &'static str {
        "Load a file into the RLM context store. Returns the context_id and stats. Use @path for workspace-relative loads."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to load (prefix with @ for workspace-relative paths)"
                },
                "context_id": {
                    "type": "string",
                    "description": "Optional context id to reuse (defaults to filename)"
                }
            },
            "required": ["path"]
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
        input: Value,
        context: &crate::tools::spec::ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path = required_str(&input, "path")?;
        let normalized = normalize_load_path(path)?;
        let context_id = optional_str(&input, "context_id").map(str::to_string);

        let resolved = context.resolve_path(&normalized)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock RLM session"))?;

        let base_id = context_id.unwrap_or_else(|| context_id_from_path(&resolved));
        let id = if session.contexts.contains_key(&base_id) {
            base_id.clone()
        } else {
            unique_context_id(&session, &base_id)
        };

        let (line_count, char_count) = session
            .load_file(&id, &resolved)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        let mut result = ToolResult::success(format!(
            "Loaded {} as '{}' ({} lines, {} chars)",
            resolved.display(),
            id,
            line_count,
            char_count
        ));
        result.metadata = Some(json!({
            "context_id": id,
            "line_count": line_count,
            "char_count": char_count,
            "source_path": resolved.to_string_lossy(),
        }));
        Ok(result)
    }
}

/// Summarize RLM session state.
pub struct RlmStatusTool {
    session: SharedRlmSession,
}

impl RlmStatusTool {
    #[must_use]
    pub fn new(session: SharedRlmSession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl ToolSpec for RlmStatusTool {
    fn name(&self) -> &'static str {
        "rlm_status"
    }

    fn description(&self) -> &'static str {
        "Show RLM session status (contexts, usage, variables)."
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
        _context: &crate::tools::spec::ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock RLM session"))?;
        Ok(ToolResult::success(session_summary(&session)))
    }
}

/// Execute a sub-query against a chunk of the active context.
/// Supports recursive sub-calls per the RLM paper with depth limiting.
pub struct RlmQueryTool {
    session: SharedRlmSession,
    client: Option<DeepSeekClient>,
    model: String,
    current_depth: u32,
}

impl RlmQueryTool {
    #[must_use]
    pub fn new(session: SharedRlmSession, client: Option<DeepSeekClient>, model: String) -> Self {
        Self {
            session,
            client,
            model,
            current_depth: 0,
        }
    }

    /// Create a sub-query tool at increased depth for recursive calls
    fn with_depth(&self, depth: u32) -> Self {
        Self {
            session: self.session.clone(),
            client: self.client.clone(),
            model: self.model.clone(),
            current_depth: depth,
        }
    }

    /// Check if recursive sub-calls are allowed at current depth
    fn can_recurse(&self) -> bool {
        self.current_depth < MAX_RECURSION_DEPTH
    }
}

#[async_trait]
impl ToolSpec for RlmQueryTool {
    fn name(&self) -> &'static str {
        "rlm_query"
    }

    fn description(&self) -> &'static str {
        "Run a focused LLM query over a context slice. Provide line/char range or chunk index; use batch for multiple queries or auto_chunks for chunk_auto batching."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Question to answer about the chunk" },
                "context_id": { "type": "string", "description": "Optional context id (defaults to active context)" },
                "chunk_index": { "type": "integer", "description": "Chunk index from chunk() output" },
                "chunk_size": { "type": "integer", "description": "Chunk size (default: 2000)" },
                "overlap": { "type": "integer", "description": "Chunk overlap (default: 200)" },
                "line_start": { "type": "integer", "description": "Start line (1-based)" },
                "line_end": { "type": "integer", "description": "End line (1-based)" },
                "char_start": { "type": "integer", "description": "Start char offset" },
                "char_end": { "type": "integer", "description": "End char offset" },
                "section_index": { "type": "integer", "description": "Section index from chunk_sections()" },
                "section_size": { "type": "integer", "description": "Section chunk size (default: 20000)" },
                "mode": { "type": "string", "description": "analysis (default) or verify" },
                "store_as": { "type": "string", "description": "Store the FINAL answer in a variable" },
                "max_tokens": { "type": "integer", "description": "Override max tokens for the sub-call" },
                "auto_chunks": {
                    "type": "boolean",
                    "description": "Use chunk_auto and run the same query for each chunk"
                },
                "auto_max_chars": {
                    "type": "integer",
                    "description": "Max chars per auto chunk (default: 20000)"
                },
                "auto_max_chunks": {
                    "type": "integer",
                    "description": "Optional limit on auto chunk count"
                },
                "batch": {
                    "type": "array",
                    "description": "Batch multiple queries into a single call",
                    "items": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "context_id": { "type": "string" },
                            "chunk_index": { "type": "integer" },
                            "chunk_size": { "type": "integer" },
                            "overlap": { "type": "integer" },
                            "line_start": { "type": "integer" },
                            "line_end": { "type": "integer" },
                            "char_start": { "type": "integer" },
                            "char_end": { "type": "integer" },
                            "section_index": { "type": "integer" },
                            "section_size": { "type": "integer" }
                        },
                        "required": ["query"]
                    }
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::RequiresApproval]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(
        &self,
        input: Value,
        _context: &crate::tools::spec::ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let Some(client) = self.client.clone() else {
            return Err(ToolError::not_available("RLM query requires an API client"));
        };

        let auto_chunks = input
            .get("auto_chunks")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let batch_items = input.get("batch").and_then(|v| v.as_array());
        if auto_chunks && batch_items.is_some() {
            return Err(ToolError::invalid_input(
                "auto_chunks cannot be combined with batch queries".to_string(),
            ));
        }
        let query = if auto_chunks || batch_items.is_none() {
            required_str(&input, "query")?.to_string()
        } else {
            optional_str(&input, "query").unwrap_or("").to_string()
        };
        let context_id = optional_str(&input, "context_id").map(str::to_string);
        let mode = optional_str(&input, "mode").unwrap_or("analysis");
        let store_as = optional_str(&input, "store_as").map(str::to_string);
        let default_max = default_query_max_tokens(&self.model);
        let max_tokens = optional_u64(&input, "max_tokens", u64::from(default_max))
            .clamp(256, u64::from(MAX_QUERY_MAX_TOKENS)) as u32;
        let (prompt, used_context_id, batch_count) = if auto_chunks {
            let max_chars = optional_u64(
                &input,
                "auto_max_chars",
                DEFAULT_AUTO_CHUNK_MAX_CHARS as u64,
            ) as usize;
            let max_chunks = optional_u64(&input, "auto_max_chunks", 0) as usize;
            let (chunks, ctx_id) = self.extract_auto_chunks(context_id.as_deref(), max_chars)?;
            if chunks.is_empty() {
                return Err(ToolError::invalid_input(
                    "No chunks available for auto_chunks".to_string(),
                ));
            }
            if max_chunks > 0 && chunks.len() > max_chunks {
                return Err(ToolError::invalid_input(format!(
                    "auto_chunks produced {} chunks; reduce input or set auto_max_chunks",
                    chunks.len()
                )));
            }
            let mut queries = Vec::new();
            let mut total_len = 0usize;
            for (idx, chunk) in chunks.iter().enumerate() {
                let task = format!(
                    "TASK {}:\nContext:\n{}\n\nQuestion:\n{}\n",
                    idx + 1,
                    chunk,
                    query
                );
                total_len = total_len.saturating_add(task.len());
                if total_len > MAX_QUERY_CHARS {
                    return Err(ToolError::invalid_input(
                        "auto_chunks payload too large; lower auto_max_chars or use manual batching"
                            .to_string(),
                    ));
                }
                queries.push(task);
            }
            (
                format!(
                    "{}\n\n{}",
                    rlm_subcall_prompt(mode, true),
                    queries.join("\n")
                ),
                ctx_id,
                queries.len(),
            )
        } else if let Some(items) = batch_items {
            if items.is_empty() {
                return Err(ToolError::invalid_input(
                    "Batch must include at least one query".to_string(),
                ));
            }
            let mut queries = Vec::new();
            let mut context_for_batch = context_id.clone();
            for (idx, item) in items.iter().enumerate() {
                let item_query = item
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let (chunk, ctx_id) = self.extract_chunk(item, context_for_batch.as_deref())?;
                context_for_batch = Some(ctx_id.clone());
                queries.push(format!(
                    "TASK {}:\nContext:\n{}\n\nQuestion:\n{}\n",
                    idx + 1,
                    chunk,
                    item_query
                ));
            }
            (
                format!(
                    "{}\n\n{}",
                    rlm_subcall_prompt(mode, true),
                    queries.join("\n")
                ),
                context_for_batch.unwrap_or_else(|| "active".to_string()),
                items.len(),
            )
        } else {
            let (chunk, ctx_id) = self.extract_chunk(&input, context_id.as_deref())?;
            (
                format!(
                    "{}\n\nContext:\n{}\n\nQuestion:\n{}\n",
                    rlm_subcall_prompt(mode, false),
                    chunk,
                    query
                ),
                ctx_id,
                1,
            )
        };

        if prompt.len() > MAX_QUERY_CHARS {
            return Err(ToolError::invalid_input(format!(
                "RLM query payload is too large ({} chars). Use smaller chunks or batch less.",
                prompt.len()
            )));
        }

        // Build tools for recursive sub-calls if depth allows (per RLM paper)
        let tools = if self.can_recurse() {
            Some(build_rlm_tools_for_subcall())
        } else {
            None
        };

        let mut messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt.clone(),
                cache_control: None,
            }],
        }];

        let mut total_input_tokens = 0u32;
        let mut total_output_tokens = 0u32;
        let mut iterations = 0u32;
        let final_response_text;

        // Agentic loop: handle tool calls recursively
        loop {
            iterations += 1;
            if iterations > MAX_TOOL_ITERATIONS {
                return Err(ToolError::execution_failed(
                    "RLM sub-call exceeded maximum tool iterations",
                ));
            }

            let request = MessageRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                max_tokens,
                system: Some(SystemPrompt::Text(rlm_subcall_system_prompt(
                    mode,
                    self.can_recurse(),
                ))),
                tools: tools.clone(),
                tool_choice: None,
                metadata: None,
                thinking: None,
                stream: Some(false),
                temperature: None,
                top_p: None,
            };

            let response = client
                .create_message(request)
                .await
                .map_err(|e| ToolError::execution_failed(format!("RLM query failed: {e}")))?;

            total_input_tokens += response.usage.input_tokens;
            total_output_tokens += response.usage.output_tokens;

            // Check for tool use in response
            let tool_uses: Vec<_> = response
                .content
                .iter()
                .filter_map(|block| {
                    if let ContentBlock::ToolUse { id, name, input } = block {
                        Some((id.clone(), name.clone(), input.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            if tool_uses.is_empty() {
                // No tool calls - extract final response
                final_response_text = extract_text(&response.content);
                break;
            }

            // Execute tool calls and continue conversation
            let mut tool_results = Vec::new();
            for (tool_id, tool_name, tool_input) in tool_uses {
                let result = self
                    .execute_recursive_tool(&tool_name, &tool_input, &used_context_id)
                    .await;
                let result_text = match result {
                    Ok(text) => text,
                    Err(e) => format!("Error: {e}"),
                };
                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: tool_id,
                    content: result_text,
                });
            }

            // Add assistant response and tool results to conversation
            messages.push(Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
            });
            messages.push(Message {
                role: "user".to_string(),
                content: tool_results,
            });
        }

        self.record_usage(
            &used_context_id,
            &Usage {
                input_tokens: total_input_tokens,
                output_tokens: total_output_tokens,
            },
            prompt.len(),
            final_response_text.len(),
            &final_response_text,
            store_as,
        );

        let mut result = ToolResult::success(final_response_text);
        result.metadata = Some(json!({
            "context_id": used_context_id,
            "batch_count": batch_count,
            "input_tokens": total_input_tokens,
            "output_tokens": total_output_tokens,
            "depth": self.current_depth,
            "iterations": iterations,
        }));
        Ok(result)
    }
}

fn eval_script_mut(ctx: &mut RlmContext, code: &str) -> anyhow::Result<String> {
    let mut outputs = Vec::new();
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let result = eval_expr_mut(ctx, trimmed)?;
        if !result.trim().is_empty() {
            outputs.push(result);
        }
    }
    Ok(outputs.join("\n"))
}

impl RlmQueryTool {
    fn extract_chunk(
        &self,
        input: &Value,
        fallback_context_id: Option<&str>,
    ) -> Result<(String, String), ToolError> {
        let session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock RLM session"))?;

        let ctx_id = input
            .get("context_id")
            .and_then(|v| v.as_str())
            .or(fallback_context_id)
            .unwrap_or_else(|| session.active_context.as_str())
            .to_string();

        let ctx = session
            .get_context(&ctx_id)
            .ok_or_else(|| ToolError::invalid_input(format!("Context '{ctx_id}' not loaded")))?;

        let chunk = if let Some(text) = input.get("text").and_then(|v| v.as_str()) {
            text.to_string()
        } else if let Some(section_index) = input.get("section_index").and_then(|v| v.as_u64()) {
            let section_size = input
                .get("section_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(20_000) as usize;
            let sections = ctx.chunk_sections(section_size);
            let idx = usize::try_from(section_index).unwrap_or(0);
            let section = sections.get(idx).ok_or_else(|| {
                ToolError::invalid_input(format!("Section index {idx} out of range"))
            })?;
            ctx.peek(section.start_char, Some(section.end_char))
                .to_string()
        } else if let Some(chunk_index) = input.get("chunk_index").and_then(|v| v.as_u64()) {
            let chunk_size = input
                .get("chunk_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000) as usize;
            let overlap = input.get("overlap").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            let chunks = ctx.chunk(chunk_size, overlap);
            let idx = usize::try_from(chunk_index).unwrap_or(0);
            let chunk = chunks.get(idx).ok_or_else(|| {
                ToolError::invalid_input(format!("Chunk index {idx} out of range"))
            })?;
            ctx.peek(chunk.start_char, Some(chunk.end_char)).to_string()
        } else if let Some(start) = input.get("line_start").and_then(|v| v.as_u64()) {
            let end = input.get("line_end").and_then(|v| v.as_u64());
            extract_lines(ctx, start as usize, end.map(|v| v as usize))
        } else if let Some(start) = input.get("char_start").and_then(|v| v.as_u64()) {
            let end = input.get("char_end").and_then(|v| v.as_u64());
            ctx.peek(start as usize, end.map(|v| v as usize))
                .to_string()
        } else {
            return Err(ToolError::invalid_input(
                "Provide chunk_index, section_index, line_start, or char_start".to_string(),
            ));
        };

        Ok((chunk, ctx_id))
    }

    fn extract_auto_chunks(
        &self,
        fallback_context_id: Option<&str>,
        max_chars: usize,
    ) -> Result<(Vec<String>, String), ToolError> {
        let session = self
            .session
            .lock()
            .map_err(|_| ToolError::execution_failed("Failed to lock RLM session"))?;

        let ctx_id = fallback_context_id
            .unwrap_or_else(|| session.active_context.as_str())
            .to_string();

        let ctx = session
            .get_context(&ctx_id)
            .ok_or_else(|| ToolError::invalid_input(format!("Context '{ctx_id}' not loaded")))?;

        let chunks = ctx.chunk_auto(max_chars.max(1));
        let mut outputs = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            outputs.push(ctx.peek(chunk.start_char, Some(chunk.end_char)).to_string());
        }

        Ok((outputs, ctx_id))
    }

    fn record_usage(
        &self,
        context_id: &str,
        usage: &Usage,
        chars_sent: usize,
        chars_received: usize,
        response_text: &str,
        store_as: Option<String>,
    ) {
        let mut session = match self.session.lock() {
            Ok(session) => session,
            Err(_) => return,
        };
        session.record_query_usage(usage, chars_sent, chars_received);

        let Some(ctx) = session.get_context_mut(context_id) else {
            return;
        };

        if let Some(name) = store_as {
            let final_answer =
                extract_final(response_text).unwrap_or_else(|| response_text.trim().to_string());
            ctx.set_var(&name, final_answer);
        }

        for (name, value) in extract_final_vars(response_text) {
            ctx.set_var(&name, value);
        }
    }
}

impl RlmQueryTool {
    /// Execute a tool call from within a recursive sub-call
    async fn execute_recursive_tool(
        &self,
        tool_name: &str,
        input: &Value,
        context_id: &str,
    ) -> Result<String, ToolError> {
        match tool_name {
            "rlm_exec" => {
                let code = input.get("code").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::invalid_input("rlm_exec requires 'code' parameter")
                })?;

                let ctx_id = input
                    .get("context_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(context_id);

                let mut session = self
                    .session
                    .lock()
                    .map_err(|_| ToolError::execution_failed("Failed to lock RLM session"))?;

                let ctx = session.get_context_mut(ctx_id).ok_or_else(|| {
                    ToolError::invalid_input(format!("Context '{ctx_id}' not loaded"))
                })?;

                let output = eval_script_mut(ctx, code)
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?;

                let truncated = if output.len() > MAX_EXEC_OUTPUT_CHARS {
                    let snippet = truncate_to_boundary(&output, MAX_EXEC_OUTPUT_CHARS);
                    format!(
                        "{}\n\n[output truncated to {} chars]",
                        snippet, MAX_EXEC_OUTPUT_CHARS
                    )
                } else {
                    output
                };

                Ok(truncated)
            }
            "rlm_query" => {
                // Recursive sub-query at increased depth
                let sub_tool = self.with_depth(self.current_depth + 1);

                // Build a minimal ToolContext for the recursive call
                let dummy_context = crate::tools::spec::ToolContext::new(
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                );

                let result = sub_tool.execute(input.clone(), &dummy_context).await?;
                Ok(result.content)
            }
            _ => Err(ToolError::not_available(format!(
                "Unknown tool in RLM sub-call: {tool_name}"
            ))),
        }
    }
}

fn extract_text(blocks: &[ContentBlock]) -> String {
    let mut output = String::new();
    for block in blocks {
        if let ContentBlock::Text { text, .. } = block {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(text);
        }
    }
    output.trim().to_string()
}

fn extract_lines(ctx: &RlmContext, start: usize, end: Option<usize>) -> String {
    let start_line = start.max(1);
    let end_line = end.unwrap_or(ctx.line_count).max(start_line);
    let start_idx = start_line.saturating_sub(1);
    let end_idx = end_line.min(ctx.line_count);
    ctx.lines(start_idx, Some(end_idx))
        .iter()
        .map(|(n, l)| format!("{n:>5} {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_final(text: &str) -> Option<String> {
    let regex = Regex::new(r"(?s)FINAL\s*:?\s*(.+)$").ok()?;
    let caps = regex.captures(text)?;
    caps.get(1).map(|m| m.as_str().trim().to_string())
}

fn extract_final_vars(text: &str) -> Vec<(String, String)> {
    let regex = match Regex::new(r"(?m)^FINAL_VAR\(([^)]+)\)\s*:?\s*(.+)$") {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    regex
        .captures_iter(text)
        .filter_map(|caps| {
            let name = caps.get(1)?.as_str().trim();
            let value = caps.get(2)?.as_str().trim();
            if name.is_empty() || value.is_empty() {
                None
            } else {
                Some((name.to_string(), value.to_string()))
            }
        })
        .collect()
}

fn rlm_subcall_prompt(mode: &str, batch: bool) -> &'static str {
    match (mode, batch) {
        ("verify", true) => "You are verifying answers. Provide a brief check for each task.",
        ("verify", false) => {
            "You are verifying an answer. Provide a brief check and highlight any issues."
        }
        (_, true) => {
            "Answer each task using only the provided context. Label each response clearly."
        }
        _ => "Answer the question using only the provided context.",
    }
}

fn rlm_subcall_system_prompt(mode: &str, has_tools: bool) -> String {
    let mut prompt = String::from(
        "You are an RLM sub-call. Use ONLY the provided context. Respond concisely.\n\n\
Output format:\n- Use FINAL: <answer> for the final response.\n- Use FINAL_VAR(name): <value> to store buffer values if needed.\n",
    );
    if has_tools {
        prompt.push_str(
            "\nYou have access to RLM tools for recursive analysis:\n\
             - rlm_exec: Execute expressions to explore context (lines, search, chunk, etc.)\n\
             - rlm_query: Make recursive sub-queries on specific chunks\n\n\
             Use tools when you need to examine more context or make focused sub-queries.\n",
        );
    }
    if mode == "verify" {
        prompt.push_str("\nVerification mode: check for contradictions or missing evidence.");
    }
    prompt
}

/// Build tool definitions for recursive RLM sub-calls
fn build_rlm_tools_for_subcall() -> Vec<Tool> {
    vec![
        Tool {
            name: "rlm_exec".to_string(),
            description: "Execute RLM expressions: len, line_count, lines(start, end), search(pattern), chunk(size, overlap), chunk_auto(max_chars), get(var), set(var, value)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "RLM expression(s) to evaluate"
                    },
                    "context_id": {
                        "type": "string",
                        "description": "Optional context id"
                    }
                },
                "required": ["code"]
            }),
            cache_control: None,
        },
        Tool {
            name: "rlm_query".to_string(),
            description: "Make a recursive sub-query on a specific chunk of context. Use for focused analysis of sections.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Question to answer about the chunk"
                    },
                    "chunk_index": {
                        "type": "integer",
                        "description": "Chunk index from chunk() output"
                    },
                    "line_start": {
                        "type": "integer",
                        "description": "Start line (1-based)"
                    },
                    "line_end": {
                        "type": "integer",
                        "description": "End line (1-based)"
                    },
                    "section_index": {
                        "type": "integer",
                        "description": "Section index from chunk_sections()"
                    }
                },
                "required": ["query"]
            }),
            cache_control: None,
        },
    ]
}

fn truncate_to_boundary(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let idx = text
        .char_indices()
        .take_while(|(i, _)| *i <= max)
        .last()
        .map_or(0, |(i, _)| i);
    &text[..idx]
}

fn default_query_max_tokens(model: &str) -> u32 {
    let lower = model.to_lowercase();
    if lower.contains("claude") {
        1024
    } else {
        DEFAULT_QUERY_MAX_TOKENS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rlm::RlmContext;

    #[test]
    fn extract_final_prefers_final_marker() {
        let text = "notes\nFINAL: answer here";
        let extracted = extract_final(text).expect("final");
        assert_eq!(extracted, "answer here");
    }

    #[test]
    fn extract_final_vars_parses_lines() {
        let text = "FINAL_VAR(foo): bar\nFINAL_VAR(baz): qux";
        let vars = extract_final_vars(text);
        assert_eq!(vars.len(), 2);
        assert!(vars.iter().any(|(k, v)| k == "foo" && v == "bar"));
        assert!(vars.iter().any(|(k, v)| k == "baz" && v == "qux"));
    }

    #[test]
    fn extract_lines_formats_numbers() {
        let ctx = RlmContext::new("test", "a\nb\nc".to_string(), None);
        let lines = extract_lines(&ctx, 1, Some(2));
        assert!(lines.contains("1 a"));
        assert!(lines.contains("2 b"));
    }

    #[test]
    fn normalize_load_path_accepts_at_prefix() {
        let normalized = normalize_load_path("@docs/rlm-paper.txt").expect("normalize");
        assert_eq!(normalized, "docs/rlm-paper.txt");
    }

    #[test]
    fn normalize_load_path_strips_leading_separators() {
        let normalized = normalize_load_path("@/docs/rlm-paper.txt").expect("normalize");
        assert_eq!(normalized, "docs/rlm-paper.txt");
    }

    #[test]
    fn normalize_load_path_rejects_empty() {
        assert!(normalize_load_path("@").is_err());
        assert!(normalize_load_path("   ").is_err());
    }
}
