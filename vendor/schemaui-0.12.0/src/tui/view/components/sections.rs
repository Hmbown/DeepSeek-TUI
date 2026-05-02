use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders},
};

use crate::deepseek_palette::{BORDER_DIM, SURFACE_INK};

use super::tabstrip::render_tab_strip;

#[derive(Debug, Clone)]
pub struct RootTabsView {
    pub titles: Vec<String>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct SectionTabsView {
    pub titles: Vec<String>,
    pub selected: usize,
    pub label: String,
}

pub fn render_root_tabs(frame: &mut Frame<'_>, area: Rect, view: &RootTabsView) {
    render_tab_strip(frame, area, &view.titles, view.selected, "Root Sections");
}

pub fn render_section_tabs(frame: &mut Frame<'_>, area: Rect, view: &SectionTabsView) {
    if view.titles.is_empty() {
        let placeholder = Block::default()
            .title("Sections")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_DIM))
            .style(Style::default().bg(SURFACE_INK));
        frame.render_widget(placeholder, area);
        return;
    }
    render_tab_strip(frame, area, &view.titles, view.selected, &view.label);
}
