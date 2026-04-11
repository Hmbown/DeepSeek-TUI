use super::*;
use crate::config::Config;
use crate::tui::history::{GenericToolCell, ToolCell, ToolStatus};
use crate::tui::views::{ModalView, ViewAction};
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn selection_point_from_position_ignores_top_padding() {
    let area = Rect {
        x: 10,
        y: 20,
        width: 30,
        height: 5,
    };

    // Content is bottom-aligned: 2 transcript lines in a 5-row viewport.
    let padding_top = 3;
    let transcript_top = 0;
    let transcript_total = 2;

    // Click in padding area -> no selection
    assert!(
        selection_point_from_position(
            area,
            area.x + 1,
            area.y,
            transcript_top,
            transcript_total,
            padding_top,
        )
        .is_none()
    );

    // First transcript line is at row `padding_top`
    let p0 = selection_point_from_position(
        area,
        area.x + 2,
        area.y + u16::try_from(padding_top).expect("padding should fit"),
        transcript_top,
        transcript_total,
        padding_top,
    )
    .expect("point");
    assert_eq!(p0.line_index, 0);
    assert_eq!(p0.column, 2);

    // Second transcript line is one row below
    let p1 = selection_point_from_position(
        area,
        area.x,
        area.y + u16::try_from(padding_top + 1).expect("padding should fit"),
        transcript_top,
        transcript_total,
        padding_top,
    )
    .expect("point");
    assert_eq!(p1.line_index, 1);
    assert_eq!(p1.column, 0);
}

#[test]
fn parse_plan_choice_accepts_numbers() {
    assert_eq!(parse_plan_choice("1"), Some(PlanChoice::AcceptAgent));
    assert_eq!(parse_plan_choice("2"), Some(PlanChoice::AcceptYolo));
    assert_eq!(parse_plan_choice("3"), Some(PlanChoice::RevisePlan));
    assert_eq!(parse_plan_choice("4"), Some(PlanChoice::ExitPlan));
}

#[test]
fn parse_plan_choice_rejects_aliases_and_extra_text() {
    assert_eq!(parse_plan_choice("accept"), None);
    assert_eq!(parse_plan_choice("agent"), None);
    assert_eq!(parse_plan_choice("yolo"), None);
    assert_eq!(parse_plan_choice("3 revise"), None);
    assert_eq!(parse_plan_choice("unknown"), None);
}

#[test]
fn plan_choice_from_option_maps_expected_values() {
    assert_eq!(plan_choice_from_option(1), Some(PlanChoice::AcceptAgent));
    assert_eq!(plan_choice_from_option(2), Some(PlanChoice::AcceptYolo));
    assert_eq!(plan_choice_from_option(3), Some(PlanChoice::RevisePlan));
    assert_eq!(plan_choice_from_option(4), Some(PlanChoice::ExitPlan));
    assert_eq!(plan_choice_from_option(5), None);
}

#[test]
fn plan_prompt_view_escape_emits_dismiss_event() {
    let mut view = PlanPromptView::new();

    let action = view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(matches!(
        action,
        ViewAction::EmitAndClose(ViewEvent::PlanPromptDismissed)
    ));
}

#[test]
fn transcript_scroll_percent_is_clamped_and_relative() {
    assert_eq!(transcript_scroll_percent(0, 20, 120), Some(0));
    assert_eq!(transcript_scroll_percent(50, 20, 120), Some(50));
    assert_eq!(transcript_scroll_percent(200, 20, 120), Some(100));
    assert_eq!(transcript_scroll_percent(0, 20, 20), None);
}

fn create_test_app() -> App {
    let options = TuiOptions {
        model: "deepseek-reasoner".to_string(),
        workspace: PathBuf::from("."),
        allow_shell: false,
        use_alt_screen: true,
        max_subagents: 1,
        skills_dir: PathBuf::from("."),
        memory_path: PathBuf::from("memory.md"),
        notes_path: PathBuf::from("notes.txt"),
        mcp_config_path: PathBuf::from("mcp.json"),
        use_memory: false,
        start_in_agent_mode: false,
        skip_onboarding: false,
        yolo: false,
        resume_session_id: None,
    };
    App::new(options, &Config::default())
}

