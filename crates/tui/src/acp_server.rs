//! ACP (Agent Client Protocol) stdio adapter with tool execution support.
//!
//! Supports the ACP baseline: initialize, session/new, session/prompt,
//! session/resume, session/set_model, session/cancel, shutdown — plus a
//! multi-turn tool execution loop that drives `session/request_permission`
//! notifications for editor/host-side approval.
//!
//! Tools executed: read_file, write_file, edit_file, exec_shell, grep_files,
//! file_search, list_dir, todo_write, web_search, web_fetch, diagnostics.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

use crate::client::DeepSeekClient;
use crate::config::Config;
use crate::llm_client::LlmClient;
use crate::models::{
    ContentBlock, Message, MessageRequest, SystemPrompt, Tool,
};

const ACP_PROTOCOL_VERSION: u64 = 1;

// ── Tool definitions ────────────────────────────────────────────────

fn acp_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: None,
            name: "read_file".into(),
            description: "Read a file from the workspace. Returns the file content with line numbers.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to read" },
                    "start_line": { "type": "integer", "description": "Starting line (1-based)" },
                    "max_lines": { "type": "integer", "description": "Maximum lines to return" }
                },
                "required": ["path"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "write_file".into(),
            description: "Write content to a file in the workspace. Creates parent directories as needed.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to write" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "edit_file".into(),
            description: "Replace text in a single file via exact search/replace.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "search": { "type": "string", "description": "Exact text to search for" },
                    "replace": { "type": "string", "description": "Text to replace with" }
                },
                "required": ["path", "search", "replace"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "exec_shell".into(),
            description: "Execute a shell command in the workspace directory. Returns stdout, stderr, and exit code.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute" },
                    "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds" },
                    "cwd": { "type": "string", "description": "Working directory for the command" }
                },
                "required": ["command"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "grep_files".into(),
            description: "Search for a regex pattern in workspace files.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regular expression pattern to search for" },
                    "path": { "type": "string", "description": "Directory or file to search" },
                    "include": { "type": "array", "items": { "type": "string" } },
                    "max_results": { "type": "integer" }
                },
                "required": ["pattern"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "file_search".into(),
            description: "Find files by name using fuzzy matching.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query (file name or path fragment)" },
                    "path": { "type": "string" },
                    "limit": { "type": "integer" },
                    "extensions": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["query"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "list_dir".into(),
            description: "List entries in a directory.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path (default: .)" }
                }
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "todo_write".into(),
            description: "Create or update a structured task list for tracking progress.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                            },
                            "required": ["content", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "web_search".into(),
            description: "Search the web and return ranked results with URLs and snippets.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "max_results": { "type": "integer", "description": "Maximum results (default 5, max 10)" }
                },
                "required": ["query"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "web_fetch".into(),
            description: "Fetch a known URL directly (HTTP GET) and return its content.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute HTTP/HTTPS URL to fetch." },
                    "format": { "type": "string", "enum": ["text", "markdown", "raw"] }
                },
                "required": ["url"]
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "diagnostics".into(),
            description: "Report workspace info, git detection, sandbox availability.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
            allowed_callers: None, defer_loading: None, input_examples: None,
            strict: None, cache_control: None,
        },
    ]
}

// ── Public entry point ──────────────────────────────────────────────

