//! TUI rendering helpers for chat history and tool output.

use std::path::PathBuf;
use std::time::Instant;

use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::models::{ContentBlock, Message};
use crate::palette;
use crate::tools::review::ReviewOutput;
use crate::tui::diff_render;
use crate::tui::markdown_render;

// === Constants ===

const TOOL_COMMAND_LINE_LIMIT: usize = 5;
const TOOL_OUTPUT_LINE_LIMIT: usize = 12;
const TOOL_TEXT_LIMIT: usize = 240;

// === History Cells ===

/// Renderable history cell for user/assistant/system entries.
#[derive(Debug, Clone)]
pub enum HistoryCell {
    User { content: String },
    Assistant { content: String, streaming: bool },
    Player { content: String, streaming: bool },
    Coach { content: String, streaming: bool },
    System { content: String },
    Thinking { content: String, streaming: bool },
    Tool(ToolCell),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptRenderOptions {
    pub show_thinking: bool,
    pub show_tool_details: bool,
}

impl Default for TranscriptRenderOptions {
    fn default() -> Self {
        Self {
            show_thinking: true,
            show_tool_details: true,
        }
    }
}

impl HistoryCell {
    /// Render the cell into a set of terminal lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        match self {
            HistoryCell::User { content } => render_message("You", content, user_style(), width),
            HistoryCell::Assistant { content, streaming } => {
                let mut lines = render_message("DeepSeek", content, assistant_style(), width);
                if *streaming {
                    // Add blinking cursor to last line
                    if let Some(last) = lines.last_mut() {
                        last.spans.push(Span::styled(
                            "▋",
                            Style::default().fg(palette::DEEPSEEK_SKY),
                        ));
                    }
                }
                lines
            }
            HistoryCell::Player { content, streaming } => {
                let mut lines = render_message("Player", content, player_style(), width);
                if *streaming {
                    if let Some(last) = lines.last_mut() {
                        last.spans.push(Span::styled(
                            "▋",
                            Style::default().fg(palette::DEEPSEEK_SKY),
                        ));
                    }
                }
                lines
            }
            HistoryCell::Coach { content, streaming } => {
                let mut lines = render_message("Coach", content, coach_style(), width);
                if *streaming {
                    if let Some(last) = lines.last_mut() {
                        last.spans.push(Span::styled(
                            "▋",
                            Style::default().fg(palette::DEEPSEEK_SKY),
                        ));
                    }
                }
                lines
            }
            HistoryCell::System { content } => {
                render_message("System", content, system_style(), width)
            }
            HistoryCell::Thinking { content, streaming } => {
                let mut lines = render_thinking(content, width);
                if *streaming {
                    if let Some(last) = lines.last_mut() {
                        last.spans.push(Span::styled(
                            "▋",
                            Style::default().fg(palette::DEEPSEEK_SKY),
                        ));
                    }
                }
                lines
            }
            HistoryCell::Tool(cell) => cell.lines(width),
        }
    }

    pub fn lines_with_options(
        &self,
        width: u16,
        options: TranscriptRenderOptions,
    ) -> Vec<Line<'static>> {
        match self {
            HistoryCell::Thinking { .. } if !options.show_thinking => Vec::new(),
            HistoryCell::Tool(cell) if !options.show_tool_details => {
                let mut lines = cell.lines(width);
                if lines.len() > 2 {
                    lines.truncate(2);
                    lines.push(Line::from(Span::styled(
                        "  … details hidden (show_tool_details=off)",
                        Style::default().fg(palette::TEXT_MUTED).italic(),
                    )));
                }
                lines
            }
            _ => self.lines(width),
        }
    }

    /// Whether this cell is the continuation of a streaming assistant message.
    #[must_use]
    pub fn is_stream_continuation(&self) -> bool {
        matches!(
            self,
            HistoryCell::Assistant {
                streaming: true,
                ..
            }
        )
    }
}

/// Convert a message into history cells for rendering.
#[must_use]
pub fn history_cells_from_message(msg: &Message) -> Vec<HistoryCell> {
    history_cells_from_message_with_mode(msg, None)
}

