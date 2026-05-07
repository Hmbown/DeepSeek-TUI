//! `/theme` live picker.
//!
//! Mirrors `/statusline`: ↑/↓ changes the highlighted theme and previews it
//! immediately, Enter saves to settings.toml, Esc restores the original theme.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
};

use crate::palette;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};

const THEME_HINTS: &[&str] = &[
    "use the default DeepSeek/Whale theme",
    "current blue DeepSeek look",
    "neutral dark theme",
    "light theme",
    "currently follows the built-in dark fallback",
];

pub struct ThemePickerView {
    cursor: usize,
    original: String,
}

impl ThemePickerView {
    #[must_use]
    pub fn new(active: &str) -> Self {
        let normalized = palette::normalized_theme_or_default(active);
        let cursor = palette::THEME_CHOICES
            .iter()
            .position(|name| *name == normalized)
            .unwrap_or(0);
        Self {
            cursor,
            original: normalized.to_string(),
        }
    }

    fn move_up(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    fn move_down(&mut self) -> bool {
        let max = palette::THEME_CHOICES.len().saturating_sub(1);
        if self.cursor >= max {
            return false;
        }
        self.cursor += 1;
        true
    }

    fn current_theme(&self) -> String {
        palette::THEME_CHOICES[self.cursor].to_string()
    }

    fn live_preview_event(&self) -> ViewEvent {
        ViewEvent::ThemeUpdated {
            theme: self.current_theme(),
            final_save: false,
        }
    }

    fn final_event(&self) -> ViewEvent {
        ViewEvent::ThemeUpdated {
            theme: self.current_theme(),
            final_save: true,
        }
    }

    fn revert_event(&self) -> ViewEvent {
        ViewEvent::ThemeUpdated {
            theme: self.original.clone(),
            final_save: false,
        }
    }
}

impl ModalView for ThemePickerView {
    fn kind(&self) -> ModalKind {
        ModalKind::ThemePicker
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => ViewAction::EmitAndClose(self.revert_event()),
            KeyCode::Enter => ViewAction::EmitAndClose(self.final_event()),
            KeyCode::Up | KeyCode::Char('k') => {
                if self.move_up() {
                    ViewAction::Emit(self.live_preview_event())
                } else {
                    ViewAction::None
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.move_down() {
                    ViewAction::Emit(self.live_preview_event())
                } else {
                    ViewAction::None
                }
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let margin_x = if area.width >= 48 { 2 } else { 0 };
        let margin_y = if area.height >= 12 { 2 } else { 0 };
        let max_width = area.width.saturating_sub(margin_x * 2).max(1);
        let max_height = area.height.saturating_sub(margin_y * 2).max(1);
        let popup_width = 68.min(max_width);
        let needed_height = (palette::THEME_CHOICES.len() as u16).saturating_add(5);
        let popup_height = needed_height.min(max_height);

        let popup_area = Rect {
            x: area.x + (area.width.saturating_sub(popup_width)) / 2,
            y: area.y + (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let block = Block::default()
            .title(Line::from(Span::styled(
                " Theme ",
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(vec![
                Span::styled(" ↑↓ ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("preview "),
                Span::styled(" Enter ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("save "),
                Span::styled(" Esc ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("cancel "),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_INK))
            .padding(Padding::uniform(1));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        let mut lines: Vec<Line> = Vec::with_capacity(palette::THEME_CHOICES.len() + 2);
        lines.push(Line::from(Span::styled(
            "Pick a theme. Moving the cursor previews it immediately:",
            Style::default().fg(palette::TEXT_MUTED),
        )));
        lines.push(Line::from(""));

        for (idx, name) in palette::THEME_CHOICES.iter().enumerate() {
            let hint = THEME_HINTS.get(idx).copied().unwrap_or("");
            let is_cursor = idx == self.cursor;
            let row_style = if is_cursor {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette::TEXT_PRIMARY)
            };
            let hint_style = if is_cursor {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
            } else {
                Style::default().fg(palette::TEXT_DIM)
            };
            let pointer = if is_cursor { "▸" } else { " " };
            lines.push(Line::from(vec![
                Span::styled(format!(" {pointer} "), row_style),
                Span::styled((*name).to_string(), row_style),
                Span::raw("  "),
                Span::styled(format!("({hint})"), hint_style),
            ]));
        }

        Paragraph::new(lines).render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn arrow_keys_emit_live_preview() {
        let mut view = ThemePickerView::new("default");
        match view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)) {
            ViewAction::Emit(ViewEvent::ThemeUpdated { theme, final_save }) => {
                assert_eq!(theme, "whale");
                assert!(!final_save);
            }
            other => panic!("expected live preview, got {other:?}"),
        }
    }

    #[test]
    fn enter_emits_final_save() {
        let mut view = ThemePickerView::new("default");
        match view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            ViewAction::EmitAndClose(ViewEvent::ThemeUpdated { final_save, .. }) => {
                assert!(final_save);
            }
            other => panic!("expected final save, got {other:?}"),
        }
    }

    #[test]
    fn esc_reverts_original_theme() {
        let mut view = ThemePickerView::new("system");
        let _ = view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        match view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)) {
            ViewAction::EmitAndClose(ViewEvent::ThemeUpdated { theme, final_save }) => {
                assert_eq!(theme, "system");
                assert!(!final_save);
            }
            other => panic!("expected revert, got {other:?}"),
        }
    }

    #[test]
    fn boundary_keys_do_not_emit_when_cursor_does_not_move() {
        let mut view = ThemePickerView::new("default");
        assert!(matches!(
            view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            ViewAction::None
        ));
    }

    #[test]
    fn tiny_render_area_does_not_overflow() {
        let view = ThemePickerView::new("default");
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
        view.render(Rect::new(0, 0, 12, 4), &mut buf);
    }
}
