//! Sub-agent and background-task routing helpers for the TUI loop.

use std::time::Instant;

use crate::task_manager::{TaskRecord, TaskStatus, TaskSummary};
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::subagent::{MailboxMessage, SubAgentResult, SubAgentStatus};
use crate::tools::swarm::{SwarmOutcome, SwarmTaskStatus};
use crate::tui::app::{App, AppMode, TaskPanelEntry};
use crate::tui::history::{HistoryCell, SubAgentCell, summarize_tool_output};
use crate::tui::pager::PagerView;
use crate::tui::widgets::agent_card::{
    AgentLifecycle, DelegateCard, FanoutCard, WorkerSlot, apply_to_delegate, apply_to_fanout,
};

pub(super) fn running_agent_count(app: &App) -> usize {
    let mut ids: std::collections::HashSet<&str> =
        app.agent_progress.keys().map(String::as_str).collect();
    for agent in app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
    {
        ids.insert(agent.agent_id.as_str());
    }
    ids.len()
}

pub(super) fn active_fanout_counts(app: &App) -> Option<(usize, usize)> {
    // Canonical source: the in-progress SwarmOutcome from swarm_jobs.
    if let Some(swarm_id) = app.last_swarm_id.as_ref()
        && let Some(outcome) = app.swarm_jobs.get(swarm_id)
    {
        return Some((outcome.counts.running, outcome.counts.total));
    }

    // Card exists — read running count from the canonical slot states.
    if let Some(idx) = app.last_fanout_card_index
        && let Some(HistoryCell::SubAgent(SubAgentCell::Fanout(card))) = app.history.get(idx)
    {
        let running = card
            .workers
            .iter()
            .filter(|slot| matches!(slot.status, AgentLifecycle::Running))
            .count();
        return Some((running, card.worker_count()));
    }

    // No card yet — swarm was just dispatched but no SwarmProgress has
    // arrived. Show the declared task count so the sidebar doesn't read zero.
    if let Some(total) = app.pending_swarm_task_count {
        return Some((0, total));
    }

    None
}

pub(super) fn seed_fanout_card_from_tool_call(
    app: &mut App,
    name: &str,
    input: &serde_json::Value,
) -> bool {
    if name != "agent_swarm" {
        return false;
    }

    let Some(tasks) = input.get("tasks").and_then(serde_json::Value::as_array) else {
        return false;
    };
    if tasks.is_empty() {
        return false;
    }

    // Codex pattern: don't pre-seed a FanoutCard with all-Pending workers.
    // The card gets created by sync_fanout_card_from_swarm_outcome when the
    // first SwarmProgress carries real worker states. This eliminates the
    // "0 done · 0 running · 0 failed · N pending" vs sidebar "N running"
    // contradiction (#236/#238).
    //
    // Store the pending dispatch info so the transcript tool card (running
    // state) serves as the visual placeholder until workers start.
    app.pending_swarm_task_count = Some(tasks.len());
    true
}

