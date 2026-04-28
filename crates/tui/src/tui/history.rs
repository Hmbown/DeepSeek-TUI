//! TUI rendering helpers for chat history and tool output.

use std::path::PathBuf;
use std::time::Instant;

use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::deepseek_theme::active_theme;
use crate::models::{ContentBlock, Message};
use crate::palette;
use crate::tools::review::ReviewOutput;
use crate::tui::app::TranscriptSpacing;
use crate::tui::diff_render;
use crate::tui::markdown_render;

// === Constants ===

const TOOL_COMMAND_LINE_LIMIT: usize = 3;
const TOOL_OUTPUT_LINE_LIMIT: usize = 6;
const TOOL_TEXT_LIMIT: usize = 180;
const TOOL_RUNNING_SYMBOLS: [&str; 4] = ["·", "◦", "•", "◦"];
// Spinner cadence per glyph. The status-animation tick (UI_STATUS_ANIMATION_MS
// = 360 ms) fires every two glyphs, so a full 4-glyph "heartbeat" lands in
// ~2.88 s — fast enough that the user sees motion within a few hundred ms of
// starting a tool, slow enough to read as a pulse rather than a strobe.
const TOOL_STATUS_SYMBOL_MS: u64 = 720;
/// Visual marker for the user role at the start of their message line. Solid
/// vertical bar — no animation; user input is a finished thing.
const USER_GLYPH: &str = "\u{258E}"; // ▎
/// Visual marker for the assistant role. Solid bullet that pulses at 2s
/// cycle while the response is streaming, holds full brightness when idle.
const ASSISTANT_GLYPH: &str = "\u{25CF}"; // ●
/// Reasoning header opener. Replaces the spinner glyph on thinking cells —
/// reasoning is a slow exhale, not a tool spin.
const REASONING_OPENER: &str = "\u{2026}"; // …
/// Reasoning body left rail. Dashed (`╎`) instead of the solid `▏` block to
/// visually separate reasoning from message body and tool output.
const REASONING_RAIL: &str = "\u{254E} "; // ╎ + space
/// Trailing-line cursor on streaming reasoning. Anchored to the live colour
/// so the user sees where new tokens land.
const REASONING_CURSOR: &str = "\u{258E}"; // ▎
const TOOL_CARD_SUMMARY_LINES: usize = 4;
const THINKING_SUMMARY_LINE_LIMIT: usize = 4;
const TOOL_DONE_SYMBOL: &str = "•";
const TOOL_FAILED_SYMBOL: &str = "•";

/// Render mode controlling whether tool/thinking cells render their compact
/// "live" form (with caps and collapsed reasoning) or their full transcript
/// form (uncapped, suitable for the pager / clipboard / message export).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Live in-stream view: thinking is collapsed to a summary, tool output is
    /// truncated with a "press v for details" affordance.
    Live,
    /// Full transcript view: every line of reasoning and tool output is
    /// emitted, no caps, no affordance.
    Transcript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThinkingVisualState {
    Live,
    Done,
    Idle,
}

// === History Cells ===

/// Renderable history cell for user/assistant/system entries.
#[derive(Debug, Clone)]
pub enum HistoryCell {
    User {
        content: String,
    },
    Assistant {
        content: String,
        streaming: bool,
    },
    System {
        content: String,
    },
    Thinking {
        content: String,
        streaming: bool,
        duration_secs: Option<f32>,
    },
    Tool(ToolCell),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptRenderOptions {
    pub show_thinking: bool,
    pub show_tool_details: bool,
    pub calm_mode: bool,
    pub low_motion: bool,
    pub spacing: TranscriptSpacing,
}

impl Default for TranscriptRenderOptions {
    fn default() -> Self {
        Self {
            show_thinking: true,
            show_tool_details: true,
            calm_mode: false,
            low_motion: false,
            spacing: TranscriptSpacing::Comfortable,
        }
    }
}

impl HistoryCell {
    /// Render the cell into a set of terminal lines.
    ///
    /// This is the live-display path used by widgets that don't already pass
    /// `TranscriptRenderOptions`. Tool output is capped, but thinking is shown
    /// in full because callers using bare `lines()` historically expected the
    /// uncollapsed body. For the in-stream transcript view prefer
    /// `lines_with_options`; for the pager / clipboard prefer
    /// `transcript_lines`.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        match self {
            HistoryCell::User { content } => render_message(
                USER_GLYPH,
                user_label_style(),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::Assistant { content, streaming } => render_message(
                ASSISTANT_GLYPH,
                assistant_label_style_for(*streaming, /*low_motion*/ false),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::System { content } => render_message(
                "Note",
                system_label_style(),
                system_body_style(),
                content,
                width,
            ),
            HistoryCell::Thinking {
                content,
                streaming,
                duration_secs,
            } => render_thinking(content, width, *streaming, *duration_secs, false, false),
            HistoryCell::Tool(cell) => cell.lines_with_motion(width, false),
        }
    }