/// Convert a message into history cells with optional Duo phase context.
#[must_use]
pub fn history_cells_from_message_with_mode(
    msg: &Message,
    duo_phase: Option<crate::duo::DuoPhase>,
) -> Vec<HistoryCell> {
    let mut cells = Vec::new();
    let mut text_blocks = Vec::new();
    let mut thinking_blocks = Vec::new();

    for block in &msg.content {
        match block {
            ContentBlock::Text { text, .. } => text_blocks.push(text.clone()),
            ContentBlock::Thinking { thinking } => thinking_blocks.push(thinking.clone()),
            _ => {}
        }
    }

    if !text_blocks.is_empty() {
        let content = text_blocks.join("\n");
        match msg.role.as_str() {
            "user" => cells.push(HistoryCell::User { content }),
            "assistant" => {
                let cell = match duo_phase {
                    Some(crate::duo::DuoPhase::Player) | Some(crate::duo::DuoPhase::Init) => {
                        HistoryCell::Player {
                            content,
                            streaming: false,
                        }
                    }
                    Some(crate::duo::DuoPhase::Coach) => HistoryCell::Coach {
                        content,
                        streaming: false,
                    },
                    _ => HistoryCell::Assistant {
                        content,
                        streaming: false,
                    },
                };
                cells.push(cell);
            }
            "system" => cells.push(HistoryCell::System { content }),
            _ => {}
        }
    }

    if !thinking_blocks.is_empty() {
        let reasoning = thinking_blocks.join("\n");
        cells.push(HistoryCell::Thinking {
            content: reasoning,
            streaming: false,
        });
    }

    cells
}

// === Tool Cells ===

/// Variants describing a tool result cell.
#[derive(Debug, Clone)]
pub enum ToolCell {
    Exec(ExecCell),
    Exploring(ExploringCell),
    PlanUpdate(PlanUpdateCell),
    PatchSummary(PatchSummaryCell),
    Review(ReviewCell),
    DiffPreview(DiffPreviewCell),
    Mcp(McpToolCell),
    ViewImage(ViewImageCell),
    WebSearch(WebSearchCell),
    Generic(GenericToolCell),
}

impl ToolCell {
    /// Render the tool cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        match self {
            ToolCell::Exec(cell) => cell.lines(width),
            ToolCell::Exploring(cell) => cell.lines(width),
            ToolCell::PlanUpdate(cell) => cell.lines(width),
            ToolCell::PatchSummary(cell) => cell.lines(width),
            ToolCell::Review(cell) => cell.lines(width),
            ToolCell::DiffPreview(cell) => cell.lines(width),
            ToolCell::Mcp(cell) => cell.lines(width),
            ToolCell::ViewImage(cell) => cell.lines(width),
            ToolCell::WebSearch(cell) => cell.lines(width),
            ToolCell::Generic(cell) => cell.lines(width),
        }
    }
}

/// Overall status for a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Success,
    Failed,
}

/// Shell command execution rendering data.
#[derive(Debug, Clone)]
pub struct ExecCell {
    pub command: String,
    pub status: ToolStatus,
    pub output: Option<String>,
    pub started_at: Option<Instant>,
    pub duration_ms: Option<u64>,
    pub source: ExecSource,
    pub interaction: Option<String>,
}

