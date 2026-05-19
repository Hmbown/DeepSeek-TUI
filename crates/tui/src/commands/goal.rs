//! `/goal` command — set a session objective with token budget,
//! progress tracking, and (since #891) goal-driven auto-continue.
//!
//! ## Lifecycle
//!
//! - `/goal <text>` — set objective AND enable auto-continue. Every
//!   subsequent `TurnComplete` is intercepted: if the model didn't
//!   emit the literal sentinel `GOAL_ACHIEVED` and none of the four
//!   safety nets fired, a synthetic continuation user-message is
//!   queued so the next turn keeps pushing toward the goal.
//! - `/goal <text> | budget: <N>` — same, with a token budget that
//!   feeds the `BudgetExceeded` safety net.
//! - `/goal` — show current status (objective, elapsed, auto-continue
//!   on/off, iterations so far).
//! - `/goal stop` — pause auto-continue without clearing the
//!   objective. The user can manually send another message to
//!   continue working with the goal context intact.
//! - `/goal resume` — re-enable auto-continue (e.g. after `stop`).
//! - `/goal done` / `/goal clear` / `/goal reset` — clear everything.
//!
//! ## Safety nets (see [`decide_continuation`])
//!
//! 1. `BudgetExceeded` — total conversation tokens crossed the budget.
//! 2. `Stuck` — `STUCK_TURN_THRESHOLD` (3) consecutive turns where
//!    *both* the pending-todo count stayed flat *and* token usage
//!    barely moved (< `STUCK_TOKEN_DELTA` increment). The double
//!    condition (work-order §8 follow-up) prevents the safety net
//!    from misfiring on slow-but-substantive long tasks.
//! 3. `Idle` — `IDLE_TURN_THRESHOLD` (2) consecutive turns where the
//!    model spoke without calling any tool.
//! 4. `MaxIterations` — total continuations for this objective
//!    crossed `max_iterations` (default 50).
//!
//! The decision logic is split into a pure function ([`decide_continuation`])
//! so the safety nets can be unit-tested without standing up the full
//! TUI runtime.

use crate::tui::app::App;

use super::CommandResult;

/// Stuck safety net fires after this many consecutive flat turns.
pub const STUCK_TURN_THRESHOLD: u32 = 3;

/// Token delta below which we treat the previous turn as "no real
/// progress" for the Stuck heuristic. Picked to ignore boilerplate
/// overhead (system prompt re-injection, brief acknowledgements)
/// while catching genuine spinning.
pub const STUCK_TOKEN_DELTA: u32 = 200;

/// Idle safety net fires after this many consecutive no-tool turns.
pub const IDLE_TURN_THRESHOLD: u32 = 2;

/// Default ceiling for total auto-continuation iterations. Overridable
/// from `[goal] max_iterations` in `config.toml`.
pub const DEFAULT_MAX_ITERATIONS: u32 = 50;

/// Sentinel the model emits to declare the goal achieved. Matched
/// case-sensitively; we require it on its own line so casual mentions
/// in narrative text don't false-positive.
pub const GOAL_ACHIEVED_SENTINEL: &str = "GOAL_ACHIEVED";

/// Outcome of [`decide_continuation`]. Drives the post-turn handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalDecision {
    /// Goal is not active, or auto-continue is off — do nothing.
    Inactive,
    /// Queue a continuation user-message with the given body.
    Enqueue(String),
    /// Stop auto-continue and surface the reason in the transcript.
    Stop(GoalStopReason),
}

/// Why auto-continue stopped. Distinct values feed both the
/// transcript message and the test assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalStopReason {
    /// The model emitted the `GOAL_ACHIEVED` sentinel on its own
    /// line — the goal is treated as complete and cleared.
    Achieved,
    /// Token budget was set and total conversation tokens met or
    /// exceeded it.
    BudgetExceeded { used: u32, budget: u32 },
    /// `STUCK_TURN_THRESHOLD` flat turns AND token usage delta
    /// under `STUCK_TOKEN_DELTA` per turn — looks like spinning.
    Stuck { turns: u32 },
    /// `IDLE_TURN_THRESHOLD` consecutive turns without any tool
    /// call — the model is just talking, not acting.
    Idle { turns: u32 },
    /// Reached the iteration cap (default 50, overridable).
    MaxIterations { cap: u32 },
    /// The user (or engine) interrupted the previous turn — we
    /// stop the chain so the human can take control without
    /// fighting an auto-pusher.
    Interrupted,
    /// The previous turn ended with `TurnOutcomeStatus::Failed`
    /// (stream stall, API error, network timeout, etc.). Continuing
    /// would just hit the same failure again and burn tokens, so we
    /// stop. The user can `/goal resume` once the underlying issue
    /// is cleared.
    TurnFailed,
}