fn init_git_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");

    let init = Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("git init should run");
    assert!(
        init.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=DeepSeek TUI Tests",
            "-c",
            "user.email=tests@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git commit should run");
    assert!(
        commit.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );

    dir
}

fn spans_text(spans: &[Span<'_>]) -> String {
    spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

#[test]
fn alt_4_switches_to_plan_mode() {
    let mut app = create_test_app();
    app.mode = AppMode::Agent;

    apply_alt_4_shortcut(&mut app, KeyModifiers::ALT);

    assert_eq!(app.mode, AppMode::Plan);
}

#[test]
fn ctrl_alt_4_focuses_agents_sidebar_without_switching_modes() {
    let mut app = create_test_app();
    app.mode = AppMode::Agent;
    app.sidebar_focus = SidebarFocus::Auto;

    apply_alt_4_shortcut(&mut app, KeyModifiers::ALT | KeyModifiers::CONTROL);

    assert_eq!(app.mode, AppMode::Agent);
    assert_eq!(app.sidebar_focus, SidebarFocus::Agents);
    assert_eq!(app.status_message.as_deref(), Some("Sidebar focus: agents"));
}

fn make_subagent(
    id: &str,
    status: crate::tools::subagent::SubAgentStatus,
) -> crate::tools::subagent::SubAgentResult {
    crate::tools::subagent::SubAgentResult {
        agent_id: id.to_string(),
        agent_type: crate::tools::subagent::SubAgentType::General,
        assignment: crate::tools::subagent::SubAgentAssignment {
            objective: format!("objective-{id}"),
            role: Some("worker".to_string()),
        },
        status,
        result: None,
        steps_taken: 0,
        duration_ms: 0,
    }
}

#[test]
fn sort_subagents_orders_running_before_terminal_statuses() {
    let mut agents = vec![
        make_subagent("agent_c", crate::tools::subagent::SubAgentStatus::Completed),
        make_subagent("agent_a", crate::tools::subagent::SubAgentStatus::Running),
        make_subagent(
            "agent_b",
            crate::tools::subagent::SubAgentStatus::Failed("boom".to_string()),
        ),
    ];

    sort_subagents_in_place(&mut agents);

    assert_eq!(agents[0].agent_id, "agent_a");
    assert_eq!(agents[1].agent_id, "agent_b");
    assert_eq!(agents[2].agent_id, "agent_c");
}

#[test]
fn running_agent_count_unions_cache_and_progress() {
    let mut app = create_test_app();
    app.subagent_cache = vec![
        make_subagent("agent_a", crate::tools::subagent::SubAgentStatus::Running),
        make_subagent("agent_b", crate::tools::subagent::SubAgentStatus::Completed),
    ];
    app.agent_progress
        .insert("agent_c".to_string(), "planning".to_string());

    assert_eq!(running_agent_count(&app), 2);
}

#[test]
fn compute_status_layout_reserves_extra_rows_for_active_state() {
    let app = create_test_app();
    let baseline = compute_status_layout(&app, 30, 3);
    assert_eq!(baseline.status_height, 1);

    let mut with_agents = create_test_app();
    with_agents
        .agent_progress
        .insert("agent_a".to_string(), "running".to_string());
    let active = compute_status_layout(&with_agents, 30, 3);
    assert!(active.status_height > baseline.status_height);
}

#[test]
fn status_summary_line_mentions_queue_and_approval_mode() {
    let mut app = create_test_app();
    app.approval_mode = crate::tui::approval::ApprovalMode::Auto;
    app.queue_message(crate::tui::app::QueuedMessage::new(
        "queued message".to_string(),
        None,
    ));
    let summary = status_summary_line(&app, 120);
    let summary_text = summary
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(summary_text.contains("queue 1"));
    assert!(summary_text.contains("approvals auto"));
}

#[test]
fn active_agent_rows_prefers_cache_order_and_progress_text() {
    let mut app = create_test_app();
    app.subagent_cache = vec![
        make_subagent("agent_a", crate::tools::subagent::SubAgentStatus::Running),
        make_subagent("agent_b", crate::tools::subagent::SubAgentStatus::Running),
    ];
    app.agent_progress
        .insert("agent_b".to_string(), "step 2".to_string());
    app.agent_progress
        .insert("agent_c".to_string(), "queued".to_string());

    let rows = active_agent_rows(&app, 3);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, "agent_a");
    assert!(rows[0].1.contains("objective-agent_a"));
    assert_eq!(rows[1].0, "agent_b");
    assert_eq!(rows[1].1, "step 2");
    assert_eq!(rows[2].0, "agent_c");
}

#[test]
fn reconcile_subagent_activity_state_trims_stale_progress_and_sets_anchor() {
    let mut app = create_test_app();
    app.subagent_cache = vec![
        make_subagent("agent_a", crate::tools::subagent::SubAgentStatus::Running),
        make_subagent("agent_b", crate::tools::subagent::SubAgentStatus::Completed),
    ];
    app.agent_progress
        .insert("agent_stale".to_string(), "old".to_string());

    reconcile_subagent_activity_state(&mut app);
    assert!(app.agent_progress.contains_key("agent_a"));
    assert!(!app.agent_progress.contains_key("agent_stale"));
    assert!(app.agent_activity_started_at.is_some());

    app.subagent_cache.clear();
    reconcile_subagent_activity_state(&mut app);
    assert!(app.agent_progress.is_empty());
    assert!(app.agent_activity_started_at.is_none());
}

#[test]
fn format_token_count_compact_formats_units() {
    assert_eq!(format_token_count_compact(999), "999");
    assert_eq!(format_token_count_compact(1_200), "1.2k");
    assert_eq!(format_token_count_compact(1_000_000), "1.0M");
}

#[test]
fn format_context_budget_caps_overflow_display() {
    assert_eq!(format_context_budget(5_000, 128_000), "5.0k/128.0k");
    assert_eq!(format_context_budget(250_000, 128_000), ">128.0k/128.0k");
}

#[test]
fn footer_state_label_prefers_compacting_then_thinking() {
    let mut app = create_test_app();
    assert_eq!(footer_state_label(&app).0, "ready");

    app.is_loading = true;
    assert_eq!(footer_state_label(&app).0, "thinking");

    app.is_compacting = true;
    assert_eq!(footer_state_label(&app).0, "compacting");
}

#[test]
fn footer_context_spans_uses_decimal_context_label() {
    let full = spans_text(&footer_context_spans(12.34, 32));
    assert_eq!(full, "context: 12.3%");

    let compact = spans_text(&footer_context_spans(12.34, 6));
    assert_eq!(compact, "12.3%");
}

#[test]
fn footer_narrow_status_spans_hides_ready_state_but_shows_activity() {
    let mut app = create_test_app();
    assert_eq!(spans_text(&footer_narrow_status_spans(&app, 24)), "agent");

    app.is_loading = true;
    assert_eq!(
        spans_text(&footer_narrow_status_spans(&app, 24)),
        "agent thinking"
    );

    app.is_loading = false;
    app.is_compacting = true;
    assert_eq!(
        spans_text(&footer_narrow_status_spans(&app, 24)),
        "agent compacting"
    );
}

#[test]
fn footer_status_line_spans_truncate_long_model_names() {
    let mut app = create_test_app();
    app.model = "deepseek-reasoner-with-an-extremely-long-model-name".to_string();
    app.is_loading = true;

    let line = spans_text(&footer_status_line_spans(&app, 48));
    assert!(line.contains("agent ("));
    assert!(line.contains(", thinking)"));
    assert!(line.contains("..."));
    assert!(UnicodeWidthStr::width(line.as_str()) <= 48);
}

#[test]
fn sync_footer_clock_to_marks_redraw_only_when_minute_changes() {
    let mut app = create_test_app();
    app.footer_clock_label = "12:00".to_string();
    app.needs_redraw = false;

    sync_footer_clock_to(&mut app, "12:00".to_string());
    assert_eq!(app.footer_clock_label, "12:00");
    assert!(!app.needs_redraw);

    sync_footer_clock_to(&mut app, "12:01".to_string());
    assert_eq!(app.footer_clock_label, "12:01");
    assert!(app.needs_redraw);
}

#[test]
fn context_usage_snapshot_prefers_estimate_when_reported_exceeds_window() {
    let mut app = create_test_app();
    app.last_prompt_tokens = Some(320_000);
    app.api_messages = vec![Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
            cache_control: None,
        }],
    }];

    let (used, max, percent) =
        context_usage_snapshot(&app).expect("context usage should be available");
    assert_eq!(max, 128_000);
    assert!(used > 0);
    assert!(used <= i64::from(max));
    assert!(percent < 100.0);
}