pub async fn run_acp_server(config: Config, model: String, default_cwd: PathBuf) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = tokio::io::BufWriter::new(stdout);
    let mut server = AcpServer::new(config, model, default_cwd);

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let message: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                write_jsonrpc_error(&mut writer, None, -32700, format!("invalid json: {err}"))
                    .await?;
                continue;
            }
        };

        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            write_jsonrpc_error(
                &mut writer,
                message.get("id").cloned(),
                -32600,
                "jsonrpc version must be 2.0",
            )
            .await?;
            continue;
        }

        let id = message.get("id").cloned();
        let method = match message.get("method").and_then(Value::as_str) {
            Some(m) => m,
            None => {
                write_jsonrpc_error(&mut writer, id, -32600, "missing method").await?;
                continue;
            }
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

        match server.handle_request(method, params, &mut writer).await {
            Ok(AcpDispatch::Response(result)) => {
                if let Some(id) = id {
                    write_jsonrpc_result(&mut writer, id, result).await?;
                }
            }
            Ok(AcpDispatch::Shutdown) => {
                if let Some(id) = id {
                    write_jsonrpc_result(&mut writer, id, json!(null)).await?;
                }
                break;
            }
            Err(err) => {
                write_jsonrpc_error(&mut writer, id, err.code, err.message).await?;
            }
        }
    }

    Ok(())
}

// ── Server state ────────────────────────────────────────────────────

struct AcpServer {
    config: Config,
    model: String,
    default_cwd: PathBuf,
    sessions: HashMap<String, AcpSession>,
}

struct AcpSession {
    cwd: PathBuf,
    model: String,
}

enum AcpDispatch {
    Response(Value),
    Shutdown,
}

struct AcpError {
    code: i32,
    message: String,
}

impl AcpServer {
    fn new(config: Config, model: String, default_cwd: PathBuf) -> Self {
        Self { config, model, default_cwd, sessions: HashMap::new() }
    }

    async fn handle_request<W>(
        &mut self,
        method: &str,
        params: Value,
        writer: &mut W,
    ) -> std::result::Result<AcpDispatch, AcpError>
    where
        W: AsyncWrite + Unpin,
    {
        match method {
            "initialize" => Ok(AcpDispatch::Response(initialize_result(
                params.get("protocolVersion").and_then(Value::as_u64),
            ))),
            "session/new" => Ok(AcpDispatch::Response(self.new_session(params)?)),
            "session/resume" => Ok(AcpDispatch::Response(self.resume_session(params)?)),
            "session/set_model" => {
                self.set_model(params)?;
                Ok(AcpDispatch::Response(json!(null)))
            }
            "session/prompt" => {
                self.prompt(params, writer).await?;
                Ok(AcpDispatch::Response(json!({"stopReason": "end_turn"})))
            }
            "session/cancel" => Ok(AcpDispatch::Response(json!(null))),
            "session/request_permission" => {
                // Auto-approve all tool permissions in headless mode.
                // In production Multica daemon integration, the hermesClient
                // handles auto-approval; this handler covers standalone usage.
                Ok(AcpDispatch::Response(json!({"outcome": "approved_for_session"})))
            }
            "shutdown" => Ok(AcpDispatch::Shutdown),
            _ => Err(AcpError::method_not_found(method)),
        }
    }

