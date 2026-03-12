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
/// Layout: `mode  model                        ●`
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

    /// Build the mode badge span (no brackets, lowercase, bold).
    fn mode_badge(&self) -> Span<'static> {
        let label = self.data.mode.label().to_lowercase();
        let color = Self::mode_color(self.data.mode);
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )
    }

    /// Build the model name span (muted, truncated).
    fn model_span(&self) -> Span<'static> {
        let display_name = if self.data.model.chars().count() > 25 {
            let truncated: String = self.data.model.chars().take(22).collect();
            format!("{truncated}...")
        } else {
            self.data.model.to_string()
        };

        Span::styled(display_name, Style::default().fg(palette::TEXT_HINT))
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
}

impl Renderable for HeaderWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Build left section: mode + model
        let mode_span = self.mode_badge();
        let model_span = self.model_span();

        // Build right section: streaming indicator only. Footer owns context.
        let streaming_span = self.streaming_indicator();

        // Calculate widths
        let mode_width = mode_span.content.width();
        let model_width = model_span.content.width();
        let streaming_width = streaming_span.as_ref().map_or(0, |s| s.content.width());
        let right_width = streaming_width;

        let left_width = mode_width + 2 + model_width; // mode + "  " + model

        let available = area.width as usize;

        // Build final line based on available space
        let mut spans = Vec::new();

        if available >= left_width + right_width + 2 {
            // Full layout: mode  model  (spacer)  ●
            spans.push(mode_span);
            spans.push(Span::raw("  "));
            spans.push(model_span);

            // Spacer to push right elements to the end
            let padding_needed = available.saturating_sub(left_width + right_width);
            if padding_needed > 0 {
                spans.push(Span::raw(" ".repeat(padding_needed)));
            }

            // Streaming indicator
            if let Some(streaming) = streaming_span {
                spans.push(streaming);
            }
        } else if available >= mode_width + 2 + model_width.min(10) {
            // Compact layout: mode  truncated_model
            spans.push(mode_span);
            spans.push(Span::raw("  "));
            let model_str = self.data.model;
            let display_model = if model_str.chars().count() > 10 {
                let truncated: String = model_str.chars().take(7).collect();
                format!("{truncated}...")
            } else {
                model_str.to_string()
            };
            spans.push(Span::styled(
                display_model,
                Style::default().fg(palette::TEXT_HINT),
            ));
        } else if available >= mode_width {
            // Minimal: just mode badge
            spans.push(mode_span);
        } else {
            // Ultra-minimal: single lowercase char
            let first_char = self
                .data
                .mode
                .label()
                .chars()
                .next()
                .unwrap_or('?')
                .to_lowercase()
                .to_string();
            spans.push(Span::styled(
                first_char,
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
