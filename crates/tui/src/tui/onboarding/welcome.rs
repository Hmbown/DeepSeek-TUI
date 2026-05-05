//! Welcome screen content for onboarding.

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
    let version = env!("CARGO_PKG_VERSION");

    vec![
        Line::from(Span::styled(
            tr("onboarding_welcome_title", "DeepSeek TUI"),
            Style::default()
                .fg(palette::DEEPSEEK_BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            tr("onboarding_welcome_version", "Version {version}")
                .replace("{version}", version),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            tr("onboarding_welcome_desc1", "A focused terminal workspace for longer model sessions."),
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            tr("onboarding_welcome_desc2", "You'll add an API key, review trust for this directory, and then land in the chat."),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            tr("onboarding_welcome_desc3", "The main composer is multi-line, so you can write full prompts instead of squeezing everything into one line."),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            tr("onboarding_welcome_continue", "Press Enter to continue."),
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            tr("onboarding_welcome_exit", "Ctrl+C exits at any point."),
            Style::default().fg(palette::TEXT_MUTED),
        )),
    ]
}
