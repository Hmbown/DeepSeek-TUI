//! Command palette modal for quick command/skill insertion.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::Stylize,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::commands;
use crate::palette;
use crate::skills::SkillRegistry;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};

#[derive(Debug, Clone)]
pub struct CommandPaletteEntry {
    pub label: String,
    pub description: String,
    pub command: String,
}

pub struct CommandPaletteView {
    entries: Vec<CommandPaletteEntry>,
    filtered: Vec<usize>,
    query: String,
    selected: usize,
}

pub fn build_entries(skills_dir: &Path) -> Vec<CommandPaletteEntry> {
    let mut entries = Vec::new();

    for command in commands::COMMANDS {
        let requires_args = command.usage.contains('<');
        let command_text = if requires_args {
            format!("/{} ", command.name)
        } else {
            format!("/{}", command.name)
        };
        entries.push(CommandPaletteEntry {
            label: format!("/{}", command.name),
            description: command.description.to_string(),
            command: command_text,
        });
    }

    let skills = SkillRegistry::discover(skills_dir);
    for skill in skills.list() {
        entries.push(CommandPaletteEntry {
            label: format!("skill:{}", skill.name),
            description: skill.description.clone(),
            command: format!("/skill {}", skill.name),
        });
    }

    entries.sort_by(|a, b| a.label.cmp(&b.label));
    entries
}

impl CommandPaletteView {
    pub fn new(entries: Vec<CommandPaletteEntry>) -> Self {
        let mut view = Self {
            entries,
            filtered: Vec::new(),
            query: String::new(),
            selected: 0,
        };
        view.refilter();
        view
    }

    fn refilter(&mut self) {
        let query = self.query.trim().to_ascii_lowercase();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                if query.is_empty()
                    || entry.label.to_ascii_lowercase().contains(&query)
                    || entry.description.to_ascii_lowercase().contains(&query)
                    || entry.command.to_ascii_lowercase().contains(&query)
                {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();

        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.filtered.len() as isize;
        let next = (self.selected as isize + delta).clamp(0, len - 1) as usize;
        self.selected = next;
    }

    fn selected_entry(&self) -> Option<&CommandPaletteEntry> {
        self.filtered
            .get(self.selected)
            .and_then(|idx| self.entries.get(*idx))
    }
}

impl ModalView for CommandPaletteView {
    fn kind(&self) -> ModalKind {
        ModalKind::CommandPalette
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => ViewAction::Close,
            KeyCode::Enter => {
                if let Some(entry) = self.selected_entry() {
                    ViewAction::EmitAndClose(ViewEvent::CommandPaletteSelected {
                        command: entry.command.clone(),
                    })
                } else {
                    ViewAction::None
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.move_selection(-8);
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.move_selection(8);
                ViewAction::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter();
                ViewAction::None
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.query.push(c);
                self.refilter();
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = 90.min(area.width.saturating_sub(4));
        let popup_height = 22.min(area.height.saturating_sub(4));
        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let mut lines = Vec::new();
        let query_label = if self.query.is_empty() {
            "Type to filter…".to_string()
        } else {
            format!("Filter: {}", self.query)
        };
        lines.push(Line::from(Span::styled(
            query_label,
            Style::default().fg(palette::TEXT_MUTED),
        )));
        lines.push(Line::from(""));

        let visible = popup_height.saturating_sub(5) as usize;
        if self.filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "No matches.",
                Style::default().fg(palette::TEXT_MUTED).italic(),
            )));
        } else {
            let start = self.selected.saturating_sub(visible.saturating_sub(1));
            let end = (start + visible).min(self.filtered.len());
            for (slot, idx) in self.filtered[start..end].iter().enumerate() {
                let absolute = start + slot;
                let is_selected = absolute == self.selected;
                let entry = &self.entries[*idx];
                let style = if is_selected {
                    Style::default()
                        .fg(palette::DEEPSEEK_SKY)
                        .bg(palette::SELECTION_BG)
                } else {
                    Style::default().fg(palette::TEXT_PRIMARY)
                };

                let mut line = format!("{:<24}", entry.label);
                let desc = if entry.description.width() > 56 {
                    let mut shortened = String::new();
                    for ch in entry.description.chars() {
                        if shortened.width() >= 53 {
                            break;
                        }
                        shortened.push(ch);
                    }
                    format!("{shortened}...")
                } else {
                    entry.description.clone()
                };
                line.push_str("  ");
                line.push_str(&desc);
                lines.push(Line::from(Span::styled(line, style)));
            }
        }

        let block = Block::default()
            .title(" Command Palette ")
            .title_bottom(Line::from(vec![
                Span::styled(" Enter insert  ", Style::default().fg(palette::TEXT_MUTED)),
                Span::styled("Esc close", Style::default().fg(palette::TEXT_MUTED)),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR));

        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(popup_area, buf);
    }
}