impl GoalStopReason {
    /// User-facing reason string for the transcript / status line.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            GoalStopReason::Achieved => "goal achieved (model emitted GOAL_ACHIEVED)".into(),
            GoalStopReason::BudgetExceeded { used, budget } => {
                format!("token budget reached ({used}/{budget})")
            }
            GoalStopReason::Stuck { turns } => {
                format!("no progress after {turns} turn(s) — intervention needed")
            }
            GoalStopReason::Idle { turns } => {
                format!("{turns} consecutive turn(s) without tool calls — model is just talking")
            }
            GoalStopReason::MaxIterations { cap } => {
                format!("max iterations reached ({cap})")
            }
            GoalStopReason::Interrupted => "previous turn was interrupted".into(),
            GoalStopReason::TurnFailed => "previous turn failed (likely API / network error) — \
                 fix the underlying issue then /goal resume"
                .into(),
        }
    }
}

/// Inputs to the goal continuation decision. Bundled into a struct
/// so the decision is pure (no `&App`) and trivially testable.
///
/// The streak fields (`consecutive_no_progress_turns`,
/// `consecutive_no_tool_turns`) are post-update — callers run
/// [`update_streaks`] on the live `GoalState` *before* populating
/// this struct.
#[derive(Debug, Clone)]
pub struct GoalDecisionInputs<'a> {
    pub objective: Option<&'a str>,
    pub auto_continue_enabled: bool,
    pub last_assistant_text: &'a str,
    pub total_conversation_tokens: u32,
    pub token_budget: Option<u32>,
    pub iterations: u32,
    pub max_iterations: u32,
    pub consecutive_no_progress_turns: u32,
    pub consecutive_no_tool_turns: u32,
    pub turn_was_interrupted: bool,
    /// Previous turn ended in `TurnOutcomeStatus::Failed` (stream
    /// stall, API error, network timeout). Distinct from
    /// `turn_was_interrupted` because the user-facing stop reason
    /// and recovery story are different.
    pub turn_was_failed: bool,
}

/// Pure decision: given the post-turn snapshot, should we queue a
/// continuation, stop with a reason, or do nothing? Does **not**
/// mutate anything — caller threads the result back into
/// [`GoalState`](crate::tui::app::GoalState).
#[must_use]
pub fn decide_continuation(inputs: &GoalDecisionInputs<'_>) -> GoalDecision {
    let Some(objective) = inputs.objective else {
        return GoalDecision::Inactive;
    };
    if !inputs.auto_continue_enabled {
        return GoalDecision::Inactive;
    }

    // Achieved sentinel wins over everything else: a model emitting
    // GOAL_ACHIEVED on its own line is asserting success, and we
    // respect that (the user can always reopen with /goal <text>).
    if last_assistant_emits_achieved_sentinel(inputs.last_assistant_text) {
        return GoalDecision::Stop(GoalStopReason::Achieved);
    }

    if inputs.turn_was_interrupted {
        return GoalDecision::Stop(GoalStopReason::Interrupted);
    }

    // Infrastructure failures (stream stall, API timeout, transport
    // error) get their own stop reason so the user sees a different
    // message and knows to debug the connection rather than the
    // model. Continuing on a Failed status would just hit the same
    // wall over and over and burn budget.
    if inputs.turn_was_failed {
        return GoalDecision::Stop(GoalStopReason::TurnFailed);
    }

    if let Some(budget) = inputs.token_budget
        && inputs.total_conversation_tokens >= budget
    {
        return GoalDecision::Stop(GoalStopReason::BudgetExceeded {
            used: inputs.total_conversation_tokens,
            budget,
        });
    }

    if inputs.max_iterations > 0 && inputs.iterations >= inputs.max_iterations {
        return GoalDecision::Stop(GoalStopReason::MaxIterations {
            cap: inputs.max_iterations,
        });
    }

    // Idle counter is updated by the caller before invoking us.
    // We just compare against the threshold here.
    if inputs.consecutive_no_tool_turns >= IDLE_TURN_THRESHOLD {
        return GoalDecision::Stop(GoalStopReason::Idle {
            turns: inputs.consecutive_no_tool_turns,
        });
    }

    // Stuck: double-condition (flat pending + tiny token delta) over
    // STUCK_TURN_THRESHOLD turns. The single-condition #1242 design
    // (flat pending only) was prone to false positives on long
    // single-todo tasks where token usage was clearly advancing the
    // work; combining the two avoids that.
    if inputs.consecutive_no_progress_turns >= STUCK_TURN_THRESHOLD {
        return GoalDecision::Stop(GoalStopReason::Stuck {
            turns: inputs.consecutive_no_progress_turns,
        });
    }

    GoalDecision::Enqueue(continuation_prompt(objective))
}