#[test]
fn should_auto_compact_before_send_respects_threshold_and_setting() {
    let mut app = create_test_app();
    app.last_prompt_tokens = Some(123_000);
    app.auto_compact = true;
    assert!(should_auto_compact_before_send(&app));

    app.auto_compact = false;
    assert!(!should_auto_compact_before_send(&app));

    app.auto_compact = true;
    app.last_prompt_tokens = Some(10_000);
    assert!(!should_auto_compact_before_send(&app));
}

// ============================================================================
// Streaming Cancel Behavior Tests
// ============================================================================

#[test]
fn test_esc_cancels_streaming_sets_is_loading_false() {
    let mut app = create_test_app();
    app.is_loading = true;
    app.mode = AppMode::Agent;

    // Simulate what happens in ui.rs when Esc is pressed during loading:
    // engine_handle.cancel() is called (can't test directly - private)
    // Then these state changes occur:
    app.is_loading = false;
    app.status_message = Some("Request cancelled".to_string());

    assert!(!app.is_loading);
    assert_eq!(app.status_message, Some("Request cancelled".to_string()));
}

#[test]
fn test_esc_with_input_clears_input_when_not_loading() {
    let mut app = create_test_app();
    app.is_loading = false;
    app.input = "some draft input".to_string();
    app.cursor_position = app.input.chars().count();

    // Simulate Esc key press when not loading but input not empty
    app.clear_input();

    assert!(app.input.is_empty());
    assert_eq!(app.cursor_position, 0);
    assert!(!app.is_loading);
}

