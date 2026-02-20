//! Full-screen pager overlay for long outputs.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::palette;
use crate::tui::views::{ModalKind, ModalView, ViewAction};

pub struct PagerView {
    title: String,
    lines: Vec<Line<'static>>,
    plain_lines: Vec<String>,
    scroll: usize,
    search_input: String,
    search_matches: Vec<usize>,
    search_index: usize,
    search_mode: bool,
    pending_g: bool,
}

impl PagerView {
    pub fn new(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        let plain_lines = lines.iter().map(line_to_string).collect();
        Self {
            title: title.into(),
            lines,
            plain_lines,
            scroll: 0,
            search_input: String::new(),
            search_matches: Vec::new(),
            search_index: 0,
            search_mode: false,
            pending_g: false,
        }
    }

    pub fn from_text(title: impl Into<String>, text: &str, width: u16) -> Self {
        let mut lines = Vec::new();
        for raw in text.lines() {
            for wrapped in wrap_text(raw, width.max(1) as usize) {
                lines.push(Line::from(Span::raw(wrapped)));
            }
            if raw.is_empty() {
                lines.push(Line::from(""));
            }
        }
        Self::new(title, lines)
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    fn scroll_down(&mut self, amount: usize, max_scroll: usize) {
        self.scroll = (self.scroll + amount).min(max_scroll);
    }

    fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    fn scroll_to_bottom(&mut self, max_scroll: usize) {
        self.scroll = max_scroll;
    }

    fn start_search(&mut self) {
        self.search_mode = true;
        self.search_input.clear();
        self.search_matches.clear();
        self.search_index = 0;
    }

    fn update_search_matches(&mut self) {
        let query = self.search_input.trim();
        if query.is_empty() {
            self.search_matches.clear();
            self.search_index = 0;
            return;
        }
        let lower = query.to_ascii_lowercase();
        self.search_matches = self
            .plain_lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| {
                if line.to_ascii_lowercase().contains(&lower) {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();
        self.search_index = 0;
    }

    fn jump_to_match(&mut self) {
        if let Some(&line) = self.search_matches.get(self.search_index) {
            self.scroll = line;
        }
    }

    fn next_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_index = (self.search_index + 1) % self.search_matches.len();
        self.jump_to_match();
    }

    fn prev_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        if self.search_index == 0 {
            self.search_index = self.search_matches.len().saturating_sub(1);
        } else {
            self.search_index = self.search_index.saturating_sub(1);
        }
        self.jump_to_match();
    }
}

impl ModalView for PagerView {
    fn kind(&self) -> ModalKind {
        ModalKind::Pager
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        if self.search_mode {
            match key.code {
                KeyCode::Enter => {
                    self.search_mode = false;
                    self.update_search_matches();
                    self.jump_to_match();
                    return ViewAction::None;
                }
                KeyCode::Esc => {
                    self.search_mode = false;
                    return ViewAction::None;
                }
                KeyCode::Backspace => {
                    self.search_input.pop();
                    return ViewAction::None;
                }
                KeyCode::Char(c) => {
                    self.search_input.push(c);
                    return ViewAction::None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ViewAction::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(1);
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(1, self.lines.len().saturating_sub(1));
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.scroll_up(10);
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(10, self.lines.len().saturating_sub(1));
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Char('g') => {
                if self.pending_g {
                    self.scroll_to_top();
                    self.pending_g = false;
                } else {
                    self.pending_g = true;
                }
                ViewAction::None
            }
            KeyCode::Char('G') => {
                self.scroll_to_bottom(self.lines.len().saturating_sub(1));
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Char('/') => {
                self.start_search();
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Char('n') => {
                self.next_match();
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Char('N') => {
                self.prev_match();
                self.pending_g = false;
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = area.width.saturating_sub(2).max(1);
        let popup_height = area.height.saturating_sub(2).max(1);
        let popup_area = Rect {
            x: 1,
            y: 1,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let mut visible_height = popup_area.height.saturating_sub(2) as usize;
        if self.search_mode {
            visible_height = visible_height.saturating_sub(1);
        }
        let max_scroll = self.lines.len().saturating_sub(visible_height);
        let scroll = self.scroll.min(max_scroll);
        let end = (scroll + visible_height).min(self.lines.len());
        let mut visible_lines = if self.lines.is_empty() {
            vec![Line::from("")]
        } else {
            self.lines[scroll..end].to_vec()
        };

        if self.search_mode {
            let prompt = format!("/{}", self.search_input);
            visible_lines.push(Line::from(Span::styled(
                prompt,
                Style::default()
                    .fg(palette::WAGMII_SKY)
                    .add_modifier(Modifier::BOLD),
            )));
        } else if !self.search_matches.is_empty() {
            let status = format!(
                "match {}/{} (n/N)",
                self.search_index + 1,
                self.search_matches.len()
            );
            visible_lines.push(Line::from(Span::styled(
                status,
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }

        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::WAGMII_INK))
            .padding(Padding::uniform(1));

        let paragraph = Paragraph::new(visible_lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(popup_area, buf);
    }
}

fn line_to_string(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>()
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = word.width();
        let additional = if current.is_empty() {
            word_width
        } else {
            word_width + 1
        };
        if current_width + additional > width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
            current_width = word_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }

    if current.is_empty() {
        lines.push(String::new());
    } else {
        lines.push(current);
    }

    lines
}
