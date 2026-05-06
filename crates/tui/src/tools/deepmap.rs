//! DeepMap codebase analysis tools.
//!
//! Provides 3 model-visible tools that give AI agents a "project map"
//! before they start reading individual files.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_u64, required_str,
};

pub struct DeepMapOverviewTool;

#[async_trait]
impl ToolSpec for DeepMapOverviewTool {
    fn name(&self) -> &'static str { "deepmap_overview" }

    fn description(&self) -> &'static str {
        "Use this first when exploring an unfamiliar codebase. Scans the project, \
         extracts symbols, builds a dependency graph, runs PageRank, and returns an \
         AI-friendly project map: entry points, hotspots, key symbols ranked by \
         importance, and recommended reading order. Much faster than reading files \
         one by one."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "max_files": { "type": "integer", "description": "Max files to scan (default 2000)" },
                "max_chars": { "type": "integer", "description": "Max output chars (default 16000)" }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement { ApprovalRequirement::Auto }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let max_files = optional_u64(&input, "max_files", 2000) as usize;
        let max_chars = optional_u64(&input, "max_chars", 16000) as usize;
        let engine = deepmap::engine::RepoMapEngine::get_or_scan(&ctx.workspace, max_files, 300.0);
        let report = deepmap::renderer::render_overview_report(&engine, max_chars);
        Ok(ToolResult::success(report))
    }
}

pub struct DeepMapCallChainTool;

#[async_trait]
impl ToolSpec for DeepMapCallChainTool {
    fn name(&self) -> &'static str { "deepmap_call_chain" }

    fn description(&self) -> &'static str {
        "Trace who calls a symbol and what it calls. Use to understand the \
         impact of changing a function or to discover a symbol's dependencies. \
         Returns callers and callees sorted by PageRank importance."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "symbol_name": { "type": "string", "description": "Symbol to trace (e.g. createApp)" },
                "max_depth": { "type": "integer", "description": "Max traversal depth (default 3)" }
            },
            "required": ["symbol_name"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement { ApprovalRequirement::Auto }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let symbol_name = required_str(&input, "symbol_name")?;
        let max_depth = optional_u64(&input, "max_depth", 3) as usize;
        let engine = deepmap::engine::RepoMapEngine::get_or_scan(&ctx.workspace, 2000, 300.0);
        let report = deepmap::renderer::render_call_chain_report(&engine, symbol_name, max_depth);
        Ok(ToolResult::success(report))
    }
}

pub struct DeepMapFileDetailTool;

#[async_trait]
impl ToolSpec for DeepMapFileDetailTool {
    fn name(&self) -> &'static str { "deepmap_file_detail" }

    fn description(&self) -> &'static str {
        "Inspect a file's structure without reading it raw. Returns every symbol \
         with its signature, PageRank score, line number, and visibility. Use \
         before reading or editing a file to understand what's in it and which \
         symbols matter most."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Relative path (e.g. src/server/app.ts)" },
                "max_symbols": { "type": "integer", "description": "Max symbols to show (default 12)" },
                "max_chars": { "type": "integer", "description": "Max output chars (default 6000)" }
            },
            "required": ["file_path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement { ApprovalRequirement::Auto }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let file_path = required_str(&input, "file_path")?;
        let max_symbols = optional_u64(&input, "max_symbols", 12) as usize;
        let max_chars = optional_u64(&input, "max_chars", 6000) as usize;
        let engine = deepmap::engine::RepoMapEngine::get_or_scan(&ctx.workspace, 2000, 300.0);
        let report = deepmap::renderer::render_file_detail_report(&engine, file_path, max_symbols, max_chars);
        Ok(ToolResult::success(report))
    }
}