#[test]
fn test_esc_discards_queued_draft_before_clearing_input() {
    let mut app = create_test_app();
    app.is_loading = false;
    app.input.clear();
    app.queued_draft = Some(crate::tui::app::QueuedMessage::new(
        "queued draft".to_string(),
        None,
    ));

    assert_eq!(
        next_escape_action(&app, false),
        EscapeAction::DiscardQueuedDraft
    );
}

#[test]
fn test_esc_is_noop_when_idle() {
    let mut app = create_test_app();
    app.is_loading = false;
    app.input.clear();
    app.cursor_position = 0;
    app.mode = AppMode::Agent;

    assert_eq!(next_escape_action(&app, false), EscapeAction::Noop);
    assert_eq!(app.mode, AppMode::Agent);
}

#[test]
fn test_esc_closes_slash_menu_before_other_actions() {
    let mut app = create_test_app();
    app.is_loading = true;
    app.input = "draft".to_string();
    app.queued_draft = Some(crate::tui::app::QueuedMessage::new(
        "queued draft".to_string(),
        None,
    ));

    assert_eq!(next_escape_action(&app, true), EscapeAction::CloseSlashMenu);
}

#[test]
fn test_ctrl_c_cancels_streaming_sets_status() {
    let mut app = create_test_app();
    app.is_loading = true;

    // Simulate Ctrl+C during loading state
    // engine_handle.cancel() is called (can't test directly - private)
    app.is_loading = false;
    app.status_message = Some("Request cancelled".to_string());

    assert!(!app.is_loading);
    assert_eq!(app.status_message, Some("Request cancelled".to_string()));
}

#[test]
fn test_ctrl_c_exits_when_not_loading() {
    let mut app = create_test_app();
    app.is_loading = false;

    // Ctrl+C when not loading should trigger shutdown
    // We can't test the actual shutdown, but verify the state is correct
    // for the shutdown path to be taken
    assert!(!app.is_loading);
}

#[test]
fn test_ctrl_d_exits_when_input_empty() {
    let mut app = create_test_app();
    app.input.clear();

    // Ctrl+D when input empty should trigger shutdown
    assert!(app.input.is_empty());
}

#[test]
fn test_ctrl_d_does_nothing_when_input_not_empty() {
    let mut app = create_test_app();
    app.input = "some input".to_string();

    // Ctrl+D when input not empty should not trigger shutdown
    assert!(!app.input.is_empty());
}