impl ExecCell {
    /// Render the execution cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let (label, color) = match self.status {
            ToolStatus::Running => ("Running", palette::STATUS_WARNING),
            ToolStatus::Success => match self.source {
                ExecSource::User => ("You ran", palette::STATUS_SUCCESS),
                ExecSource::Assistant => ("Ran", palette::STATUS_SUCCESS),
            },
            ToolStatus::Failed => ("Failed", palette::STATUS_ERROR),
        };
        let dot = status_symbol(self.started_at, self.status);
        lines.push(Line::from(vec![
            Span::styled(format!("{dot} "), Style::default().fg(color)),
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));

        if let Some(interaction) = self.interaction.as_ref() {
            lines.extend(wrap_plain_line(
                &format!("  {interaction}"),
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        } else {
            lines.extend(render_command(&self.command, width));
        }

        if self.interaction.is_none() {
            if let Some(output) = self.output.as_ref() {
                lines.extend(render_exec_output(output, width, TOOL_OUTPUT_LINE_LIMIT));
            } else if self.status != ToolStatus::Running {
                lines.push(Line::from(Span::styled(
                    "  (no output)",
                    Style::default().fg(palette::TEXT_MUTED).italic(),
                )));
            }
        }

        if let Some(duration_ms) = self.duration_ms {
            let seconds = f64::from(u32::try_from(duration_ms).unwrap_or(u32::MAX)) / 1000.0;
            lines.push(Line::from(Span::styled(
                format!("  {seconds:.2}s"),
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }

        lines
    }
}

/// Source of a shell command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecSource {
    User,
    Assistant,
}

/// Aggregate cell for tool exploration runs.
#[derive(Debug, Clone)]
pub struct ExploringCell {
    pub entries: Vec<ExploringEntry>,
}

impl ExploringCell {
    /// Render the exploring cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let all_done = self
            .entries
            .iter()
            .all(|entry| entry.status != ToolStatus::Running);
        let header = if all_done { "Explored" } else { "Exploring" };
        lines.push(Line::from(Span::styled(
            header,
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::BOLD),
        )));

        for entry in &self.entries {
            let prefix = match entry.status {
                ToolStatus::Running => "•",
                ToolStatus::Success => "ok",
                ToolStatus::Failed => "!!",
            };
            let style = match entry.status {
                ToolStatus::Running => Style::default().fg(palette::DEEPSEEK_SKY),
                ToolStatus::Success => Style::default().fg(palette::STATUS_SUCCESS),
                ToolStatus::Failed => Style::default().fg(palette::STATUS_ERROR),
            };
            let line = format!("  {} {}", prefix, entry.label);
            lines.extend(wrap_plain_line(&line, style, width));
        }
        lines
    }

    /// Insert a new entry and return its index.
    #[must_use]
    pub fn insert_entry(&mut self, entry: ExploringEntry) -> usize {
        self.entries.push(entry);
        self.entries.len().saturating_sub(1)
    }
}

/// Single entry for exploring tool output.
#[derive(Debug, Clone)]
pub struct ExploringEntry {
    pub label: String,
    pub status: ToolStatus,
}

/// Cell for plan updates emitted by the plan tool.
#[derive(Debug, Clone)]
pub struct PlanUpdateCell {
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
    pub status: ToolStatus,
}

impl PlanUpdateCell {
    /// Render the plan update cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let header = match self.status {
            ToolStatus::Running => "Updating Plan",
            _ => "Updated Plan",
        };
        lines.push(Line::from(Span::styled(
            header,
            Style::default()
                .fg(palette::DEEPSEEK_BLUE)
                .add_modifier(Modifier::BOLD),
        )));

        if let Some(explanation) = self.explanation.as_ref() {
            lines.extend(render_message(" ", explanation, system_style(), width));
        }

        for step in &self.steps {
            let marker = match step.status.as_str() {
                "completed" => "[x]",
                "in_progress" => "[~]",
                _ => "[ ]",
            };
            let line = format!("  {} {}", marker, step.step);
            lines.extend(wrap_plain_line(
                &line,
                Style::default().fg(palette::DEEPSEEK_BLUE),
                width,
            ));
        }

        lines
    }
}

/// Single plan step rendered in the UI.
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub step: String,
    pub status: String,
}

/// Cell for patch summaries emitted by the patch tool.
#[derive(Debug, Clone)]
pub struct PatchSummaryCell {
    pub path: String,
    pub summary: String,
    pub status: ToolStatus,
    pub error: Option<String>,
}

