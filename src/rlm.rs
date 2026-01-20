//! Recursive Language Model (RLM) helpers and REPL workflows.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use colored::{ColoredString, Colorize};
use regex::Regex;
use rustyline::Editor;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::models::Usage;
use crate::palette;

// === Command Args ===

/// Arguments for loading a context into memory.
#[allow(dead_code)]
pub struct RlmLoadArgs {
    pub path: PathBuf,
    pub context_id: String,
}

/// Arguments for searching within a loaded context.
#[allow(dead_code)]
pub struct RlmSearchArgs {
    pub context_id: String,
    pub pattern: String,
    pub context_lines: usize,
    pub max_results: usize,
}

/// Arguments for executing code in the RLM sandbox.
#[allow(dead_code)]
pub struct RlmExecArgs {
    pub context_id: String,
    pub code: String,
}

/// Arguments for retrieving RLM status.
#[allow(dead_code)]
pub struct RlmStatusArgs {
    pub context_id: Option<String>,
}

/// Arguments for saving an RLM session to disk.
#[allow(dead_code)]
pub struct RlmSaveSessionArgs {
    pub path: PathBuf,
    pub context_id: String,
}

/// Arguments for loading a saved session from disk.
#[allow(dead_code)]
pub struct RlmLoadSessionArgs {
    pub path: PathBuf,
}

/// Arguments for entering the RLM REPL.
#[allow(dead_code)]
pub struct RlmReplArgs {
    pub context_id: String,
    pub load: Option<PathBuf>,
}

/// High-level RLM CLI commands.
#[allow(dead_code)]
pub enum RlmCommand {
    Load(RlmLoadArgs),
    Search(RlmSearchArgs),
    Exec(RlmExecArgs),
    Status(RlmStatusArgs),
    SaveSession(RlmSaveSessionArgs),
    LoadSession(RlmLoadSessionArgs),
    Repl(RlmReplArgs),
}

// === System Resources ===

/// System resource snapshot used to size RLM contexts.
#[derive(Debug, Clone)]
pub struct SystemResources {
    pub available_memory_mb: Option<u64>,
    pub recommended_max_context: usize,
}

impl SystemResources {
    /// Detect available resources and compute a recommended max context size.
    #[must_use]
    pub fn detect() -> Self {
        let available_memory_mb = Self::get_available_memory_mb();

        // Recommend context size based on available memory
        // Rule of thumb: use ~10% of available RAM for context
        let recommended_max_context = match available_memory_mb {
            Some(mem) if mem >= 32000 => 100_000_000, // 100MB for 32GB+ RAM
            Some(mem) if mem >= 16000 => 50_000_000,  // 50MB for 16GB+ RAM
            Some(mem) if mem >= 8000 => 25_000_000,   // 25MB for 8GB+ RAM
            Some(mem) if mem >= 4000 => 10_000_000,   // 10MB for 4GB+ RAM
            _ => 5_000_000,                           // 5MB default
        };

        Self {
            available_memory_mb,
            recommended_max_context,
        }
    }

    #[cfg(target_os = "macos")]
    fn get_available_memory_mb() -> Option<u64> {
        use std::process::Command;

        // Try to get memory from sysctl on macOS
        Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .map(|bytes| bytes / (1024 * 1024))
            })
    }

    #[cfg(target_os = "linux")]
    fn get_available_memory_mb() -> Option<u64> {
        use std::fs;

        // Read from /proc/meminfo on Linux
        fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|line| line.starts_with("MemTotal:"))
                    .and_then(|line| {
                        line.split_whitespace()
                            .nth(1)
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(|kb| kb / 1024)
                    })
            })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn get_available_memory_mb() -> Option<u64> {
        None
    }

    /// Print a human-readable resource summary.
    pub fn print_info(&self) {
        println!("{}", ds_blue("System Resources").bold());
        if let Some(mem) = self.available_memory_mb {
            let mem_f64 = f64::from(u32::try_from(mem).unwrap_or(u32::MAX));
            println!("  Available RAM: {} MB ({:.1} GB)", mem, mem_f64 / 1024.0);
        } else {
            println!("  Available RAM: Unknown");
        }
        let max_context_f64 =
            f64::from(u32::try_from(self.recommended_max_context).unwrap_or(u32::MAX));
        println!(
            "  Recommended max context: {} chars ({:.1} MB)",
            self.recommended_max_context,
            max_context_f64 / (1024.0 * 1024.0)
        );
    }
}

// === Context Storage ===

/// In-memory context buffer used by the RLM REPL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmContext {
    pub id: String,
    pub content: String,
    pub source_path: Option<String>,
    pub line_count: usize,
    pub char_count: usize,
    pub variables: HashMap<String, String>,
}

