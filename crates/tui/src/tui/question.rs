//! Modal for the `question` tool — highlighted prompt with freeform
//! text input.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Widget, Wrap};

use crate::palette;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};

fn modal_block(title: &str) -> Block<'static> {
    Block::default()
        .title(Line::from(vec![Span::styled(
            title.to_string(),
            Style::default().fg(palette::DEEPSEEK_BLUE).bold(),
        )]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette::BORDER_COLOR))
        .style(Style::default().bg(palette::DEEPSEEK_INK))
        .padding(Padding::uniform(1))
}

fn render_modal_chrome(area: Rect, popup_area: Rect, buf: &mut Buffer) {
    let shadow_x = popup_area.x.saturating_add(1);
    let shadow_y = popup_area.y.saturating_add(1);
    let shadow_right = area.x.saturating_add(area.width);
    let shadow_bottom = area.y.saturating_add(area.height);
    let shadow_width = popup_area.width.min(shadow_right.saturating_sub(shadow_x));
    let shadow_height = popup_area
        .height
        .min(shadow_bottom.saturating_sub(shadow_y));

    if shadow_width > 0 && shadow_height > 0 {
        Block::default()
            .style(Style::default().bg(palette::DEEPSEEK_NAVY))
            .render(
                Rect {
                    x: shadow_x,
                    y: shadow_y,
                    width: shadow_width,
                    height: shadow_height,
                },
                buf,
            );
    }

    Clear.render(popup_area, buf);
}

#[derive(Debug, Clone)]
pub struct QuestionView {
    tool_id: String,
    question: String,
    input_buffer: String,
}

impl QuestionView {
    pub fn new(tool_id: impl Into<String>, question: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
            question: question.into(),
            input_buffer: String::new(),
        }
    }
}

impl ModalView for QuestionView {
    fn kind(&self) -> ModalKind {
        ModalKind::Question
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        if key.code == KeyCode::Esc {
            return ViewAction::EmitAndClose(ViewEvent::QuestionCancelled {
                tool_id: self.tool_id.clone(),
            });
        }

        match key.code {
            KeyCode::Enter => {
                let answer = if self.input_buffer.trim().is_empty() {
                    "proceed".to_string()
                } else {
                    self.input_buffer.trim().to_string()
                };
                ViewAction::EmitAndClose(ViewEvent::QuestionAnswered {
                    tool_id: self.tool_id.clone(),
                    answer,
                })
            }
            KeyCode::Backspace => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+Backspace: delete last word
                    let trimmed = self.input_buffer.trim_end();
                    if let Some(pos) = trimmed.rfind(|c: char| c.is_whitespace()) {
                        self.input_buffer.truncate(pos);
                    } else {
                        self.input_buffer.clear();
                    }
                } else {
                    self.input_buffer.pop();
                }
                ViewAction::None
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL) && !ch.is_control() =>
            {
                self.input_buffer.push(ch);
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Banner
        lines.push(Line::from(vec![Span::styled(
            "The model needs clarification",
            Style::default().fg(palette::DEEPSEEK_SKY).bold(),
        )]));
        lines.push(Line::from(""));

        // Question text — highlighted
        lines.push(Line::from(vec![Span::styled(
            &self.question,
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .bold()
                .italic(),
        )]));
        lines.push(Line::from(""));

        // Separator
        lines.push(Line::from(vec![Span::styled(
            "─".repeat(40),
            Style::default().fg(palette::BORDER_COLOR),
        )]));
        lines.push(Line::from(""));

        // Input area
        let display_text = if self.input_buffer.is_empty() {
            "(type your response then press Enter)".to_string()
        } else {
            self.input_buffer.clone()
        };

        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(palette::DEEPSEEK_BLUE).bold()),
            Span::styled(
                display_text,
                if self.input_buffer.is_empty() {
                    Style::default().fg(palette::TEXT_MUTED)
                } else {
                    Style::default().fg(palette::DEEPSEEK_SKY)
                },
            ),
        ]));
        lines.push(Line::from(""));

        // Footer controls
        lines.push(Line::from(vec![
            Span::styled("Enter", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::styled(" submit answer  ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::styled(" cancel", Style::default().fg(palette::TEXT_MUTED)),
        ]));

        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true })
            .block(modal_block(" Question "));

        let popup_area = centered_rect(72, 50, area);
        render_modal_chrome(area, popup_area, buf);
        paragraph.render(popup_area, buf);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1]);
    horizontal[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_view(view: &QuestionView, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);

        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample_view() -> QuestionView {
        QuestionView::new("tool-1", "What port should the server listen on?")
    }

    #[test]
    fn question_modal_calls_out_clarification() {
        let rendered = render_view(&sample_view(), 90, 20);

        assert!(rendered.contains("needs clarification"));
        assert!(rendered.contains("What port should the server listen on?"));
        assert!(rendered.contains("Enter"));
        assert!(rendered.contains("submit"));
    }

    #[test]
    fn question_modal_renders_input_text() {
        let mut view = sample_view();
        view.input_buffer = "8080".to_string();

        let rendered = render_view(&view, 90, 20);

        assert!(rendered.contains("8080"));
        assert!(!rendered.contains("type your response"));
    }

    #[test]
    fn enter_with_empty_input_returns_proceed() {
        let mut view = sample_view();
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        match view.handle_key(key) {
            ViewAction::EmitAndClose(ViewEvent::QuestionAnswered { answer, .. }) => {
                assert_eq!(answer, "proceed");
            }
            other => panic!("expected QuestionAnswered with 'proceed', got {other:?}"),
        }
    }

    #[test]
    fn escape_cancels_question() {
        let mut view = sample_view();
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

        match view.handle_key(key) {
            ViewAction::EmitAndClose(ViewEvent::QuestionCancelled { .. }) => {}
            other => panic!("expected QuestionCancelled, got {other:?}"),
        }
    }

    #[test]
    fn typing_accumulates_in_buffer() {
        let mut view = sample_view();

        for ch in "hello".chars() {
            let action = view.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            assert!(matches!(action, ViewAction::None));
        }

        assert_eq!(view.input_buffer, "hello");
    }
}
