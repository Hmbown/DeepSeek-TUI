//! Sidebar rendering — Plan / Todos / Tasks / Agents panels.
//!
//! Extracted from `tui/ui.rs` (P1.2). The sidebar appears to the right of
//! the chat transcript when the available width allows it. Each section
//! reads from `App` snapshots; mutation lives in the main app loop.

use std::fmt::Write;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::deepseek_theme::active_theme;
use crate::palette;
use crate::tools::plan::StepStatus;
use crate::tools::subagent::SubAgentStatus;
use crate::tools::todo::TodoStatus;

use super::app::{App, SidebarFocus};
use super::subagent_routing::active_fanout_counts;
use super::ui::truncate_line_to_width;

pub fn render_sidebar(f: &mut Frame, area: Rect, app: &App) {
    if area.width < 24 || area.height < 8 {
        return;
    }

    match app.sidebar_focus {
        SidebarFocus::Auto => {
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Min(6),
                ])
                .split(area);

            render_sidebar_plan(f, sections[0], app);
            render_sidebar_todos(f, sections[1], app);
            render_sidebar_tasks(f, sections[2], app);
            render_sidebar_subagents(f, sections[3], app);
        }
        SidebarFocus::Plan => render_sidebar_plan(f, area, app),
        SidebarFocus::Todos => render_sidebar_todos(f, area, app),
        SidebarFocus::Tasks => render_sidebar_tasks(f, area, app),
        SidebarFocus::Agents => render_sidebar_subagents(f, area, app),
    }
}

fn render_sidebar_plan(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let theme = active_theme();
    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(usize::from(area.height).max(4));

    // Cycle indicator (issue #124). Only shown once a boundary has fired —
    // first-time users with cycle_count == 0 don't need this row of chrome.
    if app.cycle_count > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "cycles: {} (active: {})",
                app.cycle_count,
                app.cycle_count.saturating_add(1)
            ),
            Style::default().fg(theme.plan_summary_color),
        )));
    }

    match app.plan_state.try_lock() {
        Ok(plan) => {
            if plan.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No active plan",
                    Style::default().fg(theme.plan_summary_color),
                )));
            } else {
                let (pending, in_progress, completed) = plan.counts();
                let total = pending + in_progress + completed;
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}%", plan.progress_percent()),
                        Style::default().fg(theme.plan_progress_color).bold(),
                    ),
                    Span::styled(
                        format!(" complete ({completed}/{total})"),
                        Style::default().fg(theme.plan_summary_color),
                    ),
                ]));

                if let Some(explanation) = plan.explanation() {
                    lines.push(Line::from(Span::styled(
                        truncate_line_to_width(explanation, content_width.max(1)),
                        Style::default().fg(theme.plan_explanation_color),
                    )));
                }

                let usable_rows = area.height.saturating_sub(3) as usize;
                let max_steps = usable_rows.saturating_sub(lines.len());
                for step in plan.steps().iter().take(max_steps) {
                    let (prefix, color) = match &step.status {
                        StepStatus::Pending => ("[ ]", theme.plan_pending_color),
                        StepStatus::InProgress => ("[~]", theme.plan_in_progress_color),
                        StepStatus::Completed => ("[x]", theme.plan_completed_color),
                    };
                    let mut text = format!("{prefix} {}", step.text);
                    let elapsed = step.elapsed_str();
                    if !elapsed.is_empty() {
                        let _ = write!(text, " ({elapsed})");
                    }
                    lines.push(Line::from(Span::styled(
                        truncate_line_to_width(&text, content_width.max(1)),
                        Style::default().fg(color),
                    )));
                }

                let remaining = plan.steps().len().saturating_sub(max_steps);
                if remaining > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("+{remaining} more steps"),
                        Style::default().fg(theme.plan_summary_color),
                    )));
                }
            }
        }
        Err(_) => {
            lines.push(Line::from(Span::styled(
                "Plan state updating...",
                Style::default().fg(theme.plan_summary_color),
            )));
        }
    }

    render_sidebar_section(f, area, "Plan", lines);
}

