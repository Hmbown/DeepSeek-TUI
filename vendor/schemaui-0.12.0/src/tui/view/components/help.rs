use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use super::super::frame::{
    HelpErrorRender, HelpOverlayPage, HelpOverlayRender, HelpShortcutRender,
};

const SIDE_BY_SIDE_MIN_WIDTH: u16 = 110;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HelpOverlayPanelCapacities {
    pub shortcuts: usize,
    pub errors: usize,
}

pub(crate) fn help_overlay_panel_capacities(area: Rect) -> HelpOverlayPanelCapacities {
    let inner = bordered_inner(area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(inner);
    let [shortcut_area, error_area] = split_main_panels(inner, layout[1]);
    HelpOverlayPanelCapacities {
        shortcuts: table_row_capacity(shortcut_area),
        errors: table_row_capacity(error_area),
    }
}

pub(crate) fn help_overlay_error_page_capacity(area: Rect) -> usize {
    help_overlay_panel_capacities(area).errors
}

pub(crate) fn help_overlay_error_message_capacity(area: Rect) -> usize {
    let inner = bordered_inner(area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(inner);
    let [_, error_area] = split_main_panels(inner, layout[1]);
    error_message_column_capacity(error_area)
}

pub fn render_help_overlay(frame: &mut Frame<'_>, help: HelpOverlayRender<'_>) {
    let area = frame.area();
    frame.render_widget(Clear, area);

    let block = Block::default().title("Help").borders(Borders::ALL).style(
        Style::default()
            .fg(crate::deepseek_palette::ACCENT_SKY)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(block.clone(), area);
    let inner = bordered_inner(area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(inner);

    let page = help.page;
    render_header(frame, layout[0], page);

    let main = split_main_panels(inner, layout[1]);
    let capacities = help_overlay_panel_capacities(area);

    render_shortcuts_table(
        frame,
        main[0],
        &page.shortcuts,
        help.shortcut_offset,
        capacities.shortcuts,
    );
    render_errors_panel(frame, main[1], page, help.error_offset);

    let footer = Paragraph::new(help_footer_line(page))
        .style(Style::default().fg(crate::deepseek_palette::TEXT_MUTED))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, layout[2]);
}

fn bordered_inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

fn split_main_panels(inner: Rect, area: Rect) -> [Rect; 2] {
    let chunks = if inner.width >= SIDE_BY_SIDE_MIN_WIDTH {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(area)
    };
    [chunks[0], chunks[1]]
}

fn table_row_capacity(area: Rect) -> usize {
    let inner = bordered_inner(area);
    usize::from(inner.height.saturating_sub(1)).max(1)
}

fn render_header(frame: &mut Frame<'_>, area: Rect, page: &HelpOverlayPage) {
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Shortcuts",
                Style::default()
                    .fg(crate::deepseek_palette::STATUS_WARN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" {} ", page.shortcuts.len())),
            Span::styled("•", Style::default().fg(crate::deepseek_palette::TEXT_DIM)),
            Span::raw(format!(" errors {} ", page.total_errors)),
            Span::styled("•", Style::default().fg(crate::deepseek_palette::TEXT_DIM)),
            Span::raw(format!(
                " error page {}/{}",
                page.current_page, page.total_pages
            )),
        ]),
        Line::from(Span::styled(
            page.summary.as_str(),
            Style::default().fg(crate::deepseek_palette::TEXT_MUTED),
        )),
    ])
    .wrap(Wrap { trim: true });
    frame.render_widget(header, area);
}