impl RlmContext {
    /// Create a new context with derived line/char counts.
    #[must_use]
    pub fn new(id: &str, content: String, source_path: Option<String>) -> Self {
        let line_count = content.lines().count();
        let char_count = content.len();
        Self {
            id: id.to_string(),
            content,
            source_path,
            line_count,
            char_count,
            variables: HashMap::new(),
        }
    }

    /// Peek into the context by character range.
    #[must_use]
    pub fn peek(&self, start: usize, end: Option<usize>) -> &str {
        let end = end.unwrap_or(self.content.len()).min(self.content.len());
        &self.content[start.min(self.content.len())..end]
    }

    /// Return line slices with 1-based line numbers.
    #[must_use]
    pub fn lines(&self, start: usize, end: Option<usize>) -> Vec<(usize, &str)> {
        let lines: Vec<&str> = self.content.lines().collect();
        let end = end.unwrap_or(lines.len()).min(lines.len());
        lines[start.min(lines.len())..end]
            .iter()
            .enumerate()
            .map(|(i, line)| (start + i + 1, *line))
            .collect()
    }

    /// Search for regex matches with optional context lines.
    pub fn search(
        &self,
        pattern: &str,
        context_lines: usize,
        max_results: usize,
    ) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern).context("Invalid regex pattern")?;
        let lines: Vec<&str> = self.content.lines().collect();
        let mut results = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if regex.is_match(line) {
                let start = i.saturating_sub(context_lines);
                let end = (i + context_lines + 1).min(lines.len());
                let context: Vec<String> = lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(j, l)| {
                        let line_num = start + j + 1;
                        if start + j == i {
                            format!("{line_num:>5} > {l}")
                        } else {
                            format!("{line_num:>5}   {l}")
                        }
                    })
                    .collect();

                results.push(SearchResult {
                    line_num: i + 1,
                    match_line: (*line).to_string(),
                    context,
                });

                if results.len() >= max_results {
                    break;
                }
            }
        }

        Ok(results)
    }

    /// Chunk the context into fixed-size segments with overlap.
    #[must_use]
    pub fn chunk(&self, chunk_size: usize, overlap: usize) -> Vec<ChunkInfo> {
        let mut chunks = Vec::new();
        let mut start = 0;
        let mut chunk_index = 0;

        while start < self.content.len() {
            let end = (start + chunk_size).min(self.content.len());
            let preview_end = (start + 100).min(end);
            let preview = self.content[start..preview_end].to_string();

            chunks.push(ChunkInfo {
                index: chunk_index,
                start_char: start,
                end_char: end,
                preview: preview.replace('\n', " "),
            });

            start = if end == self.content.len() {
                end
            } else {
                (end - overlap).max(start + 1)
            };
            chunk_index += 1;
        }

        chunks
    }

    /// Chunk the context by paragraph/heading boundaries up to a max char size.
    #[must_use]
    pub fn chunk_sections(&self, max_chars: usize) -> Vec<ChunkInfo> {
        let max_chars = max_chars.max(1);
        let mut sections = Vec::new();
        let mut start = 0;
        let mut offset = 0;

        for segment in self.content.split_inclusive('\n') {
            let line = segment.trim_end_matches('\n');
            let trimmed = line.trim();
            let is_heading = trimmed.starts_with('#');
            let is_blank = trimmed.is_empty();

            if is_heading && offset > start {
                sections.push((start, offset));
                start = offset;
            }

            offset += segment.len();

            if is_blank && offset > start {
                sections.push((start, offset));
                start = offset;
            }
        }

        if offset > start {
            sections.push((start, offset));
        }

        let mut chunks = Vec::new();
        let mut chunk_start = 0;
        let mut chunk_end = 0;
        let mut chunk_index = 0;

        for (section_start, section_end) in sections {
            if chunk_end == 0 {
                chunk_start = section_start;
            }
            if section_end - chunk_start > max_chars && chunk_end > chunk_start {
                chunks.push(build_chunk_info(
                    &self.content,
                    chunk_index,
                    chunk_start,
                    chunk_end,
                ));
                chunk_index += 1;
                chunk_start = section_start;
            }
            chunk_end = section_end;
        }

        if chunk_end > chunk_start {
            chunks.push(build_chunk_info(
                &self.content,
                chunk_index,
                chunk_start,
                chunk_end,
            ));
        }

        chunks
    }

    /// Chunk the context by line count.
    #[must_use]
    pub fn chunk_lines(&self, max_lines: usize) -> Vec<ChunkInfo> {
        let max_lines = max_lines.max(1);
        let mut chunks = Vec::new();
        let mut chunk_start = 0;
        let mut offset = 0;
        let mut line_count = 0;
        let mut chunk_index = 0;

        for segment in self.content.split_inclusive('\n') {
            line_count += 1;
            offset += segment.len();

            if line_count >= max_lines {
                chunks.push(build_chunk_info(
                    &self.content,
                    chunk_index,
                    chunk_start,
                    offset,
                ));
                chunk_index += 1;
                chunk_start = offset;
                line_count = 0;
            }
        }

        if chunk_start < self.content.len() {
            chunks.push(build_chunk_info(
                &self.content,
                chunk_index,
                chunk_start,
                self.content.len(),
            ));
        }

        chunks
    }

    /// Chunk the context using headings, paragraphs, and code fences.
    #[must_use]
    pub fn chunk_auto(&self, max_chars: usize) -> Vec<ChunkInfo> {
        let max_chars = max_chars.max(1);
        let mut segments = Vec::new();
        let mut start = 0;
        let mut offset = 0;
        let mut in_code_block = false;

        for segment in self.content.split_inclusive('\n') {
            let line = segment.trim_end_matches('\n');
            let trimmed = line.trim();
            let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
            let is_heading = trimmed.starts_with('#');
            let is_blank = trimmed.is_empty();

            if is_fence {
                if !in_code_block {
                    if offset > start {
                        segments.push((start, offset));
                    }
                    start = offset;
                    in_code_block = true;
                } else {
                    in_code_block = false;
                }
            } else if !in_code_block {
                if is_heading && offset > start {
                    segments.push((start, offset));
                    start = offset;
                }

                if is_blank && offset > start {
                    segments.push((start, offset));
                    start = offset;
                }
            }

            offset += segment.len();

            if is_fence && !in_code_block && offset > start {
                segments.push((start, offset));
                start = offset;
            }
        }

        if offset > start {
            segments.push((start, offset));
        }

        let mut normalized = Vec::new();
        for (seg_start, seg_end) in segments {
            let mut cursor = seg_start;
            while cursor < seg_end {
                let end = (cursor + max_chars).min(seg_end);
                normalized.push((cursor, end));
                cursor = end;
            }
        }

        let mut chunks = Vec::new();
        let mut chunk_start = 0;
        let mut chunk_end = 0;
        let mut chunk_index = 0;

        for (seg_start, seg_end) in normalized {
            if chunk_end == 0 {
                chunk_start = seg_start;
            }
            if seg_end - chunk_start > max_chars && chunk_end > chunk_start {
                chunks.push(build_chunk_info(
                    &self.content,
                    chunk_index,
                    chunk_start,
                    chunk_end,
                ));
                chunk_index += 1;
                chunk_start = seg_start;
            }
            chunk_end = seg_end;
        }

        if chunk_end > chunk_start {
            chunks.push(build_chunk_info(
                &self.content,
                chunk_index,
                chunk_start,
                chunk_end,
            ));
        }

        chunks
    }

    #[must_use]
    pub fn get_var(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }

    pub fn set_var(&mut self, name: &str, value: String) {
        self.variables.insert(name.to_string(), value);
    }

    pub fn append_var(&mut self, name: &str, value: String) {
        self.variables
            .entry(name.to_string())
            .and_modify(|existing| {
                if !existing.is_empty() {
                    existing.push('\n');
                }
                existing.push_str(&value);
            })
            .or_insert(value);
    }

    pub fn remove_var(&mut self, name: &str) -> Option<String> {
        self.variables.remove(name)
    }
}

