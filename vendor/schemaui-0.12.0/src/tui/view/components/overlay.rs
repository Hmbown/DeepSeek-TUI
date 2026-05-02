use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::deepseek_palette::{ACCENT_SKY, BORDER_DIM, STATUS_WARN, SURFACE_RAISED, TEXT_MUTED};
use crate::tui::state::FormState;

use super::super::frame::CompositeOverlay;
use super::{body::render_body, layout::popup_rect, tabstrip::render_tab_strip};

pub fn render_composite_overlay(
    frame: &mut Frame<'_>,
    overlay: &CompositeOverlay,
    overlay_form: &mut FormState,
) {
    let base = frame.area();
    let width = base.width.saturating_sub(base.width / 4).max(40);
    let height = base.height.saturating_sub(base.height / 5).max(12);
    let area = popup_rect(base, width, height);
    frame.render_widget(Clear, area);

    let mut block_title = format!("Overlay {} – {}", overlay.level, overlay.title);
    if overlay.dirty {
        block_title.push_str("  • DIRTY");
    }
    let block = Block::default()
        .title(block_title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .title_style(
            Style::default()
                .fg(ACCENT_SKY)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(SURFACE_RAISED));
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let mut constraints = Vec::new();
    if overlay.list_entries.is_some() {
        constraints.push(Constraint::Length(3));
    }
    constraints.push(Constraint::Min(5));
    constraints.push(Constraint::Length(2));

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut next = 0usize;
    if let Some(entries) = &overlay.list_entries {
        let label = overlay.entry_label.as_deref().unwrap_or("Entries");
        render_tab_strip(
            frame,
            layout[next],
            entries,
            overlay.list_selected.unwrap_or(0),
            label,
        );
        next += 1;
    }

    let body_area = layout[next];
    let layout_desc = layout_section_description(overlay_form);
    let combined_description = if overlay.description.is_some() || layout_desc.is_some() {
        let mut text = String::new();
        if let Some(desc) = &overlay.description {
            text.push_str(desc);
        }
        if let Some(layout) = layout_desc {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&layout);
        }
        Some(text)
    } else {
        None
    };

    if let Some(description) = combined_description {
        let sub = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Min(1)])
            .split(body_area);
        let desc = Paragraph::new(description)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(TEXT_MUTED).bg(SURFACE_RAISED));
        frame.render_widget(desc, sub[0]);
        render_body(frame, sub[1], overlay_form, true);
    } else {
        render_body(frame, body_area, overlay_form, true);
    }

    let footer_area = layout.last().copied().unwrap_or(body_area);
    let footer = Paragraph::new(overlay.instructions.clone())
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(STATUS_WARN).bg(SURFACE_RAISED))
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(BORDER_DIM))
                .title("Overlay Controls")
                .style(Style::default().bg(SURFACE_RAISED)),
        );
    frame.render_widget(footer, footer_area);
}

pub(crate) fn layout_section_description(form: &FormState) -> Option<String> {
    let nav = form.layout_nav()?;
    let pointer = form
        .focused_field()
        .map(|field| field.schema.pointer.clone())?;

    for root in &nav.roots {
        for section in &root.sections {
            if section.pointers.iter().any(|p| p == &pointer) {
                return Some(format!("Section: {} - {}", root.title, section.title));
            }
        }
    }

    None
}
