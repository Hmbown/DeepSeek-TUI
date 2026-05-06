//! Onboarding flow rendering and helpers.

pub mod api_key;
pub mod language;
pub mod trust_directory;
pub mod welcome;

use std::path::{Path, PathBuf};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::palette;
use crate::tui::app::{App, OnboardingState};

/// Build the ordered onboarding screens for one session.
pub(crate) fn steps_for(include_api_key: bool, include_trust: bool) -> Vec<OnboardingState> {
    let mut steps = vec![OnboardingState::Welcome, OnboardingState::Language];
    if include_api_key {
        steps.push(OnboardingState::ApiKey);
    }
    if include_trust {
        steps.push(OnboardingState::TrustDirectory);
    }
    steps.push(OnboardingState::Tips);
    steps
}

/// Recompute the onboarding plan when onboarding is reopened mid-session.
pub(crate) fn reset_steps_for_current_needs(app: &mut App) {
    let include_trust = !app.trust_mode && needs_trust(&app.workspace);
    app.onboarding_steps = steps_for(app.onboarding_needs_api_key, include_trust);
}

/// Move to the next screen in the frozen onboarding plan.
pub(crate) fn advance(app: &mut App) {
    app.status_message = None;
    if let Some(next) = planned_neighbor(app, 1) {
        app.onboarding = next;
        return;
    }

    app.onboarding = match app.onboarding {
        OnboardingState::Welcome => OnboardingState::Language,
        OnboardingState::Language => next_setup_step(app),
        OnboardingState::ApiKey => next_setup_step(app),
        OnboardingState::TrustDirectory => OnboardingState::Tips,
        OnboardingState::Tips | OnboardingState::None => app.onboarding,
    };
}

/// Move to the previous screen in the frozen onboarding plan.
pub(crate) fn retreat(app: &mut App) {
    app.status_message = None;
    if let Some(previous) = planned_neighbor(app, -1) {
        app.onboarding = previous;
        return;
    }

    app.onboarding = match app.onboarding {
        OnboardingState::Language => OnboardingState::Welcome,
        OnboardingState::ApiKey => OnboardingState::Welcome,
        OnboardingState::TrustDirectory => {
            if app.onboarding_needs_api_key {
                OnboardingState::ApiKey
            } else {
                OnboardingState::Language
            }
        }
        OnboardingState::Tips => next_setup_step(app),
        OnboardingState::Welcome | OnboardingState::None => app.onboarding,
    };
}

fn next_setup_step(app: &App) -> OnboardingState {
    if app.onboarding_needs_api_key {
        OnboardingState::ApiKey
    } else if !app.trust_mode && needs_trust(&app.workspace) {
        OnboardingState::TrustDirectory
    } else {
        OnboardingState::Tips
    }
}