fn render_sidebar_todos(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(usize::from(area.height).max(4));

    match app.todos.try_lock() {
        Ok(todos) => {
            let snapshot = todos.snapshot();
            if snapshot.items.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No todos",
                    Style::default().fg(palette::TEXT_MUTED),
                )));
            } else {
                let total = snapshot.items.len();
                let completed = snapshot
                    .items
                    .iter()
                    .filter(|item| item.status == TodoStatus::Completed)
                    .count();
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}%", snapshot.completion_pct),
                        Style::default().fg(palette::STATUS_SUCCESS).bold(),
                    ),
                    Span::styled(
                        format!(" complete ({completed}/{total})"),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                ]));

                let usable_rows = area.height.saturating_sub(3) as usize;
                let max_items = usable_rows.saturating_sub(lines.len());
                for item in snapshot.items.iter().take(max_items) {
                    let (prefix, color) = match item.status {
                        TodoStatus::Pending => ("[ ]", palette::TEXT_MUTED),
                        TodoStatus::InProgress => ("[~]", palette::STATUS_WARNING),
                        TodoStatus::Completed => ("[x]", palette::STATUS_SUCCESS),
                    };
                    let text = format!("{prefix} #{} {}", item.id, item.content);
                    lines.push(Line::from(Span::styled(
                        truncate_line_to_width(&text, content_width.max(1)),
                        Style::default().fg(color),
                    )));
                }

                let remaining = snapshot.items.len().saturating_sub(max_items);
                if remaining > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("+{remaining} more todos"),
                        Style::default().fg(palette::TEXT_MUTED),
                    )));
                }
            }
        }
        Err(_) => {
            lines.push(Line::from(Span::styled(
                "Todo list updating...",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }
    }

    render_sidebar_section(f, area, "Todos", lines);
}

fn render_sidebar_tasks(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(usize::from(area.height).max(4));

    if let Some(turn_id) = app.runtime_turn_id.as_ref() {
        let status = app
            .runtime_turn_status
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(
                &format!("turn {} ({status})", truncate_line_to_width(turn_id, 12)),
                content_width.max(1),
            ),
            Style::default().fg(palette::DEEPSEEK_SKY),
        )));
    }

    if app.task_panel.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks",
            Style::default().fg(palette::TEXT_MUTED),
        )));
    } else {
        let running = app
            .task_panel
            .iter()
            .filter(|task| task.status == "running")
            .count();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{running} running"),
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            ),
            Span::styled(
                format!(" / {}", app.task_panel.len()),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]));

        let usable_rows = area.height.saturating_sub(3) as usize;
        let max_items = usable_rows.saturating_sub(lines.len());
        for task in app.task_panel.iter().take(max_items) {
            let color = match task.status.as_str() {
                "queued" => palette::TEXT_MUTED,
                "running" => palette::STATUS_WARNING,
                "completed" => palette::STATUS_SUCCESS,
                "failed" => palette::STATUS_ERROR,
                "canceled" => palette::TEXT_DIM,
                _ => palette::TEXT_MUTED,
            };
            let duration = task
                .duration_ms
                .map(|ms| format!("{:.1}s", ms as f64 / 1000.0))
                .unwrap_or_else(|| "-".to_string());
            let label = format!(
                "{} {} {}",
                truncate_line_to_width(&task.id, 10),
                task.status,
                duration
            );
            lines.push(Line::from(Span::styled(
                truncate_line_to_width(&label, content_width.max(1)),
                Style::default().fg(color),
            )));
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate_line_to_width(
                        &task.prompt_summary,
                        content_width.saturating_sub(2).max(1)
                    )
                ),
                Style::default().fg(palette::TEXT_DIM),
            )));
        }
    }

    render_sidebar_section(f, area, "Tasks", lines);
}

fn render_sidebar_subagents(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;

    // Demoted to navigator (issue #128): the in-transcript DelegateCard /
    // FanoutCard now carries the live action tree and dot-grid. The sidebar
    // shows just count + role-mix so the user can scan parallel work at a
    // glance and scroll to the matching transcript card for detail.
    let cached_ids: std::collections::HashSet<&str> = app
        .subagent_cache
        .iter()
        .map(|agent| agent.agent_id.as_str())
        .collect();
    let progress_only_count = app
        .agent_progress
        .keys()
        .filter(|id| !cached_ids.contains(id.as_str()))
        .count();
    let cached_running = app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
        .count();
    let role_counts: std::collections::BTreeMap<String, usize> =
        app.subagent_cache
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut acc, agent| {
                *acc.entry(agent.agent_type.as_str().to_string())
                    .or_insert(0) += 1;
                acc
            });
    let (fanout_running, fanout_total) = active_fanout_counts(app)
        .map(|(running, total)| (running, Some(total)))
        .unwrap_or((0, None));

    let summary = SidebarSubagentSummary {
        cached_total: app.subagent_cache.len(),
        cached_running,
        progress_only_count,
        fanout_total,
        fanout_running,
        role_counts,
    };
    let lines = subagent_navigator_lines(&summary, content_width);

    render_sidebar_section(f, area, "Agents", lines);
}

/// Minimal projection of the data the sub-agent sidebar needs. Lifted out
/// of `render_sidebar_subagents` so the rendering can be snapshot-tested
/// without a full `App`.
#[derive(Debug, Clone, Default)]
pub struct SidebarSubagentSummary {
    pub cached_total: usize,
    pub cached_running: usize,
    pub progress_only_count: usize,
    pub fanout_total: Option<usize>,
    pub fanout_running: usize,
    pub role_counts: std::collections::BTreeMap<String, usize>,
}

