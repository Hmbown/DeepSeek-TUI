//! `/statusline` modal: multi-select picker for footer status items (#95).
//!
//! The picker is a single-pane checklist over [`StatusItem::all`]. The user
//! navigates with `↑↓`, toggles with `Space`, picks all/none with `a` / `n`,
//! and confirms with `Enter` (or `Esc` to cancel). On apply we emit a
//! [`ViewEvent::StatusItemsApplied`] carrying the new ordered selection;
//! the UI handler updates `App.status_items`, persists to
//! `~/.config/deepseek/settings.toml`, and the next redraw picks up the
//! new footer composition.
//!
//! `Mode` and `Model` are listed so users can see them, but their
//! checkmarks are locked — toggling them would silently drop information
//! every footer needs.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::palette;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};
use crate::tui::widgets::StatusItem;

pub struct StatusPickerView {
    /// Every selectable row in fixed order. The picker doesn't reorder
    /// items — toggling preserves the user's perception of "this list is
    /// the catalog of available chips."
    rows: Vec<StatusItem>,
    /// `selected[i]` is true when `rows[i]` is enabled. Locked rows
    /// (Mode/Model) are always true and can't be toggled off.
    selected: Vec<bool>,
    cursor: usize,
}

impl StatusPickerView {
    /// Build a picker pre-populated with the user's current selection.
    /// Items not in `current` start unchecked; locked items are always on.
    #[must_use]
    pub fn new(current: &[StatusItem]) -> Self {
        let rows: Vec<StatusItem> = StatusItem::all().to_vec();
        let selected = rows
            .iter()
            .map(|item| item.always_on() || current.contains(item))
            .collect();
        Self {
            rows,
            selected,
            cursor: 0,
        }
    }

    /// Resolve the picker state into the ordered list of enabled items
    /// (in canonical [`StatusItem::all`] order).
    #[must_use]
    pub fn enabled_items(&self) -> Vec<StatusItem> {
        self.rows
            .iter()
            .zip(&self.selected)
            .filter_map(|(item, on)| if *on { Some(*item) } else { None })
            .collect()
    }

    fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn move_down(&mut self) {
        let max = self.rows.len().saturating_sub(1);
        if self.cursor < max {
            self.cursor += 1;
        }
    }

    fn toggle(&mut self) {
        if let Some(item) = self.rows.get(self.cursor)
            && !item.always_on()
            && let Some(slot) = self.selected.get_mut(self.cursor)
        {
            *slot = !*slot;
        }
    }

    fn select_all(&mut self) {
        for slot in &mut self.selected {
            *slot = true;
        }
    }

    fn select_none(&mut self) {
        for (item, slot) in self.rows.iter().zip(self.selected.iter_mut()) {
            *slot = item.always_on();
        }
    }

    fn build_event(&self) -> ViewEvent {
        ViewEvent::StatusItemsApplied {
            items: self.enabled_items(),
        }
    }
}