fn planned_neighbor(app: &App, offset: isize) -> Option<OnboardingState> {
    let index = app
        .onboarding_steps
        .iter()
        .position(|state| *state == app.onboarding)?;
    let next = index.checked_add_signed(offset)?;
    app.onboarding_steps.get(next).copied()
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().style(Style::default().bg(palette::DEEPSEEK_INK));
    f.render_widget(block, area);

    let content_width = 76.min(area.width.saturating_sub(4));
    let content_height = 20.min(area.height.saturating_sub(4));
    let content_area = Rect {
        x: (area.width - content_width) / 2,
        y: (area.height - content_height) / 2,
        width: content_width,
        height: content_height,
    };

    let lines = match app.onboarding {
        OnboardingState::Welcome => welcome::lines(),
        OnboardingState::Language => language::lines(app),
        OnboardingState::ApiKey => api_key::lines(app),
        OnboardingState::TrustDirectory => trust_directory::lines(app),
        OnboardingState::Tips => tips_lines(),
        OnboardingState::None => Vec::new(),
    };

    if !lines.is_empty() {
        let (step, total) = onboarding_step(app);
        let panel = Block::default()
            .title(Line::from(Span::styled(
                " DeepSeek TUI ",
                Style::default()
                    .fg(palette::DEEPSEEK_BLUE)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(Span::styled(
                format!(" Step {step}/{total} "),
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_SLATE))
            .padding(Padding::new(2, 2, 1, 1));
        let inner = panel.inner(content_area);
        f.render_widget(panel, content_area);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(paragraph, inner);
    }
}

fn onboarding_step(app: &App) -> (usize, usize) {
    if !app.onboarding_steps.is_empty() {
        let total = app.onboarding_steps.len();
        let step = app
            .onboarding_steps
            .iter()
            .position(|state| *state == app.onboarding)
            .map(|index| index + 1)
            .unwrap_or(total);
        return (step, total);
    }

    let needs_trust = !app.trust_mode && needs_trust(&app.workspace);
    // Welcome + Language + Tips are always shown.
    let mut total = 3;
    if app.onboarding_needs_api_key {
        total += 1;
    }
    if needs_trust {
        total += 1;
    }

    let step = match app.onboarding {
        OnboardingState::Welcome => 1,
        OnboardingState::Language => 2,
        OnboardingState::ApiKey => 3,
        OnboardingState::TrustDirectory => {
            // Welcome (1) + Language (2) + optional ApiKey
            if app.onboarding_needs_api_key { 4 } else { 3 }
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
            "Start Simple",
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::raw(
            "Write the task in plain language. Use /help or Ctrl+K when you want a command.",
        )),
        Line::from(Span::raw(
            "The bottom composer is multi-line: Enter sends, Alt+Enter or Ctrl+J adds a new line.",
        )),
        Line::from(Span::raw(
            "Switch modes only when the job changes: Plan for review-first work, Agent for execution, YOLO when you want auto-approval.",
        )),
        Line::from(Span::raw(
            "Ctrl+R resumes earlier sessions, and Esc backs out of the current draft or overlay.",
        )),
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(palette::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " to open the workspace",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::TuiOptions;
    use tempfile::TempDir;

    fn test_app() -> (TempDir, App) {
        let tmp = TempDir::new().expect("tempdir");
        let config = Config {
            api_key: Some("sk-test-onboarding".to_string()),
            ..Config::default()
        };
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &config);
        app.onboarding = OnboardingState::Welcome;
        app.onboarding_steps.clear();
        app.onboarding_needs_api_key = false;
        app.trust_mode = false;
        (tmp, app)
    }

    #[test]
    fn step_counter_keeps_api_key_step_after_key_is_saved() {
        let (_tmp, mut app) = test_app();
        app.onboarding_steps = steps_for(true, false);
        app.onboarding_needs_api_key = true;

        app.onboarding = OnboardingState::Welcome;
        assert_eq!(onboarding_step(&app), (1, 4));
        app.onboarding = OnboardingState::Language;
        assert_eq!(onboarding_step(&app), (2, 4));
        app.onboarding = OnboardingState::ApiKey;
        assert_eq!(onboarding_step(&app), (3, 4));

        app.onboarding_needs_api_key = false;
        app.onboarding = OnboardingState::Tips;
        assert_eq!(onboarding_step(&app), (4, 4));
    }

    #[test]
    fn advance_uses_frozen_plan_after_api_key_need_is_cleared() {
        let (_tmp, mut app) = test_app();
        app.onboarding_steps = steps_for(true, false);
        app.onboarding_needs_api_key = true;

        advance(&mut app);
        assert_eq!(app.onboarding, OnboardingState::Language);
        advance(&mut app);
        assert_eq!(app.onboarding, OnboardingState::ApiKey);

        app.onboarding_needs_api_key = false;
        advance(&mut app);
        assert_eq!(app.onboarding, OnboardingState::Tips);
    }

    #[test]
    fn step_counter_keeps_trust_step_after_workspace_is_trusted() {
        let (_tmp, mut app) = test_app();
        app.onboarding_steps = steps_for(false, true);

        app.onboarding = OnboardingState::TrustDirectory;
        assert_eq!(onboarding_step(&app), (3, 4));

        app.trust_mode = true;
        app.onboarding = OnboardingState::Tips;
        assert_eq!(onboarding_step(&app), (4, 4));
    }

    #[test]
    fn retreat_uses_previous_planned_step() {
        let (_tmp, mut app) = test_app();
        app.onboarding_steps = steps_for(true, false);
        app.onboarding = OnboardingState::ApiKey;

        retreat(&mut app);
        assert_eq!(app.onboarding, OnboardingState::Language);
    }
}