/// Sentinel matcher. The work order requires the literal token on
/// its own line so a model that says "look for the GOAL_ACHIEVED
/// marker when you're done" mid-paragraph doesn't trip the check.
#[must_use]
pub fn last_assistant_emits_achieved_sentinel(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim() == GOAL_ACHIEVED_SENTINEL)
}

/// Continuation message body. Written verbatim per the work order
/// rather than generated dynamically — predictable string content
/// keeps the prefix-cache hit ratio high and the model's behaviour
/// stable across turns.
#[must_use]
pub fn continuation_prompt(objective: &str) -> String {
    format!(
        "Goal still active: \"{objective}\"\n\
         Continue executing toward the goal. If achieved, emit the literal token\n\
         GOAL_ACHIEVED on its own line and stop. Otherwise, take the next concrete\n\
         step now without asking for confirmation."
    )
}

/// First-turn kickstart prompt — sent automatically the moment
/// `/goal <text>` is processed so the user doesn't have to type
/// a placeholder message to get the auto-continue loop started.
/// Phrased differently from [`continuation_prompt`] so the model
/// sees "this is the start" rather than "we were already going."
#[must_use]
pub fn kickstart_prompt(objective: &str) -> String {
    format!(
        "New goal just set: \"{objective}\"\n\
         Begin executing toward this goal now. Take the first concrete step \
         without asking for confirmation. If the goal turns out to already be \
         satisfied, emit the literal token GOAL_ACHIEVED on its own line and stop."
    )
}

/// Update the streak counters that feed [`decide_continuation`].
/// Called by the post-turn handler BEFORE invoking the decision so
/// counters are current. Pure-ish — only touches `goal`'s streak
/// fields.
///
/// Returns the `progress_total_tokens` snapshot that the caller
/// should persist for the next turn's delta computation.
pub fn update_streaks(
    goal: &mut crate::tui::app::GoalState,
    pending_todo_count: usize,
    total_conversation_tokens: u32,
    last_turn_had_tool_call: bool,
) {
    // Idle streak: pure tool-call presence check.
    if last_turn_had_tool_call {
        goal.consecutive_no_tool_turns = 0;
    } else {
        goal.consecutive_no_tool_turns = goal.consecutive_no_tool_turns.saturating_add(1);
    }

    // Progress detection: both pending count change *and* large
    // token delta count as progress. Either resets the streak.
    let pending_changed = goal.last_pending_count != Some(pending_todo_count);
    let token_delta = total_conversation_tokens.saturating_sub(goal.last_progress_total_tokens);
    let token_jumped = token_delta >= STUCK_TOKEN_DELTA;
    let made_progress = pending_changed || token_jumped;

    if made_progress {
        goal.consecutive_no_progress_turns = 0;
        goal.last_progress_total_tokens = total_conversation_tokens;
    } else {
        goal.consecutive_no_progress_turns = goal.consecutive_no_progress_turns.saturating_add(1);
    }
    goal.last_pending_count = Some(pending_todo_count);
}

/// Set or show the current goal. Subcommands:
/// - `clear` / `reset` / `done` — wipe everything (including persisted file)
/// - `stop` — pause auto-continue (keep objective)
/// - `resume` — re-enable auto-continue
/// - any other non-empty text — set objective + enable auto-continue
/// - empty arg — show status
pub fn goal(app: &mut App, arg: Option<&str>) -> CommandResult {
    match arg.map(str::trim) {
        Some("clear") | Some("reset") | Some("done") => clear_goal(app),
        Some("stop") => stop_auto_continue(app),
        Some("resume") | Some("start") => resume_auto_continue(app),
        Some(text) if !text.is_empty() => set_goal(app, text),
        _ => show_goal(app),
    }
}

