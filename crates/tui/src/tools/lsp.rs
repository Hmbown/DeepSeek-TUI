//! LSP code-intelligence tool: 9 named operations exposed as model-visible
//! tool calls (hover, definition, references, rename, codeAction,
//! completion, signatureHelp, documentSymbol, workspaceSymbol).
//!
//! The tool accepts a required `operation` string plus operation-specific
//! parameters. The underlying `LspManager` handles transport lifecycle and
//! language detection.
//!
//! # Operations
//!
//! | operation          | LSP method                        | key params                            |
//! |--------------------|-----------------------------------|---------------------------------------|
//! | hover              | textDocument/hover                | path, line, character                 |
//! | definition         | textDocument/definition           | path, line, character                 |
//! | references         | textDocument/references            | path, line, character                 |
//! | rename             | textDocument/rename               | path, line, character, newName        |
//! | codeAction         | textDocument/codeAction           | path, range start/end, context        |
//! | completion         | textDocument/completion           | path, line, character                 |
//! | signatureHelp      | textDocument/signatureHelp        | path, line, character                 |
//! | documentSymbol     | textDocument/documentSymbol       | path                                 |
//! | workspaceSymbol    | workspace/symbol                  | query                                |

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_u64, required_str,
};

/// The `lsp` tool — dispatches to one of 9 LSP code-intelligence operations.
pub struct LspTool;

#[async_trait]
impl ToolSpec for LspTool {
    fn name(&self) -> &'static str {
        "lsp"
    }

    fn description(&self) -> &'static str {
        "Execute an LSP code-intelligence operation: hover, definition, references, rename, \
         codeAction, completion, signatureHelp, documentSymbol, or workspaceSymbol. \
         Returns the result from the LSP server as JSON."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "hover",
                        "definition",
                        "references",
                        "rename",
                        "codeAction",
                        "completion",
                        "signatureHelp",
                        "documentSymbol",
                        "workspaceSymbol"
                    ],
                    "description": "The LSP operation to perform"
                },
                "path": {
                    "type": "string",
                    "description": "Path to the file (required for all operations except workspaceSymbol)"
                },
                "line": {
                    "type": "integer",
                    "description": "0-based line number for position-based operations"
                },
                "character": {
                    "type": "integer",
                    "description": "0-based character offset for position-based operations"
                },
                "newName": {
                    "type": "string",
                    "description": "New symbol name (required for rename operation)"
                },
                "query": {
                    "type": "string",
                    "description": "Search query (required for workspaceSymbol)"
                },
                "rangeStartLine": {
                    "type": "integer",
                    "description": "Range start line (0-based, for codeAction)"
                },
                "rangeStartCharacter": {
                    "type": "integer",
                    "description": "Range start character (0-based, for codeAction)"
                },
                "rangeEndLine": {
                    "type": "integer",
                    "description": "Range end line (0-based, for codeAction)"
                },
                "rangeEndCharacter": {
                    "type": "integer",
                    "description": "Range end character (0-based, for codeAction)"
                },
                "context": {
                    "type": "object",
                    "description": "Optional extra context JSON for codeAction (e.g. diagnostics)"
                }
            },
            "required": ["operation"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let operation = required_str(&input, "operation")?;

        // Grab the LSP manager from runtime services.
        let lsp_manager = context
            .runtime
            .lsp_manager
            .clone()
            .ok_or_else(|| ToolError::not_available(
                "LSP is not available in this context (no LspManager configured)"
            ))?;

        // Dispatch to the requested operation.
        let result = match operation {
            "hover" => op_hover(&input, &lsp_manager, context).await,
            "definition" => op_definition(&input, &lsp_manager, context).await,
            "references" => op_references(&input, &lsp_manager, context).await,
            "rename" => op_rename(&input, &lsp_manager, context).await,
            "codeAction" => op_code_action(&input, &lsp_manager, context).await,
            "completion" => op_completion(&input, &lsp_manager, context).await,
            "signatureHelp" => op_signature_help(&input, &lsp_manager, context).await,
            "documentSymbol" => op_document_symbol(&input, &lsp_manager, context).await,
            "workspaceSymbol" => op_workspace_symbol(&input, &lsp_manager, context).await,
            other => return Err(ToolError::invalid_input(format!(
                "unknown LSP operation '{other}'; expected one of: \
                 hover, definition, references, rename, codeAction, \
                 completion, signatureHelp, documentSymbol, workspaceSymbol"
            ))),
        }?;

        Ok(ToolResult::success(result))
    }
}

// ── Position helpers ──────────────────────────────────────────────────────

/// Build a LSP Position `{line, character}` from the input (0-based).
fn position_from_input(input: &Value) -> Result<Value, ToolError> {
    let line = optional_u64(input, "line", 0);
    let character = optional_u64(input, "character", 0);
    Ok(json!({ "line": line, "character": character }))
}

/// Build a LSP Range from start/end fields in the input.
fn range_from_input(input: &Value) -> Result<Value, ToolError> {
    let start_line = optional_u64(input, "rangeStartLine", 0);
    let start_char = optional_u64(input, "rangeStartCharacter", 0);
    let end_line = optional_u64(input, "rangeEndLine", 0);
    let end_char = optional_u64(input, "rangeEndCharacter", 0);
    Ok(json!({
        "start": { "line": start_line, "character": start_char },
        "end":   { "line": end_line,   "character": end_char }
    }))
}

/// Resolve the file path from input, relative to the workspace.
fn resolve_path(input: &Value, context: &ToolContext) -> Result<PathBuf, ToolError> {
    let raw = required_str(input, "path")?;
    context.resolve_path(raw)
}

// ── Operation implementations ─────────────────────────────────────────────