    pub fn lines_with_options(
        &self,
        width: u16,
        options: TranscriptRenderOptions,
    ) -> Vec<Line<'static>> {
        match self {
            HistoryCell::Thinking { .. } if !options.show_thinking => Vec::new(),
            HistoryCell::Thinking {
                content,
                streaming,
                duration_secs,
            } => render_thinking(
                content,
                width,
                *streaming,
                *duration_secs,
                !*streaming,
                options.low_motion,
            ),
            HistoryCell::Tool(cell) if !options.show_tool_details => {
                let mut lines = cell.lines_with_motion(width, options.low_motion);
                if lines.len() > 2 {
                    lines.truncate(2);
                    lines.push(details_affordance_line(
                        "details hidden",
                        Style::default().fg(palette::TEXT_MUTED).italic(),
                    ));
                }
                lines
            }
            HistoryCell::Tool(cell) if options.calm_mode => {
                let mut lines = cell.lines_with_motion(width, options.low_motion);
                if lines.len() > TOOL_CARD_SUMMARY_LINES {
                    lines.truncate(TOOL_CARD_SUMMARY_LINES);
                    lines.push(details_affordance_line(
                        "press v for details",
                        Style::default().fg(palette::TEXT_MUTED).italic(),
                    ));
                }
                lines
            }
            HistoryCell::Tool(cell) => cell.lines_with_motion(width, options.low_motion),
            HistoryCell::User { content } => render_message(
                USER_GLYPH,
                user_label_style(),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::Assistant { content, streaming } => render_message(
                ASSISTANT_GLYPH,
                assistant_label_style_for(*streaming, options.low_motion),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::System { .. } => self.lines(width),
        }
    }

    /// Render the cell in transcript mode: full content, no caps, no
    /// "press v for details" affordances.
    ///
    /// Use this for the pager (`v` / `Ctrl+O`), clipboard exports, and any
    /// surface that wants the complete body rather than the live summary.
    /// For most variants (User / Assistant / System) this matches `lines()`;
    /// `Thinking` and `Tool` are where the live and transcript surfaces
    /// diverge.
    pub fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        match self {
            HistoryCell::User { content } => render_message(
                USER_GLYPH,
                user_label_style(),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::Assistant { content, streaming } => render_message(
                ASSISTANT_GLYPH,
                // Pager / clipboard surface — pin the glyph at full
                // brightness so a screenshot reads the same as a live frame.
                assistant_label_style_for(*streaming, /*low_motion*/ true),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::System { .. } => self.lines(width),
            HistoryCell::Thinking {
                content,
                streaming,
                duration_secs,
            } => render_thinking(
                content,
                width,
                *streaming,
                *duration_secs,
                /*collapsed*/ false,
                /*low_motion*/ false,
            ),
            HistoryCell::Tool(cell) => cell.transcript_lines(width),
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

    #[must_use]
    pub fn is_conversational(&self) -> bool {
        matches!(
            self,
            HistoryCell::User { .. } | HistoryCell::Assistant { .. } | HistoryCell::Thinking { .. }
        )
    }
}

/// Convert a message into history cells for rendering.
#[must_use]
pub fn history_cells_from_message(msg: &Message) -> Vec<HistoryCell> {
    let mut cells = Vec::new();

    for block in &msg.content {
        match block {
            ContentBlock::Text { text, .. } => match msg.role.as_str() {
                "user" => {
                    if let Some(HistoryCell::User { content }) = cells.last_mut() {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(text);
                    } else {
                        cells.push(HistoryCell::User {
                            content: text.clone(),
                        });
                    }
                }
                "assistant" => {
                    if let Some(HistoryCell::Assistant { content, .. }) = cells.last_mut() {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(text);
                    } else {
                        cells.push(HistoryCell::Assistant {
                            content: text.clone(),
                            streaming: false,
                        });
                    }
                }
                "system" => {
                    if let Some(HistoryCell::System { content }) = cells.last_mut() {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(text);
                    } else {
                        cells.push(HistoryCell::System {
                            content: text.clone(),
                        });
                    }
                }
                _ => {}
            },
            ContentBlock::Thinking { thinking } => {
                if let Some(HistoryCell::Thinking { content, .. }) = cells.last_mut() {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(thinking);
                } else {
                    cells.push(HistoryCell::Thinking {
                        content: thinking.clone(),
                        streaming: false,
                        duration_secs: None,
                    });
                }
            }
            _ => {}
        }
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
        self.lines_with_motion(width, false)
    }

    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        self.render(width, low_motion, RenderMode::Live)
    }

    /// Full-content rendering for the pager / clipboard. Tool output that
    /// would be capped + suffixed with "press v for details" in the live view
    /// is emitted in full here.
    pub fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.render(width, /*low_motion*/ false, RenderMode::Transcript)
    }

    fn render(&self, width: u16, low_motion: bool, mode: RenderMode) -> Vec<Line<'static>> {
        match self {
            ToolCell::Exec(cell) => cell.render(width, low_motion, mode),
            ToolCell::Exploring(cell) => cell.lines_with_motion(width, low_motion),
            ToolCell::PlanUpdate(cell) => cell.lines_with_motion(width, low_motion),
            ToolCell::PatchSummary(cell) => cell.render(width, low_motion, mode),
            ToolCell::Review(cell) => cell.render(width, low_motion, mode),
            ToolCell::DiffPreview(cell) => cell.lines_with_motion(width, low_motion),
            ToolCell::Mcp(cell) => cell.render(width, low_motion, mode),
            ToolCell::ViewImage(cell) => cell.lines_with_motion(width, low_motion),
            ToolCell::WebSearch(cell) => cell.lines_with_motion(width, low_motion),
            ToolCell::Generic(cell) => cell.lines_with_mode(width, low_motion, mode),
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
    /// Render the execution cell into lines (live view, capped output).
    #[cfg(test)]
    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        self.render(width, low_motion, RenderMode::Live)
    }

    pub(super) fn render(
        &self,
        width: u16,
        low_motion: bool,
        mode: RenderMode,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Shell",
            tool_status_label(self.status),
            self.status,
            self.started_at,
            low_motion,
        ));

        if self.status == ToolStatus::Success && self.source == ExecSource::User {
            lines.extend(render_compact_kv(
                "source",
                "started by you",
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        }

        if let Some(interaction) = self.interaction.as_ref() {
            lines.extend(wrap_plain_line(
                &format!("  {interaction}"),
                Style::default().fg(palette::TEXT_MUTED),
                width,
            ));
        } else {
            lines.extend(render_command_mode(&self.command, width, mode));
        }

        if self.interaction.is_none() {
            if let Some(output) = self.output.as_ref() {
                lines.extend(render_exec_output_mode(
                    output,
                    width,
                    TOOL_OUTPUT_LINE_LIMIT,
                    mode,
                ));
            } else if self.status != ToolStatus::Running {
                lines.push(Line::from(Span::styled(
                    "  (no output)",
                    Style::default().fg(palette::TEXT_MUTED).italic(),
                )));
            }
        }

        if let Some(duration_ms) = self.duration_ms {
            let seconds = f64::from(u32::try_from(duration_ms).unwrap_or(u32::MAX)) / 1000.0;
            lines.extend(render_compact_kv(
                "time",
                &format!("{seconds:.2}s"),
                Style::default().fg(palette::TEXT_DIM),
                width,
            ));
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
    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let all_done = self
            .entries
            .iter()
            .all(|entry| entry.status != ToolStatus::Running);
        let status = if all_done {
            ToolStatus::Success
        } else {
            ToolStatus::Running
        };
        lines.push(render_tool_header(
            "Workspace",
            if all_done { "done" } else { "running" },
            status,
            None,
            low_motion,
        ));

        for entry in &self.entries {
            let prefix = match entry.status {
                ToolStatus::Running => "live",
                ToolStatus::Success => "done",
                ToolStatus::Failed => "issue",
            };
            lines.extend(render_compact_kv(
                prefix,
                &entry.label,
                tool_value_style(),
                width,
            ));
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
    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Plan",
            tool_status_label(self.status),
            self.status,
            None,
            low_motion,
        ));

        if let Some(explanation) = self.explanation.as_ref() {
            lines.extend(render_message(
                "",
                system_label_style(),
                system_body_style(),
                explanation,
                width,
            ));
        }

        for step in &self.steps {
            let marker = match step.status.as_str() {
                "completed" => "done",
                "in_progress" => "live",
                _ => "next",
            };
            lines.extend(render_compact_kv(
                marker,
                &step.step,
                tool_value_style(),
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
    pub(super) fn render(
        &self,
        width: u16,
        low_motion: bool,
        mode: RenderMode,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Patch",
            tool_status_label(self.status),
            self.status,
            None,
            low_motion,
        ));
        lines.extend(render_compact_kv(
            "file",
            &self.path,
            tool_value_style(),
            width,
        ));
        lines.extend(render_tool_output_mode(
            &self.summary,
            width,
            TOOL_COMMAND_LINE_LIMIT,
            mode,
        ));
        if let Some(error) = self.error.as_ref() {
            lines.extend(render_tool_output_mode(
                error,
                width,
                TOOL_COMMAND_LINE_LIMIT,
                mode,
            ));
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
    pub(super) fn render(
        &self,
        width: u16,
        low_motion: bool,
        mode: RenderMode,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Review",
            tool_status_label(self.status),
            self.status,
            None,
            low_motion,
        ));

        if !self.target.trim().is_empty() {
            lines.extend(render_compact_kv(
                "target",
                self.target.trim(),
                tool_value_style(),
                width,
            ));
        }

        if self.status == ToolStatus::Running {
            return lines;
        }

        if let Some(error) = self.error.as_ref() {
            lines.extend(render_tool_output_mode(
                error,
                width,
                TOOL_COMMAND_LINE_LIMIT,
                mode,
            ));
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
    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Diff",
            "done",
            ToolStatus::Success,
            None,
            low_motion,
        ));
        lines.extend(render_compact_kv(
            "title",
            &self.title,
            tool_value_style(),
            width,
        ));
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
    pub(super) fn render(
        &self,
        width: u16,
        low_motion: bool,
        mode: RenderMode,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Tool",
            tool_status_label(self.status),
            self.status,
            None,
            low_motion,
        ));
        lines.extend(render_compact_kv(
            "name",
            &self.tool,
            tool_value_style(),
            width,
        ));

        if self.is_image {
            lines.extend(render_compact_kv(
                "result",
                "image",
                tool_value_style(),
                width,
            ));
        }

        if let Some(content) = self.content.as_ref() {
            lines.extend(render_tool_output_mode(
                content,
                width,
                TOOL_COMMAND_LINE_LIMIT,
                mode,
            ));
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
    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        let mut lines = vec![render_tool_header(
            "Image",
            "done",
            ToolStatus::Success,
            None,
            low_motion,
        )];
        lines.extend(render_compact_kv(
            "path",
            &self.path.display().to_string(),
            tool_value_style(),
            width,
        ));
        lines
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
    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Search",
            tool_status_label(self.status),
            self.status,
            None,
            low_motion,
        ));
        lines.extend(render_compact_kv(
            "query",
            &self.query,
            tool_value_style(),
            width,
        ));
        if let Some(summary) = self.summary.as_ref() {
            lines.extend(render_compact_kv(
                "result",
                summary,
                tool_value_style(),
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
    /// Optional list of per-child prompts. When populated (by any future
    /// fan-out tool), each prompt is shown on its own indented row instead
    /// of the inline `args:` summary. `None` for ordinary tools.
    pub prompts: Option<Vec<String>>,
}

impl GenericToolCell {
    /// Render the generic tool cell into lines.
    ///
    /// `mode` controls multi-line output handling: `Live` caps at
    /// `TOOL_OUTPUT_LINE_LIMIT` rows with a "+N more" affordance;
    /// `Transcript` emits the full output.
    pub fn lines_with_mode(
        &self,
        width: u16,
        low_motion: bool,
        mode: RenderMode,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_tool_header(
            "Tool",
            tool_status_label(self.status),
            self.status,
            None,
            low_motion,
        ));
        lines.extend(render_compact_kv(
            "name",
            &self.name,
            tool_value_style(),
            width,
        ));

        // Prefer per-prompt rows over the generic args summary when the tool
        // exposes a list of child prompts. One row per child with a `[i]`
        // index makes the fan-out legible without expanding JSON.
        let show_prompts = matches!(self.status, ToolStatus::Running) || self.output.is_none();
        if show_prompts
            && let Some(prompts) = self.prompts.as_ref()
            && !prompts.is_empty()
        {
            for (idx, prompt) in prompts.iter().enumerate() {
                let label = if idx == 0 { "prompts" } else { "" };
                let value = format!("[{idx}] {}", truncate_text(prompt.trim(), 200));
                lines.extend(render_card_detail_line(
                    if label.is_empty() { None } else { Some(label) },
                    &value,
                    tool_value_style(),
                    width,
                ));
            }
        } else {
            let show_args = matches!(self.status, ToolStatus::Running) || self.output.is_none();
            if show_args && let Some(summary) = self.input_summary.as_ref() {
                lines.extend(render_compact_kv(
                    "args",
                    summary,
                    tool_value_style(),
                    width,
                ));
            }
        }

        if let Some(output) = self.output.as_ref() {
            // Multi-line outputs (diff stats, file lists, todo snapshots) used
            // to be crushed into one line by `render_compact_kv` because its
            // wrapper joined the entire string before wrapping. Route through
            // `render_tool_output_mode` so each `\n` becomes a real row, with
            // a `+N more lines` affordance in live mode (#80).
            lines.extend(render_tool_output_mode(
                output,
                width,
                TOOL_OUTPUT_LINE_LIMIT,
                mode,
            ));
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

fn render_thinking(
    content: &str,
    width: u16,
    streaming: bool,
    duration_secs: Option<f32>,
    collapsed: bool,
    low_motion: bool,
) -> Vec<Line<'static>> {
    let state = thinking_visual_state(streaming, duration_secs);
    let style = thinking_style();
    // 12% reasoning surface tint over the app ink — the only deliberately
    // warm element in the transcript. Dropped on Ansi-16 terminals where the
    // tint would distort the named palette.
    let depth = palette::ColorDepth::detect();
    let body_bg = palette::reasoning_surface_tint(depth);
    let body_style = match body_bg {
        Some(bg) => style.italic().bg(bg),
        None => style.italic(),
    };
    let mut lines = Vec::new();

    // Header: `…` opener (replaces the spinner; reasoning isn't a tool, it's
    // a slow exhale) followed by the `thinking` label and live status.
    let mut header_spans = vec![
        Span::styled(
            format!("{REASONING_OPENER} "),
            Style::default().fg(thinking_state_accent(state)),
        ),
        Span::styled("thinking", thinking_title_style()),
    ];
    header_spans.push(Span::styled(" ", Style::default()));
    header_spans.push(Span::styled(
        thinking_status_label(state),
        thinking_status_style(state),
    ));
    if let Some(dur) = duration_secs {
        header_spans.push(Span::styled(" · ", Style::default().fg(palette::TEXT_DIM)));
        header_spans.push(Span::styled(format!("{dur:.1}s"), thinking_meta_style()));
    }
    lines.push(Line::from(header_spans));

    let content_width = width.saturating_sub(3).max(1);
    let body_text = if collapsed {
        extract_reasoning_summary(content).unwrap_or_else(|| content.trim().to_string())
    } else {
        content.to_string()
    };
    let mut rendered = markdown_render::render_markdown(&body_text, content_width, body_style);
    let mut truncated = false;
    if collapsed && rendered.len() > THINKING_SUMMARY_LINE_LIMIT {
        rendered.truncate(THINKING_SUMMARY_LINE_LIMIT);
        truncated = true;
    }

    let rail_style = Style::default().fg(thinking_state_accent(state));
    let cursor_style = Style::default().fg(palette::ACCENT_REASONING_LIVE);

    if rendered.is_empty() && streaming {
        let mut spans = vec![Span::styled(REASONING_RAIL.to_string(), rail_style)];
        spans.push(Span::styled(
            "reasoning in progress...",
            body_style.italic(),
        ));
        if !low_motion {
            spans.push(Span::styled(
                format!(" {REASONING_CURSOR}"),
                cursor_style,
            ));
        }
        lines.push(Line::from(spans));
    }

    let last_idx = rendered.len().saturating_sub(1);
    for (idx, line) in rendered.into_iter().enumerate() {
        let mut spans = vec![Span::styled(REASONING_RAIL.to_string(), rail_style)];
        spans.extend(line.spans);
        // Trailing cursor on the very last body line while streaming —
        // signals "still generating" without churning every line.
        if streaming && !low_motion && idx == last_idx {
            spans.push(Span::styled(
                format!(" {REASONING_CURSOR}"),
                cursor_style,
            ));
        }
        lines.push(Line::from(spans));
    }

    if collapsed && (!streaming && (truncated || body_text.trim() != content.trim())) {
        lines.push(Line::from(vec![
            Span::styled(REASONING_RAIL.to_string(), rail_style),
            Span::styled(
                "thinking collapsed; press Ctrl+O for full text",
                Style::default().fg(palette::TEXT_MUTED).italic(),
            ),
        ]));
    }

    lines
}

fn render_message(
    prefix: &str,
    label_style: Style,
    body_style: Style,
    content: &str,
    width: u16,
) -> Vec<Line<'static>> {
    let prefix_width = UnicodeWidthStr::width(prefix);
    let prefix_width_u16 = u16::try_from(prefix_width.saturating_add(2)).unwrap_or(u16::MAX);
    let content_width = usize::from(width.saturating_sub(prefix_width_u16).max(1));
    let mut lines = Vec::new();
    let rendered = markdown_render::render_markdown(content, content_width as u16, body_style);
    for (idx, line) in rendered.into_iter().enumerate() {
        if idx == 0 {
            let mut spans = Vec::new();
            if !prefix.is_empty() {
                spans.push(Span::styled(
                    prefix.to_string(),
                    label_style.add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" "));
            }
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        } else {
            let indent = if prefix.is_empty() {
                String::new()
            } else {
                " ".repeat(prefix_width + 1)
            };
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

fn render_command_mode(command: &str, width: u16, mode: RenderMode) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let cap = match mode {
        RenderMode::Live => TOOL_COMMAND_LINE_LIMIT,
        RenderMode::Transcript => usize::MAX,
    };
    for (count, chunk) in wrap_text(command, width.saturating_sub(4).max(1) as usize)
        .into_iter()
        .enumerate()
    {
        if count >= cap {
            lines.push(details_affordance_line(
                "command clipped; press v for details",
                Style::default().fg(palette::TEXT_MUTED),
            ));
            break;
        }
        lines.extend(render_card_detail_line(
            if count == 0 { Some("command") } else { None },
            chunk.as_str(),
            tool_value_style(),
            width,
        ));
    }
    lines
}

fn render_compact_kv(label: &str, value: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    render_card_detail_line(Some(label.trim_end_matches(':')), value, style, width)
}

fn render_tool_output_mode(
    output: &str,
    width: u16,
    line_limit: usize,
    mode: RenderMode,
) -> Vec<Line<'static>> {
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
    let effective_limit = match mode {
        RenderMode::Live => line_limit,
        RenderMode::Transcript => usize::MAX,
    };
    for (idx, line) in all_lines.into_iter().enumerate() {
        if idx >= effective_limit {
            let omitted = total.saturating_sub(effective_limit);
            if omitted > 0 {
                lines.push(details_affordance_line(
                    &format!("+{omitted} more lines; press v for details"),
                    Style::default().fg(palette::TEXT_MUTED),
                ));
            }
            break;
        }
        lines.extend(render_card_detail_line(
            if idx == 0 { Some("result") } else { None },
            &line,
            tool_value_style(),
            width,
        ));
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

fn render_exec_output_mode(
    output: &str,
    width: u16,
    line_limit: usize,
    mode: RenderMode,
) -> Vec<Line<'static>> {
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

    if matches!(mode, RenderMode::Transcript) {
        // Full-content path: emit every wrapped line with no head/tail split,
        // no "+N more" affordance.
        for (idx, line) in all_lines.iter().enumerate() {
            lines.extend(render_card_detail_line(
                if idx == 0 { Some("output") } else { None },
                line,
                tool_value_style(),
                width,
            ));
        }
        return lines;
    }

    let head_end = total.min(line_limit);
    for (idx, line) in all_lines[..head_end].iter().enumerate() {
        lines.extend(render_card_detail_line(
            if idx == 0 { Some("output") } else { None },
            line,
            tool_value_style(),
            width,
        ));
    }

    if total > 2 * line_limit {
        let omitted = total.saturating_sub(2 * line_limit);
        lines.push(details_affordance_line(
            &format!("+{omitted} more lines; press v for details"),
            Style::default().fg(palette::TEXT_MUTED),
        ));
        let tail_start = total.saturating_sub(line_limit);
        for line in &all_lines[tail_start..] {
            lines.extend(render_card_detail_line(
                None,
                line,
                tool_value_style(),
                width,
            ));
        }
    } else if total > head_end {
        for line in &all_lines[head_end..] {
            lines.extend(render_card_detail_line(
                None,
                line,
                tool_value_style(),
                width,
            ));
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
    let mut current_width = 0usize;

    for ch in text.chars() {
        let ch_width = if ch == '\t' {
            4
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
        };

        if current_width + ch_width > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }

        current.push(ch);
        current_width = current_width.saturating_add(ch_width);
    }

    lines.push(current);

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn status_symbol(started_at: Option<Instant>, status: ToolStatus, low_motion: bool) -> String {
    match status {
        ToolStatus::Running => {
            if low_motion {
                return TOOL_RUNNING_SYMBOLS[0].to_string();
            }
            let elapsed_ms = started_at.map_or_else(
                || {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |duration| duration.as_millis())
                },
                |t| t.elapsed().as_millis(),
            );
            let cycle = u128::from(TOOL_STATUS_SYMBOL_MS);
            let idx = elapsed_ms
                .checked_div(cycle)
                .map_or(0, |d| d % (TOOL_RUNNING_SYMBOLS.len() as u128));
            TOOL_RUNNING_SYMBOLS[usize::try_from(idx).unwrap_or_default()].to_string()
        }
        ToolStatus::Success => TOOL_DONE_SYMBOL.to_string(),
        ToolStatus::Failed => TOOL_FAILED_SYMBOL.to_string(),
    }
}

fn details_affordance_line(text: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled("▏ ", Style::default().fg(palette::TEXT_DIM)),
        Span::styled(text.to_string(), style),
    ])
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

fn user_label_style() -> Style {
    Style::default().fg(palette::TEXT_MUTED)
}

/// Style for the assistant glyph (`●`). When the cell is streaming and
/// motion is allowed, the foreground pulses on a 2s cycle between 30% and
/// 100% brightness — the only deliberately animated element in a calm
/// transcript. When idle (or low_motion is on) it sits at the full DeepSeek
/// sky color so finished turns read as solid rather than dim.
fn assistant_label_style_for(streaming: bool, low_motion: bool) -> Style {
    let color = if streaming && !low_motion {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        palette::pulse_brightness(palette::DEEPSEEK_SKY, now_ms)
    } else {
        palette::DEEPSEEK_SKY
    };
    Style::default().fg(color)
}

fn system_label_style() -> Style {
    Style::default().fg(palette::TEXT_DIM)
}

fn message_body_style() -> Style {
    Style::default().fg(palette::TEXT_PRIMARY)
}

fn system_body_style() -> Style {
    Style::default().fg(palette::TEXT_MUTED).italic()
}

fn thinking_style() -> Style {
    Style::default().fg(palette::TEXT_TOOL_OUTPUT)
}

fn render_tool_header(
    title: &str,
    state: &str,
    status: ToolStatus,
    started_at: Option<Instant>,
    low_motion: bool,
) -> Line<'static> {
    // For long-running tools, append elapsed seconds so the user can see the
    // call isn't stuck. Threshold matches the eye's "did this hang?" reflex
    // — under 3s we stay quiet so quick reads/greps don't visually churn.
    let state_owned: String = if state == "running"
        && status == ToolStatus::Running
        && let Some(started) = started_at
    {
        running_status_label_with_elapsed(started.elapsed().as_secs())
    } else {
        state.to_string()
    };

    Line::from(vec![
        Span::styled(
            format!("{} ", status_symbol(started_at, status, low_motion)),
            Style::default().fg(tool_state_color(status)),
        ),
        Span::styled(title.to_string(), tool_title_style()),
        Span::styled(" ", Style::default()),
        Span::styled(state_owned, tool_status_style(status)),
    ])
}

/// Build the "running" label with an elapsed-seconds badge for long-running
/// tools. Below 3s the badge is suppressed to avoid visual churn for tools
/// that resolve in milliseconds; at 3s and beyond the badge appears and ticks
/// every second the tool stays in flight.
pub(crate) fn running_status_label_with_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 3 {
        "running".to_string()
    } else {
        format!("running ({elapsed_secs}s)")
    }
}

fn render_card_detail_line(
    label: Option<&str>,
    value: &str,
    value_style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    let label_text = label.map(|text| format!("{text}:"));
    let prefix_width = UnicodeWidthStr::width("▏ ")
        + label_text.as_deref().map_or(0, UnicodeWidthStr::width)
        + usize::from(label.is_some());
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);

    let mut lines = Vec::new();
    for (idx, part) in wrap_text(value, content_width).into_iter().enumerate() {
        let mut spans = vec![Span::styled("▏ ", Style::default().fg(palette::TEXT_DIM))];
        if idx == 0 {
            if let Some(label_text) = label_text.as_deref() {
                spans.push(Span::styled(
                    label_text.to_string(),
                    tool_detail_label_style(),
                ));
                spans.push(Span::raw(" "));
            }
        } else if let Some(label_text) = label_text.as_deref() {
            spans.push(Span::raw(
                " ".repeat(UnicodeWidthStr::width(label_text) + 1),
            ));
        }
        spans.push(Span::styled(part, value_style));
        lines.push(Line::from(spans));
    }
    lines
}

fn tool_title_style() -> Style {
    active_theme().tool_title_style()
}

fn tool_status_style(status: ToolStatus) -> Style {
    active_theme().tool_status_style(status)
}

fn tool_detail_label_style() -> Style {
    active_theme().tool_label_style()
}

fn tool_state_color(status: ToolStatus) -> Color {
    active_theme().tool_status_color(status)
}

fn tool_status_label(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Running => "running",
        ToolStatus::Success => "done",
        ToolStatus::Failed => "issue",
    }
}

fn tool_value_style() -> Style {
    active_theme().tool_value_style()
}

fn thinking_visual_state(streaming: bool, duration_secs: Option<f32>) -> ThinkingVisualState {
    if streaming {
        ThinkingVisualState::Live
    } else if duration_secs.is_some() {
        ThinkingVisualState::Done
    } else {
        ThinkingVisualState::Idle
    }
}

fn thinking_status_label(state: ThinkingVisualState) -> &'static str {
    match state {
        ThinkingVisualState::Live => "live",
        ThinkingVisualState::Done => "done",
        ThinkingVisualState::Idle => "idle",
    }
}

fn thinking_title_style() -> Style {
    Style::default()
        .fg(palette::TEXT_SOFT)
        .add_modifier(Modifier::BOLD)
}

fn thinking_status_style(state: ThinkingVisualState) -> Style {
    Style::default().fg(match state {
        ThinkingVisualState::Live => palette::ACCENT_REASONING_LIVE,
        ThinkingVisualState::Done => palette::TEXT_DIM,
        ThinkingVisualState::Idle => palette::TEXT_DIM,
    })
}

fn thinking_meta_style() -> Style {
    Style::default().fg(palette::TEXT_DIM)
}

fn thinking_state_accent(state: ThinkingVisualState) -> Color {
    match state {
        ThinkingVisualState::Live => palette::ACCENT_REASONING_LIVE,
        ThinkingVisualState::Done => palette::TEXT_DIM,
        ThinkingVisualState::Idle => palette::TEXT_DIM,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ASSISTANT_GLYPH, ExecCell, ExecSource, GenericToolCell, HistoryCell, PlanStep,
        PlanUpdateCell, REASONING_CURSOR, REASONING_OPENER, REASONING_RAIL, TOOL_RUNNING_SYMBOLS,
        TOOL_STATUS_SYMBOL_MS, ToolCell, ToolStatus, TranscriptRenderOptions, USER_GLYPH,
        assistant_label_style_for, extract_reasoning_summary, render_thinking,
        running_status_label_with_elapsed,
    };
    use crate::palette;
    use crate::deepseek_theme::Theme;
    use ratatui::style::Modifier;
    use std::time::{Duration, Instant};

    // ---- elapsed-seconds badge for long-running tools ----
    //
    // Below 3s the label stays "running" — quick reads/greps shouldn't
    // visually churn. From 3s onward the badge appears and ticks each
    // second so the user can tell the call hasn't hung.
    #[test]
    fn running_status_label_omits_elapsed_below_threshold() {
        assert_eq!(running_status_label_with_elapsed(0), "running");
        assert_eq!(running_status_label_with_elapsed(1), "running");
        assert_eq!(running_status_label_with_elapsed(2), "running");
    }

    #[test]
    fn running_status_label_appends_elapsed_at_three_seconds() {
        assert_eq!(running_status_label_with_elapsed(3), "running (3s)");
        assert_eq!(running_status_label_with_elapsed(7), "running (7s)");
        assert_eq!(running_status_label_with_elapsed(120), "running (120s)");
    }

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

    #[test]
    fn render_thinking_collapsed_shows_details_affordance() {
        let lines = render_thinking(
            "Summary: First line\nSecond line\nThird line\nFourth line\nFifth line",
            80,
            false,
            Some(2.0),
            true,
            false,
        );
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(text.contains("thinking collapsed; press Ctrl+O for full text"));
        assert!(text.contains("thinking"));
    }

    #[test]
    fn tool_lines_with_options_respects_low_motion_in_default_path() {
        // Use a 2× cycle offset so the animated frame lands on index 2,
        // which is maximally far from index 0. This avoids flaky failures on
        // platforms with coarse timer resolution (Windows ≈ 15.6 ms) and
        // gives 3600 ms of headroom before the index could wrap back to 0
        // (indices 2 → 3 → 0 requires two more full cycles).
        let started_at = Some(Instant::now() - Duration::from_millis(TOOL_STATUS_SYMBOL_MS * 2));
        let cell = HistoryCell::Tool(ToolCell::Exec(ExecCell {
            command: "echo hi".to_string(),
            status: ToolStatus::Running,
            output: None,
            started_at,
            duration_ms: None,
            source: ExecSource::Assistant,
            interaction: None,
        }));

        let animated = cell.lines_with_options(80, TranscriptRenderOptions::default());
        let low_motion = cell.lines_with_options(
            80,
            TranscriptRenderOptions {
                low_motion: true,
                ..TranscriptRenderOptions::default()
            },
        );

        let animated_symbol = animated[0].spans[0].content.trim();
        let low_motion_symbol = low_motion[0].spans[0].content.trim();

        // low_motion always pins to the first (static) frame.
        assert_eq!(low_motion_symbol, TOOL_RUNNING_SYMBOLS[0]);
        // The animated path should be on a different frame (index 2).
        assert_ne!(animated_symbol, TOOL_RUNNING_SYMBOLS[0]);
    }

    // === Speaker glyph tests (v0.6.6 UI redesign) ===
    //
    // The literal "Assistant" / "You" labels are replaced by the calmer
    // bullet/bar glyphs (`●` / `▎`). Only the assistant glyph pulses, and
    // only while the cell is streaming — finished turns sit at the source
    // sky color so the transcript reads as solid history.

    #[test]
    fn user_cell_renders_with_bar_glyph_not_literal_label() {
        let cell = HistoryCell::User {
            content: "hello".to_string(),
        };
        let lines = cell.lines(80);
        let head = &lines[0];
        assert_eq!(head.spans[0].content.as_ref(), USER_GLYPH);
        // No "You" literal anywhere in the rendered head line.
        let visible: String = head
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(!visible.contains("You"), "user label dropped: {visible:?}");
        assert!(visible.contains("hello"));
    }

    #[test]
    fn assistant_cell_renders_with_bullet_glyph_not_literal_label() {
        let cell = HistoryCell::Assistant {
            content: "ready".to_string(),
            streaming: false,
        };
        let lines = cell.lines(80);
        let head = &lines[0];
        assert_eq!(head.spans[0].content.as_ref(), ASSISTANT_GLYPH);
        let visible: String = head
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(
            !visible.contains("Assistant"),
            "assistant label dropped: {visible:?}"
        );
        assert!(visible.contains("ready"));
    }

    #[test]
    fn assistant_glyph_holds_full_brightness_when_idle() {
        // Idle (streaming=false) and low_motion both pin the colour to the
        // source sky — pulse only fires when actively streaming.
        let idle = assistant_label_style_for(false, false);
        let low_motion = assistant_label_style_for(true, true);
        assert_eq!(idle.fg, Some(palette::DEEPSEEK_SKY));
        assert_eq!(low_motion.fg, Some(palette::DEEPSEEK_SKY));
    }

    #[test]
    fn assistant_glyph_pulses_when_streaming_and_motion_allowed() {
        // The streaming path runs through `pulse_brightness`, which yields
        // an RGB colour scaled within 30%..100% of the source. Sample twice
        // — at least one of the samples must fall below 100% brightness, or
        // the test wouldn't be exercising the pulse at all. (We can't pin
        // the value because the function reads SystemTime::now().)
        use ratatui::style::Color;
        let mut saw_dimmed = false;
        for _ in 0..50 {
            if let Some(Color::Rgb(_, _, b)) =
                assistant_label_style_for(true, false).fg
            {
                let Color::Rgb(_, _, src_b) = palette::DEEPSEEK_SKY else {
                    panic!("DEEPSEEK_SKY must be RGB");
                };
                if b < src_b {
                    saw_dimmed = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            saw_dimmed,
            "expected the streaming pulse to dip below source brightness at least once",
        );
    }

    // === Reasoning treatment tests (v0.6.6 UI redesign) ===

    #[test]
    fn render_thinking_uses_dotted_opener_in_header() {
        let lines = render_thinking("Step one\nStep two", 80, false, Some(2.0), false, true);
        let header = &lines[0];
        // First span carries `…` followed by a space.
        assert!(
            header.spans[0].content.starts_with(REASONING_OPENER),
            "header opener: {:?}",
            header.spans[0].content
        );
    }

    #[test]
    fn render_thinking_body_lines_use_dashed_rail_and_italic() {
        let lines = render_thinking(
            "concrete reasoning content",
            80,
            /*streaming*/ false,
            Some(1.0),
            /*collapsed*/ false,
            /*low_motion*/ true,
        );
        // Header is index 0; first body line is index 1.
        assert!(lines.len() >= 2, "expected at least one body line");
        let body = &lines[1];
        assert_eq!(
            body.spans[0].content.as_ref(),
            REASONING_RAIL,
            "body rail must be the dashed `╎ ` glyph"
        );
        // The body span should carry italic.
        let italic_seen = body
            .spans
            .iter()
            .skip(1)
            .any(|span| span.style.add_modifier.contains(Modifier::ITALIC));
        assert!(italic_seen, "body content should carry italic modifier");
    }

    #[test]
    fn render_thinking_streaming_appends_cursor_when_motion_allowed() {
        let lines = render_thinking(
            "ongoing reasoning...",
            80,
            /*streaming*/ true,
            None,
            /*collapsed*/ false,
            /*low_motion*/ false,
        );
        // Last line is the most recent body line — cursor lives there.
        let last = lines.last().expect("body line present");
        let last_span = last.spans.last().expect("trailing span present");
        assert!(
            last_span.content.contains(REASONING_CURSOR),
            "expected trailing cursor `▎` on last streaming body line, got {:?}",
            last_span.content
        );
    }

    #[test]
    fn render_thinking_streaming_omits_cursor_when_low_motion() {
        let lines = render_thinking(
            "ongoing reasoning...",
            80,
            /*streaming*/ true,
            None,
            /*collapsed*/ false,
            /*low_motion*/ true,
        );
        let last = lines.last().expect("body line present");
        let visible: String = last
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(
            !visible.contains(REASONING_CURSOR),
            "low_motion must suppress the streaming cursor: {visible:?}"
        );
    }

    // === Theme parity tests ===
    //
    // These lock the visible color/style choices for one plan cell and one
    // tool cell against `deepseek_theme::Theme::dark()`. The render path is
    // unchanged in shape; the assertions just guarantee a future skin swap
    // (or accidental drift) is caught here instead of at runtime.

    #[test]
    fn plan_update_cell_renders_with_dark_theme_tokens() {
        let theme = Theme::dark();
        let cell = PlanUpdateCell {
            explanation: None,
            steps: vec![
                PlanStep {
                    step: "scan repo".to_string(),
                    status: "completed".to_string(),
                },
                PlanStep {
                    step: "extract theme".to_string(),
                    status: "in_progress".to_string(),
                },
                PlanStep {
                    step: "land tests".to_string(),
                    status: "pending".to_string(),
                },
            ],
            status: ToolStatus::Running,
        };

        let lines = cell.lines_with_motion(80, true);

        // Header: "<symbol> Plan <state>"
        let header = &lines[0];
        let symbol_span = &header.spans[0];
        let title_span = &header.spans[1];
        let state_span = &header.spans[3];

        assert_eq!(
            symbol_span.style.fg,
            Some(theme.tool_running_accent),
            "running header symbol should use the dark theme running accent"
        );
        assert_eq!(
            title_span.content.as_ref(),
            "Plan",
            "tool title text is locked"
        );
        assert_eq!(title_span.style.fg, Some(theme.tool_title_color));
        assert!(
            title_span.style.add_modifier.contains(Modifier::BOLD),
            "tool title should be bold"
        );
        assert_eq!(
            state_span.content.as_ref(),
            "running",
            "running PlanUpdate should label state as 'running'"
        );
        assert_eq!(state_span.style.fg, Some(theme.tool_running_accent));

        // Each step row: ["▏ ", "<marker>:", " ", "<step>"]
        let step_line = &lines[1];
        let label_span = &step_line.spans[1];
        let value_span = &step_line.spans[3];
        assert_eq!(
            label_span.style.fg,
            Some(theme.tool_label_color),
            "step label should use theme.tool_label_color"
        );
        assert_eq!(
            value_span.style.fg,
            Some(theme.tool_value_color),
            "step value should use theme.tool_value_color"
        );

        // Plain content stays identical so visible output does not move.
        let visible = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(visible[1].trim_end(), "▏ done: scan repo");
        assert_eq!(visible[2].trim_end(), "▏ live: extract theme");
        assert_eq!(visible[3].trim_end(), "▏ next: land tests");
    }

    #[test]
    fn exec_cell_failed_status_renders_with_dark_theme_tokens() {
        let theme = Theme::dark();
        let cell = ExecCell {
            command: "false".to_string(),
            status: ToolStatus::Failed,
            output: Some("boom".to_string()),
            started_at: None,
            duration_ms: Some(42),
            source: ExecSource::Assistant,
            interaction: None,
        };

        let lines = cell.lines_with_motion(80, true);

        let header = &lines[0];
        let symbol_span = &header.spans[0];
        let title_span = &header.spans[1];
        let state_span = &header.spans[3];

        assert_eq!(
            symbol_span.style.fg,
            Some(theme.tool_failed_accent),
            "failed exec header symbol should use the dark theme failed accent"
        );
        assert_eq!(
            title_span.content.as_ref(),
            "Shell",
            "exec title text is locked"
        );
        assert_eq!(title_span.style.fg, Some(theme.tool_title_color));
        assert!(title_span.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(state_span.content.as_ref(), "issue");
        assert_eq!(state_span.style.fg, Some(theme.tool_failed_accent));
    }

    // === display_lines (lines_with_options) vs transcript_lines parity ===
    //
    // These lock the contract for CX#8: live view compresses thinking and
    // caps tool output, transcript view shows the full body. Both surfaces
    // must contain the first paragraph / first line of the underlying
    // content so users never lose the lede.

    fn line_text(line: &ratatui::text::Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn lines_text(lines: &[ratatui::text::Line<'static>]) -> String {
        lines.iter().map(line_text).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn long_thinking_display_is_shorter_than_transcript() {
        // Build a multi-paragraph thinking body so the live view has
        // something to compress. The first paragraph is the lede; both
        // surfaces must keep it.
        let body = "First paragraph lede.\n\
                    Second sentence of the first paragraph.\n\n\
                    Second paragraph: deeper analysis follows.\n\
                    More detail in paragraph two.\n\n\
                    Third paragraph: even more reasoning.\n\
                    With another line.\n\n\
                    Fourth paragraph: the conclusion.\n\
                    And one more line for good measure.";
        let cell = HistoryCell::Thinking {
            content: body.to_string(),
            streaming: false,
            duration_secs: Some(3.2),
        };

        let live = cell.lines_with_options(
            80,
            TranscriptRenderOptions {
                low_motion: true,
                ..TranscriptRenderOptions::default()
            },
        );
        let transcript = cell.transcript_lines(80);

        assert!(
            live.len() < transcript.len(),
            "live thinking should compress (live = {} lines, transcript = {} lines)",
            live.len(),
            transcript.len()
        );

        let live_text = lines_text(&live);
        let transcript_text = lines_text(&transcript);

        assert!(
            live_text.contains("First paragraph lede"),
            "live thinking must keep the lede: {live_text}"
        );
        assert!(
            transcript_text.contains("First paragraph lede"),
            "transcript thinking must keep the lede"
        );
        assert!(
            transcript_text.contains("Fourth paragraph"),
            "transcript thinking must keep the full body"
        );
        assert!(
            !live_text.contains("Fourth paragraph"),
            "live thinking must drop the tail when collapsed"
        );
        assert!(
            live_text.contains("press Ctrl+O for full text"),
            "live thinking must offer the pager affordance"
        );
        assert!(
            !transcript_text.contains("press Ctrl+O for full text"),
            "transcript thinking must not include the live affordance"
        );
    }

    #[test]
    fn short_thinking_display_equals_transcript() {
        // A single-line thinking body has nothing to compress; live and
        // transcript surfaces should agree.
        let cell = HistoryCell::Thinking {
            content: "One brief reasoning step.".to_string(),
            streaming: false,
            duration_secs: Some(0.4),
        };

        let live = cell.lines_with_options(
            80,
            TranscriptRenderOptions {
                low_motion: true,
                ..TranscriptRenderOptions::default()
            },
        );
        let transcript = cell.transcript_lines(80);

        let live_text = lines_text(&live);
        let transcript_text = lines_text(&transcript);

        assert_eq!(
            live_text, transcript_text,
            "short thinking must render identically on both surfaces"
        );
        assert!(
            !live_text.contains("press Ctrl+O for full text"),
            "short thinking must not show the collapse affordance"
        );
    }

    #[test]
    fn tool_exec_live_caps_output_transcript_does_not() {
        // Synthesize an exec output that comfortably exceeds the live cap
        // (TOOL_OUTPUT_LINE_LIMIT = 6). The live view should hit the cap
        // and emit a "+N more lines; press v for details" affordance; the
        // transcript view should emit every wrapped line uncapped.
        let total_output_lines = 30usize;
        let output = (0..total_output_lines)
            .map(|i| format!("output line {i:02}"))
            .collect::<Vec<_>>()
            .join("\n");

        let cell = HistoryCell::Tool(ToolCell::Exec(ExecCell {
            command: "noisy_script.sh".to_string(),
            status: ToolStatus::Success,
            output: Some(output),
            started_at: None,
            duration_ms: Some(120),
            source: ExecSource::Assistant,
            interaction: None,
        }));

        let live = cell.lines_with_options(
            80,
            TranscriptRenderOptions {
                low_motion: true,
                ..TranscriptRenderOptions::default()
            },
        );
        let transcript = cell.transcript_lines(80);

        let live_text = lines_text(&live);
        let transcript_text = lines_text(&transcript);

        assert!(
            live.len() < transcript.len(),
            "live exec output must be shorter than transcript exec output (live={}, transcript={})",
            live.len(),
            transcript.len()
        );
        assert!(
            live_text.contains("press v for details"),
            "live exec output must surface the pager affordance: {live_text}"
        );
        assert!(
            !transcript_text.contains("press v for details"),
            "transcript exec output must not include the pager affordance"
        );
        // First line is always emitted on both surfaces.
        assert!(live_text.contains("output line 00"));
        assert!(transcript_text.contains("output line 00"));
        // The middle should only appear in the transcript, since the live
        // view truncates the head/tail around the cap.
        assert!(
            transcript_text.contains("output line 15"),
            "transcript must include the middle of the exec output"
        );
        // Last line should appear in both because the live view shows
        // head + tail around an omission marker.
        let last = format!("output line {:02}", total_output_lines - 1);
        assert!(transcript_text.contains(&last));
    }

    #[test]
    fn generic_tool_cell_renders_prompts_as_indexed_rows() {
        // When prompts are populated by a fan-out tool, each child shows on
        // its own row instead of the inline `args:` summary so the user can
        // read what each child was asked.
        let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: "future_fanout_tool".to_string(),
            status: ToolStatus::Running,
            input_summary: Some("prompts: <3 items>".to_string()),
            output: None,
            prompts: Some(vec![
                "Summarize the README".to_string(),
                "List the public types in client.rs".to_string(),
                "Diff this commit against main".to_string(),
            ]),
        }));
        let text = lines_text(&cell.lines(80));

        assert!(text.contains("[0] Summarize the README"));
        assert!(text.contains("[1] List the public types in client.rs"));
        assert!(text.contains("[2] Diff this commit against main"));
        // The inline args summary must not also be emitted — we replaced it
        // with the per-child rows.
        assert!(
            !text.contains("args: prompts:"),
            "inline `args:` summary must be suppressed when per-prompt rows render"
        );
    }

    #[test]
    fn generic_tool_cell_falls_back_to_args_when_prompts_none() {
        // Non-fan-out tools keep the existing `args:` summary so behavior
        // doesn't drift for everything else.
        let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: "file_search".to_string(),
            status: ToolStatus::Running,
            input_summary: Some("query: foo".to_string()),
            output: None,
            prompts: None,
        }));
        let text = lines_text(&cell.lines(80));
        assert!(text.contains("query: foo"));
    }

    #[test]
    fn generic_tool_cell_preserves_multi_line_output_in_transcript() {
        // Repro for #80: a `git diff --stat`-shaped tool result should keep
        // its newlines on the transcript surface — one file per row, not
        // squashed into a single line.
        let diff_stat = "Cargo.lock                |  1 +\n\
                         crates/cli/Cargo.toml     |  1 +\n\
                         crates/cli/src/main.rs    | 47 ++++++\n\
                         crates/config/src/lib.rs  | 27 ++++\n\
                         crates/tui/src/mcp.rs     | 384 +++++";

        let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: "exec_shell".to_string(),
            status: ToolStatus::Success,
            input_summary: Some("command: git diff --stat".to_string()),
            output: Some(diff_stat.to_string()),
            prompts: None,
        }));