fn clear_goal(app: &mut App) -> CommandResult {
    app.goal.goal_objective = None;
    app.goal.goal_token_budget = None;
    app.goal.goal_started_at = None;
    app.goal.goal_started_at_utc = None;
    app.goal.auto_continue_enabled = false;
    app.goal.iterations = 0;
    app.goal.last_pending_count = None;
    app.goal.last_progress_total_tokens = 0;
    app.goal.consecutive_no_progress_turns = 0;
    app.goal.consecutive_no_tool_turns = 0;
    app.goal.session_id_for_persist = None;
    crate::goal_state::clear_goal();
    CommandResult::message("Goal cleared.")
}

fn stop_auto_continue(app: &mut App) -> CommandResult {
    if app.goal.goal_objective.is_none() {
        return CommandResult::message("No goal is set — nothing to stop.");
    }
    if !app.goal.auto_continue_enabled {
        return CommandResult::message("Auto-continue is already off.");
    }
    app.goal.auto_continue_enabled = false;
    persist_current_goal(app);
    CommandResult::message(
        "Auto-continue paused. Goal objective is preserved — send another \
         message to keep working, or /goal resume to re-enable auto-continue.",
    )
}

fn resume_auto_continue(app: &mut App) -> CommandResult {
    let Some(objective) = app.goal.goal_objective.clone() else {
        return CommandResult::message(
            "No goal is set. Use /goal <objective> to start a new goal.",
        );
    };
    if app.goal.auto_continue_enabled {
        return CommandResult::message("Auto-continue is already on.");
    }
    app.goal.auto_continue_enabled = true;
    // Reset the streak counters so we don't immediately trip a
    // safety net on resume just because the pre-stop turn was idle.
    app.goal.consecutive_no_tool_turns = 0;
    app.goal.consecutive_no_progress_turns = 0;
    persist_current_goal(app);
    // Kick the chain back into motion — same dispatcher path as
    // `set_goal` uses for its first-turn kickstart. Without this
    // the user has to type a placeholder message after `/goal resume`
    // before anything happens, which mirrors the original `/goal X`
    // friction we already fixed (regression spotted by user testing).
    // Use `continuation_prompt` (not `kickstart_prompt`) because the
    // objective is *already* known to the model from the system prompt
    // and the prior conversation — "still active" framing is more
    // accurate than "new goal just set" here.
    CommandResult::with_message_and_action(
        "Auto-continue resumed — pushing the next turn now.",
        crate::tui::app::AppAction::SendMessage(continuation_prompt(&objective)),
    )
}

fn set_goal(app: &mut App, text: &str) -> CommandResult {
    let (objective, budget) = parse_goal_budget(text);
    // Default-on for parity with #1242's UX. `[goal] auto_continue_default`
    // in config.toml lets conservative users flip the default off; that
    // value is threaded through via `App::new` (see `config.rs`) into
    // `app.goal_auto_continue_default` so we don't have to re-read the
    // config file on every `/goal`.
    let auto_default = app.goal_auto_continue_default;
    app.goal.goal_objective = Some(objective.clone());
    app.goal.goal_token_budget = budget;
    app.goal.goal_started_at = Some(std::time::Instant::now());
    app.goal.goal_started_at_utc = Some(chrono::Utc::now());
    app.goal.auto_continue_enabled = auto_default;
    app.goal.iterations = 0;
    app.goal.last_pending_count = None;
    app.goal.last_progress_total_tokens = app.session.total_conversation_tokens;
    app.goal.consecutive_no_progress_turns = 0;
    app.goal.consecutive_no_tool_turns = 0;
    app.goal.session_id_for_persist = app.current_session_id.clone();
    persist_current_goal(app);
    let budget_str = budget
        .map(|b| format!(" (budget: {b} tokens)"))
        .unwrap_or_default();
    // When auto-continue is on, kickstart the first turn automatically
    // so the user doesn't have to type a placeholder message. The
    // `CommandResult.action = SendMessage(...)` path is the same one
    // a typed user message goes through, so this turn looks identical
    // to the engine — submit-and-respond, then subsequent turns enter
    // the auto-continue loop via `maybe_trigger_goal_continuation`.
    // When auto-continue is OFF (user set `[goal] auto_continue_default
    // = false`), we don't kickstart — that mode is "passive tracking"
    // and a forced first turn would surprise the user.
    if auto_default {
        let kickstart = kickstart_prompt(&objective);
        CommandResult::with_message_and_action(
            format!(
                "Goal set: \"{objective}\"{budget_str} — kickstarting first turn now; /goal stop to pause."
            ),
            crate::tui::app::AppAction::SendMessage(kickstart),
        )
    } else {
        CommandResult::message(format!(
            "Goal set: \"{objective}\"{budget_str} — tracking only; /goal resume to enable auto-continue."
        ))
    }
}