/// Search match result with surrounding context lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub line_num: usize,
    pub match_line: String,
    pub context: Vec<String>,
}

/// Chunk metadata for context navigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub index: usize,
    pub start_char: usize,
    pub end_char: usize,
    pub preview: String,
}

fn build_chunk_info(content: &str, index: usize, start: usize, end: usize) -> ChunkInfo {
    let safe_start = start.min(content.len());
    let safe_end = end.min(content.len());
    let preview_end = (safe_start + 100).min(safe_end);
    let preview = content[safe_start..preview_end].replace('\n', " ");

    ChunkInfo {
        index,
        start_char: safe_start,
        end_char: safe_end,
        preview,
    }
}

/// Usage stats for RLM sub-queries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RlmUsage {
    pub queries: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_chars_sent: u64,
    pub total_chars_received: u64,
}

impl RlmUsage {
    pub fn record(&mut self, usage: &Usage, chars_sent: usize, chars_received: usize) {
        self.queries = self.queries.saturating_add(1);
        self.input_tokens = self
            .input_tokens
            .saturating_add(u64::from(usage.input_tokens));
        self.output_tokens = self
            .output_tokens
            .saturating_add(u64::from(usage.output_tokens));
        self.total_chars_sent = self
            .total_chars_sent
            .saturating_add(u64::try_from(chars_sent).unwrap_or(u64::MAX));
        self.total_chars_received = self
            .total_chars_received
            .saturating_add(u64::try_from(chars_received).unwrap_or(u64::MAX));
    }
}