pub(super) fn sync_fanout_card_from_tool_result(
    app: &mut App,
    name: &str,
    result: &Result<ToolResult, ToolError>,
) -> bool {
    if name != "agent_swarm" {
        return false;
    }
    let Ok(tool_result) = result else {
        return false;
    };
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&tool_result.content) else {
        return false;
    };
    let Some(tasks) = payload
        .get("tasks")
        .and_then(serde_json::Value::as_array)
        .filter(|tasks| !tasks.is_empty())
    else {
        return false;
    };

    if let Ok(outcome) = serde_json::from_value::<SwarmOutcome>(payload.clone()) {
        return sync_fanout_card_from_swarm_outcome(app, &outcome);
    }

    let workers = tasks
        .iter()
        .enumerate()
        .map(|(idx, task)| {
            let task_id = task
                .get("task_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| idx.to_string());
            let agent_id = task
                .get("agent_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("task:{task_id}"));
            let status = task
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(status_to_lifecycle)
                .unwrap_or(AgentLifecycle::Pending);
            let mut slot =
                WorkerSlot::with_agent(format!("task:{task_id}"), Some(agent_id), status);
            slot.label = task
                .get("label")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            slot.model = task
                .get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            slot.nickname = task
                .get("nickname")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            slot
        })
        .collect::<Vec<_>>();

    let Some(idx) = app.last_fanout_card_index else {
        return false;
    };
    let Some(HistoryCell::SubAgent(SubAgentCell::Fanout(card))) = app.history.get_mut(idx) else {
        return false;
    };
    card.workers = workers;
    app.mark_history_updated();
    true
}

pub(super) fn sync_fanout_card_from_swarm_outcome(app: &mut App, outcome: &SwarmOutcome) -> bool {
    app.swarm_jobs
        .insert(outcome.swarm_id.clone(), outcome.clone());
    app.last_swarm_id = Some(outcome.swarm_id.clone());

    let workers = outcome
        .tasks
        .iter()
        .map(worker_slot_from_swarm_task)
        .collect::<Vec<_>>();

    if workers.is_empty() {
        return false;
    }

    // Bind this swarm to a card by id so concurrent fanouts each update
    // their own visualization. Order of preference:
    //   1) existing binding for this swarm_id (idempotent updates)
    //   2) the most recently seeded card (last_fanout_card_index) — which
    //      typically corresponds to the fresh `agent_swarm` invocation
    //      that just emitted this outcome's initial event
    //   3) allocate a fresh card and append it to history
    // Once chosen, the swarm_id↔card_index pair is cached so subsequent
    // SwarmProgress events for the *same* swarm always update the right
    // card even if `last_fanout_card_index` has since moved to another
    // overlapping fanout.
    let idx = if let Some(&bound) = app.swarm_card_index.get(&outcome.swarm_id)
        && matches!(
            app.history.get(bound),
            Some(HistoryCell::SubAgent(SubAgentCell::Fanout(_)))
        ) {
        bound
    } else if let Some(idx) = app.last_fanout_card_index
        && matches!(
            app.history.get(idx),
            Some(HistoryCell::SubAgent(SubAgentCell::Fanout(_)))
        )
        && !app.swarm_card_index.values().any(|bound| *bound == idx)
    {
        // The most recently-seeded card has no swarm bound to it yet; this
        // outcome's first SwarmProgress claims it. Any subsequent overlapping
        // fanout will allocate its own card below.
        idx
    } else {
        let card = FanoutCard::new("agent_swarm".to_string());
        app.add_message(HistoryCell::SubAgent(SubAgentCell::Fanout(card)));
        let idx = app.history.len().saturating_sub(1);
        app.last_fanout_card_index = Some(idx);
        idx
    };
    app.swarm_card_index.insert(outcome.swarm_id.clone(), idx);

    app.pending_swarm_task_count = None;

    let Some(HistoryCell::SubAgent(SubAgentCell::Fanout(card))) = app.history.get_mut(idx) else {
        return false;
    };
    card.kind = "agent_swarm".to_string();
    card.workers = workers;
    for task in &outcome.tasks {
        if let Some(agent_id) = task.agent_id.as_ref() {
            app.subagent_card_index.insert(agent_id.clone(), idx);
        }
    }

    if outcome.status.is_terminal() {
        app.pending_subagent_dispatch = None;
    }

    app.mark_history_updated();
    true
}

fn worker_slot_from_swarm_task(task: &crate::tools::swarm::SwarmTaskOutcome) -> WorkerSlot {
    let worker_id = if task.worker_id.trim().is_empty() {
        format!("task:{}", task.task_id)
    } else {
        task.worker_id.clone()
    };
    let agent_id = task
        .agent_id
        .clone()
        .or_else(|| Some(format!("task:{}", task.task_id)));
    let mut slot = WorkerSlot::with_agent(
        worker_id,
        agent_id,
        swarm_task_status_to_lifecycle(&task.status),
    );
    if !task.label.trim().is_empty() {
        slot.label = Some(task.label.clone());
    }
    if !task.model.trim().is_empty() {
        slot.model = Some(task.model.clone());
    }
    slot.nickname = task.nickname.clone();
    slot
}

fn status_to_lifecycle(status: &str) -> AgentLifecycle {
    match status.trim().to_ascii_lowercase().as_str() {
        "completed" => AgentLifecycle::Completed,
        "running" => AgentLifecycle::Running,
        "failed" | "interrupted" => AgentLifecycle::Failed,
        "cancelled" | "canceled" | "skipped" => AgentLifecycle::Cancelled,
        _ => AgentLifecycle::Pending,
    }
}

fn swarm_task_status_to_lifecycle(status: &SwarmTaskStatus) -> AgentLifecycle {
    match status {
        SwarmTaskStatus::Completed => AgentLifecycle::Completed,
        SwarmTaskStatus::Running => AgentLifecycle::Running,
        SwarmTaskStatus::Failed | SwarmTaskStatus::Interrupted => AgentLifecycle::Failed,
        SwarmTaskStatus::Cancelled | SwarmTaskStatus::Skipped => AgentLifecycle::Cancelled,
        SwarmTaskStatus::Pending => AgentLifecycle::Pending,
    }
}

pub(super) fn reconcile_subagent_activity_state(app: &mut App) {
    let running_agents: Vec<(String, String)> = app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
        .map(|agent| {
            (
                agent.agent_id.clone(),
                summarize_tool_output(&agent.assignment.objective),
            )
        })
        .collect();

    let running_ids: std::collections::HashSet<String> =
        running_agents.iter().map(|(id, _)| id.clone()).collect();
    app.agent_progress
        .retain(|id, _| running_ids.contains(id.as_str()));
    for (id, objective) in running_agents {
        app.agent_progress.entry(id).or_insert(objective);
    }

    if running_ids.is_empty() {
        app.agent_activity_started_at = None;
    } else if app.agent_activity_started_at.is_none() {
        app.agent_activity_started_at = Some(Instant::now());
    }
}

fn subagent_status_rank(status: &SubAgentStatus) -> u8 {
    match status {
        SubAgentStatus::Running => 0,
        SubAgentStatus::Interrupted(_) => 1,
        SubAgentStatus::Failed(_) => 2,
        SubAgentStatus::Completed => 3,
        SubAgentStatus::Cancelled => 4,
    }
}

pub(super) fn sort_subagents_in_place(agents: &mut [SubAgentResult]) {
    agents.sort_by(|a, b| {
        subagent_status_rank(&a.status)
            .cmp(&subagent_status_rank(&b.status))
            .then_with(|| a.agent_type.as_str().cmp(b.agent_type.as_str()))
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
}

/// Route a `MailboxMessage` envelope to the matching in-transcript card,
/// allocating a `DelegateCard` or `FanoutCard` on first sight (issue #128).
pub(super) fn handle_subagent_mailbox(app: &mut App, seq: u64, message: &MailboxMessage) {
    // Accumulate sub-agent token costs for the real-time footer counter (#166).
    if let MailboxMessage::TokenUsage { model, usage, .. } = message {
        if app.subagent_cost_event_seqs.insert(seq)
            && let Some(cost) = crate::pricing::calculate_turn_cost_from_usage(model, usage)
        {
            app.accrue_subagent_cost(cost);
        }
        return; // No card visual change needed; the footer handles display.
    }

    // Resolve (or allocate) the target cell for this envelope. ChildSpawned
    // is special — it always belongs to the active fanout card if one
    // exists; otherwise it seeds a new one.
    let agent_id = message.agent_id().to_string();
    let belongs_to_known_swarm = app.swarm_jobs.values().any(|outcome| {
        !outcome.status.is_terminal()
            && outcome
                .tasks
                .iter()
                .any(|task| task.agent_id.as_deref() == Some(agent_id.as_str()))
    });

    if (matches!(message, MailboxMessage::ChildSpawned { .. }) || belongs_to_known_swarm)
        && let Some(idx) = app.last_fanout_card_index
        && let Some(HistoryCell::SubAgent(SubAgentCell::Fanout(card))) = app.history.get_mut(idx)
    {
        apply_to_fanout(card, message);
        app.subagent_card_index.insert(agent_id, idx);
        app.mark_history_updated();
        return;
    }

    // Existing card for this agent_id? Mutate in place.
    if let Some(&idx) = app.subagent_card_index.get(&agent_id) {
        let updated = match app.history.get_mut(idx) {
            Some(HistoryCell::SubAgent(SubAgentCell::Delegate(card))) => {
                apply_to_delegate(card, message)
            }
            Some(HistoryCell::SubAgent(SubAgentCell::Fanout(card))) => {
                apply_to_fanout(card, message)
            }
            _ => false,
        };
        if updated {
            app.mark_history_updated();
        }
        return;
    }

    // No existing card — only `Started` reasonably opens one. Anything else
    // for an unknown agent_id is dropped (likely arrived after the cell was
    // cleared, e.g. session-resume edge cases).
    let MailboxMessage::Started { agent_type, .. } = message else {
        return;
    };

    let dispatch_kind = app.pending_subagent_dispatch.as_deref();
    let is_fanout = matches!(
        dispatch_kind,
        Some("agent_swarm" | "spawn_agents_on_csv" | "rlm")
    ) || belongs_to_known_swarm;

    if is_fanout {
        // Reuse the active fanout card for sibling spawns; otherwise create
        // one anchored at this position so subsequent siblings join it.
        if let Some(idx) = app.last_fanout_card_index
            && let Some(HistoryCell::SubAgent(SubAgentCell::Fanout(card))) =
                app.history.get_mut(idx)
        {
            card.claim_pending_worker(&agent_id, AgentLifecycle::Running);
            app.subagent_card_index.insert(agent_id, idx);
        } else {
            let mut card = FanoutCard::new(dispatch_kind.unwrap_or("swarm").to_string());
            card.upsert_worker(&agent_id, AgentLifecycle::Running);
            app.add_message(HistoryCell::SubAgent(SubAgentCell::Fanout(card)));
            let idx = app.history.len().saturating_sub(1);
            app.last_fanout_card_index = Some(idx);
            app.subagent_card_index.insert(agent_id, idx);
        }
    } else {
        let card = DelegateCard::new(agent_id.clone(), agent_type.clone());
        app.add_message(HistoryCell::SubAgent(SubAgentCell::Delegate(card)));
        let idx = app.history.len().saturating_sub(1);
        app.subagent_card_index.insert(agent_id, idx);
        // Single delegate consumes the pending dispatch label so a follow-on
        // tool call doesn't accidentally inherit it.
        app.pending_subagent_dispatch = None;
    }

    app.mark_history_updated();
}

pub(super) fn task_mode_label(mode: AppMode) -> &'static str {
    mode.as_setting()
}

pub(super) fn task_summary_to_panel_entry(summary: TaskSummary) -> TaskPanelEntry {
    TaskPanelEntry {
        id: summary.id,
        status: task_status_label(summary.status).to_string(),
        prompt_summary: summary.prompt_summary,
        duration_ms: summary.duration_ms,
    }
}

fn task_status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Canceled => "canceled",
    }
}

