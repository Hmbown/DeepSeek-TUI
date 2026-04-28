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
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(usize::from(area.height).max(4));

    // The footer's `running_agent_count` takes the union of `agent_progress`
    // (live engine progress events) and `subagent_cache` (the snapshot that
    // arrives async via `Op::ListSubAgents`). When 5 agents are spawning, the
    // footer chip says "5 agents" because progress events update immediately,
    // but `subagent_cache` is empty until the engine responds — so the
    // sidebar would say "No agents" while the footer says 5 (#63).
    //
    // Mirror the footer's union here. Cached entries get the full status
    // line; progress-only IDs get a single "starting…" row using the latest
    // progress message, so the sidebar matches the footer in real time.
    let cached_ids: std::collections::HashSet<&str> = app
        .subagent_cache
        .iter()
        .map(|agent| agent.agent_id.as_str())
        .collect();
    let progress_only: Vec<(&str, &str)> = app
        .agent_progress
        .iter()
        .filter(|(id, _)| !cached_ids.contains(id.as_str()))
        .map(|(id, msg)| (id.as_str(), msg.as_str()))
        .collect();

    if app.subagent_cache.is_empty() && progress_only.is_empty() {
        lines.push(Line::from(Span::styled(
            "No agents",
            Style::default().fg(palette::TEXT_MUTED),
        )));
    } else {
        let cached_running = app
            .subagent_cache
            .iter()
            .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
            .count();
        let live_running = cached_running + progress_only.len();
        let total = app.subagent_cache.len() + progress_only.len();
        let done = total.saturating_sub(live_running);
        // When agents have all finished, "0 running / 1" reads as broken.
        // Switch to "1 done" once nothing is in flight; only show the
        // running/total split while activity is live.
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

        let usable_rows = area.height.saturating_sub(3) as usize;
        let max_agents = usable_rows.saturating_sub(lines.len());

        let push_agent_row =
            |lines: &mut Vec<Line<'static>>, summary: &str, detail: &str, color| {
                lines.push(Line::from(Span::styled(
                    truncate_line_to_width(summary, content_width.max(1)),
                    Style::default().fg(color),
                )));
                lines.push(Line::from(Span::styled(
                    format!(
                        "  {}",
                        truncate_line_to_width(detail, content_width.saturating_sub(2).max(1))
                    ),
                    Style::default().fg(palette::TEXT_DIM),
                )));
            };

        // Live (progress-only) agents first — they're the freshest signal.
        let mut rendered = 0usize;
        for (id, msg) in progress_only.iter().take(max_agents) {
            let summary = format!("{} starting", truncate_line_to_width(id, 10));
            push_agent_row(&mut lines, &summary, msg, palette::STATUS_WARNING);
            rendered += 1;
        }

        // Then the cached snapshot for everything that's already settled into
        // `subagent_cache`.
        let remaining_budget = max_agents.saturating_sub(rendered);
        for agent in app.subagent_cache.iter().take(remaining_budget) {
            let (status_label, status_color) = match &agent.status {
                SubAgentStatus::Running => ("running", palette::STATUS_WARNING),
                SubAgentStatus::Completed => ("done", palette::STATUS_SUCCESS),
                SubAgentStatus::Interrupted(_) => ("interrupted", palette::STATUS_WARNING),
                SubAgentStatus::Failed(_) => ("failed", palette::STATUS_ERROR),
                SubAgentStatus::Cancelled => ("cancelled", palette::TEXT_MUTED),
            };
            let agent_type = agent.agent_type.as_str();
            let role = agent.assignment.role.as_deref().unwrap_or("default");
            let summary = format!(
                "{} {agent_type}/{role} {status_label} ({} steps)",
                truncate_line_to_width(&agent.agent_id, 10),
                agent.steps_taken
            );
            push_agent_row(
                &mut lines,
                &summary,
                &agent.assignment.objective,
                status_color,
            );
            rendered += 1;
        }

        let remaining = total.saturating_sub(rendered);
        if remaining > 0 {
            lines.push(Line::from(Span::styled(
                format!("+{remaining} more agents"),
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }
    }

    render_sidebar_section(f, area, "Agents", lines);
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
