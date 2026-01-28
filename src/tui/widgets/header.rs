//! Header bar widget displaying mode, model, context usage, and streaming state.

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
    pub context_percent: Option<u8>,
    pub background: ratatui::style::Color,
}

impl<'a> HeaderData<'a> {
    /// Create header data from common app fields.
    #[must_use]
    pub fn new(
        mode: AppMode,
        model: &'a str,
        context_used: u32,
        is_streaming: bool,
        background: ratatui::style::Color,
    ) -> Self {
        // Calculate context percentage
        let context_percent = crate::models::context_window_for_model(model).map(|max| {
            let max_u32 = max;
            let pct = (context_used * 100 / max_u32.max(1)).min(100) as u8;
            pct
        });

        Self {
            model,
            mode,
            is_streaming,
            context_percent,
            background,
        }
    }
}

/// Header bar widget (1 line height).
///
/// Layout: `[MODE] model-name | Context: XX% [streaming indicator]`
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
            AppMode::Rlm => palette::MODE_RLM,
            AppMode::Duo => palette::MODE_DUO,
        }
    }

    /// Build the mode badge span.
    fn mode_badge(&self) -> Span<'static> {
        let label = self.data.mode.label();
        let color = Self::mode_color(self.data.mode);
        Span::styled(
            format!("[{label}]"),
            Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD),
        )
    }

    /// Build the model name span.
    fn model_span(&self) -> Span<'static> {
        // Truncate long model names
        let display_name = if self.data.model.len() > 25 {
            format!("{}...", &self.data.model[..22])
        } else {
            self.data.model.to_string()
        };

        Span::styled(display_name, Style::default().fg(palette::TEXT_MUTED))
    }

    /// Build the context usage span.
    fn context_span(&self) -> Option<Span<'static>> {
        let pct = self.data.context_percent?;
        let color = if pct < 50 {
            palette::TEXT_DIM
        } else if pct < 80 {
            palette::STATUS_WARNING
        } else {
            palette::STATUS_ERROR
        };

        Some(Span::styled(
            format!(" {pct}% "),
            Style::default().fg(color),
        ))
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

        // Build left section: mode badge + model name
        let mode_span = self.mode_badge();
        let model_span = self.model_span();

        // Build right section: context percentage + streaming indicator
        let context_span = self.context_span();
        let streaming_span = self.streaming_indicator();

        // Calculate widths
        let mode_width = mode_span.content.width();
        let model_width = model_span.content.width();
        let context_width = context_span.as_ref().map_or(0, |s| s.content.width());
        let streaming_width = streaming_span.as_ref().map_or(0, |s| s.content.width());

        let left_width = mode_width + 1 + model_width; // mode + space + model
        let right_width = context_width + streaming_width;

        let available = area.width as usize;

        // Build final line based on available space
        let mut spans = Vec::new();

        if available >= left_width + right_width + 2 {
            // Full layout: [MODE] model | (spacer) | context streaming
            spans.push(mode_span);
            spans.push(Span::raw(" "));
            spans.push(model_span);

            // Spacer to push right elements to the end
            let padding_needed = available.saturating_sub(left_width + right_width);
            if padding_needed > 0 {
                spans.push(Span::raw(" ".repeat(padding_needed)));
            }

            // Add context percentage
            if let Some(context) = context_span {
                spans.push(context);
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
            let display_model = if model_str.len() > 10 {
                format!("{}...", &model_str[..7])
            } else {
                model_str.to_string()
            };
            spans.push(Span::styled(display_model, Style::default().fg(palette::TEXT_MUTED)));
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