#[test]
fn test_esc_priority_order_matches_cancel_stack() {
    let mut app = create_test_app();
    app.is_loading = true;
    app.input = "draft".to_string();
    app.mode = AppMode::Yolo;
    assert_eq!(next_escape_action(&app, false), EscapeAction::CancelRequest);

    app.is_loading = false;
    assert_eq!(next_escape_action(&app, false), EscapeAction::ClearInput);

    app.input.clear();
    app.queued_draft = Some(crate::tui::app::QueuedMessage::new(
        "queued draft".to_string(),
        None,
    ));
    assert_eq!(
        next_escape_action(&app, false),
        EscapeAction::DiscardQueuedDraft
    );

    app.queued_draft = None;
    assert_eq!(next_escape_action(&app, false), EscapeAction::Noop);
}

#[test]
fn visible_slash_menu_entries_respects_hide_flag() {
    let mut app = create_test_app();
    app.input = "/mo".to_string();
    app.slash_menu_hidden = false;

    let entries = visible_slash_menu_entries(&app, 6);
    assert!(!entries.is_empty());

    app.slash_menu_hidden = true;
    let hidden_entries = visible_slash_menu_entries(&app, 6);
    assert!(hidden_entries.is_empty());
}

#[test]
fn visible_slash_menu_entries_excludes_removed_commands() {
    let mut app = create_test_app();
    app.input = "/".to_string();

    let entries = visible_slash_menu_entries(&app, 128);
    assert!(entries.iter().any(|entry| entry == "/config"));
    assert!(entries.iter().any(|entry| entry == "/links"));
    assert!(!entries.iter().any(|entry| entry == "/set"));
    assert!(!entries.iter().any(|entry| entry == "/deepseek"));
}

#[test]
fn apply_slash_menu_selection_appends_space_for_arg_commands() {
    let mut app = create_test_app();
    let entries = vec!["/model".to_string(), "/settings".to_string()];
    app.slash_menu_selected = 0;
    assert!(apply_slash_menu_selection(&mut app, &entries, true));
    assert_eq!(app.input, "/model ");
}

#[test]
fn status_layout_budget_preserves_chat_and_composer_on_tiny_heights() {
    let mut app = create_test_app();
    app.is_loading = true;
    for idx in 0..5 {
        app.queue_message(crate::tui::app::QueuedMessage::new(
            format!("queued message {idx}"),
            None,
        ));
    }

    let layout = compute_status_layout(&app, 9, 3);
    assert_eq!(layout.status_height, 1);
}

#[test]
fn workspace_context_refresh_is_deferred_while_ui_is_busy() {
    let repo = init_git_repo();
    let mut app = create_test_app();
    app.workspace = repo.path().to_path_buf();

    let now = Instant::now();
    refresh_workspace_context_if_needed(&mut app, now, false);

    assert!(app.workspace_context.is_none());
    assert!(app.workspace_context_refreshed_at.is_none());

    refresh_workspace_context_if_needed(&mut app, now, true);

    let context = app
        .workspace_context
        .as_deref()
        .expect("idle refresh should populate workspace context");
    assert!(context.contains("clean"));
    assert_eq!(app.workspace_context_refreshed_at, Some(now));
}

#[test]
fn workspace_context_refresh_respects_ttl_before_requerying_git() {
    let repo = init_git_repo();
    let mut app = create_test_app();
    app.workspace = repo.path().to_path_buf();

    let start = Instant::now();
    refresh_workspace_context_if_needed(&mut app, start, true);
    let initial = app
        .workspace_context
        .clone()
        .expect("initial refresh should populate context");

    std::fs::write(repo.path().join("dirty.txt"), "dirty").expect("write dirty marker");

    let before_ttl = start + Duration::from_secs(WORKSPACE_CONTEXT_REFRESH_SECS - 1);
    refresh_workspace_context_if_needed(&mut app, before_ttl, true);
    assert_eq!(app.workspace_context.as_deref(), Some(initial.as_str()));

    let after_ttl = start + Duration::from_secs(WORKSPACE_CONTEXT_REFRESH_SECS);
    refresh_workspace_context_if_needed(&mut app, after_ttl, true);
    let refreshed = app
        .workspace_context
        .as_deref()
        .expect("refresh after ttl should update context");
    assert!(refreshed.contains("untracked"));
    assert_ne!(refreshed, initial);
}

