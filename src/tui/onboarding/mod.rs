//! Onboarding flow rendering and helpers.

pub mod api_key;
pub mod trust_directory;
pub mod welcome;

use std::path::{Path, PathBuf};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::palette;
use crate::tui::app::{App, OnboardingState};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().style(Style::default().bg(palette::DEEPSEEK_INK));
    f.render_widget(block, area);

    let content_width = 72.min(area.width.saturating_sub(4));
    let content_height = 26.min(area.height.saturating_sub(4));
    let content_area = Rect {
        x: (area.width - content_width) / 2,
        y: (area.height - content_height) / 2,
        width: content_width,
        height: content_height,
    };

    let lines = match app.onboarding {
        OnboardingState::Welcome => welcome::lines(),
        OnboardingState::ApiKey => api_key::lines(app),
        OnboardingState::TrustDirectory => trust_directory::lines(app),
        OnboardingState::Tips => tips_lines(),
        OnboardingState::None => Vec::new(),
    };

    if !lines.is_empty() {
        let (step, total) = onboarding_step(app);
        let mut decorated = vec![
            Line::from(Span::styled(
                format!("Step {step}/{total}"),
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];
        decorated.extend(lines);
        let paragraph = Paragraph::new(decorated).wrap(Wrap { trim: false });
        f.render_widget(paragraph, content_area);
    }
}

fn onboarding_step(app: &App) -> (usize, usize) {
    let needs_trust = !app.trust_mode && needs_trust(&app.workspace);
    let mut total = 2; // Welcome + Tips
    if app.onboarding_needs_api_key {
        total += 1;
    }
    if needs_trust {
        total += 1;
    }

    let step = match app.onboarding {
        OnboardingState::Welcome => 1,
        OnboardingState::ApiKey => 2,
        OnboardingState::TrustDirectory => {
            if app.onboarding_needs_api_key {
                3
            } else {
                2
            }
        }
        OnboardingState::Tips => total,
        OnboardingState::None => total,
    };

    (step, total)
}

pub fn tips_lines() -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::Modifier;
    use ratatui::text::{Line, Span};

    vec![
        Line::from(Span::styled(
            "Quick Tips",
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::raw(
            "  - Tab cycles modes (Normal → Agent → YOLO → Plan), Shift+Tab reverses",
        )),
        Line::from(Span::raw(
            "  - Alt+1/2/3/4 switch modes (Normal/Agent/YOLO/Plan)",
        )),
        Line::from(Span::raw(
            "  - Alt+!/@/#/$/) focus sidebar sections (Plan/Todos/Tasks/Agents/Auto)",
        )),
        Line::from(Span::raw("  - Ctrl+R opens the session picker")),
        Line::from(Span::raw("  - l opens the pager for the last message")),
        Line::from(Span::raw("  - Ctrl+C cancels or exits")),
        Line::from(Span::raw("  - /help lists all commands")),
        Line::from(Span::raw(
            "  - Start with /config or /model for a quick check",
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(palette::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " to start chatting",
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]),
    ]
}

pub fn default_marker_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join(".onboarded"))
}

pub fn is_onboarded() -> bool {
    default_marker_path().is_some_and(|path| path.exists())
}

pub fn mark_onboarded() -> std::io::Result<PathBuf> {
    let path = default_marker_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, "")?;
    Ok(path)
}

pub fn needs_trust(workspace: &Path) -> bool {
    let markers = [
        workspace.join(".deepseek").join("trusted"),
        workspace.join(".deepseek").join("trust.json"),
    ];
    !markers.iter().any(|path| path.exists())
}

pub fn mark_trusted(workspace: &Path) -> std::io::Result<PathBuf> {
    let dir = workspace.join(".deepseek");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("trusted");
    std::fs::write(&path, "")?;
    Ok(path)
}
