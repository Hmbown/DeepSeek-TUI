//! Header bar widget displaying mode, model, and streaming state.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::palette;
use crate::tui::app::AppMode;

use super::Renderable;

/// Format a token count for compact display (e.g., "12.3k", "1.2M").
fn format_token_count(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{tokens}")
    }
}

/// Data required to render the header bar.
pub struct HeaderData<'a> {
    pub model: &'a str,
    pub mode: AppMode,
    pub is_streaming: bool,
    pub background: ratatui::style::Color,
    /// Total tokens used in this session (cumulative, for display).
    pub total_tokens: u32,
    /// Context window size for the model (if known).
    pub context_window: Option<u32>,
    /// Accumulated session cost in USD.
    pub session_cost: f64,
    /// Input tokens from the most recent API call (current context utilization).
    pub last_prompt_tokens: Option<u32>,
}

impl<'a> HeaderData<'a> {
    /// Create header data from common app fields.
    #[must_use]
    pub fn new(
        mode: AppMode,
        model: &'a str,
        is_streaming: bool,
        background: ratatui::style::Color,
    ) -> Self {
        Self {
            model,
            mode,
            is_streaming,
            background,
            total_tokens: 0,
            context_window: None,
            session_cost: 0.0,
            last_prompt_tokens: None,
        }
    }

    /// Set token/cost fields.
    #[must_use]
    pub fn with_usage(
        mut self,
        total_tokens: u32,
        context_window: Option<u32>,
        session_cost: f64,
        last_prompt_tokens: Option<u32>,
    ) -> Self {
        self.total_tokens = total_tokens;
        self.context_window = context_window;
        self.session_cost = session_cost;
        self.last_prompt_tokens = last_prompt_tokens;
        self
    }
}

/// Header bar widget (1 line height).
///
/// Layout: `[MODE] model-name | [streaming indicator]`
pub struct HeaderWidget<'a> {
    data: HeaderData<'a>,
}

impl<'a> HeaderWidget<'a> {
    #[must_use]
    pub fn new(data: HeaderData<'a>) -> Self {
        Self { data }
    }

    /// Get the color for a mode.
    fn mode_color(mode: AppMode) -> ratatui::style::Color {
        match mode {
            AppMode::Normal => palette::MODE_NORMAL,
            AppMode::Agent => palette::MODE_AGENT,
            AppMode::Yolo => palette::MODE_YOLO,
            AppMode::Plan => palette::MODE_PLAN,
        }
    }

    /// Build the mode badge span.
    fn mode_badge(&self) -> Span<'static> {
        let label = self.data.mode.label();
        let color = Self::mode_color(self.data.mode);
        Span::styled(
            format!("[{label}]"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )
    }

    /// Build the model name span.
    fn model_span(&self) -> Span<'static> {
        // Truncate long model names (char-safe to avoid panics on multi-byte UTF-8)
        let display_name = if self.data.model.chars().count() > 25 {
            let truncated: String = self.data.model.chars().take(22).collect();
            format!("{truncated}...")
        } else {
            self.data.model.to_string()
        };

