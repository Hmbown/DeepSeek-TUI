//! Session resume picker view for the TUI.

use std::collections::HashMap;

use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::palette;
use crate::session_manager::{SavedSession, SessionManager, SessionMetadata};
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};

#[derive(Debug, Clone, Copy)]
enum SortMode {
    Recent,
    Name,
    Size,
}

pub struct SessionPickerView {
    sessions: Vec<SessionMetadata>,
    filtered: Vec<SessionMetadata>,
    selected: usize,
    search_input: String,
    search_mode: bool,
    sort_mode: SortMode,
    preview_cache: HashMap<String, Vec<String>>,
    current_preview: Vec<String>,
    confirm_delete: bool,
    status: Option<String>,
}

impl SessionPickerView {
    pub fn new() -> Self {
        let sessions = SessionManager::default_location()
            .and_then(|manager| manager.list_sessions())
            .unwrap_or_default();

        let mut view = Self {
            sessions,
            filtered: Vec::new(),
            selected: 0,
            search_input: String::new(),
            search_mode: false,
            sort_mode: SortMode::Recent,
            preview_cache: HashMap::new(),
            current_preview: Vec::new(),
            confirm_delete: false,
            status: None,
        };
        view.apply_sort_and_filter();
        view.refresh_preview();
        view
    }

    fn apply_sort_and_filter(&mut self) {
        match self.sort_mode {
            SortMode::Recent => {
                self.sessions
                    .sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            }
            SortMode::Name => {
                self.sessions.sort_by(|a, b| a.title.cmp(&b.title));
            }
            SortMode::Size => {
                self.sessions
                    .sort_by(|a, b| b.message_count.cmp(&a.message_count));
            }
        }

        let query = self.search_input.trim().to_ascii_lowercase();
        if query.is_empty() {
            self.filtered = self.sessions.clone();
        } else {
            self.filtered = self
                .sessions
                .iter()
                .cloned()
                .filter(|session| fuzzy_match(&query, session))
                .collect();
        }

        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }

        self.refresh_preview();
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.filtered.len() as isize;
        let next = (self.selected as isize + delta).clamp(0, len - 1) as usize;
        self.selected = next;
        self.refresh_preview();
    }

    fn selected_session(&self) -> Option<&SessionMetadata> {
        self.filtered.get(self.selected)
    }

    fn cycle_sort(&mut self) {
        self.sort_mode = match self.sort_mode {
            SortMode::Recent => SortMode::Name,
            SortMode::Name => SortMode::Size,
            SortMode::Size => SortMode::Recent,
        };
        self.apply_sort_and_filter();
        self.status = Some(format!("Sort: {}", self.sort_label()));
    }

    fn sort_label(&self) -> &'static str {
        match self.sort_mode {
            SortMode::Recent => "recent",
            SortMode::Name => "name",
            SortMode::Size => "size",
        }
    }

    fn enter_search(&mut self) {
        self.search_mode = true;
        self.search_input.clear();
        self.status = Some("Search: type to filter, Enter to apply".to_string());
    }

    fn exit_search(&mut self) {
        self.search_mode = false;
        self.apply_sort_and_filter();
        self.status = None;
    }

    fn delete_selected(&mut self) -> Option<ViewEvent> {
        let Some(session) = self.selected_session().cloned() else {
            return None;
        };
        let manager = SessionManager::default_location().ok()?;
        if let Err(err) = manager.delete_session(&session.id) {
            self.status = Some(format!("Delete failed: {err}"));
            return None;
        }
        self.sessions.retain(|s| s.id != session.id);
        self.apply_sort_and_filter();
        self.refresh_preview();
        self.status = Some(format!("Deleted session {}", &session.id[..8]));
        Some(ViewEvent::SessionDeleted {
            session_id: session.id,
            title: session.title,
        })
    }

    fn refresh_preview(&mut self) {
        let Some(session) = self.selected_session() else {
            self.current_preview = vec!["No sessions found.".to_string()];
            return;
        };

        if let Some(lines) = self.preview_cache.get(&session.id) {
            self.current_preview = lines.clone();
            return;
        }

        let manager = match SessionManager::default_location() {
            Ok(manager) => manager,
            Err(_) => {
                self.current_preview = vec!["Failed to open sessions directory.".to_string()];
                return;
            }
        };

        let saved = match manager.load_session(&session.id) {
            Ok(saved) => saved,
            Err(_) => {
                self.current_preview = vec!["Failed to load session preview.".to_string()];
                return;
            }
        };

        let preview = build_preview_lines(&saved);
        self.preview_cache
            .insert(session.id.clone(), preview.clone());
        self.current_preview = preview;
    }
}

