//! Modal for request_user_input tool prompts.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};

use crate::palette;
use crate::tools::user_input::{
    UserInputAnswer, UserInputQuestion, UserInputRequest, UserInputResponse,
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Selecting,
    OtherInput,
}

#[derive(Debug, Clone)]
pub struct UserInputView {
    tool_id: String,
    request: UserInputRequest,
    question_index: usize,
    selected: usize,
    mode: InputMode,
    other_input: String,
    answers: Vec<UserInputAnswer>,
}

impl UserInputView {
    pub fn new(tool_id: impl Into<String>, request: UserInputRequest) -> Self {
        Self {
            tool_id: tool_id.into(),
            request,
            question_index: 0,
            selected: 0,
            mode: InputMode::Selecting,
            other_input: String::new(),
            answers: Vec::new(),
        }
    }

    fn current_question(&self) -> &UserInputQuestion {
        &self.request.questions[self.question_index]
    }

    fn option_count(&self) -> usize {
        self.current_question().options.len() + 1
    }

    fn is_other_selected(&self) -> bool {
        self.selected + 1 == self.option_count()
    }

    fn advance_question(&mut self, answer: UserInputAnswer) -> ViewAction {
        self.answers.push(answer);
        if self.question_index + 1 >= self.request.questions.len() {
            let response = UserInputResponse {
                answers: self.answers.clone(),
            };
            return ViewAction::EmitAndClose(ViewEvent::UserInputSubmitted {
                tool_id: self.tool_id.clone(),
                response,
            });
        }
        self.question_index += 1;
        self.selected = 0;
        self.mode = InputMode::Selecting;
        self.other_input.clear();
        ViewAction::None
    }

    fn handle_selecting_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.option_count().saturating_sub(1));
                ViewAction::None
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                let Some(number) = ch.to_digit(10) else {
                    return ViewAction::None;
                };
                if number == 0 {
                    return ViewAction::None;
                }
                let index = usize::try_from(number - 1).unwrap_or(usize::MAX);
                if index >= self.option_count() {
                    return ViewAction::None;
                }
                self.selected = index;
                if self.is_other_selected() {
                    self.mode = InputMode::OtherInput;
                    self.other_input.clear();
                    ViewAction::None
                } else {
                    let question = self.current_question();
                    let option = &question.options[self.selected];
                    let answer = UserInputAnswer {
                        id: question.id.clone(),
                        label: option.label.clone(),
                        value: option.label.clone(),
                    };
                    self.advance_question(answer)
                }
            }
            KeyCode::Enter => {
                if self.is_other_selected() {
                    self.mode = InputMode::OtherInput;
                    self.other_input.clear();
                    ViewAction::None
                } else {
                    let question = self.current_question();
                    let option = &question.options[self.selected];
                    let answer = UserInputAnswer {
                        id: question.id.clone(),
                        label: option.label.clone(),
                        value: option.label.clone(),
                    };
                    self.advance_question(answer)
                }
            }
            KeyCode::Esc => ViewAction::EmitAndClose(ViewEvent::UserInputCancelled {
                tool_id: self.tool_id.clone(),
            }),
            _ => ViewAction::None,
        }
    }

    fn handle_other_input_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Selecting;
                self.other_input.clear();
                ViewAction::None
            }
            KeyCode::Enter => {
                let question = self.current_question();
                let answer = UserInputAnswer {
                    id: question.id.clone(),
                    label: "Other".to_string(),
                    value: self.other_input.trim().to_string(),
                };
                self.advance_question(answer)
            }
            KeyCode::Backspace => {
                self.other_input.pop();
                ViewAction::None
            }
            KeyCode::Char(ch) => {
                if !ch.is_control() {
                    self.other_input.push(ch);
                }
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }
}

impl ModalView for UserInputView {
    fn kind(&self) -> ModalKind {
        ModalKind::UserInput
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match self.mode {
            InputMode::Selecting => self.handle_selecting_key(key),
            InputMode::OtherInput => self.handle_other_input_key(key),
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let question = self.current_question();
        let total = self.request.questions.len();
        let header = format!(
            " {} ({}/{}) ",
            question.header,
            self.question_index + 1,
            total
        );

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![Span::styled(
            question.question.clone(),
            Style::default().fg(palette::TEXT_PRIMARY).bold(),
        )]));
        lines.push(Line::from(""));

        for (idx, option) in question.options.iter().enumerate() {
            let selected = self.selected == idx;
            let prefix = if selected { ">" } else { " " };
            let number = idx + 1;
            if selected {
                // Single span with consistent foreground and background
                let content = format!("{prefix} {number}) {} - {}", option.label, option.description);
                lines.push(Line::from(Span::styled(
                    content,
                    Style::default()
                        .fg(palette::DEEPSEEK_SKY)
                        .bg(palette::SELECTION_BG)
                        .bold(),
                )));
            } else {
                // Keep original multi‑span formatting
                lines.push(Line::from(vec![
                    Span::raw(format!("{prefix} {number}) ")),
                    Span::styled(option.label.clone(), Style::default().fg(palette::TEXT_PRIMARY)),
                    Span::raw(" - "),
                    Span::styled(
                        option.description.clone(),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                ]));
            }
        }

        let other_index = question.options.len();
        let other_selected = self.selected == other_index;
        let other_number = other_index + 1;
        if other_selected {
            let content = format!("> {other_number}) Other - Provide a custom response");
            lines.push(Line::from(Span::styled(
                content,
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .bg(palette::SELECTION_BG)
                    .bold(),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::raw(format!("  {other_number}) ")),
                Span::styled("Other", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::raw(" - "),
                Span::styled(
                    "Provide a custom response",
                    Style::default().fg(palette::TEXT_MUTED),
                ),
            ]));
        }

        if self.mode == InputMode::OtherInput {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Other:", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::raw(" "),
                Span::styled(
                    if self.other_input.is_empty() {
                        "(type your response)".to_string()
                    } else {
                        self.other_input.clone()
                    },
                    Style::default().fg(palette::DEEPSEEK_BLUE),
                ),
            ]));
        }

        lines.push(Line::from(""));
        let hint = if self.mode == InputMode::OtherInput {
            "Enter=submit, Esc=back"
        } else {
            "Number keys=quick pick, Up/Down=select, Enter=confirm, Esc=cancel"
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(palette::TEXT_MUTED),
        )));

        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true })
            .block(modal_block(&header));

        let popup_area = centered_rect(80, 60, area);
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
