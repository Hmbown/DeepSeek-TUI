//! DeepMap codebase analysis tools for DeepSeek-TUI.
//!
//! Provides AI-driven repository mapping: symbol extraction, dependency graph,
//! PageRank ranking, call chains, hotspots, and more.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_u64, required_str,
};

pub struct DeepMapOverviewTool;

#[async_trait]
impl ToolSpec for DeepMapOverviewTool {
    fn name(&self) -> &'static str {
        "deepmap_overview"
    }

    fn description(&self) -> &'static str {
        "Use this first when exploring or working on an unfamiliar codebase. \
         Scans the entire project, extracts all symbols (functions, classes, etc.), \
         builds a dependency graph, runs PageRank to rank importance, and returns an \
         AI-friendly summary: entry points, hotspots, key symbols by file, module \
         structure, and recommended reading order. Much faster than reading files \
         one by one — gives you a mental map of the codebase in one call."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "max_files": {
                    "type": "integer",
                    "description": "Maximum files to scan (default: 2000)."
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum output characters (default: 16000)."
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let max_files = optional_u64(&input, "max_files", 2000) as usize;
        let max_chars = optional_u64(&input, "max_chars", 16000) as usize;
        let workspace = &context.workspace;

        let engine = deepmap::engine::RepoMapEngine::get_or_scan(workspace, max_files, 300.0);

        let report = deepmap::renderer::render_overview_report(&engine, max_chars);
        Ok(ToolResult::success(report))
    }
}

pub struct DeepMapCallChainTool;

#[async_trait]
impl ToolSpec for DeepMapCallChainTool {
    fn name(&self) -> &'static str {
        "deepmap_call_chain"
    }

    fn description(&self) -> &'static str {
        "Trace who calls a symbol and what it calls. Use this when you need to \
         understand the impact of changing a function, find all callers of an API, \
         or discover what dependencies a piece of code has. Returns callers and \
         callees sorted by PageRank importance, up to a configurable depth."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "symbol_name": {
                    "type": "string",
                    "description": "The symbol name to trace (e.g., 'createApp', 'handleLogin')."
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum traversal depth (default: 3)."
                }
            },
            "required": ["symbol_name"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let symbol_name = required_str(&input, "symbol_name")?;
        let max_depth = optional_u64(&input, "max_depth", 3) as usize;
        let workspace = &context.workspace;

        let engine = deepmap::engine::RepoMapEngine::get_or_scan(workspace, 2000, 300.0);

        let report = deepmap::renderer::render_call_chain_report(&engine, symbol_name, max_depth);
        Ok(ToolResult::success(report))
    }
}

pub struct DeepMapFileDetailTool;

#[async_trait]
impl ToolSpec for DeepMapFileDetailTool {
    fn name(&self) -> &'static str {
        "deepmap_file_detail"
    }

    fn description(&self) -> &'static str {
        "Inspect a specific file's structure without reading it raw. Returns every \
         symbol defined in the file (functions, classes, methods) with their \
         signatures, PageRank importance scores, line numbers, and visibility. \
         Use this before reading or editing a file to quickly understand what's \
         in it and which symbols matter most."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Relative file path within the project (e.g., 'src/server/app.ts')."
                },
                "max_symbols": {
                    "type": "integer",
                    "description": "Maximum symbols to show (default: 12)."
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum output characters (default: 6000)."
                }
            },
            "required": ["file_path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let file_path = required_str(&input, "file_path")?;
        let max_symbols = optional_u64(&input, "max_symbols", 12) as usize;
        let max_chars = optional_u64(&input, "max_chars", 6000) as usize;
        let workspace = &context.workspace;

        let engine = deepmap::engine::RepoMapEngine::get_or_scan(workspace, 2000, 300.0);

        let report = deepmap::renderer::render_file_detail_report(
            &engine,
            file_path,
            max_symbols,
            max_chars,
        );
        Ok(ToolResult::success(report))
    }
}