fn render_shortcuts_table(
    frame: &mut Frame<'_>,
    area: Rect,
    shortcuts: &[HelpShortcutRender],
    shortcut_offset: usize,
    visible_rows: usize,
) {
    let max_offset = shortcuts.len().saturating_sub(visible_rows);
    let start = shortcut_offset.min(max_offset);
    let end = (start + visible_rows).min(shortcuts.len());
    let title = if shortcuts.len() <= visible_rows {
        "Shortcuts".to_string()
    } else {
        format!("Shortcuts {}-{} / {}", start + 1, end, shortcuts.len())
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::deepseek_palette::TEXT_DIM));
    let rows = shortcuts[start..end].iter().map(shortcut_row);
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(18),
            Constraint::Min(24),
        ],
    )
    .header(
        Row::new(vec!["Scope", "Keys", "Action"]).style(
            Style::default()
                .fg(crate::deepseek_palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(block)
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn shortcut_row(entry: &HelpShortcutRender) -> Row<'static> {
    let scope_style = match entry.scope.as_str() {
        "Form" => Style::default().fg(crate::deepseek_palette::ACCENT_SKY),
        "List" => Style::default().fg(crate::deepseek_palette::ACCENT_PURPLE),
        "Overlay" => Style::default().fg(crate::deepseek_palette::STATUS_WARN),
        "Help" => Style::default().fg(crate::deepseek_palette::ACCENT_BLUE),
        _ => Style::default().fg(crate::deepseek_palette::TEXT_MUTED),
    };
    Row::new(vec![
        Cell::from(entry.scope.clone()).style(scope_style),
        Cell::from(entry.keys.clone()).style(
            Style::default()
                .fg(crate::deepseek_palette::STATUS_OK)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(entry.action.clone()),
    ])
}

fn render_errors_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    page: &HelpOverlayPage,
    error_offset: usize,
) {
    let title = if page.total_errors == 0 {
        "Errors".to_string()
    } else {
        let start = page.errors.first().map(|error| error.index).unwrap_or(0);
        let end = page.errors.last().map(|error| error.index).unwrap_or(0);
        format!("Errors {start}-{end} / {}", page.total_errors)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::deepseek_palette::TEXT_DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if page.errors.is_empty() {
        let paragraph = Paragraph::new(vec![
            Line::from(Span::styled(
                "No validation errors.",
                Style::default()
                    .fg(crate::deepseek_palette::STATUS_OK)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("Current form state is clean."),
        ])
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(crate::deepseek_palette::TEXT_MUTED));
        frame.render_widget(paragraph, inner);
        return;
    }

    let message_width = error_message_column_capacity(area);
    let rows = page
        .errors
        .iter()
        .map(|entry| error_row(entry, error_offset, message_width));
    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(18),
            Constraint::Min(18),
        ],
    )
    .header(
        Row::new(vec!["#", "Field", "Message"]).style(
            Style::default()
                .fg(crate::deepseek_palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .column_spacing(1);
    frame.render_widget(table, inner);
}

fn error_row(entry: &HelpErrorRender, error_offset: usize, message_width: usize) -> Row<'static> {
    Row::new(vec![
        Cell::from(entry.index.to_string()).style(Style::default().fg(crate::deepseek_palette::STATUS_ERROR)),
        Cell::from(entry.pointer.clone()).style(Style::default().fg(crate::deepseek_palette::ACCENT_SKY)),
        Cell::from(horizontal_window(
            &entry.message,
            error_offset,
            message_width,
        ))
        .style(Style::default().fg(crate::deepseek_palette::TEXT_PRIMARY)),
    ])
}

fn error_message_column_capacity(area: Rect) -> usize {
    let inner = bordered_inner(area);
    usize::from(inner.width.saturating_sub(24)).max(1)
}

fn horizontal_window(input: &str, offset: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let chars: Vec<char> = input.chars().collect();
    if chars.len() <= width {
        return input.to_string();
    }

    let start = offset.min(chars.len().saturating_sub(1));
    let remaining = chars.len().saturating_sub(start);
    let mut visible_width = width;
    let has_left_hidden = start > 0;
    let has_right_hidden = remaining > width;

    if has_left_hidden {
        visible_width = visible_width.saturating_sub(1);
    }
    if has_right_hidden {
        visible_width = visible_width.saturating_sub(1);
    }
    visible_width = visible_width.max(1);

    let end = (start + visible_width).min(chars.len());
    let mut output = String::new();
    if has_left_hidden {
        output.push('…');
    }
    output.extend(chars[start..end].iter());
    if end < chars.len() {
        output.push('…');
    }
    output
}

fn help_footer_line(page: &HelpOverlayPage) -> Line<'static> {
    let actions = page
        .shortcuts
        .iter()
        .filter(|entry| entry.scope == "Help")
        .collect::<Vec<_>>();

    if actions.is_empty() {
        return Line::from("Help shortcuts are not available.");
    }

    let mut spans = Vec::new();
    for (idx, entry) in actions.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            entry.keys.clone(),
            Style::default().fg(crate::deepseek_palette::STATUS_WARN),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::raw(entry.action.clone()));
    }
    Line::from(spans)
}