fn show_goal(app: &mut App) -> CommandResult {
    if let Some(ref obj) = app.goal.goal_objective {
        // #447: render long elapsed times as `2d 3h` rather than
        // Rust's default Debug Duration (which produces e.g.
        // `188415.234s` for multi-day goals).
        let elapsed = app
            .goal
            .goal_started_at
            .map(|t| crate::tui::notifications::humanize_duration(t.elapsed()))
            .unwrap_or_else(|| "unknown".to_string());
        let budget_str = app
            .goal
            .goal_token_budget
            .map(|b| {
                let used = app.session.total_conversation_tokens;
                let pct = if b > 0 {
                    (f64::from(used) / f64::from(b) * 100.0).min(100.0)
                } else {
                    0.0
                };
                format!(" | tokens: {used}/{b} ({pct:.0}%)")
            })
            .unwrap_or_default();
        let auto_str = if app.goal.auto_continue_enabled {
            format!(" | auto-continue: on (turn #{})", app.goal.iterations)
        } else {
            " | auto-continue: off".to_string()
        };
        CommandResult::message(format!(
            "Goal: \"{obj}\" — elapsed: {elapsed}{budget_str}{auto_str}"
        ))
    } else {
        CommandResult::message(
            "No goal set. Use /goal <objective> [budget: N] to set one.\n\
             /goal stop|resume — pause/resume auto-continue without clearing.\n\
             /goal clear — remove the current goal.",
        )
    }
}

/// Snapshot the current GoalState into a [`PersistedGoal`] and
/// write it to disk. Caller controls clearing via [`clear_goal`].
fn persist_current_goal(app: &App) {
    if let Some(objective) = app.goal.goal_objective.clone() {
        let envelope = crate::goal_state::GoalFile {
            schema_version: 1,
            current: Some(crate::goal_state::PersistedGoal {
                objective,
                token_budget: app.goal.goal_token_budget,
                auto_continue_enabled: app.goal.auto_continue_enabled,
                started_at: app.goal.goal_started_at_utc,
                iterations: app.goal.iterations,
                session_id: app.goal.session_id_for_persist.clone(),
            }),
        };
        crate::goal_state::save_goal(&envelope);
    }
}