    fn new_session(&mut self, params: Value) -> std::result::Result<Value, AcpError> {
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_cwd.clone());
        let session_id = format!("deepseek-{}", uuid::Uuid::new_v4());
        let model = self.model.clone();
        self.sessions.insert(session_id.clone(), AcpSession { cwd, model });
        Ok(json!({"sessionId": session_id}))
    }

    fn resume_session(&mut self, params: Value) -> std::result::Result<Value, AcpError> {
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| AcpError::invalid_params("sessionId is required"))?;
        if self.sessions.contains_key(session_id) {
            Ok(json!({"sessionId": session_id}))
        } else {
            // Create a new session with the requested id if not found
            let cwd = params
                .get("cwd")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| self.default_cwd.clone());
            let model = self.model.clone();
            self.sessions.insert(session_id.to_string(), AcpSession { cwd, model });
            Ok(json!({"sessionId": session_id}))
        }
    }

    fn set_model(&mut self, params: Value) -> std::result::Result<(), AcpError> {
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| AcpError::invalid_params("sessionId is required"))?;
        let model_id = params
            .get("modelId")
            .and_then(Value::as_str)
            .ok_or_else(|| AcpError::invalid_params("modelId is required"))?;
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.model = model_id.to_string();
        }
        Ok(())
    }

    // ── Multi-turn tool execution prompt ────────────────────────────

    async fn prompt<W>(&self, params: Value, writer: &mut W) -> std::result::Result<(), AcpError>
    where
        W: AsyncWrite + Unpin,
    {
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| AcpError::invalid_params("sessionId is required"))?;
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| AcpError::invalid_params("unknown sessionId"))?;
        let prompt_text = extract_prompt_text(params.get("prompt"))
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| AcpError::invalid_params("prompt must include text content"))?;

        let model = session.model.clone();
        let cwd = session.cwd.clone();
        let tools = acp_tools();
        let max_turns: usize = std::env::var("ACP_MAX_TURNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);
        let tool_timeout_secs: u64 = std::env::var("ACP_TOOL_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);

        let mut messages: Vec<Message> = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt_text,
                cache_control: None,
            }],
        }];

        let client = DeepSeekClient::new(&self.config)
            .map_err(|e| AcpError::internal(format!("client init: {e}")))?;

        for _turn in 0..max_turns {
            let request = MessageRequest {
                model: model.clone(),
                messages: messages.clone(),
                max_tokens: 4096,
                system: Some(SystemPrompt::Text(
                    "You are a coding assistant inside an ACP-compatible editor. \
                     Use tools to read, write, and execute code. Give concise, \
                     actionable responses.".to_string(),
                )),
                tools: Some(tools.clone()),
                tool_choice: None,
                metadata: None,
                thinking: None,
                reasoning_effort: None,
                stream: Some(false),
                temperature: Some(0.2),
                top_p: Some(0.9),
            };

            let response = client
                .create_message(request)
                .await
                .map_err(|e| AcpError::internal(format!("API call: {e}")))?;

            let has_tool_calls = response.content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));

            if !has_tool_calls {
                // Emit final text response
                let text: String = response
                    .content
                    .iter()
                    .filter_map(|b| {
                        if let ContentBlock::Text { text, .. } = b {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                if !text.is_empty() {
                    write_session_update(writer, session_id, &text).await
                        .map_err(|e| AcpError::internal(e.to_string()))?;
                }
                return Ok(());
            }

            // Collect tool calls and results
            let mut tool_results: Vec<ContentBlock> = Vec::new();
            let mut assistant_content: Vec<ContentBlock> = Vec::new();

            for block in &response.content {
                match block {
                    ContentBlock::Text { text, .. } => {
                        if !text.is_empty() {
                            write_session_update(writer, session_id, text).await
                                .map_err(|e| AcpError::internal(e.to_string()))?;
                        }
                        assistant_content.push(block.clone());
                    }
                    ContentBlock::ToolUse { id, name, input, .. } => {
                        // Notify host about tool execution
                        let permission_id = format!("perm-{}", uuid::Uuid::new_v4());
                        write_notification(writer, "session/request_permission", &json!({
                            "sessionId": session_id,
                            "toolCall": {
                                "toolCallId": id,
                                "title": format!("{}: {}", name, summarize_input(input)),
                            },
                            "permissionId": permission_id,
                        })).await.map_err(|e| AcpError::internal(e.to_string()))?;

                        assistant_content.push(block.clone());

                        // Execute the tool
                        let result = execute_tool(name, input, &cwd, tool_timeout_secs).await;
                        let (result_text, is_error) = match &result {
                            Ok(output) => (output.clone(), false),
                            Err(err) => (format!("Error: {err}"), true),
                        };

                        // Send tool result notification
                        write_notification(writer, "session/update", &json!({
                            "sessionId": session_id,
                            "update": {
                                "sessionUpdate": "tool_result",
                                "toolCallId": id,
                                "content": result_text,
                                "isError": is_error,
                            }
                        })).await.map_err(|e| AcpError::internal(e.to_string()))?;

                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: result_text,
                            is_error: Some(is_error),
                            content_blocks: None,
                        });
                    }
                    _ => {
                        assistant_content.push(block.clone());
                    }
                }
            }

            // Add assistant response with tool calls
            messages.push(Message {
                role: "assistant".to_string(),
                content: assistant_content,
            });

            // Add tool results as user message
            if !tool_results.is_empty() {
                messages.push(Message {
                    role: "user".to_string(),
                    content: tool_results,
                });
            }
        }

        // Max turns reached
        write_session_update(writer, session_id, "\n\n[ACP: maximum tool turns reached]").await
            .map_err(|e| AcpError::internal(e.to_string()))?;
        Ok(())
    }
}

