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
    pub workspace_name: &'a str,
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
        workspace_name: &'a str,
        is_streaming: bool,
        background: ratatui::style::Color,
    ) -> Self {
        Self {
            model,
            workspace_name,
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
            AppMode::Agent => palette::MODE_AGENT,
            AppMode::Yolo => palette::MODE_YOLO,
            AppMode::Plan => palette::MODE_PLAN,
        }
    }

    fn mode_name(mode: AppMode) -> &'static str {
        match mode {
            AppMode::Agent => "Agent",
            AppMode::Yolo => "Yolo",
            AppMode::Plan => "Plan",
        }
    }

    fn mode_segments(&self) -> Vec<Span<'static>> {
        let modes = [AppMode::Plan, AppMode::Agent, AppMode::Yolo];
        let mut spans = Vec::new();
        for (idx, mode) in modes.into_iter().enumerate() {
            if idx > 0 {
                spans.push(Span::raw(" "));
            }
            let is_selected = mode == self.data.mode;
            let style = if is_selected {
                Style::default()
                    .fg(self.data.background)
                    .bg(Self::mode_color(mode))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette::TEXT_HINT)
            };
            spans.push(Span::styled(format!(" {} ", Self::mode_name(mode)), style));
        }
        spans
    }

    fn context_text(&self, max_chars: usize) -> String {
        let raw = format!("{}  ·  {}", self.data.workspace_name, self.data.model);
        if raw.chars().count() <= max_chars {
            raw
        } else {
            let mut truncated = String::new();
            for ch in raw.chars().take(max_chars.saturating_sub(3)) {
                truncated.push(ch);
            }
            truncated.push_str("...");
            truncated
        }
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

        let mut left_spans = self.mode_segments();

        // Build right section: streaming indicator only. Footer owns context.
        let streaming_span = self.streaming_indicator();

        // Calculate widths
        let mut left_width: usize = left_spans.iter().map(|span| span.content.width()).sum();
        let streaming_width = streaming_span.as_ref().map_or(0, |s| s.content.width());
        let right_width = streaming_width;

        let available = area.width as usize;

        // Build final line based on available space
        let mut spans = Vec::new();

        let context_room = available
            .saturating_sub(left_width + right_width)
            .saturating_sub(2);
        if context_room >= 10 {
            let context_text = self.context_text(context_room);
            left_spans.push(Span::raw("  "));
            left_spans.push(Span::styled(
                context_text,
                Style::default().fg(palette::TEXT_HINT),
            ));
            left_width = left_spans.iter().map(|span| span.content.width()).sum();
        }

        if available >= left_width + right_width {
            spans.extend(left_spans);

            // Spacer to push right elements to the end
            let padding_needed = available.saturating_sub(left_width + right_width);
            if padding_needed > 0 {
                spans.push(Span::raw(" ".repeat(padding_needed)));
            }

            // Streaming indicator
            if let Some(streaming) = streaming_span {
                spans.push(streaming);
            }
        } else if available >= 12 {
            spans.push(Span::styled(
                format!(" {} ", Self::mode_name(self.data.mode)),
                Style::default()
                    .fg(self.data.background)
                    .bg(Self::mode_color(self.data.mode))
                    .add_modifier(Modifier::BOLD),
            ));
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
