use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph},
};

use crate::deepseek_palette::{BORDER_DIM, SURFACE_INK, TEXT_MUTED};
use crate::tui::state::FormState;

use super::{
    fields::render_fields,
    sections::{RootTabsView, SectionTabsView, render_root_tabs, render_section_tabs},
};

pub fn render_body(
    frame: &mut Frame<'_>,
    area: Rect,
    form_state: &mut FormState,
    enable_cursor: bool,
) {
    if form_state.is_empty() {
        let placeholder = Paragraph::new("No editable fields in schema")
            .style(Style::default().fg(TEXT_MUTED).bg(SURFACE_INK))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_DIM))
                    .style(Style::default().bg(SURFACE_INK)),
            );
        frame.render_widget(placeholder, area);
        return;
    }

    let root_tabs_view = build_root_tabs_view(form_state);
    let show_root_tabs = root_tabs_view
        .as_ref()
        .map(|view| view.titles.len() > 1)
        .unwrap_or(false);
    let section_tabs_view = build_section_tabs_view(form_state);
    let show_section_tabs = section_tabs_view
        .as_ref()
        .map(|view| view.titles.len() > 1)
        .unwrap_or(false);

    let mut constraints = Vec::new();
    if show_root_tabs {
        constraints.push(Constraint::Length(3));
    }
    if show_section_tabs {
        constraints.push(Constraint::Length(3));
    }
    constraints.push(Constraint::Min(1));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut index = 0;
    if show_root_tabs {
        if let Some(view) = root_tabs_view.as_ref() {
            render_root_tabs(frame, chunks[index], view);
        }
        index += 1;
    }
    if show_section_tabs {
        if let Some(view) = section_tabs_view.as_ref() {
            render_section_tabs(frame, chunks[index], view);
        }
        index += 1;
    }
    render_fields(frame, chunks[index], form_state, enable_cursor);
}

fn build_root_tabs_view(form_state: &FormState) -> Option<RootTabsView> {
    if form_state.roots.is_empty() {
        return None;
    }
    Some(RootTabsView {
        titles: form_state
            .roots
            .iter()
            .map(|root| root.title.clone())
            .collect(),
        selected: form_state.root_index(),
    })
}

fn build_section_tabs_view(form_state: &FormState) -> Option<SectionTabsView> {
    let root = form_state.active_root()?;
    Some(SectionTabsView {
        titles: root
            .sections
            .iter()
            .map(|section| {
                if section.depth == 0 {
                    section.title.clone()
                } else {
                    format!("{}{}", ">".repeat(section.depth), section.title)
                }
            })
            .collect(),
        selected: form_state.section_index(),
        label: format!("{} Sections", root.title),
    })
}