impl ModalView for SessionPickerView {
    fn kind(&self) -> ModalKind {
        ModalKind::SessionPicker
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        if self.search_mode {
            match key.code {
                KeyCode::Enter => {
                    self.exit_search();
                    return ViewAction::None;
                }
                KeyCode::Esc => {
                    self.exit_search();
                    return ViewAction::None;
                }
                KeyCode::Backspace => {
                    self.search_input.pop();
                    self.apply_sort_and_filter();
                    return ViewAction::None;
                }
                KeyCode::Char(c) => {
                    self.search_input.push(c);
                    self.apply_sort_and_filter();
                    return ViewAction::None;
                }
                _ => {}
            }
        }

        if self.confirm_delete {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm_delete = false;
                    if let Some(event) = self.delete_selected() {
                        return ViewAction::Emit(event);
                    }
                    return ViewAction::None;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.confirm_delete = false;
                    self.status = Some("Delete cancelled".to_string());
                    return ViewAction::None;
                }
                _ => return ViewAction::None,
            }
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ViewAction::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.move_selection(-5);
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.move_selection(5);
                ViewAction::None
            }
            KeyCode::Char('/') => {
                self.enter_search();
                ViewAction::None
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.cycle_sort();
                ViewAction::None
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                self.confirm_delete = true;
                self.status = Some("Delete session? (y/n)".to_string());
                ViewAction::None
            }
            KeyCode::Enter => {
                if let Some(session) = self.selected_session() {
                    ViewAction::EmitAndClose(ViewEvent::SessionSelected {
                        session_id: session.id.clone(),
                    })
                } else {
                    ViewAction::None
                }
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_area = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        Clear.render(popup_area, buf);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(popup_area);

        let list_lines = build_list_lines(
            &self.filtered,
            self.selected,
            popup_area.width,
            self.search_mode,
            &self.search_input,
            self.sort_label(),
            self.confirm_delete,
            self.status.as_deref(),
        );
        let list = Paragraph::new(list_lines)
            .block(
                Block::default()
                    .title(" Sessions ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::DEEPSEEK_SKY)),
            )
            .wrap(Wrap { trim: false });
        list.render(chunks[0], buf);

        let preview_lines = format_preview(
            &self.current_preview,
            chunks[1].width.saturating_sub(2),
            chunks[1].height as usize,
        );

        let preview = Paragraph::new(preview_lines)
            .block(
                Block::default()
                    .title(" Preview ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::DEEPSEEK_SKY)),
            )
            .wrap(Wrap { trim: false });
        preview.render(chunks[1], buf);
    }
}

fn build_list_lines(
    sessions: &[SessionMetadata],
    selected: usize,
    width: u16,
    search_mode: bool,
    search_input: &str,
    sort_label: &str,
    confirm_delete: bool,
    status: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let header = if search_mode {
        format!("/{}", search_input)
    } else {
        format!("Sort: {sort_label} | / search | s sort | d delete")
    };
    lines.push(Line::from(Span::styled(
        truncate(&header, width),
        Style::default().fg(palette::TEXT_MUTED),
    )));

    if confirm_delete {
        lines.push(Line::from(Span::styled(
            "Confirm delete (y/n)",
            Style::default()
                .fg(palette::STATUS_WARNING)
                .add_modifier(Modifier::BOLD),
        )));
    } else if let Some(status) = status {
        lines.push(Line::from(Span::styled(
            truncate(status, width),
            Style::default().fg(palette::DEEPSEEK_SKY),
        )));
    }

    if sessions.is_empty() {
        lines.push(Line::from(Span::styled(
            "No sessions available.",
            Style::default().fg(palette::TEXT_MUTED),
        )));
        return lines;
    }

    for (idx, session) in sessions.iter().enumerate() {
        let mut line = format_session_line(session);
        line = truncate(&line, width);
        let style = if idx == selected {
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(palette::TEXT_PRIMARY)
        };
        lines.push(Line::from(Span::styled(line, style)));
    }

    lines
}

fn format_session_line(session: &SessionMetadata) -> String {
    let updated = format_relative_time(&session.updated_at);
    let title = truncate(&session.title, 32);
    let mode = session
        .mode
        .as_deref()
        .unwrap_or("unknown")
        .to_ascii_lowercase();
    format!(
        "{} | {} | {} msgs | {} | {}",
        &session.id[..8],
        title,
        session.message_count,
        mode,
        updated
    )
}

fn build_preview_lines(session: &SavedSession) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("Title: {}", session.metadata.title));
    out.push(format!(
        "Updated: {}",
        session
            .metadata
            .updated_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
    ));
    out.push(format!(
        "Messages: {} | Model: {}",
        session.metadata.message_count, session.metadata.model
    ));
    if let Some(mode) = session.metadata.mode.as_deref() {
        out.push(format!("Mode: {}", mode));
    }
    out.push("".to_string());

    for message in session.messages.iter().take(6) {
        let role = message.role.to_ascii_uppercase();
        let mut text = String::new();
        for block in &message.content {
            if let crate::models::ContentBlock::Text { text: body, .. } = block {
                text.push_str(body);
            }
        }
        let preview = truncate(&text.replace('\n', " "), 120);
        out.push(format!("{role}: {preview}"));
    }
    out
}

fn format_preview(lines: &[String], width: u16, height: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let available = height.saturating_sub(2).max(1);
    for line in lines.iter().take(available) {
        out.push(Line::from(Span::styled(
            truncate(line, width),
            Style::default().fg(palette::TEXT_PRIMARY),
        )));
    }
    out
}

fn format_relative_time(dt: &DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(*dt);
    if duration.num_minutes() < 1 {
        "just now".to_string()
    } else if duration.num_hours() < 1 {
        format!("{}m ago", duration.num_minutes())
    } else if duration.num_days() < 1 {
        format!("{}h ago", duration.num_hours())
    } else {
        format!("{}d ago", duration.num_days())
    }
}

fn truncate(text: &str, width: u16) -> String {
    let max = width.max(1) as usize;
    if text.width() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut current = 0;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if current + w >= max.saturating_sub(3) {
            break;
        }
        out.push(ch);
        current += w;
    }
    out.push_str("...");
    out
}

fn fuzzy_match(query: &str, session: &SessionMetadata) -> bool {
    let haystack = format!(
        "{} {} {}",
        session.title,
        session.id,
        session.workspace.display()
    )
    .to_ascii_lowercase();
    if haystack.contains(query) {
        return true;
    }
    is_subsequence(query, &haystack)
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut chars = needle.chars();
    let mut current = match chars.next() {
        Some(c) => c,
        None => return true,
    };
    for ch in haystack.chars() {
        if ch == current {
            if let Some(next) = chars.next() {
                current = next;
            } else {
                return true;
            }
        }
    }
    false
}