/// `textDocument/hover`
async fn op_hover(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let position = position_from_input(input)?;
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
        "position": position,
    });
    let result = mgr.request_for(&path, "textDocument/hover", params).await;
    Ok(format_result("hover", result))
}

/// `textDocument/definition`
async fn op_definition(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let position = position_from_input(input)?;
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
        "position": position,
    });
    let result = mgr.request_for(&path, "textDocument/definition", params).await;
    Ok(format_result("definition", result))
}

/// `textDocument/references`
async fn op_references(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let position = position_from_input(input)?;
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
        "position": position,
        "context": { "includeDeclaration": true },
    });
    let result = mgr.request_for(&path, "textDocument/references", params).await;
    Ok(format_result("references", result))
}

/// `textDocument/rename`
async fn op_rename(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let position = position_from_input(input)?;
    let new_name = required_str(input, "newName")?;
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
        "position": position,
        "newName": new_name,
    });
    let result = mgr.request_for(&path, "textDocument/rename", params).await;
    Ok(format_result("rename", result))
}

/// `textDocument/codeAction`
async fn op_code_action(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let range = range_from_input(input)?;
    let ctx = input
        .get("context")
        .cloned()
        .unwrap_or(json!({ "diagnostics": [] }));
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
        "range": range,
        "context": ctx,
    });
    let result = mgr.request_for(&path, "textDocument/codeAction", params).await;
    Ok(format_result("codeAction", result))
}

/// `textDocument/completion`
async fn op_completion(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let position = position_from_input(input)?;
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
        "position": position,
    });
    let result = mgr.request_for(&path, "textDocument/completion", params).await;
    Ok(format_result("completion", result))
}

/// `textDocument/signatureHelp`
async fn op_signature_help(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let position = position_from_input(input)?;
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
        "position": position,
    });
    let result = mgr.request_for(&path, "textDocument/signatureHelp", params).await;
    Ok(format_result("signatureHelp", result))
}

/// `textDocument/documentSymbol`
async fn op_document_symbol(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let path = resolve_path(input, context)?;
    let params = json!({
        "textDocument": { "uri": uri_from_path(&path) },
    });
    let result = mgr.request_for(&path, "textDocument/documentSymbol", params).await;
    Ok(format_result("documentSymbol", result))
}

/// `workspace/symbol`
async fn op_workspace_symbol(
    input: &Value,
    mgr: &Arc<crate::lsp::LspManager>,
    _context: &ToolContext,
) -> Result<String, ToolError> {
    let query = required_str(input, "query")?;
    let params = json!({ "query": query });
    // workspace/symbol doesn't need a file path — pass empty path.
    let empty_path = Path::new("");
    let result = mgr.request_for(empty_path, "workspace/symbol", params).await;
    Ok(format_result("workspaceSymbol", result))
}

// ── Formatting helpers ────────────────────────────────────────────────────

/// Format an LSP response: pretty-print the JSON when Some, or "no result"
/// when None.
fn format_result(label: &str, result: Option<Value>) -> String {
    match result {
        Some(value) if value != Value::Null => {
            let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            format!("<lsp_{}>\n{}\n</lsp_{}>", label, pretty, label)
        }
        _ => format!("<lsp_{}>\nno result\n</lsp_{}>", label, label),
    }
}

/// Convert a filesystem path to a `file://` URI. Reuses the same logic from
/// the LSP client crate.
fn uri_from_path(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy();
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{}", s.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_name_and_description() {
        let tool = LspTool;
        assert_eq!(tool.name(), "lsp");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn tool_input_schema_has_required_operation() {
        let tool = LspTool;
        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap();
        assert!(props.get("operation").is_some());
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|v| v == "operation"));
    }

    #[test]
    fn schema_lists_all_nine_operations() {
        let tool = LspTool;
        let schema = tool.input_schema();
        let op_schema = schema.pointer("/properties/operation").unwrap();
        let variants = op_schema.get("enum").unwrap().as_array().unwrap();
        let names: Vec<&str> = variants.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(names.len(), 9);
        assert!(names.contains(&"hover"));
        assert!(names.contains(&"definition"));
        assert!(names.contains(&"references"));
        assert!(names.contains(&"rename"));
        assert!(names.contains(&"codeAction"));
        assert!(names.contains(&"completion"));
        assert!(names.contains(&"signatureHelp"));
        assert!(names.contains(&"documentSymbol"));
        assert!(names.contains(&"workspaceSymbol"));
    }

    #[test]
    fn tool_is_read_only() {
        let tool = LspTool;
        assert!(tool.is_read_only());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn uri_format() {
        let path = PathBuf::from("/home/user/project/src/main.rs");
        let uri = uri_from_path(&path);
        assert_eq!(uri, "file:///home/user/project/src/main.rs");
    }

    #[test]
    fn position_from_input_defaults_to_zero() {
        let input = json!({});
        let pos = position_from_input(&input).unwrap();
        assert_eq!(pos["line"], 0);
        assert_eq!(pos["character"], 0);
    }

    #[test]
    fn position_from_input_uses_supplied_values() {
        let input = json!({ "line": 10, "character": 5 });
        let pos = position_from_input(&input).unwrap();
        assert_eq!(pos["line"], 10);
        assert_eq!(pos["character"], 5);
    }

    #[test]
    fn format_result_some_non_null() {
        let result = Some(json!({ "key": "value" }));
        let output = format_result("test", result);
        assert!(output.starts_with("<lsp_test>"));
        assert!(output.contains("\"key\""));
        assert!(output.ends_with("</lsp_test>"));
    }

    #[test]
    fn format_result_none_or_null() {
        assert!(format_result("t", None).contains("no result"));
        assert!(format_result("t", Some(Value::Null)).contains("no result"));
    }
}
