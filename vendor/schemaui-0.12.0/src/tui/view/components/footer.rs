use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::tui::app::status::READY_STATUS;

use super::super::frame::UiContext;

const FOOTER_BG: Color = Color::Rgb(11, 16, 24);
const ACTION_BG: Color = Color::Rgb(15, 22, 31);
const STATUS_BG: Color = Color::Rgb(9, 13, 20);
const KEY_BG: Color = Color::Rgb(95, 196, 255);
const READY_BG: Color = Color::Rgb(88, 196, 123);
const DIRTY_BG: Color = Color::Rgb(240, 193, 84);
const ERROR_BG: Color = Color::Rgb(235, 102, 102);
const LABEL_BG: Color = Color::Rgb(69, 108, 163);
const ALERT_MAX_CHARS: usize = 72;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FooterActionItem {
    pub keys: String,
    pub action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FooterStatusTone {
    Ready,
    Dirty,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FooterStatusModel {
    pub tone: FooterStatusTone,
    pub badge: String,
    pub message: String,
    pub meta: Vec<String>,
    pub alert: Option<String>,
    pub session_title: Option<String>,
}

pub(crate) fn footer_action_items(help: Option<&str>) -> Vec<FooterActionItem> {
    let Some(help) = help.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };

    help.split('•')
        .filter_map(|snippet| {
            let trimmed = snippet.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (keys, action) = match trimmed.split_once("->") {
                Some((keys, action)) => (keys.trim(), action.trim()),
                None => (trimmed, ""),
            };
            Some(FooterActionItem {
                keys: keys.to_string(),
                action: action.to_string(),
            })
        })
        .collect()
}

pub(crate) fn footer_status_model(ctx: &UiContext<'_>) -> FooterStatusModel {
    let tone = if ctx.error_count > 0 {
        FooterStatusTone::Error
    } else if ctx.dirty {
        FooterStatusTone::Dirty
    } else {
        FooterStatusTone::Ready
    };

    let badge = match tone {
        FooterStatusTone::Error => format!("ERR {}", ctx.error_count),
        FooterStatusTone::Dirty => "DIRTY".to_string(),
        FooterStatusTone::Ready => "READY".to_string(),
    };

    let mut meta = Vec::new();
    if ctx.error_count > 0 && ctx.dirty {
        meta.push("Unsaved changes".to_string());
    }
    if let Some(label) = &ctx.focus_label {
        meta.push(format!("Focus {label}"));
    }

    let alert = ctx
        .global_errors
        .first()
        .map(|message| truncate_text(message, ALERT_MAX_CHARS))
        .filter(|message| !message.is_empty());

    FooterStatusModel {
        tone,
        badge,
        message: compact_status_message(ctx.status_message),
        meta,
        alert,
        session_title: ctx
            .session_title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string),
    }
}