/// Parse optional token budget from goal text:
/// `"Implement login | budget: 50000"` or `"Implement login budget: 50000"`.
fn parse_goal_budget(text: &str) -> (String, Option<u32>) {
    if let Some((obj, rest)) = text.split_once(" | budget:") {
        let budget = rest
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u32>().ok());
        (obj.trim().to_string(), budget)
    } else if let Some((obj, rest)) = text.split_once("budget:") {
        let budget = rest
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u32>().ok());
        (obj.trim().to_string(), budget)
    } else {
        (text.trim().to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-flash".to_string(),
            workspace: PathBuf::from("."),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: true,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());
        // `App::new` calls `restore_persisted_goal()` which reads
        // a shared `~/.deepseek/goals.v1.json` (or the path under
        // `DEEPSEEK_TUI_GOAL_PATH`). Parallel sibling tests in this
        // module persist via the same path, so a fresh test could
        // see a stale objective they left behind. Zero the goal
        // sub-state explicitly to make each test hermetic.
        app.goal = crate::tui::app::GoalState::default();
        app
    }

    fn base_inputs<'a>(objective: &'a str, last: &'a str) -> GoalDecisionInputs<'a> {
        GoalDecisionInputs {
            objective: Some(objective),
            auto_continue_enabled: true,
            last_assistant_text: last,
            total_conversation_tokens: 1_000,
            token_budget: None,
            iterations: 1,
            max_iterations: 50,
            consecutive_no_progress_turns: 0,
            consecutive_no_tool_turns: 0,
            turn_was_interrupted: false,
            turn_was_failed: false,
        }
    }

    // --- Subcommand tests (cover the existing /goal surface plus
    // the new stop/resume lifecycle) ---

    #[test]
    fn test_set_goal_enables_auto_continue() {
        let mut app = create_test_app();
        let r = goal(&mut app, Some("Fix the login bug"));
        assert!(r.message.as_ref().unwrap().contains("Goal set"));
        assert!(
            r.message.as_ref().unwrap().contains("kickstarting"),
            "the user-facing message should advertise the kickstart"
        );
        assert_eq!(
            app.goal.goal_objective.as_deref(),
            Some("Fix the login bug")
        );
        assert!(
            app.goal.auto_continue_enabled,
            "setting an objective should default to auto-continue on"
        );
        // Cleanup: don't leave a stray ~/.deepseek/goals.v1.json behind.
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn set_goal_kickstarts_first_turn_with_send_message_action() {
        // Regression: /goal X used to require the user to type a
        // separate message before anything happened. Now the command
        // returns an AppAction::SendMessage(...) so the dispatcher
        // submits the first turn immediately.
        let mut app = create_test_app();
        let r = goal(&mut app, Some("Refactor auth module"));
        match r.action {
            Some(crate::tui::app::AppAction::SendMessage(body)) => {
                assert!(body.contains("Refactor auth module"));
                assert!(body.contains("Begin executing"));
                assert!(body.contains("GOAL_ACHIEVED"));
            }
            other => panic!("expected SendMessage action, got {other:?}"),
        }
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn set_goal_does_not_kickstart_when_auto_continue_default_off() {
        let mut app = create_test_app();
        app.goal_auto_continue_default = false;
        let r = goal(&mut app, Some("Quietly tracked goal"));
        assert!(
            r.action.is_none(),
            "no kickstart when [goal] auto_continue_default = false"
        );
        assert!(r.message.as_ref().unwrap().contains("tracking only"));
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn kickstart_prompt_contains_objective_and_sentinel_contract() {
        let body = kickstart_prompt("Cache invalidation rewrite");
        assert!(body.contains("Cache invalidation rewrite"));
        assert!(body.contains("Begin executing"));
        assert!(body.contains("GOAL_ACHIEVED"));
        // Crucial difference from continuation_prompt: this one
        // signals "starting", not "still active".
        assert!(body.contains("just set"));
    }

    #[test]
    fn test_set_goal_with_budget() {
        let mut app = create_test_app();
        let _ = goal(&mut app, Some("Refactor auth | budget: 50000"));
        assert_eq!(app.goal.goal_objective.as_deref(), Some("Refactor auth"));
        assert_eq!(app.goal.goal_token_budget, Some(50_000));
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn test_clear_goal_resets_streak_state() {
        let mut app = create_test_app();
        let _ = goal(&mut app, Some("test"));
        app.goal.iterations = 5;
        app.goal.consecutive_no_progress_turns = 2;
        let _ = goal(&mut app, Some("clear"));
        assert!(app.goal.goal_objective.is_none());
        assert!(app.goal.goal_token_budget.is_none());
        assert!(!app.goal.auto_continue_enabled);
        assert_eq!(app.goal.iterations, 0);
        assert_eq!(app.goal.consecutive_no_progress_turns, 0);
    }

    #[test]
    fn test_show_goal_when_none() {
        let mut app = create_test_app();
        let result = goal(&mut app, None);
        assert!(result.message.unwrap().contains("No goal set"));
    }

    #[test]
    fn slash_goal_stop_pauses_without_clearing() {
        let mut app = create_test_app();
        let _ = goal(&mut app, Some("Refactor"));
        assert!(app.goal.auto_continue_enabled);
        let r = goal(&mut app, Some("stop"));
        assert!(r.message.unwrap().contains("paused"));
        assert!(!app.goal.auto_continue_enabled);
        assert_eq!(
            app.goal.goal_objective.as_deref(),
            Some("Refactor"),
            "stop must preserve objective"
        );
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn slash_goal_resume_reenables() {
        let mut app = create_test_app();
        let _ = goal(&mut app, Some("Refactor"));
        let _ = goal(&mut app, Some("stop"));
        app.goal.consecutive_no_tool_turns = 4;
        app.goal.consecutive_no_progress_turns = 4;
        let r = goal(&mut app, Some("resume"));
        assert!(r.message.as_ref().unwrap().contains("resumed"));
        assert!(app.goal.auto_continue_enabled);
        assert_eq!(
            app.goal.consecutive_no_tool_turns, 0,
            "streaks must reset on resume so we don't immediately trip a safety net"
        );
        assert_eq!(app.goal.consecutive_no_progress_turns, 0);
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn slash_goal_resume_kickstarts_next_turn_via_send_message_action() {
        // Regression (post-PR #1809 review): /goal resume used to
        // only flip the flag, so after Esc-recovery the user had to
        // type a placeholder message before auto-continue actually
        // pushed forward. Resume now mirrors set_goal — returns an
        // AppAction::SendMessage so the dispatcher submits the next
        // turn immediately. Uses `continuation_prompt` (not
        // `kickstart_prompt`) because the objective is already known.
        let mut app = create_test_app();
        let _ = goal(&mut app, Some("Wire up X"));
        let _ = goal(&mut app, Some("stop"));
        let r = goal(&mut app, Some("resume"));
        match r.action {
            Some(crate::tui::app::AppAction::SendMessage(body)) => {
                assert!(body.contains("Wire up X"));
                assert!(body.contains("Goal still active"));
                assert!(body.contains("GOAL_ACHIEVED"));
            }
            other => panic!("expected SendMessage action on resume, got {other:?}"),
        }
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn slash_goal_resume_with_no_goal_returns_no_action() {
        let mut app = create_test_app();
        let r = goal(&mut app, Some("resume"));
        assert!(r.action.is_none());
        assert!(r.message.as_ref().unwrap().contains("No goal"));
    }

    #[test]
    fn slash_goal_resume_when_already_on_returns_no_action() {
        // Idempotent guard: re-running /goal resume while
        // auto-continue is already on shouldn't double-fire a turn.
        let mut app = create_test_app();
        let _ = goal(&mut app, Some("Wire up X"));
        // /goal X already enabled auto-continue. Calling resume
        // here must be a noop (no SendMessage action).
        let r = goal(&mut app, Some("resume"));
        assert!(
            r.action.is_none(),
            "resume must not kickstart when already on"
        );
        assert!(r.message.as_ref().unwrap().contains("already on"));
        let _ = goal(&mut app, Some("clear"));
    }

    #[test]
    fn slash_goal_stop_when_no_goal_says_so() {
        let mut app = create_test_app();
        let r = goal(&mut app, Some("stop"));
        assert!(r.message.unwrap().contains("No goal"));
    }

    #[test]
    fn test_parse_budget() {
        assert_eq!(
            parse_goal_budget("Do a thing | budget: 50000"),
            ("Do a thing".to_string(), Some(50_000))
        );
        assert_eq!(
            parse_goal_budget("Simple goal"),
            ("Simple goal".to_string(), None)
        );
        assert_eq!(
            parse_goal_budget("Goal budget:1000"),
            ("Goal".to_string(), Some(1000))
        );
    }

    // --- Decision-function tests (the heart of the 9 specs) ---

    #[test]
    fn turn_complete_with_active_goal_enqueues_continuation() {
        let inputs = base_inputs("refactor auth module", "Read the file. Now I'll continue.");
        let decision = decide_continuation(&inputs);
        match decision {
            GoalDecision::Enqueue(body) => {
                assert!(body.contains("Goal still active"));
                assert!(body.contains("refactor auth module"));
                assert!(body.contains("GOAL_ACHIEVED"));
            }
            other => panic!("expected Enqueue, got {other:?}"),
        }
    }

    #[test]
    fn goal_achieved_sentinel_stops_continuation() {
        let inputs = base_inputs("X", "All done.\nGOAL_ACHIEVED");
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::Achieved)
        );
    }

    #[test]
    fn goal_achieved_inline_does_not_false_positive() {
        // Mention of the sentinel mid-paragraph must NOT stop.
        let inputs = base_inputs(
            "X",
            "I'll remember to emit GOAL_ACHIEVED when done, but I'm not done yet.",
        );
        assert!(matches!(
            decide_continuation(&inputs),
            GoalDecision::Enqueue(_)
        ));
    }

    #[test]
    fn budget_exceeded_stops_continuation() {
        let mut inputs = base_inputs("X", "still working");
        inputs.token_budget = Some(1_000);
        inputs.total_conversation_tokens = 1_500;
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::BudgetExceeded {
                used: 1_500,
                budget: 1_000
            })
        );
    }

    #[test]
    fn stuck_three_turns_stops_continuation() {
        let mut inputs = base_inputs("X", "still thinking");
        inputs.consecutive_no_progress_turns = STUCK_TURN_THRESHOLD;
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::Stuck {
                turns: STUCK_TURN_THRESHOLD
            })
        );
    }

    #[test]
    fn stuck_double_condition_only_flat_pending_is_not_enough() {
        // Verifies the double-condition design improvement
        // (work-order §8 follow-up): pending count flat but token
        // delta clearly above threshold means we're still working.
        let mut goal_state = crate::tui::app::GoalState {
            last_pending_count: Some(2),
            last_progress_total_tokens: 1_000,
            consecutive_no_progress_turns: 0,
            ..Default::default()
        };
        // pending stayed at 2, but tokens jumped by 5000 (way above
        // STUCK_TOKEN_DELTA=200): NOT stuck, streak resets.
        update_streaks(&mut goal_state, 2, 6_000, true);
        assert_eq!(goal_state.consecutive_no_progress_turns, 0);
    }

    #[test]
    fn stuck_double_condition_flat_plus_tiny_delta_advances_streak() {
        let mut goal_state = crate::tui::app::GoalState {
            last_pending_count: Some(2),
            last_progress_total_tokens: 1_000,
            consecutive_no_progress_turns: 0,
            ..Default::default()
        };
        // pending flat AND token delta < threshold → spinning.
        update_streaks(&mut goal_state, 2, 1_050, true);
        assert_eq!(goal_state.consecutive_no_progress_turns, 1);
    }

    #[test]
    fn idle_two_turns_stops_continuation() {
        let mut inputs = base_inputs("X", "Talking but not acting");
        inputs.consecutive_no_tool_turns = IDLE_TURN_THRESHOLD;
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::Idle {
                turns: IDLE_TURN_THRESHOLD
            })
        );
    }

    #[test]
    fn max_iterations_stops_continuation() {
        let mut inputs = base_inputs("X", "still going");
        inputs.iterations = 50;
        inputs.max_iterations = 50;
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::MaxIterations { cap: 50 })
        );
    }

    #[test]
    fn auto_continue_disabled_returns_inactive() {
        let mut inputs = base_inputs("X", "anything");
        inputs.auto_continue_enabled = false;
        assert_eq!(decide_continuation(&inputs), GoalDecision::Inactive);
    }

    #[test]
    fn no_objective_returns_inactive() {
        let mut inputs = base_inputs("X", "anything");
        inputs.objective = None;
        assert_eq!(decide_continuation(&inputs), GoalDecision::Inactive);
    }

    #[test]
    fn interrupted_turn_stops_chain() {
        let mut inputs = base_inputs("X", "partial");
        inputs.turn_was_interrupted = true;
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::Interrupted)
        );
    }

    #[test]
    fn failed_turn_stops_chain() {
        // Regression: when the engine reports the previous turn
        // failed (stream stall, API timeout, transport error)
        // we must NOT queue another continuation — that would
        // just burn tokens against the same broken backend.
        let mut inputs = base_inputs("X", "");
        inputs.turn_was_failed = true;
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::TurnFailed)
        );
    }

    #[test]
    fn interrupted_takes_precedence_over_failed() {
        // Both flags set: surfacing "Interrupted" matches the
        // user's mental model (they hit Esc) better than the
        // infrastructure-flavoured "TurnFailed" message.
        let mut inputs = base_inputs("X", "");
        inputs.turn_was_interrupted = true;
        inputs.turn_was_failed = true;
        assert_eq!(
            decide_continuation(&inputs),
            GoalDecision::Stop(GoalStopReason::Interrupted)
        );
    }

    // --- update_streaks behavior tests ---

    #[test]
    fn idle_streak_increments_when_no_tool_used() {
        let mut g = crate::tui::app::GoalState::default();
        update_streaks(&mut g, 1, 0, false);
        update_streaks(&mut g, 1, 0, false);
        assert_eq!(g.consecutive_no_tool_turns, 2);
    }

    #[test]
    fn idle_streak_resets_on_tool_use() {
        let mut g = crate::tui::app::GoalState {
            consecutive_no_tool_turns: 5,
            ..Default::default()
        };
        update_streaks(&mut g, 1, 0, true);
        assert_eq!(g.consecutive_no_tool_turns, 0);
    }

    #[test]
    fn progress_streak_resets_when_pending_count_drops() {
        let mut g = crate::tui::app::GoalState {
            last_pending_count: Some(3),
            consecutive_no_progress_turns: 2,
            ..Default::default()
        };
        update_streaks(&mut g, 2, 0, true);
        assert_eq!(g.consecutive_no_progress_turns, 0);
        assert_eq!(g.last_pending_count, Some(2));
    }

    // --- Persistence is covered exhaustively in
    // [`crate::goal_state::tests`] with per-test isolated temp
    // paths. A bridge test here would race with the other
    // sub-command tests that also persist via `DEEPSEEK_TUI_GOAL_PATH`,
    // so we deliberately don't repeat it.
}