/// Stored RLM session state for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmSession {
    pub contexts: HashMap<String, RlmContext>,
    pub active_context: String,
    #[serde(default)]
    pub usage: RlmUsage,
}

impl Default for RlmSession {
    fn default() -> Self {
        Self {
            contexts: HashMap::new(),
            active_context: "default".to_string(),
            usage: RlmUsage::default(),
        }
    }
}

pub type SharedRlmSession = Arc<Mutex<RlmSession>>;

impl RlmSession {
    pub fn load_context(&mut self, id: &str, content: String, source_path: Option<String>) {
        let ctx = RlmContext::new(id, content, source_path);
        self.contexts.insert(id.to_string(), ctx);
        self.active_context = id.to_string();
    }

    /// Load a file into a new context, returning line/char counts.
    pub(crate) fn load_file(&mut self, id: &str, path: &Path) -> Result<(usize, usize)> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        let source = path.to_string_lossy().to_string();
        self.load_context(id, content, Some(source));

        let ctx = self
            .contexts
            .get(id)
            .context("Loaded context missing from session")?;
        Ok((ctx.line_count, ctx.char_count))
    }

    pub fn get_context(&self, id: &str) -> Option<&RlmContext> {
        self.contexts.get(id)
    }

    #[allow(dead_code)]
    pub fn get_context_mut(&mut self, id: &str) -> Option<&mut RlmContext> {
        self.contexts.get_mut(id)
    }

    pub fn record_query_usage(&mut self, usage: &Usage, chars_sent: usize, chars_received: usize) {
        self.usage.record(usage, chars_sent, chars_received);
    }
}

pub fn context_id_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("context")
        .to_string()
}

pub fn unique_context_id(session: &RlmSession, base: &str) -> String {
    if !session.contexts.contains_key(base) {
        return base.to_string();
    }

    for idx in 2..=99 {
        let candidate = format!("{base}-{idx}");
        if !session.contexts.contains_key(&candidate) {
            return candidate;
        }
    }

    format!("{base}-{}", session.contexts.len() + 1)
}

pub fn session_summary(session: &RlmSession) -> String {
    if session.contexts.is_empty() {
        return "No RLM contexts loaded.".to_string();
    }

    let mut lines = Vec::new();
    lines.push(format!("Active context: {}", session.active_context));
    lines.push(format!("Loaded contexts: {}", session.contexts.len()));
    lines.push(format!(
        "Queries: {} | Input tokens: {} | Output tokens: {}",
        session.usage.queries, session.usage.input_tokens, session.usage.output_tokens
    ));

    let mut ids: Vec<_> = session.contexts.keys().collect();
    ids.sort();
    for id in ids {
        if let Some(ctx) = session.contexts.get(id) {
            let source = ctx
                .source_path
                .as_ref()
                .map(|s| format!(" (source: {s})"))
                .unwrap_or_default();
            lines.push(format!(
                "- {id}: {} lines, {} chars, {} vars{source}",
                ctx.line_count,
                ctx.char_count,
                ctx.variables.len()
            ));
        }
    }

    lines.join("\n")
}