pub fn render_footer(frame: &mut Frame<'_>, area: Rect, ctx: &UiContext<'_>) {
    frame.render_widget(Block::default().style(Style::default().bg(FOOTER_BG)), area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(2)])
        .split(area);

    let actions = footer_action_items(ctx.help);
    let action_line = build_action_line(&actions);
    let action_widget = Paragraph::new(action_line)
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(ACTION_BG).fg(Color::Gray));
    frame.render_widget(action_widget, rows[0]);

    let status = footer_status_model(ctx);
    let status_chunks = split_status_row(rows[1], status.session_title.as_deref());

    let status_line = build_status_line(&status);
    let status_widget = Paragraph::new(status_line)
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(STATUS_BG).fg(Color::White));
    frame.render_widget(status_widget, status_chunks[0]);

    if let (Some(title), Some(title_area)) = (status.session_title.as_deref(), status_chunks.get(1))
    {
        let title_widget = Paragraph::new(Line::from(vec![footer_chip(
            format!(" {} ", truncate_text(title, title_char_budget(*title_area))),
            Style::default()
                .fg(Color::Black)
                .bg(LABEL_BG)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(Alignment::Right)
        .style(Style::default().bg(STATUS_BG));
        frame.render_widget(title_widget, *title_area);
    }
}

fn build_action_line(actions: &[FooterActionItem]) -> Line<'static> {
    let mut spans = vec![
        footer_chip(
            " SHORTCUTS ",
            Style::default().fg(Color::White).bg(LABEL_BG),
        ),
        Span::raw(" "),
    ];

    if actions.is_empty() {
        spans.push(Span::styled(
            "Context shortcuts update with the current focus.",
            Style::default().fg(crate::deepseek_palette::TEXT_MUTED),
        ));
        return Line::from(spans);
    }

    for (idx, item) in actions.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled("  •  ", Style::default().fg(crate::deepseek_palette::TEXT_DIM)));
        }
        spans.push(footer_chip(
            format!(" {} ", item.keys),
            Style::default()
                .fg(Color::Black)
                .bg(KEY_BG)
                .add_modifier(Modifier::BOLD),
        ));
        if !item.action.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                item.action.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }

    Line::from(spans)
}

fn build_status_line(status: &FooterStatusModel) -> Line<'static> {
    let badge_style = match status.tone {
        FooterStatusTone::Ready => Style::default()
            .fg(Color::Black)
            .bg(READY_BG)
            .add_modifier(Modifier::BOLD),
        FooterStatusTone::Dirty => Style::default()
            .fg(Color::Black)
            .bg(DIRTY_BG)
            .add_modifier(Modifier::BOLD),
        FooterStatusTone::Error => Style::default()
            .fg(Color::Black)
            .bg(ERROR_BG)
            .add_modifier(Modifier::BOLD),
    };

    let mut spans = vec![
        footer_chip(format!(" {} ", status.badge), badge_style),
        Span::raw(" "),
        Span::styled(
            status.message.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    for entry in &status.meta {
        spans.push(Span::styled("  •  ", Style::default().fg(crate::deepseek_palette::TEXT_DIM)));
        spans.push(Span::styled(
            entry.clone(),
            Style::default().fg(crate::deepseek_palette::ACCENT_SKY),
        ));
    }

    if let Some(alert) = &status.alert {
        spans.push(Span::styled("  •  ", Style::default().fg(crate::deepseek_palette::TEXT_DIM)));
        spans.push(footer_chip(
            " ALERT ",
            Style::default()
                .fg(Color::Black)
                .bg(ERROR_BG)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            alert.clone(),
            Style::default().fg(crate::deepseek_palette::STATUS_ERROR),
        ));
    }

    Line::from(spans)
}

fn footer_chip(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(text.into(), style)
}

fn compact_status_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() || trimmed == READY_STATUS {
        return "Ready for input".to_string();
    }
    trimmed.to_string()
}

fn truncate_text(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let char_count = input.chars().count();
    if char_count <= max_chars {
        return input.to_string();
    }

    let mut result = input
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    result.push('…');
    result
}

fn split_status_row(area: Rect, session_title: Option<&str>) -> Vec<Rect> {
    let Some(title) = session_title else {
        return vec![area];
    };

    let title_width = title_display_width(title, area.width);
    if title_width == 0 {
        return vec![area];
    }

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(12), Constraint::Length(title_width)])
        .split(area)
        .to_vec()
}

fn title_display_width(title: &str, total_width: u16) -> u16 {
    if total_width <= 12 {
        return 0;
    }

    let max_title_width = total_width.saturating_sub(12).min(38);
    if max_title_width < 10 {
        return 0;
    }

    let clipped = truncate_text(title, max_title_width.saturating_sub(2) as usize);
    clipped.chars().count().saturating_add(2) as u16
}

fn title_char_budget(area: Rect) -> usize {
    area.width.saturating_sub(2).clamp(8, 36) as usize
}
