//! LSP code intelligence tools — goto definition, find references, hover,
//! and symbol browsing — exposed as model-callable tools.
//!
//! These tools give the model IDE-grade code navigation without shelling out
//! to grep + sequential file reads. Each tool delegates to the per-language
//! LSP transport managed by [`LspManager`] and degrades gracefully when the
//! LSP server is not available.
//!
//! # Tool surface
//!
//! | Tool                      | LSP method                       | Returns          |
//! |---------------------------|----------------------------------|------------------|
//! | `lsp_goto_definition`     | `textDocument/definition`       | `Location[]`     |
//! | `lsp_find_references`     | `textDocument/references`       | `Location[]`     |
//! | `lsp_hover`               | `textDocument/hover`            | `String`         |
//! | `lsp_document_symbols`    | `textDocument/documentSymbol`   | `Symbol[]`       |
//! | `lsp_workspace_symbols`   | `workspace/symbol`              | `Symbol[]`       |

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::lsp::{LspLocation, LspManager, LspSymbol};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    required_str, required_u64,
};

// ── Helper: resolve LspManager from context ───────────────────────────

fn lsp_manager(ctx: &ToolContext) -> Result<&LspManager, ToolError> {
    ctx.lsp_manager
        .as_ref()
        .ok_or_else(|| ToolError::execution_failed("LSP manager is not available"))
        .map(|arc| arc.as_ref())
}

async fn resolve_and_open(ctx: &ToolContext, file: &str) -> Result<(PathBuf, String), ToolError> {
    let resolved = ctx.resolve_path(file)?;
    let text = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
        ToolError::execution_failed(format!("failed to read {}: {e}", resolved.display()))
    })?;
    Ok((resolved, text))
}

fn format_locations(locs: &[LspLocation]) -> String {
    if locs.is_empty() {
        return "No results found.".to_string();
    }
    let mut out = String::new();
    for loc in locs {
        out.push_str(&format!("{}:{}:{}\n", loc.file, loc.line, loc.column));
    }
    out.trim_end().to_string()
}

fn format_symbols(syms: &[LspSymbol]) -> String {
    if syms.is_empty() {
        return "No symbols found.".to_string();
    }
    let mut out = String::new();
    for s in syms {
        let location = match (&s.file, s.line, s.column) {
            (Some(f), Some(l), Some(c)) => format!("{f}:{l}:{c}"),
            (Some(f), _, _) => f.clone(),
            _ => String::new(),
        };
        let container = s
            .container
            .as_deref()
            .map(|c| format!(" (in {c})"))
            .unwrap_or_default();
        if location.is_empty() {
            out.push_str(&format!("[{}] {}{}\n", s.kind, s.name, container));
        } else {
            out.push_str(&format!("[{}] {} — {}{}\n", s.kind, s.name, location, container));
        }
    }
    out.trim_end().to_string()
}

// ── LspGotoDefinitionTool ─────────────────────────────────────────────

pub struct LspGotoDefinitionTool;

#[async_trait]
impl ToolSpec for LspGotoDefinitionTool {
    fn name(&self) -> &'static str {
        "lsp_goto_definition"
    }

    fn description(&self) -> &'static str {
        "Jump to the definition of a symbol at the given file/line/column. \
         Returns a list of file locations. Use this to understand where a \
         function, type, or variable is defined without grep + manual search."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "File path (relative to workspace or absolute)"
                },
                "line": {
                    "type": "integer",
                    "description": "1-based line number"
                },
                "column": {
                    "type": "integer",
                    "description": "1-based column number"
                }
            },
            "required": ["file", "line", "column"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let file = required_str(&input, "file")?;
        let line = required_u64(&input, "line")? as u32;
        let column = required_u64(&input, "column")? as u32;

        let mgr = lsp_manager(ctx)?;
        let (resolved, _text) = resolve_and_open(ctx, file).await?;

        match mgr.goto_definition(&resolved, line, column).await {
            Some(locs) => Ok(ToolResult::success(format_locations(&locs))),
            None => Ok(ToolResult::success(
                "LSP server unavailable — try grep_files or read_file instead".to_string(),
            )),
        }
    }
}

// ── LspFindReferencesTool ─────────────────────────────────────────────

pub struct LspFindReferencesTool;

