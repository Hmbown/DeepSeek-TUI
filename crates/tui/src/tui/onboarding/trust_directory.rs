//! Workspace trust prompt for onboarding.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;
use crate::tui::app::App;

pub fn lines(app: &App) -> Vec<Line<'static>> {
    let tr = |key: &str, fallback: &str| -> String {
        crate::json_locale::tr_ui_label(app.ui_locale, key)
            .unwrap_or(fallback)
            .to_string()
    };

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        tr("onboarding_trust_title", "Trust Workspace"),
        Style::default()
            .fg(palette::DEEPSEEK_SKY)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        tr("onboarding_trust_desc", "Allow DeepSeek to access files outside this workspace?"),
        Style::default().fg(palette::TEXT_PRIMARY),
    )));
    lines.push(Line::from(Span::styled(
        tr("onboarding_trust_workspace", "Workspace: {path}")
            .replace("{path}", &crate::utils::display_path(&app.workspace)),
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        tr("onboarding_trust_y", "Y = let reviews, searches, and agents reach outside this workspace when a task needs it."),
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(Span::styled(
        tr("onboarding_trust_n", "N = keep file access scoped to this workspace and review approvals case by case."),
        Style::default().fg(palette::TEXT_MUTED),
    )));
    if let Some(message) = app.status_message.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            message.to_string(),
            Style::default().fg(palette::STATUS_WARNING),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            tr("onboarding_trust_press_y", "Press "),
            Style::default().fg(palette::TEXT_MUTED),
        ),
        Span::styled(
            "Y",
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            tr("onboarding_trust_or_n", " to trust, "),
            Style::default().fg(palette::TEXT_MUTED),
        ),
        Span::styled(
            "N",
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            tr("onboarding_trust_skip", " to skip"),
            Style::default().fg(palette::TEXT_MUTED),
        ),
    ]));
    lines
}