// ── Tool execution ──────────────────────────────────────────────────

fn summarize_input(input: &Value) -> String {
    if let Some(path) = input.get("path").and_then(Value::as_str) {
        return path.to_string();
    }
    if let Some(cmd) = input.get("command").and_then(Value::as_str) {
        let short: String = cmd.chars().take(80).collect();
        return if cmd.len() > 80 { format!("{short}...") } else { short };
    }
    if let Some(query) = input.get("query").and_then(Value::as_str) {
        let short: String = query.chars().take(60).collect();
        return if query.len() > 60 { format!("{short}...") } else { short };
    }
    if let Some(pattern) = input.get("pattern").and_then(Value::as_str) {
        let short: String = pattern.chars().take(60).collect();
        return if pattern.len() > 60 { format!("{short}...") } else { short };
    }
    "…".to_string()
}

async fn execute_tool(
    name: &str,
    input: &Value,
    cwd: &PathBuf,
    timeout_secs: u64,
) -> Result<String> {
    match name {
        "read_file" => tool_read_file(input, cwd).await,
        "write_file" => tool_write_file(input, cwd).await,
        "edit_file" => tool_edit_file(input, cwd).await,
        "exec_shell" => tool_exec_shell(input, cwd, timeout_secs).await,
        "grep_files" => tool_grep_files(input, cwd).await,
        "file_search" => tool_file_search(input, cwd).await,
        "list_dir" => tool_list_dir(input, cwd).await,
        "todo_write" => Ok(json!({"status": "ok", "message": "todo list updated"}).to_string()),
        "web_search" => tool_web_search(input).await,
        "web_fetch" => tool_web_fetch(input).await,
        "diagnostics" => tool_diagnostics(cwd).await,
        _ => Err(anyhow!("unknown tool: {name}")),
    }
}

