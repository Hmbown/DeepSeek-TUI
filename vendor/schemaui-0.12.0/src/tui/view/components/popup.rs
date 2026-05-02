use ratatui::{
    Frame,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};
use unicode_width::UnicodeWidthStr;

use crate::deepseek_palette::{ACCENT_SKY, BORDER_DIM, SURFACE_RAISED, TEXT_PRIMARY};

use super::super::frame::PopupRender;
use super::layout::popup_rect;

pub fn render_popup(frame: &mut Frame<'_>, popup: PopupRender<'_>) {
    if popup.options.is_empty() {
        return;
    }
    // Precompute rendered labels so width calculation matches actual content
    let rendered_labels: Vec<String> = popup
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            if popup.multi {
                let mark = popup
                    .active
                    .and_then(|flags| flags.get(index))
                    .copied()
                    .unwrap_or(false);
                format!("[{}] {}", if mark { "x" } else { " " }, option)
            } else {
                option.clone()
            }
        })
        .collect();

    let max_label_width = rendered_labels
        .iter()
        .map(|label| UnicodeWidthStr::width(label.as_str()))
        .max()
        .unwrap_or(10) as u16;
    let title_width = u16::try_from(UnicodeWidthStr::width(popup.title)).unwrap_or(10);
    let width_limit = frame.area().width.saturating_sub(2).max(1);
    // Extra padding for borders and highlight symbol
    let width = (max_label_width.saturating_add(8))
        .max(title_width.saturating_add(2))
        .min(width_limit);
    let height = popup
        .options
        .len()
        .saturating_add(4)
        .min(frame.area().height as usize) as u16;
    let area = popup_rect(frame.area(), width, height.max(3));
    frame.render_widget(Clear, area);

    let items: Vec<ListItem<'static>> = rendered_labels.into_iter().map(ListItem::new).collect();
    let mut state = ListState::default();
    let selected = popup.selected.min(popup.options.len().saturating_sub(1));
    state.select(Some(selected));

    let list = List::new(items)
        .block(
            Block::default()
                .title(popup.title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_DIM))
                .style(Style::default().bg(SURFACE_RAISED)),
        )
        .style(Style::default().fg(TEXT_PRIMARY).bg(SURFACE_RAISED))
        .highlight_style(
            Style::default()
                .fg(ACCENT_SKY)
                .bg(SURFACE_RAISED)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("» ");

    frame.render_stateful_widget(list, area, &mut state);
}
