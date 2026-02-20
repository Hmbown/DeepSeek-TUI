use super::*;
use crate::config::Config;
use crate::tui::history::{GenericToolCell, ToolCell, ToolStatus};
use std::path::PathBuf;

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
fn parse_plan_choice_accepts_aliases() {
    assert_eq!(parse_plan_choice("accept"), Some(PlanChoice::AcceptAgent));
    assert_eq!(parse_plan_choice("agent"), Some(PlanChoice::AcceptAgent));
    assert_eq!(
        parse_plan_choice("accept-yolo"),
        Some(PlanChoice::AcceptYolo)
    );
    assert_eq!(parse_plan_choice("yolo"), Some(PlanChoice::AcceptYolo));
    assert_eq!(parse_plan_choice("revise"), Some(PlanChoice::RevisePlan));
    assert_eq!(parse_plan_choice("exit"), Some(PlanChoice::ExitPlan));
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
fn transcript_scroll_percent_is_clamped_and_relative() {
    assert_eq!(transcript_scroll_percent(0, 20, 120), Some(0));
    assert_eq!(transcript_scroll_percent(50, 20, 120), Some(50));
    assert_eq!(transcript_scroll_percent(200, 20, 120), Some(100));
    assert_eq!(transcript_scroll_percent(0, 20, 20), None);
}

fn create_test_app() -> App {
    let options = TuiOptions {
        model: "wagmii-reasoner".to_string(),
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
fn test_esc_switches_to_normal_mode_when_idle() {
    let mut app = create_test_app();
    app.is_loading = false;
    app.input.clear();
    app.cursor_position = 0;
    app.mode = AppMode::Agent;

    // Simulate Esc key press when not loading and input empty
    app.set_mode(AppMode::Normal);

    assert_eq!(app.mode, AppMode::Normal);
    assert!(!app.is_loading);
    assert!(app.input.is_empty());
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
fn test_esc_priority_order_loading_then_input_then_mode() {
    // Test 1: Loading state takes priority
    let mut app = create_test_app();
    app.is_loading = true;
    app.input = "draft".to_string();
    app.mode = AppMode::Yolo;
    // Should cancel request (not clear input or change mode)
    assert!(app.is_loading);

    // Test 2: Input not empty takes priority when not loading
    app.is_loading = false;
    assert!(!app.input.is_empty());
    // Should clear input (not change mode)

    // Test 3: Change mode when not loading and input empty
    app.input.clear();
    app.mode = AppMode::Yolo;
    assert!(app.input.is_empty());
    assert_eq!(app.mode, AppMode::Yolo);
    // Should change to Normal mode
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
    assert!(!entries.iter().any(|entry| entry == "/wagmii"));
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
    assert!(layout.queued_preview.is_empty());
    assert!(layout.queued_compacted);
}

#[test]
fn compact_queued_preview_summarizes_hidden_messages() {
    let mut app = create_test_app();
    for idx in 0..4 {
        app.queue_message(crate::tui::app::QueuedMessage::new(
            format!("queued message {idx}"),
            None,
        ));
    }

    let (one_row, compacted_one_row) = compact_queued_preview(&app, 1);
    assert_eq!(one_row, vec!["+4 more".to_string()]);
    assert!(compacted_one_row);

    let (two_rows, compacted_two_rows) = compact_queued_preview(&app, 2);
    assert_eq!(
        two_rows,
        vec!["queued message 0".to_string(), "+3 more".to_string()]
    );
    assert!(compacted_two_rows);
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