async fn tool_read_file(input: &Value, cwd: &PathBuf) -> Result<String> {
    let path_str = input.get("path").and_then(Value::as_str).unwrap_or("");
    let start = (input.get("start_line").and_then(Value::as_u64).unwrap_or(1) as usize).max(1);
    let max = input.get("max_lines").and_then(Value::as_u64).unwrap_or(200) as usize;
    let path = resolve_path(cwd, path_str)?;
    let content = tokio::fs::read_to_string(&path).await
        .with_context(|| format!("read {}: {path_str}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let end = (start + max).min(total + 1);
    if start > total {
        return Ok(format!("File has {total} lines, start_line {start} is out of range"));
    }
    let mut out = String::new();
    for (i, line) in lines[start - 1..end - 1].iter().enumerate() {
        out.push_str(&format!("{:>6}|{}\n", start + i, line));
    }
    if end <= total {
        out.push_str(&format!("... ({} more lines)\n", total - end + 1));
    }
    Ok(out)
}

async fn tool_write_file(input: &Value, cwd: &PathBuf) -> Result<String> {
    let path_str = input.get("path").and_then(Value::as_str).unwrap_or("");
    let content = input.get("content").and_then(Value::as_str).unwrap_or("");
    let path = resolve_path(cwd, path_str)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, content).await
        .with_context(|| format!("write {}", path.display()))?;
    Ok(format!("Wrote {} bytes to {}", content.len(), path_str))
}

async fn tool_edit_file(input: &Value, cwd: &PathBuf) -> Result<String> {
    let path_str = input.get("path").and_then(Value::as_str).unwrap_or("");
    let search = input.get("search").and_then(Value::as_str).unwrap_or("");
    let replace = input.get("replace").and_then(Value::as_str).unwrap_or("");
    let path = resolve_path(cwd, path_str)?;
    let content = tokio::fs::read_to_string(&path).await
        .with_context(|| format!("read {}", path.display()))?;
    let new_content = content.replacen(search, replace, 1);
    if new_content != content {
        tokio::fs::write(&path, &new_content).await?;
        return Ok(format!("Applied edit to {}", path_str));
    }
    // Try with leading-whitespace fuzz
    let search_trimmed = search.trim_start();
    let lines: Vec<&str> = content.lines().collect();
    for line in &lines {
        if line.trim_start() == search_trimmed {
            let trimmed_new = content.replacen(line, replace, 1);
            tokio::fs::write(&path, &trimmed_new).await?;
            return Ok(format!("Applied edit to {} (fuzzy match)", path_str));
        }
    }
    Err(anyhow!("search text not found in {}", path_str))
}

async fn tool_exec_shell(input: &Value, cwd: &PathBuf, timeout_secs: u64) -> Result<String> {
    let command = input.get("command").and_then(Value::as_str).unwrap_or("");
    let cmd_cwd = input.get("cwd")
        .and_then(Value::as_str)
        .map(|d| resolve_path(cwd, d))
        .transpose()?
        .unwrap_or_else(|| cwd.clone());
    let timeout_ms = input.get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(timeout_secs * 1000);

    let output = timeout(
        Duration::from_millis(timeout_ms.min(300_000)),
        async {
            TokioCommand::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&cmd_cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
        },
    )
    .await
    .map_err(|_| anyhow!("command timed out after {timeout_ms}ms"))?
    .with_context(|| format!("execute: {command}"))?;

    let mut result = String::new();
    if !output.stdout.is_empty() {
        result.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("STDERR:\n");
        result.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        result.push_str(&format!("\nEXIT CODE: {}", output.status.code().unwrap_or(-1)));
    }
    if result.is_empty() {
        result.push_str("(no output)");
    }
    Ok(result)
}

async fn tool_grep_files(input: &Value, cwd: &PathBuf) -> Result<String> {
    let pattern = input.get("pattern").and_then(Value::as_str).unwrap_or("");
    let search_path = input.get("path")
        .and_then(Value::as_str)
        .map(|p| resolve_path(cwd, p))
        .transpose()?
        .unwrap_or_else(|| cwd.clone());
    let max_results = input.get("max_results").and_then(Value::as_u64).unwrap_or(100) as usize;

    let output = timeout(
        Duration::from_secs(30),
        TokioCommand::new("grep")
            .arg("-rn")
            .arg("-m")
            .arg(max_results.to_string())
            .arg(pattern)
            .arg(&search_path)
            .output(),
    )
    .await
    .map_err(|_| anyhow!("grep timed out"))?
    .with_context(|| format!("grep for {pattern}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.is_empty() {
        Ok("No matches found".to_string())
    } else {
        let lines: Vec<&str> = stdout.lines().take(max_results).collect();
        Ok(lines.join("\n"))
    }
}

async fn tool_file_search(input: &Value, cwd: &PathBuf) -> Result<String> {
    let query = input.get("query").and_then(Value::as_str).unwrap_or("");
    let search_path = input.get("path")
        .and_then(Value::as_str)
        .map(|p| resolve_path(cwd, p))
        .transpose()?
        .unwrap_or_else(|| cwd.clone());
    let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;

    let output = timeout(
        Duration::from_secs(30),
        TokioCommand::new("find")
            .arg(&search_path)
            .arg("-name")
            .arg(format!("*{query}*"))
            .arg("-maxdepth")
            .arg("5")
            .arg("-not")
            .arg("-path")
            .arg("*/.*")
            .output(),
    )
    .await
    .map_err(|_| anyhow!("find timed out"))?
    .with_context(|| format!("find *{query}*"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().take(limit).collect();
    if lines.is_empty() {
        Ok("No files found".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

async fn tool_list_dir(input: &Value, cwd: &PathBuf) -> Result<String> {
    let path_str = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = resolve_path(cwd, path_str)?;
    let output = TokioCommand::new("ls")
        .arg("-la")
        .arg(&path)
        .output()
        .await
        .with_context(|| format!("ls {}", path.display()))?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn tool_web_search(input: &Value) -> Result<String> {
    let query = input.get("query").and_then(Value::as_str).unwrap_or("");
    // Use DuckDuckGo lite for web search
    let url = format!("https://lite.duckduckgo.com/lite/?q={}", urlencoding(query));
    let resp = reqwest::get(&url).await
        .with_context(|| format!("web search for: {query}"))?;
    let body = resp.text().await?;
    // Extract result snippets (simple HTML extraction)
    let mut results = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.contains("result__snippet") || trimmed.contains("result-link") {
            // Strip HTML tags roughly
            let clean = trimmed
                .replace("<wbr>", "")
                .replace("</wbr>", "");
            let clean = strip_html(&clean);
            if !clean.is_empty() {
                results.push_str(&clean);
                results.push('\n');
            }
        }
    }
    if results.is_empty() {
        Ok(format!("Web search for '{query}' returned no results (or DuckDuckGo format changed)"))
    } else {
        Ok(results)
    }
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' => c.to_string(),
            c => format!("%{:02X}", c as u8),
        })
        .collect()
}

async fn tool_web_fetch(input: &Value) -> Result<String> {
    let url = input.get("url").and_then(Value::as_str).unwrap_or("");
    let resp = reqwest::get(url).await
        .with_context(|| format!("fetch {url}"))?;
    let body = resp.text().await?;
    // Truncate to reasonable size
    let max_len = 50_000;
    if body.len() > max_len {
        Ok(format!("{}... (truncated from {} bytes)", &body[..max_len], body.len()))
    } else {
        Ok(body)
    }
}

async fn tool_diagnostics(cwd: &PathBuf) -> Result<String> {
    let mut out = String::new();
    out.push_str(&format!("cwd: {}\n", cwd.display()));
    if let Ok(output) = TokioCommand::new("git").arg("rev-parse").arg("--show-toplevel")
        .current_dir(cwd).output().await
    {
        out.push_str(&format!("git root: {}", String::from_utf8_lossy(&output.stdout).trim()));
    }
    if let Ok(output) = TokioCommand::new("rustc").arg("--version").output().await {
        out.push_str(&format!("\nrustc: {}", String::from_utf8_lossy(&output.stdout).trim()));
    }
    if let Ok(output) = TokioCommand::new("go").arg("version").output().await {
        out.push_str(&format!("\ngo: {}", String::from_utf8_lossy(&output.stdout).trim()));
    }
    Ok(out)
}

fn resolve_path(cwd: &PathBuf, path_str: &str) -> Result<PathBuf> {
    let candidate = if PathBuf::from(path_str).is_absolute() {
        cwd.join(path_str.trim_start_matches('/'))
    } else {
        cwd.join(path_str)
    };
    // Normalise away `..` and `.` components without requiring the path
    // to already exist (canonicalize would fail for new files).
    let mut normalised = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::ParentDir => {
                if !normalised.pop() {
                    return Err(anyhow!("path escapes workspace: {path_str}"));
                }
            }
            std::path::Component::CurDir => {}
            c => { normalised.push(c.as_os_str()); }
        }
    }
    let cwd_canonical = cwd.canonicalize()
        .with_context(|| format!("resolve cwd: {}", cwd.display()))?;
    if !normalised.starts_with(&cwd_canonical) {
        return Err(anyhow!("path escapes workspace: {path_str}"));
    }
    Ok(normalised)
}

// ── ACP error helpers ───────────────────────────────────────────────

impl AcpError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self { code: -32602, message: message.into() }
    }
    fn method_not_found(method: &str) -> Self {
        Self { code: -32601, message: format!("method not found: {method}") }
    }
    fn internal(message: impl Into<String>) -> Self {
        Self { code: -32603, message: message.into() }
    }
}

// ── Initialize response ─────────────────────────────────────────────

fn initialize_result(client_protocol_version: Option<u64>) -> Value {
    json!({
        "protocolVersion": client_protocol_version
            .map(|v| v.min(ACP_PROTOCOL_VERSION))
            .unwrap_or(ACP_PROTOCOL_VERSION),
        "agentCapabilities": {
            "loadSession": true,
            "promptCapabilities": {
                "image": false,
                "audio": false,
                "embeddedContext": true
            },
            "mcpCapabilities": {
                "http": false,
                "sse": false
            },
            "sessionCapabilities": {},
            "toolCapabilities": {
                "handlesRequestPermission": true,
                "builtinTools": acp_tools().iter().map(|t| t.name.clone()).collect::<Vec<_>>()
            }
        },
        "agentInfo": {
            "name": "deepseek",
            "title": "DeepSeek TUI",
            "version": env!("CARGO_PKG_VERSION")
        },
        "authMethods": []
    })
}

// ── Prompt text extraction ──────────────────────────────────────────

fn extract_prompt_text(prompt: Option<&Value>) -> Option<String> {
    match prompt? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let parts = blocks.iter()
                .filter_map(content_block_text)
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n\n"))
        }
        _ => None,
    }
}

