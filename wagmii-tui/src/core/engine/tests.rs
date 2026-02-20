use super::*;

use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::tempdir;

fn build_engine_with_capacity(capacity: CapacityControllerConfig) -> Engine {
    let mut engine_config = EngineConfig::default();
    engine_config.capacity = capacity;
    let (engine, _handle) = Engine::new(engine_config, &Config::default());
    engine
}

fn make_plan(
    read_only: bool,
    supports_parallel: bool,
    approval_required: bool,
    interactive: bool,
) -> ToolExecutionPlan {
    ToolExecutionPlan {
        index: 0,
        id: "tool-1".to_string(),
        name: "grep_files".to_string(),
        input: json!({"pattern": "test"}),
        caller: None,
        interactive,
        approval_required,
        approval_description: "desc".to_string(),
        supports_parallel,
        read_only,
        blocked_error: None,
    }
}

#[test]
fn engine_handle_cancel_tracks_latest_turn_token() {
    let (mut engine, handle) = Engine::new(EngineConfig::default(), &Config::default());
    let stale_token = engine.cancel_token.clone();

    engine.reset_cancel_token();
    handle.cancel();

    assert!(engine.cancel_token.is_cancelled());
    assert!(handle.is_cancelled());
    assert!(!stale_token.is_cancelled());
}

#[test]
fn parallel_batch_requires_read_only_parallel_tools() {
    let plans = vec![make_plan(true, true, false, false)];
    assert!(should_parallelize_tool_batch(&plans));

    let plans = vec![
        make_plan(true, true, false, false),
        make_plan(true, true, false, false),
    ];
    assert!(should_parallelize_tool_batch(&plans));

    let plans = vec![make_plan(false, true, false, false)];
    assert!(!should_parallelize_tool_batch(&plans));

    let plans = vec![make_plan(true, false, false, false)];
    assert!(!should_parallelize_tool_batch(&plans));

    let plans = vec![make_plan(true, true, true, false)];
    assert!(!should_parallelize_tool_batch(&plans));

    let plans = vec![make_plan(true, true, false, true)];
    assert!(!should_parallelize_tool_batch(&plans));
}

#[test]
fn tool_error_messages_include_actionable_hints() {
    let path_error = ToolError::path_escape(PathBuf::from("../escape.txt"));
    let formatted = format_tool_error(&path_error, "read_file");
    assert!(formatted.contains("escapes workspace"));

    let missing_field = ToolError::missing_field("path");
    let formatted = format_tool_error(&missing_field, "read_file");
    assert!(formatted.contains("missing required field"));

    let timeout = ToolError::Timeout { seconds: 5 };
    let formatted = format_tool_error(&timeout, "exec_shell");
    assert!(formatted.contains("timed out"));
}

#[test]
fn tool_exec_outcome_tracks_duration() {
    let outcome = ToolExecOutcome {
        index: 0,
        id: "tool-1".to_string(),
        name: "grep_files".to_string(),
        input: json!({"pattern": "test"}),
        started_at: Instant::now(),
        result: Ok(ToolResult::success("ok")),
    };

    assert!(outcome.started_at.elapsed().as_nanos() > 0);
}

#[test]
fn detects_context_length_errors_from_provider_payloads() {
    let msg = r#"SSE stream request failed: HTTP 400 Bad Request: {"error":{"message":"This model's maximum context length is 131072 tokens. However, you requested 153056 tokens (148960 in the messages, 4096 in the completion).","type":"invalid_request_error"}}"#;
    assert!(is_context_length_error_message(msg));
    assert!(!is_context_length_error_message(
        "SSE stream request failed: HTTP 400 Bad Request: model not found"
    ));
}

#[test]
fn context_budget_reserves_output_and_headroom() {
    let budget = context_input_budget("wagmii-reasoner", TURN_MAX_OUTPUT_TOKENS)
        .expect("wagmii models should have known context window");
    let expected = 128_000usize - 4_096usize - 1_024usize;
    assert_eq!(budget, expected);
}