impl PatchSummaryCell {
    /// Render the patch summary cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let header = match self.status {
            ToolStatus::Running => "Applying Patch",
            ToolStatus::Success => "Patch Applied",
            ToolStatus::Failed => "Patch Failed",
        };
        let color = match self.status {
            ToolStatus::Running => palette::STATUS_WARNING,
            ToolStatus::Success => palette::STATUS_SUCCESS,
            ToolStatus::Failed => palette::STATUS_ERROR,
        };
        lines.push(Line::from(Span::styled(
            header,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        lines.extend(wrap_plain_line(
            &format!("  {}", self.path),
            Style::default().fg(palette::TEXT_MUTED),
            width,
        ));
        lines.extend(render_tool_output(
            &self.summary,
            width,
            TOOL_COMMAND_LINE_LIMIT,
        ));
        if let Some(error) = self.error.as_ref() {
            lines.extend(render_tool_output(error, width, TOOL_COMMAND_LINE_LIMIT));
        }
        lines
    }
}

/// Cell for structured review output.
#[derive(Debug, Clone)]
pub struct ReviewCell {
    pub target: String,
    pub status: ToolStatus,
    pub output: Option<ReviewOutput>,
    pub error: Option<String>,
}

impl ReviewCell {
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let (label, color) = match self.status {
            ToolStatus::Running => ("Reviewing", palette::STATUS_WARNING),
            ToolStatus::Success => ("Review Complete", palette::STATUS_SUCCESS),
            ToolStatus::Failed => ("Review Failed", palette::STATUS_ERROR),
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));

        if !self.target.trim().is_empty() {
            lines.extend(wrap_plain_line(
                &format!("  Target: {}", self.target.trim()),
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        }

        if self.status == ToolStatus::Running {
            return lines;
        }

        if let Some(error) = self.error.as_ref() {
            lines.extend(render_tool_output(error, width, TOOL_COMMAND_LINE_LIMIT));
            return lines;
        }

        let Some(output) = self.output.as_ref() else {
            return lines;
        };

        if !output.summary.trim().is_empty() {
            lines.extend(wrap_plain_line(
                &format!("Summary: {}", output.summary.trim()),
                Style::default().fg(palette::TEXT_PRIMARY),
                width,
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Issues",
            Style::default()
                .fg(palette::DEEPSEEK_BLUE)
                .add_modifier(Modifier::BOLD),
        )));
        if output.issues.is_empty() {
            lines.extend(wrap_plain_line(
                "  (none)",
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        } else {
            for issue in &output.issues {
                let severity = issue.severity.trim().to_ascii_lowercase();
                let color = review_severity_color(&severity);
                let location = format_review_location(issue.path.as_ref(), issue.line);
                let label = if location.is_empty() {
                    format!("  - [{}] {}", severity, issue.title.trim())
                } else {
                    format!("  - [{}] {} ({})", severity, issue.title.trim(), location)
                };
                lines.extend(wrap_plain_line(&label, Style::default().fg(color), width));
                if !issue.description.trim().is_empty() {
                    lines.extend(wrap_plain_line(
                        &format!("    {}", issue.description.trim()),
                        Style::default().fg(palette::TEXT_MUTED),
                        width,
                    ));
                }
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Suggestions",
            Style::default()
                .fg(palette::DEEPSEEK_BLUE)
                .add_modifier(Modifier::BOLD),
        )));
        if output.suggestions.is_empty() {
            lines.extend(wrap_plain_line(
                "  (none)",
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        } else {
            for suggestion in &output.suggestions {
                let location = format_review_location(suggestion.path.as_ref(), suggestion.line);
                let label = if location.is_empty() {
                    format!("  - {}", suggestion.suggestion.trim())
                } else {
                    format!("  - {} ({})", suggestion.suggestion.trim(), location)
                };
                lines.extend(wrap_plain_line(
                    &label,
                    Style::default().fg(palette::TEXT_PRIMARY),
                    width,
                ));
            }
        }

        if !output.overall_assessment.trim().is_empty() {
            lines.push(Line::from(""));
            lines.extend(wrap_plain_line(
                &format!("Overall: {}", output.overall_assessment.trim()),
                Style::default().fg(palette::TEXT_PRIMARY),
                width,
            ));
        }

        lines
    }
}

/// Cell for showing a diff preview before applying changes.
#[derive(Debug, Clone)]
pub struct DiffPreviewCell {
    pub title: String,
    pub diff: String,
}

impl DiffPreviewCell {
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            self.title.clone(),
            Style::default()
                .fg(palette::DEEPSEEK_BLUE)
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(diff_render::render_diff(&self.diff, width));
        lines
    }
}

/// Cell representing an MCP tool execution.
#[derive(Debug, Clone)]
pub struct McpToolCell {
    pub tool: String,
    pub status: ToolStatus,
    pub content: Option<String>,
    pub is_image: bool,
}

impl McpToolCell {
    /// Render the MCP tool cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let header = match self.status {
            ToolStatus::Running => format!("Calling {}", self.tool),
            _ => format!("Called {}", self.tool),
        };
        let color = if self.status == ToolStatus::Failed {
            palette::STATUS_ERROR
        } else {
            palette::DEEPSEEK_SKY
        };
        lines.push(Line::from(Span::styled(
            header,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));

        if self.is_image {
            lines.push(Line::from(Span::styled(
                "  (image result)",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }

        if let Some(content) = self.content.as_ref() {
            lines.extend(render_tool_output(content, width, TOOL_COMMAND_LINE_LIMIT));
        }
        lines
    }
}

/// Cell for image view actions.
#[derive(Debug, Clone)]
pub struct ViewImageCell {
    pub path: PathBuf,
}

impl ViewImageCell {
    /// Render the image view cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let header = format!("Viewed Image {}", self.path.display());
        wrap_plain_line(&header, Style::default().fg(palette::DEEPSEEK_SKY), width)
    }
}

/// Cell for web search tool output.
#[derive(Debug, Clone)]
pub struct WebSearchCell {
    pub query: String,
    pub status: ToolStatus,
    pub summary: Option<String>,
}

impl WebSearchCell {
    /// Render the web search cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let header = match self.status {
            ToolStatus::Running => "Searching",
            _ => "Searched",
        };
        lines.push(Line::from(Span::styled(
            header,
            Style::default()
                .fg(palette::DEEPSEEK_BLUE)
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(wrap_plain_line(
            &format!("  {}", self.query),
            Style::default().fg(palette::TEXT_MUTED),
            width,
        ));
        if let Some(summary) = self.summary.as_ref() {
            lines.extend(render_compact_kv(
                "result:",
                summary,
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        }
        lines
    }
}

/// Generic cell for tool output when no specialized rendering exists.
#[derive(Debug, Clone)]
pub struct GenericToolCell {
    pub name: String,
    pub status: ToolStatus,
    pub input_summary: Option<String>,
    pub output: Option<String>,
}

impl GenericToolCell {
    /// Render the generic tool cell into lines.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let header = match self.status {
            ToolStatus::Running => format!("Calling {}", self.name),
            _ => format!("Called {}", self.name),
        };
        let color = if self.status == ToolStatus::Failed {
            palette::STATUS_ERROR
        } else {
            palette::DEEPSEEK_SKY
        };
        lines.push(Line::from(Span::styled(
            header,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        let show_args = matches!(self.status, ToolStatus::Running) || self.output.is_none();
        if show_args && let Some(summary) = self.input_summary.as_ref() {
            lines.extend(render_compact_kv(
                "args:",
                summary,
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        }
        if let Some(output) = self.output.as_ref() {
            let style = if self.status == ToolStatus::Failed {
                Style::default().fg(palette::STATUS_ERROR)
            } else {
                Style::default().fg(palette::TEXT_MUTED)
            };
            lines.extend(render_compact_kv("result:", output, style, width));
        }
        lines
    }
}

fn summarize_string_value(text: &str, max_len: usize, count_only: bool) -> String {
    let trimmed = text.trim();
    let len = trimmed.chars().count();
    if count_only || len > max_len {
        return format!("<{len} chars>");
    }
    truncate_text(trimmed, max_len)
}

fn summarize_inline_value(value: &Value, max_len: usize, count_only: bool) -> String {
    match value {
        Value::String(s) => summarize_string_value(s, max_len, count_only),
        Value::Array(items) => format!("<{} items>", items.len()),
        Value::Object(map) => format!("<{} keys>", map.len()),
        Value::Bool(b) => b.to_string(),
        Value::Number(num) => num.to_string(),
        Value::Null => "null".to_string(),
    }
}

#[must_use]
pub fn summarize_tool_args(input: &Value) -> Option<String> {
    let obj = input.as_object()?;
    if obj.is_empty() {
        return None;
    }

    let mut parts = Vec::new();

    if let Some(value) = obj.get("path") {
        parts.push(format!(
            "path: {}",
            summarize_inline_value(value, 80, false)
        ));
    }
    if let Some(value) = obj.get("command") {
        parts.push(format!(
            "command: {}",
            summarize_inline_value(value, 80, false)
        ));
    }
    if let Some(value) = obj.get("query") {
        parts.push(format!(
            "query: {}",
            summarize_inline_value(value, 80, false)
        ));
    }
    if let Some(value) = obj.get("prompt") {
        parts.push(format!(
            "prompt: {}",
            summarize_inline_value(value, 80, false)
        ));
    }
    if let Some(value) = obj.get("text") {
        parts.push(format!(
            "text: {}",
            summarize_inline_value(value, 80, false)
        ));
    }
    if let Some(value) = obj.get("pattern") {
        parts.push(format!(
            "pattern: {}",
            summarize_inline_value(value, 80, false)
        ));
    }
    if let Some(value) = obj.get("model") {
        parts.push(format!(
            "model: {}",
            summarize_inline_value(value, 40, false)
        ));
    }
    if let Some(value) = obj.get("file_id") {
        parts.push(format!(
            "file_id: {}",
            summarize_inline_value(value, 40, false)
        ));
    }
    if let Some(value) = obj.get("task_id") {
        parts.push(format!(
            "task_id: {}",
            summarize_inline_value(value, 40, false)
        ));
    }
    if let Some(value) = obj.get("voice_id") {
        parts.push(format!(
            "voice_id: {}",
            summarize_inline_value(value, 40, false)
        ));
    }
    if let Some(value) = obj.get("content") {
        parts.push(format!(
            "content: {}",
            summarize_inline_value(value, 0, true)
        ));
    }

    if parts.is_empty()
        && let Some((key, value)) = obj.iter().next()
    {
        return Some(format!(
            "{}: {}",
            key,
            summarize_inline_value(value, 80, false)
        ));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

#[must_use]
pub fn summarize_tool_output(output: &str) -> String {
    if let Ok(json) = serde_json::from_str::<Value>(output) {
        if let Some(obj) = json.as_object() {
            if let Some(error) = obj.get("error").or(obj.get("status_msg")) {
                return format!("Error: {}", summarize_inline_value(error, 120, false));
            }

            let mut parts = Vec::new();

            if let Some(status) = obj.get("status").and_then(|v| v.as_str()) {
                parts.push(format!("status: {status}"));
            }
            if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
                parts.push(truncate_text(message, TOOL_TEXT_LIMIT));
            }
            if let Some(task_id) = obj.get("task_id").and_then(|v| v.as_str()) {
                parts.push(format!("task_id: {task_id}"));
            }
            if let Some(file_id) = obj.get("file_id").and_then(|v| v.as_str()) {
                parts.push(format!("file_id: {file_id}"));
            }
            if let Some(url) = obj
                .get("file_url")
                .or_else(|| obj.get("url"))
                .and_then(|v| v.as_str())
            {
                parts.push(format!("url: {}", truncate_text(url, 120)));
            }
            if let Some(data) = obj.get("data") {
                parts.push(format!("data: {}", summarize_inline_value(data, 80, true)));
            }

            if !parts.is_empty() {
                return parts.join(" | ");
            }

            if let Some(content) = obj
                .get("content")
                .or(obj.get("result"))
                .or(obj.get("output"))
            {
                return summarize_inline_value(content, TOOL_TEXT_LIMIT, false);
            }
        }

        return summarize_inline_value(&json, TOOL_TEXT_LIMIT, true);
    }

    truncate_text(output, TOOL_TEXT_LIMIT)
}

// === MCP Output Summaries ===

/// Summary information extracted from an MCP tool output payload.
pub struct McpOutputSummary {
    pub content: Option<String>,
    pub is_image: bool,
    pub is_error: Option<bool>,
}

/// Summarize raw MCP output into UI-friendly content.
#[must_use]
pub fn summarize_mcp_output(output: &str) -> McpOutputSummary {
    if let Ok(json) = serde_json::from_str::<Value>(output) {
        let is_error = json
            .get("isError")
            .and_then(serde_json::Value::as_bool)
            .or_else(|| json.get("is_error").and_then(serde_json::Value::as_bool));

        if let Some(blocks) = json.get("content").and_then(|v| v.as_array()) {
            let mut lines = Vec::new();
            let mut is_image = false;

            for block in blocks {
                let block_type = block
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                match block_type {
                    "text" => {
                        let text = block.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        if !text.is_empty() {
                            lines.push(format!("- text: {}", truncate_text(text, 200)));
                        }
                    }
                    "image" | "image_url" => {
                        is_image = true;
                        let url = block
                            .get("url")
                            .or_else(|| block.get("image_url"))
                            .and_then(|v| v.as_str());
                        if let Some(url) = url {
                            lines.push(format!("- image: {}", truncate_text(url, 200)));
                        } else {
                            lines.push("- image".to_string());
                        }
                    }
                    "resource" | "resource_link" => {
                        let uri = block
                            .get("uri")
                            .or_else(|| block.get("url"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("<resource>");
                        lines.push(format!("- resource: {}", truncate_text(uri, 200)));
                    }
                    other => {
                        lines.push(format!("- {other} content"));
                    }
                }
            }

            return McpOutputSummary {
                content: if lines.is_empty() {
                    None
                } else {
                    Some(lines.join("\n"))
                },
                is_image,
                is_error,
            };
        }
    }

    McpOutputSummary {
        content: Some(summarize_tool_output(output)),
        is_image: output_is_image(output),
        is_error: None,
    }
}

#[must_use]
pub fn output_is_image(output: &str) -> bool {
    let lower = output.to_lowercase();

    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tiff", ".ppm",
    ]
    .iter()
    .any(|ext| lower.contains(ext))
}

#[cfg(test)]
#[must_use]
pub fn extract_reasoning_summary(text: &str) -> Option<String> {
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("summary") {
            let mut summary = String::new();
            if let Some((_, rest)) = trimmed.split_once(':')
                && !rest.trim().is_empty()
            {
                summary.push_str(rest.trim());
                summary.push('\n');
            }
            while let Some(next) = lines.peek() {
                let next_trimmed = next.trim();
                if next_trimmed.is_empty() {
                    break;
                }
                if next_trimmed.starts_with('#') || next_trimmed.starts_with("**") {
                    break;
                }
                summary.push_str(next_trimmed);
                summary.push('\n');
                lines.next();
            }
            let summary = summary.trim().to_string();
            return if summary.is_empty() {
                None
            } else {
                Some(summary)
            };
        }
    }
    let fallback = text.trim();
    if fallback.is_empty() {
        None
    } else {
        Some(fallback.to_string())
    }
}

fn render_thinking(content: &str, width: u16) -> Vec<Line<'static>> {
    let style = thinking_style();
    let prefix = "│ ";

    let content_width = usize::from(width.saturating_sub(2).max(1));
    let rendered = markdown_render::render_markdown(content, content_width as u16, style);

    let mut lines = Vec::new();

    for line in rendered {
        let mut spans = vec![Span::styled(prefix, style)];
        spans.extend(line.spans);
        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(prefix, style)]));
    }
    lines
}

fn render_message(prefix: &str, content: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    let prefix_width = UnicodeWidthStr::width(prefix);
    let prefix_width_u16 = u16::try_from(prefix_width.saturating_add(2)).unwrap_or(u16::MAX);
    let content_width = usize::from(width.saturating_sub(prefix_width_u16).max(1));
    let mut lines = Vec::new();
    let rendered = markdown_render::render_markdown(content, content_width as u16, style);
    for (idx, line) in rendered.into_iter().enumerate() {
        if idx == 0 {
            let mut spans = vec![
                Span::styled(prefix.to_string(), style.add_modifier(Modifier::BOLD)),
                Span::raw(" "),
            ];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        } else {
            let indent = " ".repeat(prefix_width + 1);
            let mut spans = vec![Span::raw(indent)];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines
}

fn render_command(command: &str, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (count, chunk) in wrap_text(command, width.saturating_sub(4).max(1) as usize)
        .into_iter()
        .enumerate()
    {
        if count >= TOOL_COMMAND_LINE_LIMIT {
            lines.push(Line::from(Span::styled(
                "  ...",
                Style::default().fg(palette::TEXT_MUTED),
            )));
            break;
        }
        lines.push(Line::from(vec![
            Span::styled("  $ ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(chunk, Style::default().fg(palette::TEXT_PRIMARY)),
        ]));
    }
    lines
}

fn render_compact_kv(label: &str, value: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    let line = format!("  {label} {value}");
    wrap_plain_line(&line, style, width)
}

fn render_tool_output(output: &str, width: u16, line_limit: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if output.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no output)",
            Style::default().fg(palette::TEXT_MUTED).italic(),
        )));
        return lines;
    }
    let mut all_lines = Vec::new();
    for line in output.lines() {
        all_lines.extend(wrap_text(line, width.saturating_sub(4).max(1) as usize));
    }
    let total = all_lines.len();
    for (idx, line) in all_lines.into_iter().enumerate() {
        if idx >= line_limit {
            let omitted = total.saturating_sub(line_limit);
            if omitted > 0 {
                lines.push(Line::from(Span::styled(
                    format!("  ... +{omitted} lines"),
                    Style::default().fg(palette::TEXT_MUTED),
                )));
            }
            break;
        }
        lines.push(Line::from(vec![
            Span::styled("  | ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(line, Style::default().fg(palette::TEXT_MUTED)),
        ]));
    }
    lines
}

fn review_severity_color(severity: &str) -> Color {
    match severity {
        "error" => palette::STATUS_ERROR,
        "warning" => palette::STATUS_WARNING,
        _ => palette::STATUS_INFO,
    }
}

fn format_review_location(path: Option<&String>, line: Option<u32>) -> String {
    let path = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    match (path, line) {
        (Some(path), Some(line)) => format!("{path}:{line}"),
        (Some(path), None) => path,
        (None, Some(line)) => format!("line {line}"),
        (None, None) => String::new(),
    }
}

fn render_exec_output(output: &str, width: u16, line_limit: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if output.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no output)",
            Style::default().fg(palette::TEXT_MUTED).italic(),
        )));
        return lines;
    }

    let mut all_lines = Vec::new();
    for line in output.lines() {
        all_lines.extend(wrap_text(line, width.saturating_sub(4).max(1) as usize));
    }

    let total = all_lines.len();
    let head_end = total.min(line_limit);
    for line in &all_lines[..head_end] {
        lines.push(Line::from(vec![
            Span::styled("  | ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(line.to_string(), Style::default().fg(palette::TEXT_MUTED)),
        ]));
    }

    if total > 2 * line_limit {
        let omitted = total.saturating_sub(2 * line_limit);
        lines.push(Line::from(Span::styled(
            format!("  ... +{omitted} lines"),
            Style::default().fg(palette::TEXT_MUTED),
        )));
        let tail_start = total.saturating_sub(line_limit);
        for line in &all_lines[tail_start..] {
            lines.push(Line::from(vec![
                Span::styled("  | ", Style::default().fg(palette::TEXT_MUTED)),
                Span::styled(line.to_string(), Style::default().fg(palette::TEXT_MUTED)),
            ]));
        }
    } else if total > head_end {
        for line in &all_lines[head_end..] {
            lines.push(Line::from(vec![
                Span::styled("  | ", Style::default().fg(palette::TEXT_MUTED)),
                Span::styled(line.to_string(), Style::default().fg(palette::TEXT_MUTED)),
            ]));
        }
    }

    lines
}

fn wrap_plain_line(line: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for part in wrap_text(line, width.max(1) as usize) {
        lines.push(Line::from(Span::styled(part, style)));
    }
    lines
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current_width == 0 {
            current.push_str(word);
            current_width = word_width;
            continue;
        }

        if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(current);
            current = word.to_string();
            current_width = word_width;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn status_symbol(started_at: Option<Instant>, status: ToolStatus) -> String {
    match status {
        ToolStatus::Running => {
            let elapsed_ms = started_at.map_or(0, |t| t.elapsed().as_millis());
            if (elapsed_ms / 400).is_multiple_of(2) {
                "*".to_string()
            } else {
                ".".to_string()
            }
        }
        ToolStatus::Success => "o".to_string(),
        ToolStatus::Failed => "x".to_string(),
    }
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max_len.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn user_style() -> Style {
    Style::default()
        .fg(palette::TEXT_PRIMARY)
        .add_modifier(Modifier::BOLD)
}

fn assistant_style() -> Style {
    Style::default().fg(palette::DEEPSEEK_SKY)
}

fn player_style() -> Style {
    Style::default().fg(palette::MODE_DUO)
}

fn coach_style() -> Style {
    Style::default()
        .fg(palette::DEEPSEEK_BLUE)
        .add_modifier(Modifier::BOLD)
}

fn system_style() -> Style {
    Style::default().fg(palette::TEXT_MUTED).italic()
}

fn thinking_style() -> Style {
    Style::default()
        .fg(palette::TEXT_MUTED)
        .add_modifier(Modifier::ITALIC | Modifier::DIM)
}

#[cfg(test)]
mod tests {
    use super::extract_reasoning_summary;

    #[test]
    fn extract_reasoning_summary_prefers_summary_block() {
        let text = "Thinking...\nSummary: First line\nSecond line\n\nTail";
        let summary = extract_reasoning_summary(text).expect("summary should exist");
        assert_eq!(summary, "First line\nSecond line");
    }

    #[test]
    fn extract_reasoning_summary_falls_back_to_full_text() {
        let text = "Line one\nLine two";
        let summary = extract_reasoning_summary(text).expect("summary should exist");
        assert_eq!(summary, "Line one\nLine two");
    }
}
