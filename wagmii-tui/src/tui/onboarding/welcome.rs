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
            0 => palette::WAGMII_BLUE,
            1 => palette::WAGMII_SKY,
            _ => palette::WAGMII_RED,
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
            "Wagmii CLI",
            Style::default()
                .fg(palette::WAGMII_BLUE)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Unofficial CLI for the Wagmii API",
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(Span::styled(
        "Not affiliated with Wagmii Inc.",
        Style::default().fg(palette::TEXT_MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press Enter to continue setup.",
        Style::default().fg(palette::TEXT_PRIMARY),
    )));
    lines.push(Line::from(Span::styled(
        "Press Ctrl+C to exit.",
        Style::default().fg(palette::TEXT_MUTED),
    )));

    lines
}
