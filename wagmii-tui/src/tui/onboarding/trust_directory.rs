//! Workspace trust prompt for onboarding.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;
use crate::tui::app::App;

pub fn lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Trust Workspace",
        Style::default()
            .fg(palette::WAGMII_SKY)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Allow Wagmii to access files outside this workspace?",
        Style::default().fg(palette::TEXT_PRIMARY),
    )));
    lines.push(Line::from(Span::styled(
        format!("Workspace: {}", app.workspace.display()),
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Y = allow tools/agents to read outside this workspace when needed.",
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(Span::styled(
        "N = keep access scoped to this workspace only (safer default).",
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
        Span::styled("Press ", Style::default().fg(palette::TEXT_MUTED)),
        Span::styled(
            "Y",
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to trust, ", Style::default().fg(palette::TEXT_MUTED)),
        Span::styled(
            "N",
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to skip", Style::default().fg(palette::TEXT_MUTED)),
    ]));
    lines
}
