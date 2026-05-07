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
            .fg(palette::DEEPSEEK_SKY)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Do you trust the contents of this directory?",
        Style::default().fg(palette::TEXT_PRIMARY),
    )));
    lines.push(Line::from(Span::styled(
        format!("You are in {}", crate::utils::display_path(&app.workspace)),
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Working with untrusted contents comes with higher risk of prompt injection.",
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(Span::styled(
        "Trusting this directory records it in global config and enables trusted workspace mode.",
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
            "1/Y",
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to trust and continue, ",
            Style::default().fg(palette::TEXT_MUTED),
        ),
        Span::styled(
            "2/N",
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to quit", Style::default().fg(palette::TEXT_MUTED)),
    ]));
    lines
}