#[allow(dead_code)]
pub fn handle_command(command: RlmCommand, _config: &Config) -> Result<()> {
    let mut session = RlmSession::default();

    match command {
        RlmCommand::Load(args) => {
            let content = fs::read_to_string(&args.path)
                .with_context(|| format!("Failed to read file: {}", args.path.display()))?;
            let source = args.path.to_string_lossy().to_string();
            session.load_context(&args.context_id, content, Some(source));

            let ctx = session
                .get_context(&args.context_id)
                .expect("context should exist after load_context");
            println!("{}", ds_aqua("Context loaded successfully!"));
            println!("  ID: {}", ds_blue(&ctx.id));
            println!("  Source: {}", ctx.source_path.as_deref().unwrap_or("N/A"));
            println!("  Lines: {}", ctx.line_count);
            println!("  Characters: {}", ctx.char_count);
        }
        RlmCommand::Search(args) => {
            let content = load_context_from_stdin_or_error(&args.context_id)?;
            let ctx = RlmContext::new(&args.context_id, content, None);

            let results = ctx.search(&args.pattern, args.context_lines, args.max_results)?;

            if results.is_empty() {
                println!("{}", ds_sky("No matches found."));
            } else {
                println!("{} matches found:\n", ds_aqua(&results.len().to_string()));
                for result in results {
                    println!("{}", "─".repeat(60).dimmed());
                    for line in &result.context {
                        println!("{line}");
                    }
                }
                println!("{}", "─".repeat(60).dimmed());
            }
        }
        RlmCommand::Exec(args) => {
            let content = load_context_from_stdin_or_error(&args.context_id)?;
            let ctx = RlmContext::new(&args.context_id, content, None);

            let result = eval_expr(&ctx, &args.code)?;
            println!("{result}");
        }
        RlmCommand::Status(args) => {
            if let Some(id) = args.context_id {
                println!("Context '{id}' status: (no persistent session)");
            } else {
                println!("{}", ds_blue("RLM Session Status").bold());
                println!("Note: For persistent sessions, use 'rlm repl' or save/load session.");
            }
        }
        RlmCommand::SaveSession(args) => {
            let json = serde_json::to_string_pretty(&session)?;
            fs::write(&args.path, json)?;
            println!("Session saved to {}", args.path.display());
        }
        RlmCommand::LoadSession(args) => {
            let content = fs::read_to_string(&args.path)?;
            session = serde_json::from_str(&content)?;
            println!("Session loaded from {}", args.path.display());
            println!(
                "Contexts: {:?}",
                session.contexts.keys().collect::<Vec<_>>()
            );
        }
        RlmCommand::Repl(args) => {
            run_repl(&args.context_id, args.load.as_deref())?;
        }
    }

    Ok(())
}

fn load_context_from_stdin_or_error(context_id: &str) -> Result<String> {
    // For now, return an error - real implementation would track sessions
    anyhow::bail!(
        "Failed to load context '{context_id}': no context loaded. Use 'rlm load' or 'rlm repl'."
    )
}

pub fn eval_in_session(session: &mut RlmSession, code: &str) -> Result<String> {
    let active = session.active_context.clone();
    let ctx = session
        .get_context_mut(&active)
        .context("No context loaded. Use /load <path> first.")?;
    eval_expr_mut(ctx, code)
}

pub fn eval_expr(ctx: &RlmContext, code: &str) -> Result<String> {
    eval_expr_internal(ctx, code)
}

pub fn eval_expr_mut(ctx: &mut RlmContext, code: &str) -> Result<String> {
    let code = code.trim();

    if code == "vars" || code == "vars()" {
        if ctx.variables.is_empty() {
            return Ok("No variables set.".to_string());
        }
        let mut names: Vec<_> = ctx.variables.keys().collect();
        names.sort();
        let mut lines = Vec::new();
        for name in names {
            if let Some(value) = ctx.variables.get(name) {
                let preview = value.chars().take(80).collect::<String>();
                lines.push(format!("{name}: {} chars | {preview}", value.len()));
            }
        }
        return Ok(lines.join("\n"));
    }

    if code.starts_with("get(") && code.ends_with(')') {
        let arg = &code[4..code.len() - 1];
        let name = parse_string_arg(arg);
        return ctx
            .get_var(&name)
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("Unknown variable '{name}'"));
    }

    if code.starts_with("set(") && code.ends_with(')') {
        let args = &code[4..code.len() - 1];
        let (name, value) = parse_two_args(args)?;
        ctx.set_var(&name, value);
        return Ok(format!("Set variable '{name}'."));
    }

    if code.starts_with("append(") && code.ends_with(')') {
        let args = &code[7..code.len() - 1];
        let (name, value) = parse_two_args(args)?;
        ctx.append_var(&name, value);
        return Ok(format!("Appended to variable '{name}'."));
    }

    if code.starts_with("del(") && code.ends_with(')') {
        let arg = &code[4..code.len() - 1];
        let name = parse_string_arg(arg);
        if ctx.remove_var(&name).is_some() {
            return Ok(format!("Deleted variable '{name}'."));
        }
        return Ok(format!("Variable '{name}' not found."));
    }

    if code == "clear_vars" || code == "clear_vars()" {
        ctx.variables.clear();
        return Ok("Cleared all variables.".to_string());
    }

    eval_expr_internal(ctx, code)
}

