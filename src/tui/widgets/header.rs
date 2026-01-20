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
    pub is_streaming: bool,
    pub background: ratatui::style::Color,
}

impl<'a> HeaderData<'a> {
    /// Create header data from common app fields.
    #[must_use]
    pub fn new(
        _mode: AppMode,
        model: &'a str,
        _context_used: u32,
        is_streaming: bool,
        background: ratatui::style::Color,
    ) -> Self {
        Self {
            model,
            is_streaming,
            background,
        }
    }
}

/// Header bar widget (1 line height).
///
/// Layout: `[MODE] | model-name | Context: XX% | [streaming indicator]`
pub struct HeaderWidget<'a> {
    data: HeaderData<'a>,
}

impl<'a> HeaderWidget<'a> {
    #[must_use]
    pub fn new(data: HeaderData<'a>) -> Self {
        Self { data }
    }

    /// Build the model name span.
    fn model_span(&self) -> Span<'static> {
        // Truncate long model names
        let display_name = if self.data.model.len() > 20 {
            format!("{}...", &self.data.model[..17])
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
            " streaming... ",
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

        // Build left section: model name only (Mode is in footer)
        let mut left_spans = vec![self.model_span()];

        // Build right section: streaming indicator
        let streaming_span = self.streaming_indicator();

        // Calculate widths
        let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();
        let right_width = streaming_span.as_ref().map_or(0, |s| s.content.width());

        let total_content = left_width + right_width + 2; // + padding
        let available = area.width as usize;

        // Build final line based on available space
        let mut spans = Vec::new();

        if available >= total_content {
            // Full layout: left | (spacer) | right
            spans.append(&mut left_spans);

            // Spacer
            let padding_needed = available.saturating_sub(left_width + right_width);
            if padding_needed > 0 {
                spans.push(Span::raw(" ".repeat(padding_needed)));
            }

            // Add streaming on right
            if let Some(streaming) = streaming_span {
                spans.push(streaming);
            }
        } else if available >= left_width {
            // Minimal: just model
            spans.append(&mut left_spans);
        } else {
            // Ultra-minimal: just model
            spans.push(self.model_span());
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).style(Style::default().bg(self.data.background));
        paragraph.render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1 // Header is always 1 line
    }
}
