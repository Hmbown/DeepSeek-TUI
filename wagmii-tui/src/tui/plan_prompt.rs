//! Modal prompt for selecting what to do after a plan is generated.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};

use crate::palette;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};

const PLAN_OPTIONS: [(&str, &str); 4] = [
    (
        "Accept plan (Agent)",
        "Start implementation in Agent mode with approvals",
    ),
    (
        "Accept plan (YOLO)",
        "Start implementation in YOLO mode (auto-approve)",
    ),
    ("Revise plan", "Ask follow-ups or request plan changes"),
    (
        "Exit Plan mode",
        "Return to Agent mode without implementation",
    ),
];

#[derive(Debug, Clone, Default)]
pub struct PlanPromptView {
    selected: usize,
}

impl PlanPromptView {
    pub fn new() -> Self {
        Self::default()
    }

    fn max_index(&self) -> usize {
        PLAN_OPTIONS.len().saturating_sub(1)
    }

    fn submit_selected(&self) -> ViewAction {
        ViewAction::EmitAndClose(ViewEvent::PlanPromptSelected {
            option: self.selected + 1,
        })
    }

    fn submit_number(number: u32) -> ViewAction {
        if (1..=u32::try_from(PLAN_OPTIONS.len()).unwrap_or(0)).contains(&number) {
            ViewAction::EmitAndClose(ViewEvent::PlanPromptSelected {
                option: usize::try_from(number).unwrap_or(1),
            })
        } else {
            ViewAction::None
        }
    }
}

impl ModalView for PlanPromptView {
    fn kind(&self) -> ModalKind {
        ModalKind::PlanPrompt
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.max_index());
                ViewAction::None
            }
            KeyCode::Char('1') => {
                self.selected = 0;
                self.submit_selected()
            }
            KeyCode::Char('2') => {
                self.selected = 1;
                self.submit_selected()
            }
            KeyCode::Char('3') => {
                self.selected = 2;
                self.submit_selected()
            }
            KeyCode::Char('4') => {
                self.selected = 3;
                self.submit_selected()
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.selected = 0;
                self.submit_selected()
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.selected = 1;
                self.submit_selected()
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.selected = 2;
                self.submit_selected()
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Char('e') | KeyCode::Char('E') => {
                self.selected = 3;
                self.submit_selected()
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                let number = ch.to_digit(10).unwrap_or(0);
                Self::submit_number(number)
            }
            KeyCode::Enter => self.submit_selected(),
            KeyCode::Esc => ViewAction::Close,
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![Span::styled(
            "Plan ready. Confirm next step:",
            Style::default().fg(palette::TEXT_PRIMARY).bold(),
        )]));
        lines.push(Line::from(""));

        for (idx, (label, description)) in PLAN_OPTIONS.iter().enumerate() {
            let selected = self.selected == idx;
            let prefix = if selected { ">" } else { " " };
            let number = idx + 1;
            let style = if selected {
                Style::default()
                    .fg(palette::WAGMII_SKY)
                    .bg(palette::SELECTION_BG)
                    .bold()
            } else {
                Style::default().fg(palette::TEXT_PRIMARY)
            };
            lines.push(Line::from(vec![
                Span::raw(format!("{prefix} {number}) ")),
                Span::styled((*label).to_string(), style),
                Span::raw(" — "),
                Span::styled(
                    (*description).to_string(),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "1-4 / a / y / r / q = quick pick, Up/Down=select, Enter=confirm, Esc=close",
            Style::default().fg(palette::TEXT_MUTED),
        )));

        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(Line::from(vec![Span::styled(
                        " Plan Confirmation ",
                        Style::default().fg(palette::WAGMII_BLUE).bold(),
                    )]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::BORDER_COLOR))
                    .style(Style::default().bg(palette::WAGMII_INK))
                    .padding(Padding::uniform(1)),
            );

        let popup_area = centered_rect(66, 42, area);
        paragraph.render(popup_area, buf);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