fn eval_expr_internal(ctx: &RlmContext, code: &str) -> Result<String> {
    // Simple expression evaluator for RLM
    // Supports: len(ctx), lines(start, end), search("pattern"), peek(start, end), chunk(size)
    let code = code.trim();

    if code == "len(ctx)" || code == "len" {
        return Ok(format!("{}", ctx.char_count));
    }

    if code == "line_count" || code == "lines" {
        return Ok(format!("{}", ctx.line_count));
    }

    if code.starts_with("peek(") && code.ends_with(')') {
        let args = &code[5..code.len() - 1];
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        let start: usize = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let end: Option<usize> = parts.get(1).and_then(|s| s.parse().ok());
        return Ok(ctx.peek(start, end).to_string());
    }

    if code.starts_with("lines(") && code.ends_with(')') {
        let args = &code[6..code.len() - 1];
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        let start_line = parse_line_arg(parts.first(), 1);
        let end_line = parse_line_arg_opt(parts.get(1).copied());
        let lines = format_lines(ctx, start_line, end_line);
        return Ok(lines);
    }

    if code.starts_with("search(") && code.ends_with(')') {
        let pattern = &code[7..code.len() - 1].trim_matches('"').trim_matches('\'');
        let results = ctx.search(pattern, 2, 20)?;
        if results.is_empty() {
            return Ok("No matches found.".to_string());
        }
        let mut output = Vec::new();
        for result in results {
            output.push(format!("Line {}: {}", result.line_num, result.match_line));
        }
        return Ok(output.join("\n"));
    }

    if code.starts_with("chunk(") && code.ends_with(')') {
        let args = &code[6..code.len() - 1];
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        let size: usize = parts.first().and_then(|s| s.parse().ok()).unwrap_or(2000);
        let overlap: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);
        let chunks = ctx.chunk(size, overlap);
        let output: Vec<String> = chunks
            .iter()
            .map(|c| {
                format!(
                    "Chunk {}: chars {}..{} - {}",
                    c.index,
                    c.start_char,
                    c.end_char,
                    &c.preview[..50.min(c.preview.len())]
                )
            })
            .collect();
        return Ok(output.join("\n"));
    }

    if code.starts_with("chunk_sections(") && code.ends_with(')') {
        let args = &code[15..code.len() - 1];
        let size: usize = args.trim().parse().unwrap_or(20_000);
        let chunks = ctx.chunk_sections(size);
        let output: Vec<String> = chunks
            .iter()
            .map(|c| {
                format!(
                    "Section {}: chars {}..{} - {}",
                    c.index,
                    c.start_char,
                    c.end_char,
                    &c.preview[..50.min(c.preview.len())]
                )
            })
            .collect();
        return Ok(output.join("\n"));
    }

    if code.starts_with("chunk_lines(") && code.ends_with(')') {
        let args = &code[12..code.len() - 1];
        let size: usize = args.trim().parse().unwrap_or(200);
        let chunks = ctx.chunk_lines(size);
        let output: Vec<String> = chunks
            .iter()
            .map(|c| {
                format!(
                    "Lines {}: chars {}..{} - {}",
                    c.index,
                    c.start_char,
                    c.end_char,
                    &c.preview[..50.min(c.preview.len())]
                )
            })
            .collect();
        return Ok(output.join("\n"));
    }

    if code.starts_with("chunk_auto(") && code.ends_with(')') {
        let args = &code[11..code.len() - 1];
        let size: usize = args.trim().parse().unwrap_or(20_000);
        let chunks = ctx.chunk_auto(size);
        let output: Vec<String> = chunks
            .iter()
            .map(|c| {
                format!(
                    "Auto {}: chars {}..{} - {}",
                    c.index,
                    c.start_char,
                    c.end_char,
                    &c.preview[..50.min(c.preview.len())]
                )
            })
            .collect();
        return Ok(output.join("\n"));
    }

    if code == "head" || code == "head()" {
        return Ok(format_lines(ctx, 1, Some(10)));
    }

    if code == "tail" || code == "tail()" {
        let start_line = ctx.line_count.saturating_sub(9).max(1);
        return Ok(format_lines(ctx, start_line, None));
    }

    anyhow::bail!(
        "Failed to evaluate expression: unknown expression '{code}'. Supported: len, line_count, peek(start, end), lines(start, end), search(pattern), chunk(size, overlap), chunk_sections(max_chars), chunk_lines(max_lines), chunk_auto(max_chars), vars, get(name), set(name, value), append(name, value), del(name), clear_vars, head, tail"
    )
}

fn parse_line_arg(input: Option<&&str>, default: usize) -> usize {
    input
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(default)
        .max(1)
}

fn parse_line_arg_opt(input: Option<&str>) -> Option<usize> {
    let value = input.and_then(|s| s.parse::<usize>().ok())?;
    Some(value.max(1))
}