impl ModalView for StatusPickerView {
    fn kind(&self) -> ModalKind {
        ModalKind::StatusPicker
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        // Ctrl+C / Ctrl+D act as cancel mirrors of Esc — terminal users
        // tend to reach for Ctrl+C first.
        if (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return ViewAction::Close;
        }

        match key.code {
            KeyCode::Esc => ViewAction::Close,
            KeyCode::Up => {
                self.move_up();
                ViewAction::None
            }
            KeyCode::Down => {
                self.move_down();
                ViewAction::None
            }
            KeyCode::Char(' ') => {
                self.toggle();
                ViewAction::None
            }
            KeyCode::Enter => {
                // Enter confirms — toggle behaviour was overloaded onto
                // Space alone so confirm works from any row, including
                // locked ones. Codex's status_line_setup follows the same
                // convention.
                ViewAction::EmitAndClose(self.build_event())
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.select_all();
                ViewAction::None
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.select_none();
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = 64.min(area.width.saturating_sub(4)).max(40);
        let popup_height = (self.rows.len() as u16 + 6)
            .min(area.height.saturating_sub(4))
            .max(10);
        let popup_area = Rect {
            x: area.x + (area.width.saturating_sub(popup_width)) / 2,
            y: area.y + (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let outer = Block::default()
            .title(Line::from(Span::styled(
                " Status line ",
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(vec![
                Span::styled(
                    " \u{2191}\u{2193} ",
                    Style::default().fg(palette::TEXT_MUTED),
                ),
                Span::raw("move "),
                Span::styled(" Space ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("toggle "),
                Span::styled(" a/n ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("all/none "),
                Span::styled(" Enter ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("apply "),
                Span::styled(" Esc ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw("cancel"),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::DEEPSEEK_SKY))
            .style(Style::default().bg(palette::DEEPSEEK_INK));
        let inner = outer.inner(popup_area);
        outer.render(popup_area, buf);

        let mut lines: Vec<Line<'static>> = Vec::with_capacity(self.rows.len());
        for (idx, item) in self.rows.iter().enumerate() {
            let is_selected_row = idx == self.cursor;
            let on = self.selected[idx];
            let locked = item.always_on();

            let checkbox = if on { "[x]" } else { "[ ]" };
            let checkbox_color = if locked {
                palette::TEXT_DIM
            } else if on {
                palette::DEEPSEEK_SKY
            } else {
                palette::TEXT_MUTED
            };

            let label_style = if is_selected_row {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette::TEXT_PRIMARY)
            };
            let desc_style = if is_selected_row {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
            } else {
                Style::default().fg(palette::TEXT_MUTED)
            };

            let marker = if is_selected_row { "▸" } else { " " };
            let mut spans = vec![
                Span::raw(" "),
                Span::styled(marker, label_style),
                Span::raw(" "),
                Span::styled(checkbox.to_string(), Style::default().fg(checkbox_color)),
                Span::raw(" "),
                Span::styled(item.label().to_string(), label_style),
            ];
            if locked {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    "(locked)".to_string(),
                    Style::default().fg(palette::TEXT_DIM),
                ));
            }
            spans.push(Span::raw("  "));
            spans.push(Span::styled(item.description().to_string(), desc_style));
            lines.push(Line::from(spans));
        }

        Paragraph::new(lines).render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_char(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn new_picker_seeds_from_current_selection() {
        let current = vec![StatusItem::SessionCost, StatusItem::Agents];
        let picker = StatusPickerView::new(&current);
        let enabled = picker.enabled_items();
        // Locked rows are always present even if they weren't in `current`.
        assert!(enabled.contains(&StatusItem::Mode));
        assert!(enabled.contains(&StatusItem::Model));
        assert!(enabled.contains(&StatusItem::SessionCost));
        assert!(enabled.contains(&StatusItem::Agents));
        assert!(!enabled.contains(&StatusItem::GitBranch));
    }

    #[test]
    fn space_toggles_unlocked_row() {
        let mut picker = StatusPickerView::new(&[]);
        // Cursor starts on row 0 (Mode — locked). Move past locked rows.
        picker.handle_key(key(KeyCode::Down));
        picker.handle_key(key(KeyCode::Down));
        let item_at_cursor = picker.rows[picker.cursor];
        assert!(!item_at_cursor.always_on(), "row 2 should be unlocked");
        let was_on = picker.selected[picker.cursor];
        picker.handle_key(key_char(' '));
        assert_ne!(picker.selected[picker.cursor], was_on);
    }

    #[test]
    fn space_does_not_toggle_locked_row() {
        let mut picker = StatusPickerView::new(&[]);
        // Find a locked row.
        let locked_idx = picker
            .rows
            .iter()
            .position(|i| i.always_on())
            .expect("at least one locked row");
        picker.cursor = locked_idx;
        picker.handle_key(key_char(' '));
        assert!(
            picker.selected[locked_idx],
            "locked rows must remain enabled"
        );
    }

    #[test]
    fn select_all_then_none_keeps_locked_on() {
        let mut picker = StatusPickerView::new(&[]);
        picker.handle_key(key_char('a'));
        assert!(picker.selected.iter().all(|on| *on));
        picker.handle_key(key_char('n'));
        for (item, on) in picker.rows.iter().zip(&picker.selected) {
            if item.always_on() {
                assert!(*on, "{} must remain on after 'n'", item.id());
            } else {
                assert!(!*on, "{} must be off after 'n'", item.id());
            }
        }
    }

    #[test]
    fn enter_emits_apply_event_with_current_state() {
        let mut picker = StatusPickerView::new(&[StatusItem::SessionCost]);
        let action = picker.handle_key(key(KeyCode::Enter));
        match action {
            ViewAction::EmitAndClose(ViewEvent::StatusItemsApplied { items }) => {
                assert!(items.contains(&StatusItem::Mode));
                assert!(items.contains(&StatusItem::Model));
                assert!(items.contains(&StatusItem::SessionCost));
            }
            other => panic!("expected EmitAndClose with StatusItemsApplied, got {other:?}"),
        }
    }

    #[test]
    fn esc_closes_without_emitting() {
        let mut picker = StatusPickerView::new(&[]);
        match picker.handle_key(key(KeyCode::Esc)) {
            ViewAction::Close => {}
            other => panic!("expected Close, got {other:?}"),
        }
    }
}