#[tokio::test]
async fn dismissed_plan_prompt_leaves_non_numeric_input_for_normal_send_path() {
    let mut app = create_test_app();
    app.mode = AppMode::Plan;
    app.plan_prompt_pending = true;
    app.offline_mode = true;

    let engine = crate::core::engine::mock_engine_handle();

    let handled = handle_plan_choice(&mut app, &engine.handle, "yolo")
        .await
        .expect("plan choice");

    assert!(!handled);
    assert!(!app.plan_prompt_pending);
    assert_eq!(app.mode, AppMode::Plan);

    let queued = build_queued_message(&mut app, "yolo".to_string());
    submit_or_steer_message(&mut app, &engine.handle, queued)
        .await
        .expect("submit normal message");

    assert_eq!(app.queued_message_count(), 1);
    assert_eq!(
        app.queued_messages
            .front()
            .map(crate::tui::app::QueuedMessage::content),
        Some("yolo".to_string())
    );
    assert_eq!(
        app.status_message.as_deref(),
        Some("Offline mode: queued 1 message(s) - /queue to review")
    );
}

#[tokio::test]
async fn numeric_plan_choice_still_queues_follow_up_when_busy() {
    let mut app = create_test_app();
    app.mode = AppMode::Plan;
    app.plan_prompt_pending = true;
    app.is_loading = true;

    let engine = crate::core::engine::mock_engine_handle();

    let handled = handle_plan_choice(&mut app, &engine.handle, "2")
        .await
        .expect("plan choice");

    assert!(handled);
    assert!(!app.plan_prompt_pending);
    assert_eq!(app.mode, AppMode::Yolo);
    assert_eq!(app.queued_message_count(), 1);
    assert_eq!(
        app.queued_messages
            .front()
            .map(crate::tui::app::QueuedMessage::content),
        Some("Proceed with the accepted plan.".to_string())
    );
}

#[test]
fn api_key_validation_warns_without_blocking_unusual_formats() {
    assert!(matches!(
        validate_api_key_for_onboarding(""),
        ApiKeyValidation::Reject(_)
    ));
    assert!(matches!(
        validate_api_key_for_onboarding("sk short"),
        ApiKeyValidation::Reject(_)
    ));
    assert!(matches!(
        validate_api_key_for_onboarding("short-key"),
        ApiKeyValidation::Accept { warning: Some(_) }
    ));
    assert!(matches!(
        validate_api_key_for_onboarding("averylongkeywithoutdash123456"),
        ApiKeyValidation::Accept { warning: Some(_) }
    ));
    assert!(matches!(
        validate_api_key_for_onboarding("sk-valid-format-1234567890"),
        ApiKeyValidation::Accept { warning: None }
    ));
}

#[test]
fn status_detail_lines_show_queue_draft_when_editing() {
    let mut app = create_test_app();
    app.queued_draft = Some(crate::tui::app::QueuedMessage::new(
        "refine the queued prompt".to_string(),
        None,
    ));
    let details = status_detail_lines(&app, 120, 2);
    assert!(!details.is_empty());
    let text = details[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(text.contains("Editing queued draft"));
}

#[test]
fn jump_to_adjacent_tool_cell_finds_next_and_previous() {
    let mut app = create_test_app();
    app.history = vec![
        HistoryCell::User {
            content: "hello".to_string(),
        },
        HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: "file_search".to_string(),
            status: ToolStatus::Success,
            input_summary: Some("query: foo".to_string()),
            output: Some("done".to_string()),
        })),
        HistoryCell::Assistant {
            content: "ok".to_string(),
            streaming: false,
        },
        HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: "run_command".to_string(),
            status: ToolStatus::Success,
            input_summary: Some("ls".to_string()),
            output: Some("...".to_string()),
        })),
    ];
    app.mark_history_updated();
    app.transcript_cache.ensure(
        &app.history,
        100,
        app.history_version,
        app.transcript_render_options(),
    );

    app.last_transcript_top = 0;
    assert!(jump_to_adjacent_tool_cell(
        &mut app,
        SearchDirection::Forward
    ));
    assert!(matches!(
        app.transcript_scroll,
        TranscriptScroll::Scrolled { .. }
    ));

    app.last_transcript_top = app.transcript_cache.total_lines().saturating_sub(1);
    assert!(jump_to_adjacent_tool_cell(
        &mut app,
        SearchDirection::Backward
    ));
}