        Span::styled(display_name, Style::default().fg(palette::TEXT_MUTED))
    }

    /// Build the streaming indicator span.
    fn streaming_indicator(&self) -> Option<Span<'static>> {
        if !self.data.is_streaming {
            return None;
        }

        Some(Span::styled(
            "●",
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::BOLD),
        ))
    }

    /// Build the token/cost info span for the right side of the header.
    fn usage_span(&self) -> Option<Span<'static>> {
        if self.data.total_tokens == 0 && self.data.session_cost < 0.0001 {
            return None;
        }

        let mut parts = Vec::new();

        // Token count with context window percentage
        if self.data.total_tokens > 0 {
            let token_str = format_token_count(self.data.total_tokens);
            // Use last_prompt_tokens for % (current context utilization)
            if let Some(ctx_window) = self.data.context_window {
                if let Some(prompt_tokens) = self.data.last_prompt_tokens {
                    let pct = (prompt_tokens as f64 / ctx_window as f64 * 100.0) as u32;
                    let pct_str = format!("{token_str} ({pct}%)");
                    parts.push(pct_str);
                } else {
                    parts.push(token_str);
                }
            } else {
                parts.push(token_str);
            }
        }

        // Cost
        if self.data.session_cost >= 0.0001 {
            parts.push(crate::pricing::format_cost(self.data.session_cost));
        }

        if parts.is_empty() {
            return None;
        }

        Some(Span::styled(
            parts.join(" · "),
            Style::default().fg(palette::TEXT_MUTED),
        ))
    }

    /// Build a subtle separator span.
    fn separator_span(&self) -> Span<'static> {
        Span::styled(" │ ", Style::default().fg(palette::BORDER_COLOR))
    }
}

impl Renderable for HeaderWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Build left section: mode badge + model name
        let mode_span = self.mode_badge();
        let model_span = self.model_span();

        // Build right section: usage info + streaming indicator
        let streaming_span = self.streaming_indicator();
        let usage_span = self.usage_span();

        // Subtle separator (vertical bar)
        let separator_span = self.separator_span();
        let separator_width = separator_span.content.width();

        // Calculate widths
        let mode_width = mode_span.content.width();
        let model_width = model_span.content.width();
        let streaming_width = streaming_span.as_ref().map_or(0, |s| s.content.width());
        let usage_width = usage_span.as_ref().map_or(0, |s| s.content.width());
        let right_width = streaming_width
            + usage_width
            + if streaming_width > 0 && usage_width > 0 {
                1
            } else {
                0
            };

        let left_width = mode_width + 1 + model_width; // mode + space + model (without separator)

        // Determine if separator should be shown (when there's right‑side content)
        let show_separator = usage_span.is_some() || streaming_span.is_some();
        let separator_visible_width = if show_separator { separator_width } else { 0 };

        let available = area.width as usize;

        // Build final line based on available space
        let mut spans = Vec::new();

        if available >= left_width + separator_visible_width + right_width + 2 {
            // Full layout: [MODE] model | separator (optional) | (spacer) | usage streaming
            spans.push(mode_span);
            spans.push(Span::raw(" "));
            spans.push(model_span);

            // Add separator if there is right‑side content
            if show_separator {
                spans.push(separator_span);
            }

            // Spacer to push right elements to the end
            let padding_needed =
                available.saturating_sub(left_width + separator_visible_width + right_width);
            if padding_needed > 0 {
                spans.push(Span::raw(" ".repeat(padding_needed)));
            }

            // Add usage info (right side)
            if let Some(usage) = usage_span {
                spans.push(usage);
                if streaming_span.is_some() {
                    spans.push(Span::raw(" "));
                }
            }

            // Add streaming indicator
            if let Some(streaming) = streaming_span {
                spans.push(streaming);
            }
        } else if available >= mode_width + 1 + model_width.min(10) {
            // Compact layout: [MODE] truncated_model
            spans.push(mode_span);
            spans.push(Span::raw(" "));
            // Truncate model if needed
            let model_str = self.data.model;
            let display_model = if model_str.chars().count() > 10 {
                let truncated: String = model_str.chars().take(7).collect();
                format!("{truncated}...")
            } else {
                model_str.to_string()
            };
            spans.push(Span::styled(
                display_model,
                Style::default().fg(palette::TEXT_MUTED),
            ));
        } else if available >= mode_width {
            // Minimal: just mode badge
            spans.push(mode_span);
        } else {
            // Ultra-minimal: truncated mode
            spans.push(Span::styled(
                &self.data.mode.label()[..1.min(self.data.mode.label().len())],
                Style::default().fg(Self::mode_color(self.data.mode)),
            ));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).style(Style::default().bg(self.data.background));
        paragraph.render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1 // Header is always 1 line
    }
}