fn parse_string_arg(arg: &str) -> String {
    arg.trim().trim_matches('"').trim_matches('\'').to_string()
}

fn parse_two_args(input: &str) -> Result<(String, String)> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '\0';

    for ch in input.chars() {
        if (ch == '"' || ch == '\'') && (!in_quotes || ch == quote_char) {
            if in_quotes && ch == quote_char {
                in_quotes = false;
            } else if !in_quotes {
                in_quotes = true;
                quote_char = ch;
            }
            current.push(ch);
            continue;
        }

        if ch == ',' && !in_quotes {
            parts.push(current.trim().to_string());
            current.clear();
            continue;
        }

        current.push(ch);
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    if parts.len() < 2 {
        anyhow::bail!("Expected two arguments separated by a comma");
    }

    let left = parse_string_arg(&parts[0]);
    let right = parse_string_arg(&parts[1]);
    Ok((left, right))
}

fn format_lines(ctx: &RlmContext, start_line: usize, end_line: Option<usize>) -> String {
    let start_line = start_line.max(1);
    let end_line = end_line.unwrap_or(ctx.line_count).max(start_line);
    let start_idx = start_line.saturating_sub(1);
    let end_idx = end_line.min(ctx.line_count);
    let lines = ctx.lines(start_idx, Some(end_idx));
    lines
        .iter()
        .map(|(n, l)| format!("{n:>5} {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_repl(context_id: &str, initial_load: Option<&std::path::Path>) -> Result<()> {
    println!("{}", ds_blue("DeepSeek RLM Sandbox").bold());
    println!("Recursive Language Model - Local REPL Environment");
    println!("Type expressions or /help for commands.\n");

    // Detect and display system resources
    let resources = SystemResources::detect();
    resources.print_info();
    println!();

    let mut session = RlmSession::default();

    // Load initial file if provided
    if let Some(path) = initial_load {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        let source = path.to_string_lossy().to_string();
        session.load_context(context_id, content, Some(source));

        let ctx = session
            .get_context(context_id)
            .expect("context should exist after load_context");
        println!("{}", ds_aqua("Context loaded!"));
        println!("  Lines: {} | Chars: {}\n", ctx.line_count, ctx.char_count);
    }

    let mut editor = Editor::<(), DefaultHistory>::new()?;
    let history_path = dirs::home_dir()
        .map(|h| h.join(".deepseek").join("rlm_history"))
        .unwrap_or_default();
    let _ = editor.load_history(&history_path);

    loop {
        let prompt = format!("{}> ", ds_blue("rlm"));
        match editor.readline(&prompt) {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                editor.add_history_entry(input)?;

                if input == "/exit" || input == "/quit" || input == "/q" {
                    break;
                }

                if input == "/help" {
                    print_repl_help();
                    continue;
                }

                if input == "/status" {
                    print_status(&session);
                    continue;
                }

                if let Some(rest) = input.strip_prefix("/load ") {
                    let path = Path::new(rest.trim());
                    match fs::read_to_string(path) {
                        Ok(content) => {
                            let source = path.to_string_lossy().to_string();
                            session.load_context(context_id, content, Some(source));
                            let ctx = session
                                .get_context(context_id)
                                .expect("context should exist after load_context");
                            println!("{}", ds_aqua("Loaded!"));
                            println!("  Lines: {} | Chars: {}", ctx.line_count, ctx.char_count);
                        }
                        Err(e) => {
                            println!("{}: {}", ds_red("Error"), e);
                        }
                    }
                    continue;
                }

                if let Some(rest) = input.strip_prefix("/save ") {
                    let path = Path::new(rest.trim());
                    let json = serde_json::to_string_pretty(&session)?;
                    fs::write(path, json)?;
                    println!("Session saved to {}", path.display());
                    continue;
                }

                // Execute expression
                if let Some(ctx) = session.get_context(context_id) {
                    match eval_expr(ctx, input) {
                        Ok(result) => println!("{result}"),
                        Err(e) => println!("{}: {}", ds_red("Error"), e),
                    }
                } else {
                    println!("{}: No context loaded. Use /load <path>", ds_sky("Error"));
                }
            }
            Err(ReadlineError::Interrupted) => {}
            Err(ReadlineError::Eof) => break,
            Err(err) => {
                println!("{}: {}", ds_red("Error"), err);
                break;
            }
        }
    }

    let _ = editor.save_history(&history_path);
    Ok(())
}

fn print_repl_help() {
    println!("{}", ds_blue("RLM Sandbox Commands").bold());
    println!();
    println!("  /load <path>   Load a file into context");
    println!("  /save <path>   Save session to file");
    println!("  /status        Show session status");
    println!("  /help          Show this help");
    println!("  /exit          Exit REPL");
    println!();
    println!("{}", ds_blue("Expressions").bold());
    println!();
    println!("  len              Character count");
    println!("  line_count       Line count");
    println!("  head             First 10 lines");
    println!("  tail             Last 10 lines");
    println!("  peek(s, e)       Characters from s to e");
    println!("  lines(s, e)      Lines from s to e");
    println!("  search(pattern)  Regex search");
    println!("  chunk(size, overlap)  Split into chunks");
    println!("  chunk_sections(max)   Chunk by headings/paragraphs");
    println!("  chunk_lines(max)      Chunk by line count");
    println!("  chunk_auto(max)       Chunk by headings + paragraphs + code fences");
}

fn print_status(session: &RlmSession) {
    println!("{}", ds_blue("Session Status").bold());
    println!("  Active context: {}", session.active_context);
    println!("  Loaded contexts: {}", session.contexts.len());
    for (id, ctx) in &session.contexts {
        println!(
            "    {}: {} lines, {} chars",
            id, ctx.line_count, ctx.char_count
        );
        if let Some(ref source) = ctx.source_path {
            println!("      Source: {source}");
        }
    }
}

fn ds_blue(text: &str) -> ColoredString {
    let (r, g, b) = palette::DEEPSEEK_BLUE_RGB;
    text.truecolor(r, g, b)
}

fn ds_sky(text: &str) -> ColoredString {
    let (r, g, b) = palette::DEEPSEEK_SKY_RGB;
    text.truecolor(r, g, b)
}

fn ds_aqua(text: &str) -> ColoredString {
    let (r, g, b) = palette::DEEPSEEK_SKY_RGB;
    text.truecolor(r, g, b)
}

fn ds_red(text: &str) -> ColoredString {
    let (r, g, b) = palette::DEEPSEEK_RED_RGB;
    text.truecolor(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    fn format_lines(start: usize, end: usize) -> String {
        (start..=end)
            .map(|i| format!("{i:>5} line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn rlm_exec_len_head_tail_lines() -> Result<()> {
        let content = (1..=15)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let ctx = RlmContext::new("test", content, None);

        let len_output = eval_expr(&ctx, "len")?;
        assert_eq!(len_output, ctx.char_count.to_string());

        let head_output = eval_expr(&ctx, "head")?;
        assert_eq!(head_output, format_lines(1, 10));

        let tail_output = eval_expr(&ctx, "tail")?;
        assert_eq!(tail_output, format_lines(6, 15));

        let lines_output = eval_expr(&ctx, "lines(1, 10)")?;
        assert_eq!(lines_output, format_lines(1, 10));

        Ok(())
    }

    #[test]
    fn rlm_load_file_populates_session() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        writeln!(file, "alpha")?;
        writeln!(file, "beta")?;

        let mut session = RlmSession::default();
        let (line_count, char_count) = session.load_file("ctx", file.path())?;

        assert_eq!(session.active_context, "ctx");
        assert_eq!(line_count, 2);
        assert_eq!(char_count, "alpha\nbeta\n".len());

        Ok(())
    }

    #[test]
    fn rlm_variables_set_get_append() -> Result<()> {
        let content = "line 1\nline 2\n".to_string();
        let mut ctx = RlmContext::new("test", content, None);

        let _ = eval_expr_mut(&mut ctx, "set(\"answer\", \"alpha\")")?;
        assert_eq!(ctx.get_var("answer"), Some("alpha"));

        let _ = eval_expr_mut(&mut ctx, "append(\"answer\", \"beta\")")?;
        let value = ctx.get_var("answer").unwrap_or("");
        assert!(value.contains("alpha"));
        assert!(value.contains("beta"));

        let vars = eval_expr_mut(&mut ctx, "vars()")?;
        assert!(vars.contains("answer"));

        Ok(())
    }

    #[test]
    fn rlm_chunk_sections_splits_on_headings() {
        let content = "# Title\nalpha\n\n## Section\nbeta\n\npara".to_string();
        let ctx = RlmContext::new("test", content, None);
        let chunks = ctx.chunk_sections(20);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn rlm_chunk_auto_splits_on_paragraphs_and_fences() {
        let content = "# Title\nalpha\n\n```rust\ncode\n```\n\nbeta".to_string();
        let ctx = RlmContext::new("test", content, None);
        let chunks = ctx.chunk_auto(20);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|chunk| !chunk.preview.is_empty()));
    }
}
