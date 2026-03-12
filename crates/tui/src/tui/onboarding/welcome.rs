//! Welcome screen content for onboarding.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;

const LOGO: &str = r"
██████╗ ███████╗███████╗██████╗ ███████╗███████╗███████╗██╗  ██╗
██╔══██╗██╔════╝██╔════╝██╔══██╗██╔════╝██╔════╝██╔════╝██║ ██╔╝
██║  ██║█████╗  █████╗  ██████╔╝███████╗█████╗  █████╗  █████╔╝
██║  ██║██╔══╝  ██╔══╝  ██╔═══╝ ╚════██║██╔══╝  ██╔══╝  ██╔═██╗
██████╔╝███████╗███████╗██║     ███████║███████╗███████╗██║  ██╗
╚═════╝ ╚══════╝╚══════╝╚═╝     ╚══════╝╚══════╝╚══════╝╚═╝  ╚═╝
";

pub fn lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for (i, line) in LOGO.lines().enumerate() {
        let color = match i % 3 {
            0 => palette::DEEPSEEK_BLUE,
            1 => palette::DEEPSEEK_SKY,
            _ => palette::DEEPSEEK_RED,
        };
        lines.push(Line::from(Span::styled(
            line,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Welcome to ", Style::default().fg(palette::TEXT_PRIMARY)),
        Span::styled(
            "DeepSeek TUI",
            Style::default()
                .fg(palette::DEEPSEEK_BLUE)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Agent workflows for the DeepSeek API in your terminal",
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(Span::styled(
        "Set up your key, pick a mode, and watch approvals, queue state, and agents as work runs.",
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(Span::styled(
        "Not affiliated with DeepSeek Inc.",
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press Enter to start setup.",
        Style::default().fg(palette::TEXT_PRIMARY),
    )));
    lines.push(Line::from(Span::styled(
        "Ctrl+C exits at any point.",
        Style::default().fg(palette::TEXT_MUTED),
    )));

    lines
}