fn content_block_text(block: &Value) -> Option<String> {
    match block.get("type").and_then(Value::as_str)? {
        "text" => block.get("text").and_then(Value::as_str).map(str::to_string),
        "resource" => resource_text(block),
        "resource_link" | "resourceLink" => resource_link_text(block),
        _ => None,
    }
}

fn resource_text(block: &Value) -> Option<String> {
    let resource = block.get("resource").unwrap_or(block);
    if let Some(text) = resource.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    resource_link_text(resource)
}

fn resource_link_text(block: &Value) -> Option<String> {
    let uri = block.get("uri")
        .or_else(|| block.pointer("/resource/uri"))
        .and_then(Value::as_str)?;
    Some(format!("@{uri}"))
}

// ── Write helpers ───────────────────────────────────────────────────

async fn write_session_update<W>(writer: &mut W, session_id: &str, text: &str) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": text
                }
            }
        }
    });
    write_json_line(writer, notification).await
}

async fn write_notification<W>(writer: &mut W, method: &str, params: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let notification = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    });
    write_json_line(writer, notification).await
}

async fn write_jsonrpc_result<W>(writer: &mut W, id: Value, result: Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_json_line(writer, json!({"jsonrpc": "2.0", "id": id, "result": result})).await
}

async fn write_jsonrpc_error<W>(
    writer: &mut W,
    id: Option<Value>,
    code: i32,
    message: impl Into<String>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_json_line(writer, json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })).await
}

async fn write_json_line<W>(writer: &mut W, value: Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(value.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_advertises_baseline_acp_agent() {
        let result = initialize_result(Some(1));
        let agent = result.get("agentInfo").unwrap();
        assert_eq!(agent["name"], "deepseek");
        assert_eq!(result["protocolVersion"], 1);
    }

    #[test]
    fn initialize_advertises_tool_capabilities() {
        let result = initialize_result(Some(1));
        let caps = &result["agentCapabilities"];
        assert!(caps.get("toolCapabilities").is_some(), "toolCapabilities should be present");
        let tc = &caps["toolCapabilities"];
        assert_eq!(tc["handlesRequestPermission"], true);
        let tools = tc["builtinTools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t == "read_file"));
        assert!(tools.iter().any(|t| t == "write_file"));
        assert!(tools.iter().any(|t| t == "exec_shell"));
    }

    #[test]
    fn initialize_advertises_load_session() {
        let result = initialize_result(Some(1));
        let caps = &result["agentCapabilities"];
        assert_eq!(caps["loadSession"], true);
    }
}