        let transcript_text = lines_text(&cell.transcript_lines(80));

        // Each file path must appear on its own row in the transcript.
        for needle in [
            "Cargo.lock",
            "crates/cli/Cargo.toml",
            "crates/cli/src/main.rs",
            "crates/config/src/lib.rs",
            "crates/tui/src/mcp.rs",
        ] {
            assert!(
                transcript_text.contains(needle),
                "transcript missing '{needle}': {transcript_text}"
            );
        }
        // The pre-fix bug: result line containing
        // "Cargo.lock | 1 + crates/cli/Cargo.toml" — joined into one row.
        // With the fix, the diff-stat pipes are still present per-line, but
        // adjacent file paths are on separate rendered rows. Assert that the
        // first file's line ends before the second begins.
        let lines: Vec<&str> = transcript_text.lines().collect();
        let cargo_lock_line = lines
            .iter()
            .find(|l| l.contains("Cargo.lock"))
            .expect("Cargo.lock row must exist");
        assert!(
            !cargo_lock_line.contains("crates/cli/Cargo.toml"),
            "Cargo.lock row must not also contain the second file: {cargo_lock_line}"
        );
    }

    #[test]
    fn generic_tool_cell_caps_multi_line_output_in_live_with_affordance() {
        // Live (in-progress / active-cell) view caps long output at
        // TOOL_OUTPUT_LINE_LIMIT (=6) and shows a "+N more lines" affordance.
        let total = 30usize;
        let output = (0..total)
            .map(|i| format!("row {i:02}: payload"))
            .collect::<Vec<_>>()
            .join("\n");

        let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: "exec_shell".to_string(),
            status: ToolStatus::Success,
            input_summary: Some("command: ls".to_string()),
            output: Some(output),
            prompts: None,
        }));

        let live = cell.lines_with_options(80, TranscriptRenderOptions::default());
        let transcript = cell.transcript_lines(80);

        assert!(
            live.len() < transcript.len(),
            "live generic-tool output must be shorter than transcript (live={}, transcript={})",
            live.len(),
            transcript.len(),
        );
        let live_text = lines_text(&live);
        assert!(
            live_text.contains("press v for details"),
            "live view must show pager affordance: {live_text}"
        );
        // First line shows up in both; later rows only in transcript.
        assert!(live_text.contains("row 00"));
        let transcript_text = lines_text(&transcript);
        assert!(transcript_text.contains("row 29"));
    }
}