#[tokio::test]
async fn pre_request_refresh_invoked_when_medium_risk() {
    let mut capacity = CapacityControllerConfig::default();
    capacity.enabled = true;
    capacity.low_risk_max = 0.0;
    capacity.medium_risk_max = 1.0;
    capacity.min_turns_before_guardrail = 0;

    let mut engine = build_engine_with_capacity(capacity.clone());
    engine.config.capacity = capacity.clone();
    engine.capacity_controller = CapacityController::new(capacity);
    engine.turn_counter = 5;
    engine
        .capacity_controller
        .mark_turn_start(engine.turn_counter);

    let long = "x".repeat(5_000);
    for _ in 0..200 {
        engine.session.messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: long.clone(),
                cache_control: None,
            }],
        });
    }

    let before = engine.estimated_input_tokens();
    let turn = TurnContext::new(10);
    let applied = engine
        .run_capacity_pre_request_checkpoint(&turn, None, AppMode::Agent)
        .await;
    let after = engine.estimated_input_tokens();

    assert!(applied);
    assert!(after < before);
}

#[tokio::test]
async fn post_tool_replay_invoked_when_high_non_severe_risk() {
    let tmp = tempdir().expect("tempdir");
    fs::write(tmp.path().join("sample.txt"), "hello replay").expect("write");

    let mut capacity = CapacityControllerConfig::default();
    capacity.enabled = true;
    capacity.low_risk_max = 0.0;
    capacity.medium_risk_max = 0.0;
    capacity.severe_min_slack = -10.0;
    capacity.severe_violation_ratio = 2.0;
    capacity.min_turns_before_guardrail = 0;

    let mut engine = build_engine_with_capacity(capacity.clone());
    engine.session.workspace = tmp.path().to_path_buf();
    engine.config.workspace = tmp.path().to_path_buf();
    engine.config.capacity = capacity.clone();
    engine.capacity_controller = CapacityController::new(capacity);
    engine.turn_counter = 4;
    engine
        .capacity_controller
        .mark_turn_start(engine.turn_counter);

    let mut turn = TurnContext::new(10);
    let mut tool_call = TurnToolCall::new(
        "tool_read_1".to_string(),
        "read_file".to_string(),
        json!({ "path": "sample.txt" }),
    );
    tool_call.set_result(
        "hello replay".to_string(),
        std::time::Duration::from_millis(1),
    );
    turn.record_tool_call(tool_call);

    let registry = ToolRegistryBuilder::new()
        .with_read_only_file_tools()
        .build(engine.build_tool_context(AppMode::Agent));

    let restarted = engine
        .run_capacity_post_tool_checkpoint(
            &turn,
            AppMode::Agent,
            Some(&registry),
            Arc::new(RwLock::new(())),
            None,
            0,
            0,
        )
        .await;

    assert!(!restarted);
    let has_verification_note = engine.session.messages.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::ToolResult { content, .. } => content.contains("[verification replay]"),
            _ => false,
        })
    });
    assert!(has_verification_note);
}

#[tokio::test]
async fn error_escalation_triggers_replan_when_severe_or_repeated_failures() {
    let tmp = tempdir().expect("tempdir");
    // Safety: scoped to test process; reset at end.
    unsafe {
        std::env::set_var(
            "WAGMII_CAPACITY_MEMORY_DIR",
            tmp.path().to_string_lossy().to_string(),
        );
    }

    let mut capacity = CapacityControllerConfig::default();
    capacity.enabled = true;
    capacity.low_risk_max = 0.0;
    capacity.medium_risk_max = 0.0;
    capacity.min_turns_before_guardrail = 0;

    let mut engine = build_engine_with_capacity(capacity.clone());
    engine.config.capacity = capacity.clone();
    engine.capacity_controller = CapacityController::new(capacity);
    engine.turn_counter = 6;
    engine
        .capacity_controller
        .mark_turn_start(engine.turn_counter);

    for i in 0..10 {
        engine.session.messages.push(Message {
            role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
            content: vec![ContentBlock::Text {
                text: format!("noise message {i}"),
                cache_control: None,
            }],
        });
    }
    engine.session.messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: "Please finish task".to_string(),
            cache_control: None,
        }],
    });

    let before_len = engine.session.messages.len();
    let turn = TurnContext::new(10);
    let restarted = engine
        .run_capacity_error_escalation_checkpoint(&turn, AppMode::Agent, 2, 2)
        .await;

    assert!(restarted);
    assert!(engine.session.messages.len() < before_len);
    assert!(engine.session.messages.len() <= 2);

    let records = load_last_k_capacity_records(&engine.session.id, 1).expect("load memory");
    assert!(!records.is_empty());
    assert!(!records[0].canonical_state.goal.is_empty());
    unsafe {
        std::env::remove_var("WAGMII_CAPACITY_MEMORY_DIR");
    }
}