/// Build the demoted navigator lines from a summary projection. Public
/// for the snapshot test in this module.
pub fn subagent_navigator_lines(
    summary: &SidebarSubagentSummary,
    content_width: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(4);

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0 && summary.progress_only_count == 0 && fanout_total == 0 {
        lines.push(Line::from(Span::styled(
            "No agents",
            Style::default().fg(palette::TEXT_MUTED),
        )));
        return lines;
    }

    let live_running =
        (summary.cached_running + summary.progress_only_count).max(summary.fanout_running);
    let total = (summary.cached_total + summary.progress_only_count).max(fanout_total);
    let done = total.saturating_sub(live_running);
    let header = if live_running > 0 {
        vec![
            Span::styled(
                format!("{live_running} running"),
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            ),
            Span::styled(
                format!(" / {total}"),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]
    } else {
        vec![Span::styled(
            format!("{done} done"),
            Style::default().fg(palette::STATUS_SUCCESS),
        )]
    };
    lines.push(Line::from(header));

    if !summary.role_counts.is_empty() {
        let mix: Vec<String> = summary
            .role_counts
            .iter()
            .map(|(role, count)| format!("{count} {role}"))
            .collect();
        let role_line = mix.join(" \u{00B7} ");
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&role_line, content_width.max(1)),
            Style::default().fg(palette::TEXT_DIM),
        )));
    }

    lines.push(Line::from(Span::styled(
        "(see transcript card for detail)",
        Style::default().fg(palette::TEXT_MUTED).italic(),
    )));

    lines
}

fn render_sidebar_section(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>) {
    if area.width < 4 || area.height < 3 {
        return;
    }

    let theme = active_theme();
    // Truncate the panel title so it always fits within the section width
    // even after a resize. The title occupies up to 4 chars of border chrome
    // (two spaces + one space on each side), so the max title length is
    // area.width.saturating_sub(4) when borders are enabled.
    let max_title_width = area.width.saturating_sub(4).max(1) as usize;
    let display_title = truncate_line_to_width(title, max_title_width);

    let section = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .title(Line::from(vec![Span::styled(
                format!(" {display_title} "),
                Style::default().fg(theme.section_title_color).bold(),
            )]))
            .borders(theme.section_borders)
            .border_type(theme.section_border_type)
            .border_style(Style::default().fg(theme.section_border_color))
            .style(Style::default().bg(theme.section_bg))
            .padding(theme.section_padding),
    );

    f.render_widget(section, area);
}

#[cfg(test)]
mod tests {
    use super::{SidebarSubagentSummary, subagent_navigator_lines};
    use ratatui::text::Line;

    fn lines_to_text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn navigator_empty_state_says_no_agents() {
        let summary = SidebarSubagentSummary::default();
        let lines = subagent_navigator_lines(&summary, 32);
        let text = lines_to_text(&lines);
        assert_eq!(text, vec!["No agents".to_string()]);
    }

    #[test]
    fn navigator_running_state_renders_count_role_and_navigator_hint() {
        // Two general agents (one running, one done) + one explore (running).
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("general".to_string(), 2);
        role_counts.insert("explore".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 3,
            cached_running: 2,
            progress_only_count: 0,
            fanout_total: None,
            fanout_running: 0,
            role_counts,
        };
        let text = lines_to_text(&subagent_navigator_lines(&summary, 64));
        assert!(text[0].contains("2 running"), "header: {:?}", text[0]);
        assert!(text[0].contains("/ 3"), "total in header: {:?}", text[0]);
        assert!(
            text[1].contains("1 explore") && text[1].contains("2 general"),
            "role mix line: {:?}",
            text[1]
        );
        assert!(
            text.iter().any(|l| l.contains("transcript card")),
            "navigator hint must defer to transcript: {text:?}",
        );
    }

    #[test]
    fn navigator_uses_fanout_total_when_swarm_has_seeded_slots() {
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            progress_only_count: 0,
            fanout_total: Some(6),
            fanout_running: 1,
            role_counts: std::collections::BTreeMap::new(),
        };

        let text = lines_to_text(&subagent_navigator_lines(&summary, 64));

        assert!(text[0].contains("1 running"), "header: {:?}", text[0]);
        assert!(text[0].contains("/ 6"), "fanout total: {:?}", text[0]);
    }

    #[test]
    fn navigator_settled_state_says_done() {
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("general".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 0,
            progress_only_count: 0,
            fanout_total: None,
            fanout_running: 0,
            role_counts,
        };
        let text = lines_to_text(&subagent_navigator_lines(&summary, 32));
        assert!(text[0].contains("1 done"), "settled header: {:?}", text[0]);
    }

    #[test]
    fn navigator_truncates_long_role_mix_to_content_width() {
        // Build a wide role mix; assert it doesn't blow past content_width.
        let mut role_counts = std::collections::BTreeMap::new();
        for role in ["general", "explore", "plan", "review", "custom", "extra"] {
            role_counts.insert(role.to_string(), 1);
        }
        let summary = SidebarSubagentSummary {
            cached_total: 6,
            cached_running: 6,
            progress_only_count: 0,
            fanout_total: None,
            fanout_running: 0,
            role_counts,
        };
        let lines = subagent_navigator_lines(&summary, 16);
        let role_line: &str = lines[1]
            .spans
            .first()
            .map(|s| s.content.as_ref())
            .unwrap_or("");
        assert!(
            role_line.chars().count() <= 16,
            "role line {role_line:?} exceeded content_width"
        );
    }
}