pub(super) fn format_task_list(tasks: &[TaskSummary]) -> String {
    if tasks.is_empty() {
        return "No tasks found.".to_string();
    }

    let mut lines = vec![
        format!("Tasks ({})", tasks.len()),
        "----------------------------------------".to_string(),
    ];
    for task in tasks {
        let duration = task
            .duration_ms
            .map(|ms| format!("{:.2}s", ms as f64 / 1000.0))
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!(
            "{}  {:9}  {}  {}",
            task.id,
            task_status_label(task.status),
            duration,
            task.prompt_summary
        ));
    }
    lines.push("Use /task show <id> for timeline details.".to_string());
    lines.join("\n")
}

pub(super) fn open_task_pager(app: &mut App, task: &TaskRecord) {
    let width = app
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(100)
        .saturating_sub(4);
    app.view_stack.push(PagerView::from_text(
        format!("Task {}", task.id),
        &format_task_detail(task),
        width.max(60),
    ));
}

fn format_task_detail(task: &TaskRecord) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Task: {}", task.id));
    lines.push(format!("Status: {}", task_status_label(task.status)));
    lines.push(format!("Mode: {}", task.mode));
    lines.push(format!("Model: {}", task.model));
    lines.push(format!(
        "Workspace: {}",
        crate::utils::display_path(&task.workspace)
    ));
    if let Some(thread_id) = task.thread_id.as_ref() {
        lines.push(format!("Runtime Thread: {thread_id}"));
    }
    if let Some(turn_id) = task.turn_id.as_ref() {
        lines.push(format!("Runtime Turn: {turn_id}"));
    }
    if task.runtime_event_count > 0 {
        lines.push(format!("Runtime Events: {}", task.runtime_event_count));
    }
    lines.push(format!("Created: {}", task.created_at));
    if let Some(started_at) = task.started_at {
        lines.push(format!("Started: {}", started_at));
    }
    if let Some(ended_at) = task.ended_at {
        lines.push(format!("Ended: {}", ended_at));
    }
    if let Some(duration) = task.duration_ms {
        lines.push(format!("Duration: {:.2}s", duration as f64 / 1000.0));
    }
    lines.push(String::new());
    lines.push("Prompt:".to_string());
    lines.push(task.prompt.clone());

    if let Some(summary) = task.result_summary.as_ref() {
        lines.push(String::new());
        lines.push("Result Summary:".to_string());
        lines.push(summary.clone());
    }
    if let Some(path) = task.result_detail_path.as_ref() {
        lines.push(format!("Result Artifact: {}", path.display()));
    }
    if let Some(error) = task.error.as_ref() {
        lines.push(String::new());
        lines.push(format!("Error: {error}"));
    }

    lines.push(String::new());
    lines.push("Tool Calls:".to_string());
    if task.tool_calls.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for tool in &task.tool_calls {
            let status = match tool.status {
                crate::task_manager::TaskToolStatus::Running => "running",
                crate::task_manager::TaskToolStatus::Success => "success",
                crate::task_manager::TaskToolStatus::Failed => "failed",
                crate::task_manager::TaskToolStatus::Canceled => "canceled",
            };
            let mut line = format!(
                "- {} [{}] {}",
                tool.name,
                status,
                tool.output_summary.as_deref().unwrap_or("(no summary)")
            );
            if let Some(duration) = tool.duration_ms {
                line.push_str(&format!(" ({:.2}s)", duration as f64 / 1000.0));
            }
            lines.push(line);
            if let Some(path) = tool.detail_path.as_ref() {
                lines.push(format!("  detail: {}", path.display()));
            }
            if let Some(path) = tool.patch_ref.as_ref() {
                lines.push(format!("  patch: {}", path.display()));
            }
        }
    }

    lines.push(String::new());
    lines.push("Timeline:".to_string());
    if task.timeline.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for entry in &task.timeline {
            lines.push(format!(
                "- [{}] {}: {}",
                entry.timestamp, entry.kind, entry.summary
            ));
            if let Some(path) = entry.detail_path.as_ref() {
                lines.push(format!("  detail: {}", path.display()));
            }
        }
    }

    lines.join("\n")
}