#[tokio::test]
async fn controller_disabled_keeps_behavior_unchanged() {
    let mut capacity = CapacityControllerConfig::default();
    capacity.enabled = false;

    let mut engine = build_engine_with_capacity(capacity.clone());
    engine.config.capacity = capacity.clone();
    engine.capacity_controller = CapacityController::new(capacity);
    engine.turn_counter = 3;
    engine
        .capacity_controller
        .mark_turn_start(engine.turn_counter);

    let long = "y".repeat(5_000);
    for _ in 0..120 {
        engine.session.messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: long.clone(),
                cache_control: None,
            }],
        });
    }

    let before = engine.estimated_input_tokens();
    let before_len = engine.session.messages.len();
    let turn = TurnContext::new(10);
    let applied = engine
        .run_capacity_pre_request_checkpoint(&turn, None, AppMode::Agent)
        .await;
    let after = engine.estimated_input_tokens();
    let after_len = engine.session.messages.len();

    assert!(!applied);
    assert_eq!(before, after);
    assert_eq!(before_len, after_len);
}

#[test]
fn caller_policy_defaults_to_direct() {
    let tool = Tool {
        tool_type: None,
        name: "read_file".to_string(),
        description: "Read".to_string(),
        input_schema: json!({"type":"object"}),
        allowed_callers: Some(vec!["direct".to_string()]),
        defer_loading: Some(false),
        input_examples: None,
        strict: None,
        cache_control: None,
    };
    let direct = ToolCaller {
        caller_type: "direct".to_string(),
        tool_id: None,
    };
    let code = ToolCaller {
        caller_type: "code_execution_20250825".to_string(),
        tool_id: Some("srvtoolu_1".to_string()),
    };
    assert!(caller_allowed_for_tool(Some(&direct), Some(&tool)));
    assert!(!caller_allowed_for_tool(Some(&code), Some(&tool)));
    assert!(caller_allowed_for_tool(None, Some(&tool)));
}

#[test]
fn tool_search_activates_discovered_deferred_tools() {
    let mut catalog = vec![
        Tool {
            tool_type: None,
            name: "read_file".to_string(),
            description: "Read files".to_string(),
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"}}}),
            allowed_callers: Some(vec!["direct".to_string()]),
            defer_loading: Some(true),
            input_examples: None,
            strict: None,
            cache_control: None,
        },
        Tool {
            tool_type: None,
            name: "grep_files".to_string(),
            description: "Search files".to_string(),
            input_schema: json!({"type":"object","properties":{"pattern":{"type":"string"}}}),
            allowed_callers: Some(vec!["direct".to_string()]),
            defer_loading: Some(true),
            input_examples: None,
            strict: None,
            cache_control: None,
        },
    ];
    ensure_advanced_tooling(&mut catalog);
    let mut active = initial_active_tools(&catalog);
    let result = execute_tool_search(
        TOOL_SEARCH_BM25_NAME,
        &json!({"query":"read file"}),
        &catalog,
        &mut active,
    )
    .expect("search succeeds");
    assert!(result.success);
    assert!(active.contains("read_file"));
}

#[tokio::test]
async fn code_execution_runs_python_and_returns_result_payload() {
    let tmp = tempdir().expect("tempdir");
    let result =
        execute_code_execution_tool(&json!({"code":"print('hello from code exec')"}), tmp.path())
            .await
            .expect("code execution should run");
    assert!(result.content.contains("hello from code exec"));
    assert!(result.content.contains("return_code"));
}
