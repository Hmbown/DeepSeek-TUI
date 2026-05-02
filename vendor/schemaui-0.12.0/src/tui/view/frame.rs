use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::Block,
};

use crate::deepseek_palette::SURFACE_INK;
use crate::tui::state::FormState;

use super::components::{
    render_body, render_composite_overlay, render_footer, render_help_overlay, render_popup,
};

pub struct UiContext<'a> {
    pub status_message: &'a str,
    pub dirty: bool,
    pub error_count: usize,
    pub help: Option<&'a str>,
    pub global_errors: &'a [String],
    pub focus_label: Option<String>,
    pub session_title: Option<&'a str>,
    pub popup: Option<PopupRender<'a>>,
    pub composite_overlay: Option<CompositeOverlay>,
    pub help_overlay: Option<HelpOverlayRender<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpShortcutRender {
    pub scope: String,
    pub keys: String,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpErrorRender {
    pub index: usize,
    pub pointer: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct HelpOverlayPage {
    pub summary: String,
    pub current_page: usize,
    pub total_pages: usize,
    pub shortcuts: Vec<HelpShortcutRender>,
    pub errors: Vec<HelpErrorRender>,
    pub total_errors: usize,
}

pub struct PopupRender<'a> {
    pub title: &'a str,
    pub options: &'a [String],
    pub selected: usize,
    pub multi: bool,
    pub active: Option<&'a [bool]>,
}

#[derive(Debug, Clone, Copy)]
pub struct HelpOverlayRender<'a> {
    pub page: &'a HelpOverlayPage,
    pub shortcut_offset: usize,
    pub error_offset: usize,
}

#[derive(Debug, Clone)]
pub struct CompositeOverlay {
    pub title: String,
    pub description: Option<String>,
    pub dirty: bool,
    pub instructions: String,
    pub list_entries: Option<Vec<String>>,
    pub list_selected: Option<usize>,
    pub entry_label: Option<String>,
    pub level: usize,
}

pub fn draw(
    frame: &mut Frame<'_>,
    form_state: &mut FormState,
    overlay_form: Option<&mut FormState>,
    ctx: UiContext<'_>,
) {
    // Paint a navy backdrop across the entire frame so any cell that the
    // child widgets don't explicitly fill (gaps between panels, scrollbar
    // troughs, the area beyond a short table) reads as navy ink rather
    // than the terminal default. Forked schemaui detail — upstream renders
    // on whatever the terminal background happens to be, which clashed
    // with the rest of deepseek-tui's navy chrome.
    frame.render_widget(
        Block::default().style(Style::default().bg(SURFACE_INK)),
        frame.area(),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(7), Constraint::Length(4)])
        .split(frame.area());

    let cursor_enabled = ctx.popup.is_none() && ctx.composite_overlay.is_none();
    render_body(frame, chunks[0], form_state, cursor_enabled);
    render_footer(frame, chunks[1], &ctx);

    // When both an overlay and a popup are present, render the overlay first
    // and the popup last so that the popup always appears on top. Drawing the
    // overlay after the popup would clear and cover the popup contents.
    if let (Some(meta), Some(overlay_state)) = (ctx.composite_overlay.as_ref(), overlay_form) {
        render_composite_overlay(frame, meta, overlay_state);
    }

    if let Some(popup) = ctx.popup {
        render_popup(frame, popup);
    }

    if let Some(help) = ctx.help_overlay {
        render_help_overlay(frame, help);
    }
}