#[async_trait]
impl ToolSpec for LspFindReferencesTool {
    fn name(&self) -> &'static str {
        "lsp_find_references"
    }

    fn description(&self) -> &'static str {
        "Find all references to the symbol at the given file/line/column. \
         Returns a list of file locations. Use this to understand where a \
         function, type, or variable is used across the codebase."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "File path (relative to workspace or absolute)"
                },
                "line": {
                    "type": "integer",
                    "description": "1-based line number"
                },
                "column": {
                    "type": "integer",
                    "description": "1-based column number"
                }
            },
            "required": ["file", "line", "column"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let file = required_str(&input, "file")?;
        let line = required_u64(&input, "line")? as u32;
        let column = required_u64(&input, "column")? as u32;

        let mgr = lsp_manager(ctx)?;
        let (resolved, _text) = resolve_and_open(ctx, file).await?;

        match mgr.find_references(&resolved, line, column).await {
            Some(locs) => Ok(ToolResult::success(format_locations(&locs))),
            None => Ok(ToolResult::success(
                "LSP server unavailable — try grep_files instead".to_string(),
            )),
        }
    }
}

// ── LspHoverTool ──────────────────────────────────────────────────────

pub struct LspHoverTool;

#[async_trait]
impl ToolSpec for LspHoverTool {
    fn name(&self) -> &'static str {
        "lsp_hover"
    }

    fn description(&self) -> &'static str {
        "Get type information and documentation for the symbol at the given \
         file/line/column. Returns a string with the hover content (type \
         signature, docs, etc.). Use this to quickly understand a variable \
         or function's type without reading its full definition."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "File path (relative to workspace or absolute)"
                },
                "line": {
                    "type": "integer",
                    "description": "1-based line number"
                },
                "column": {
                    "type": "integer",
                    "description": "1-based column number"
                }
            },
            "required": ["file", "line", "column"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let file = required_str(&input, "file")?;
        let line = required_u64(&input, "line")? as u32;
        let column = required_u64(&input, "column")? as u32;

        let mgr = lsp_manager(ctx)?;
        let (resolved, _text) = resolve_and_open(ctx, file).await?;

        match mgr.hover(&resolved, line, column).await {
            Some(text) => Ok(ToolResult::success(text)),
            None => Ok(ToolResult::success(
                "No hover information available".to_string(),
            )),
        }
    }
}

// ── LspDocumentSymbolsTool ────────────────────────────────────────────

pub struct LspDocumentSymbolsTool;

#[async_trait]
impl ToolSpec for LspDocumentSymbolsTool {
    fn name(&self) -> &'static str {
        "lsp_document_symbols"
    }

    fn description(&self) -> &'static str {
        "List all symbols (functions, types, variables, etc.) defined in \
         the given file. Returns a list of symbol names with their kind \
         and location. Use this to get an overview of a file's structure \
         without reading it line by line."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "File path (relative to workspace or absolute)"
                }
            },
            "required": ["file"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let file = required_str(&input, "file")?;

        let mgr = lsp_manager(ctx)?;
        let (resolved, _text) = resolve_and_open(ctx, file).await?;

        match mgr.document_symbols(&resolved).await {
            Some(syms) => Ok(ToolResult::success(format_symbols(&syms))),
            None => Ok(ToolResult::success(
                "LSP server unavailable — try grep_files or read_file instead".to_string(),
            )),
        }
    }
}

// ── LspWorkspaceSymbolsTool ───────────────────────────────────────────

pub struct LspWorkspaceSymbolsTool;

#[async_trait]
impl ToolSpec for LspWorkspaceSymbolsTool {
    fn name(&self) -> &'static str {
        "lsp_workspace_symbols"
    }

    fn description(&self) -> &'static str {
        "Search for symbols across the entire workspace by name. Returns \
         a list of matching symbols with their kind, file location, and \
         container. Use this to find where a function or type is defined \
         without knowing which file it's in."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Symbol name to search for (partial match)"
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;

        let mgr = lsp_manager(ctx)?;

        match mgr.workspace_symbols(query).await {
            Some(syms) => Ok(ToolResult::success(format_symbols(&syms))),
            None => Ok(ToolResult::success(
                "LSP server unavailable — try grep_files instead".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_locations_single() {
        let locs = vec![LspLocation {
            file: "src/main.rs".to_string(),
            line: 10,
            column: 5,
        }];
        let out = format_locations(&locs);
        assert_eq!(out, "src/main.rs:10:5");
    }

    #[test]
    fn format_locations_empty() {
        assert_eq!(format_locations(&[]), "No results found.");
    }

    #[test]
    fn format_symbols_with_all_fields() {
        let syms = vec![LspSymbol {
            name: "main".to_string(),
            kind: "function".to_string(),
            file: Some("src/main.rs".to_string()),
            line: Some(1),
            column: Some(1),
            container: Some("my_module".to_string()),
        }];
        let out = format_symbols(&syms);
        assert!(out.contains("[function] main"));
        assert!(out.contains("src/main.rs:1:1"));
        assert!(out.contains("(in my_module)"));
    }
}
