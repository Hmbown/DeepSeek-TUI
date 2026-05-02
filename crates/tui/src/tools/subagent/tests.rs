use super::*;
use tempfile::tempdir;

fn make_assignment() -> SubAgentAssignment {
    SubAgentAssignment::new("prompt".to_string(), Some("worker".to_string()))
}

fn make_snapshot(status: SubAgentStatus) -> SubAgentResult {
    SubAgentResult {
        agent_id: "agent_test".to_string(),
        agent_type: SubAgentType::General,
        assignment: make_assignment(),
        model: "deepseek-v4-flash".to_string(),
        nickname: None,
        status,
        result: None,
        steps_taken: 0,
        duration_ms: 0,
    }
}

#[test]
fn test_agent_type_from_str() {
    assert_eq!(
        SubAgentType::from_str("general"),
        Some(SubAgentType::General)
    );
    assert_eq!(
        SubAgentType::from_str("explore"),
        Some(SubAgentType::Explore)
    );
    assert_eq!(SubAgentType::from_str("PLAN"), Some(SubAgentType::Plan));
    assert_eq!(
        SubAgentType::from_str("code-review"),
        Some(SubAgentType::Review)
    );
    assert_eq!(
        SubAgentType::from_str("worker"),
        Some(SubAgentType::General)
    );
    assert_eq!(
        SubAgentType::from_str("default"),
        Some(SubAgentType::General)
    );
    assert_eq!(
        SubAgentType::from_str("explorer"),
        Some(SubAgentType::Explore)
    );
    assert_eq!(SubAgentType::from_str("awaiter"), Some(SubAgentType::Plan));
    assert_eq!(SubAgentType::from_str("invalid"), None);
}

#[test]
fn test_parse_spawn_request_accepts_message_and_agent_type_aliases() {
    let input = json!({
        "message": "Find references to Foo",
        "agent_type": "explorer"
    });
    let parsed = parse_spawn_request(&input).expect("spawn request should parse");
    assert_eq!(parsed.prompt, "Find references to Foo");
    assert_eq!(parsed.agent_type, SubAgentType::Explore);
    assert_eq!(parsed.assignment.role.as_deref(), Some("explorer"));
}

#[test]
fn test_parse_spawn_request_accepts_objective_and_role_alias() {
    let input = json!({
        "objective": "Coordinate and wait",
        "role": "awaiter"
    });
    let parsed = parse_spawn_request(&input).expect("spawn request should parse");
    assert_eq!(parsed.prompt, "Coordinate and wait");
    assert_eq!(parsed.agent_type, SubAgentType::Plan);
    assert_eq!(parsed.assignment.role.as_deref(), Some("awaiter"));
}

#[test]
fn test_parse_spawn_request_accepts_items_payload() {
    let input = json!({
        "items": [
            {"type": "text", "text": "Analyze module"},
            {"type": "mention", "name": "drive", "path": "app://drive"}
        ],
        "agent_name": "explorer"
    });
    let parsed = parse_spawn_request(&input).expect("spawn request should parse");
    assert!(parsed.prompt.contains("Analyze module"));
    assert!(parsed.prompt.contains("[mention:$drive](app://drive)"));
    assert_eq!(parsed.agent_type, SubAgentType::Explore);
}

#[test]
fn test_parse_spawn_request_rejects_text_and_items_together() {
    let input = json!({
        "prompt": "Analyze module",
        "items": [{"type": "text", "text": "dup"}]
    });
    let err = parse_spawn_request(&input).expect_err("text+items should fail");
    assert!(err.to_string().contains("either prompt text or items"));
}

#[test]
fn test_parse_spawn_request_rejects_invalid_role() {
    let input = json!({
        "prompt": "do work",
        "role": "unknown_role"
    });
    let err = parse_spawn_request(&input).expect_err("invalid role should fail");
    assert!(err.to_string().contains("Invalid role alias"));
}

#[test]
fn test_parse_spawn_request_rejects_conflicting_type_and_role() {
    let input = json!({
        "prompt": "inspect internals",
        "type": "explore",
        "role": "worker"
    });
    let err = parse_spawn_request(&input).expect_err("conflicting type+role should fail");
    assert!(
        err.to_string()
            .contains("Conflicting type/agent_type and role/agent_role")
    );
}

#[test]
fn test_parse_assign_request_accepts_aliases() {
    let input = json!({
        "id": "agent_1234",
        "objective": "re-check failing tests",
        "agent_role": "explorer",
        "input": "focus on tests only",
        "interrupt": false
    });
    let request = parse_assign_request(&input).expect("assign request should parse");
    assert_eq!(request.agent_id, "agent_1234");
    assert_eq!(request.objective.as_deref(), Some("re-check failing tests"));
    assert_eq!(request.role.as_deref(), Some("explorer"));
    assert_eq!(request.message.as_deref(), Some("focus on tests only"));
    assert!(!request.interrupt);
}

#[test]
fn test_parse_assign_request_rejects_invalid_role() {
    let input = json!({
        "agent_id": "agent_1234",
        "role": "unknown"
    });
    let err = parse_assign_request(&input).expect_err("invalid role should fail");
    assert!(err.to_string().contains("Invalid role alias"));
}

#[test]
fn test_parse_assign_request_requires_update_fields() {
    let input = json!({
        "agent_id": "agent_1234"
    });
    let err = parse_assign_request(&input).expect_err("missing update fields should fail");
    assert!(
        err.to_string().contains(
            "Provide at least one of objective, role/agent_role, message/input, or items"
        )
    );
}

#[test]
fn test_send_input_schema_does_not_require_message_field() {
    let manager = Arc::new(Mutex::new(SubAgentManager::new(PathBuf::from("."), 1)));
    let schema = AgentSendInputTool::new(manager, "send_input").input_schema();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        !required
            .iter()
            .any(|entry| entry.as_str().is_some_and(|name| name == "message")),
        "send_input schema should allow items-only payloads"
    );
}

#[test]
fn test_build_allowed_tools_independent_of_allow_shell() {
    // v0.6.6: allow_shell no longer filters at the build_allowed_tools
    // level — the registry builder controls shell-tool registration.
    // Both calls return None (full inheritance) for a default General
    // agent.
    let with_shell = build_allowed_tools(&SubAgentType::General, None, true).unwrap();
    let without_shell = build_allowed_tools(&SubAgentType::General, None, false).unwrap();
    assert!(with_shell.is_none());
    assert!(without_shell.is_none());
}

#[test]
fn test_allowed_tools_are_deduplicated() {
    let tools = build_allowed_tools(
        &SubAgentType::Custom,
        Some(vec![
            "read_file".to_string(),
            "read_file".to_string(),
            "  ".to_string(),
            "grep_files".to_string(),
        ]),
        true,
    )
    .unwrap();
    assert_eq!(
        tools,
        Some(vec!["read_file".to_string(), "grep_files".to_string()])
    );
}

#[test]
fn test_custom_agent_requires_allowed_tools() {
    let err = build_allowed_tools(&SubAgentType::Custom, None, true).unwrap_err();
    assert!(err.to_string().contains("requires"));
}

#[test]
fn test_wait_mode_condition_any_and_all() {
    let one_done = vec![
        make_snapshot(SubAgentStatus::Running),
        make_snapshot(SubAgentStatus::Completed),
    ];
    let all_done = vec![
        make_snapshot(SubAgentStatus::Completed),
        make_snapshot(SubAgentStatus::Cancelled),
    ];

    assert!(WaitMode::Any.condition_met(&one_done));
    assert!(!WaitMode::All.condition_met(&one_done));
    assert!(WaitMode::All.condition_met(&all_done));
}

#[test]
fn test_parse_wait_mode() {
    assert_eq!(parse_wait_mode(&json!({})).unwrap(), WaitMode::Any);
    assert_eq!(
        parse_wait_mode(&json!({"wait_mode": "all"})).unwrap(),
        WaitMode::All
    );
    assert_eq!(
        parse_wait_mode(&json!({"wait_mode": "first"})).unwrap(),
        WaitMode::Any
    );
    assert!(parse_wait_mode(&json!({"wait_mode": "invalid"})).is_err());
}

#[test]
fn test_parse_wait_ids_accepts_aliases() {
    let ids = parse_wait_ids(&json!({
        "ids": ["agent_a", "agent_b"],
        "agent_id": "agent_c",
        "id": "agent_a"
    }));

    assert_eq!(ids, vec!["agent_a", "agent_b", "agent_c"]);
}

#[test]
fn test_parse_wait_ids_empty_when_omitted() {
    let ids = parse_wait_ids(&json!({}));
    assert!(ids.is_empty());
}

#[test]
fn test_build_assignment_prompt_includes_metadata() {
    let assignment = SubAgentAssignment::new(
        "Inspect parser behavior".to_string(),
        Some("explorer".to_string()),
    );
    let prompt = build_assignment_prompt(
        "Inspect parser behavior",
        &assignment,
        &SubAgentType::Explore,
    );
    assert!(prompt.contains("Assignment metadata"));
    assert!(prompt.contains("resolved_type: explore"));
    assert!(prompt.contains("role: explorer"));
}

#[test]
fn test_subagent_tool_registry_reports_unavailable_tools() {
    let tmp = tempdir().expect("tempdir");
    let mut runtime = stub_runtime();
    runtime.context = ToolContext::new(tmp.path().to_path_buf());
    runtime.allow_shell = false;
    let registry = SubAgentToolRegistry::new(
        runtime,
        Some(vec!["read_file".to_string(), "missing_tool".to_string()]),
        Arc::new(Mutex::new(TodoList::new())),
        Arc::new(Mutex::new(PlanState::default())),
    );
    assert_eq!(
        registry.unavailable_allowed_tools(),
        vec!["missing_tool".to_string()]
    );
}

#[tokio::test]
async fn test_wait_for_result_reports_timeout_when_still_running() {
    let manager = Arc::new(Mutex::new(SubAgentManager::new(PathBuf::from("."), 2)));
    let (input_tx, _input_rx) = mpsc::unbounded_channel();
    let agent = SubAgent::new(
        SubAgentType::Explore,
        "prompt".to_string(),
        make_assignment(),
        "deepseek-v4-flash".to_string(),
        Some("Blue".to_string()),
        Some(vec!["read_file".to_string()]),
        input_tx,
    );
    let agent_id = agent.id.clone();
    {
        let mut guard = manager.lock().await;
        guard.agents.insert(agent_id.clone(), agent);
    }

    let (snapshot, timed_out) = wait_for_result(&manager, &agent_id, Duration::from_millis(10))
        .await
        .expect("wait_for_result should succeed");
    assert!(timed_out);
    assert_eq!(snapshot.status, SubAgentStatus::Running);
}

#[test]
fn test_running_count_respects_limit() {
    let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
    let (input_tx, _input_rx) = mpsc::unbounded_channel();
    let mut agent = SubAgent::new(
        SubAgentType::Explore,
        "prompt".to_string(),
        make_assignment(),
        "deepseek-v4-flash".to_string(),
        Some("Blue".to_string()),
        Some(vec!["read_file".to_string()]),
        input_tx,
    );
    agent.status = SubAgentStatus::Running;
    manager.agents.insert(agent.id.clone(), agent);

    assert_eq!(manager.running_count(), 1);
}

#[tokio::test]
async fn test_running_count_ignores_finished_task_handles() {
    let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
    let (input_tx, _input_rx) = mpsc::unbounded_channel();
    let mut agent = SubAgent::new(
        SubAgentType::Explore,
        "prompt".to_string(),
        make_assignment(),
        "deepseek-v4-flash".to_string(),
        Some("Blue".to_string()),
        Some(vec!["read_file".to_string()]),
        input_tx,
    );
    agent.status = SubAgentStatus::Running;
    let handle = tokio::spawn(async {});
    handle.await.expect("dummy task should finish immediately");
    agent.task_handle = Some(tokio::spawn(async {}));
    if let Some(handle) = agent.task_handle.as_ref() {
        while !handle.is_finished() {
            tokio::task::yield_now().await;
        }
    }
    manager.agents.insert(agent.id.clone(), agent);

    assert_eq!(manager.running_count(), 0);
}

#[test]
fn test_assign_updates_running_agent_and_sends_message() {
    let mut manager = SubAgentManager::new(PathBuf::from("."), 2);
    let (input_tx, mut input_rx) = mpsc::unbounded_channel();
    let agent = SubAgent::new(
        SubAgentType::General,
        "work".to_string(),
        make_assignment(),
        "deepseek-v4-flash".to_string(),
        Some("Blue".to_string()),
        Some(vec!["read_file".to_string()]),
        input_tx,
    );
    let agent_id = agent.id.clone();
    manager.agents.insert(agent_id.clone(), agent);

    let snapshot = manager
        .assign(
            &agent_id,
            Some("Re-check module boundaries".to_string()),
            Some("explorer".to_string()),
            None,
            true,
        )
        .expect("assignment should succeed");
    assert_eq!(snapshot.assignment.objective, "Re-check module boundaries");
    assert_eq!(snapshot.assignment.role.as_deref(), Some("explorer"));

    let dispatched = input_rx
        .try_recv()
        .expect("running agent should receive assignment update");
    assert!(dispatched.interrupt);
    assert!(dispatched.text.contains("Assignment updated"));
    assert!(dispatched.text.contains("objective"));
}

#[test]
fn test_assign_rejects_message_for_non_running_agent() {
    let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
    let (input_tx, _input_rx) = mpsc::unbounded_channel();
    let mut agent = SubAgent::new(
        SubAgentType::Explore,
        "prompt".to_string(),
        make_assignment(),
        "deepseek-v4-flash".to_string(),
        Some("Blue".to_string()),
        Some(vec!["read_file".to_string()]),
        input_tx,
    );
    agent.status = SubAgentStatus::Completed;
    let agent_id = agent.id.clone();
    manager.agents.insert(agent_id.clone(), agent);

    let err = manager
        .assign(&agent_id, None, None, Some("keep going".to_string()), true)
        .expect_err("non-running agent cannot receive assignment message");
    assert!(err.to_string().contains("is not running"));
}

#[test]
fn test_assign_updates_non_running_metadata_without_message() {
    let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
    let (input_tx, _input_rx) = mpsc::unbounded_channel();
    let mut agent = SubAgent::new(
        SubAgentType::Plan,
        "prompt".to_string(),
        make_assignment(),
        "deepseek-v4-flash".to_string(),
        Some("Blue".to_string()),
        Some(vec!["read_file".to_string()]),
        input_tx,
    );
    agent.status = SubAgentStatus::Completed;
    let agent_id = agent.id.clone();
    manager.agents.insert(agent_id.clone(), agent);

    let snapshot = manager
        .assign(
            &agent_id,
            Some("Draft retry plan".to_string()),
            Some("awaiter".to_string()),
            None,
            true,
        )
        .expect("metadata update should succeed");
    assert_eq!(snapshot.assignment.objective, "Draft retry plan");
    assert_eq!(snapshot.assignment.role.as_deref(), Some("awaiter"));
}

#[test]
fn test_persist_and_reload_marks_running_agent_as_interrupted() {
    let tmp = tempdir().expect("tempdir");
    let workspace = tmp.path().to_path_buf();
    let state_path = default_state_path(tmp.path());

    let mut manager = SubAgentManager::new(workspace.clone(), 2).with_state_path(state_path);
    let (input_tx, _input_rx) = mpsc::unbounded_channel();
    let running = SubAgent::new(
        SubAgentType::General,
        "work".to_string(),
        make_assignment(),
        "deepseek-v4-flash".to_string(),
        Some("Blue".to_string()),
        Some(vec!["read_file".to_string()]),
        input_tx,
    );
    let running_id = running.id.clone();
    manager.agents.insert(running_id.clone(), running);
    manager.persist_state().expect("persist state");

    let mut reloaded =
        SubAgentManager::new(workspace, 2).with_state_path(default_state_path(tmp.path()));
    reloaded.load_state().expect("load state");
    let snapshot = reloaded
        .get_result(&running_id)
        .expect("reloaded agent should exist");
    assert!(matches!(
        snapshot.status,
        SubAgentStatus::Interrupted(ref message)
            if message.contains(SUBAGENT_RESTART_REASON)
    ));
}

#[test]
fn test_interrupted_status_name_and_summary() {
    let snapshot = make_snapshot(SubAgentStatus::Interrupted(
        SUBAGENT_RESTART_REASON.to_string(),
    ));
    assert_eq!(subagent_status_name(&snapshot.status), "interrupted");
    assert!(summarize_subagent_result(&snapshot).contains(SUBAGENT_RESTART_REASON));
}

// === Deprecation notice tests ===

/// Helper: build a plain ToolResult with a JSON payload.
fn make_plain_result(payload: serde_json::Value) -> crate::tools::spec::ToolResult {
    crate::tools::spec::ToolResult::json(&payload).expect("json result")
}

#[test]
fn test_wrap_with_deprecation_notice_adds_deprecation_block() {
    let result = make_plain_result(json!({"agent_id": "abc"}));
    let wrapped = wrap_with_deprecation_notice(result, "spawn_agent", "agent_spawn");

    let meta = wrapped.metadata.expect("metadata should be present");
    let dep = &meta["_deprecation"];
    assert_eq!(dep["this_tool"], "spawn_agent");
    assert_eq!(dep["use_instead"], "agent_spawn");
    assert_eq!(dep["removed_in"], DEPRECATION_REMOVAL_VERSION);
    assert!(
        dep["message"]
            .as_str()
            .unwrap_or("")
            .contains("spawn_agent")
    );
}

#[test]
fn test_wrap_with_deprecation_notice_preserves_existing_metadata() {
    let result = make_plain_result(json!({"agent_id": "abc"}))
        .with_metadata(json!({"status": "Running", "snapshot": {}}));
    let wrapped = wrap_with_deprecation_notice(result, "close_agent", "agent_cancel");

    let meta = wrapped.metadata.expect("metadata should be present");
    // Existing metadata key must survive.
    assert_eq!(meta["status"], "Running");
    // Deprecation block must be present alongside.
    assert_eq!(meta["_deprecation"]["this_tool"], "close_agent");
    assert_eq!(meta["_deprecation"]["use_instead"], "agent_cancel");
}

#[test]
fn test_canonical_agent_send_input_has_no_deprecation() {
    let manager = Arc::new(Mutex::new(SubAgentManager::new(PathBuf::from("."), 1)));
    // The canonical name "agent_send_input" must NOT receive a deprecation notice.
    // We verify this by inspecting the tool's name — the deprecation branch
    // only fires when name == "send_input".
    let tool = AgentSendInputTool::new(manager.clone(), "agent_send_input");
    assert_eq!(tool.name(), "agent_send_input");

    let alias = AgentSendInputTool::new(manager, "send_input");
    assert_eq!(alias.name(), "send_input");
}

#[test]
fn test_wrap_with_deprecation_notice_all_alias_mappings() {
    let cases = [
        ("spawn_agent", "agent_spawn"),
        ("delegate_to_agent", "agent_spawn"),
        ("close_agent", "agent_cancel"),
        ("send_input", "agent_send_input"),
    ];

    for (alias, canonical) in cases {
        let result = make_plain_result(json!({"ok": true}));
        let wrapped = wrap_with_deprecation_notice(result, alias, canonical);
        let meta = wrapped.metadata.expect("metadata for alias {alias}");
        assert_eq!(meta["_deprecation"]["this_tool"], alias, "alias={alias}");
        assert_eq!(
            meta["_deprecation"]["use_instead"], canonical,
            "alias={alias}"
        );
        assert_eq!(
            meta["_deprecation"]["removed_in"], DEPRECATION_REMOVAL_VERSION,
            "alias={alias}"
        );
    }
}

// === v0.6.6 — sub-agent authority unification ===

#[test]
fn build_allowed_tools_general_returns_none_for_full_inheritance() {
    // Default behavior: General agent with no explicit list inherits the
    // parent's full registry (None signals no narrowing).
    let result = build_allowed_tools(&SubAgentType::General, None, true).unwrap();
    assert!(
        result.is_none(),
        "General with no explicit_tools should default to full inheritance (None), got {result:?}"
    );
}

#[test]
fn build_allowed_tools_explore_returns_none_for_full_inheritance() {
    // Per-type allowlists are now advisory — Explore also gets the full
    // surface unless an explicit list is passed.
    let result = build_allowed_tools(&SubAgentType::Explore, None, true).unwrap();
    assert!(
        result.is_none(),
        "Explore with no explicit_tools should default to full inheritance"
    );
}

#[test]
fn build_allowed_tools_custom_requires_explicit_list() {
    // Custom is the one type that REQUIRES explicit allowed_tools.
    let err = build_allowed_tools(&SubAgentType::Custom, None, true).unwrap_err();
    assert!(
        err.to_string().contains("Custom sub-agent requires"),
        "got: {err}"
    );
}

#[test]
fn build_allowed_tools_explicit_list_returned_as_some() {
    let explicit = vec!["read_file".to_string(), "list_dir".to_string()];
    let result = build_allowed_tools(&SubAgentType::Custom, Some(explicit.clone()), true).unwrap();
    assert_eq!(result, Some(explicit));
}

#[test]
fn build_allowed_tools_explicit_list_dedupes_and_trims() {
    let explicit = vec![
        "read_file".to_string(),
        "  read_file  ".to_string(), // trim + dedupe
        "list_dir".to_string(),
        "".to_string(), // skip empty
    ];
    let result = build_allowed_tools(&SubAgentType::Custom, Some(explicit), true).unwrap();
    assert_eq!(
        result,
        Some(vec!["read_file".to_string(), "list_dir".to_string()])
    );
}

#[test]
fn parse_spawn_request_extracts_cwd_when_present() {
    let input = json!({
        "prompt": "build feature A",
        "cwd": ".worktrees/feature-a"
    });
    let parsed = parse_spawn_request(&input).expect("spawn request should parse");
    assert_eq!(
        parsed.cwd.as_ref().map(|p| p.to_string_lossy().to_string()),
        Some(".worktrees/feature-a".to_string())
    );
}

#[test]
fn parse_spawn_request_cwd_absent_yields_none() {
    let input = json!({ "prompt": "no cwd" });
    let parsed = parse_spawn_request(&input).expect("spawn request should parse");
    assert!(parsed.cwd.is_none());
}

#[test]
fn parse_spawn_request_cwd_empty_string_yields_none() {
    let input = json!({ "prompt": "empty cwd", "cwd": "   " });
    let parsed = parse_spawn_request(&input).expect("spawn request should parse");
    assert!(parsed.cwd.is_none(), "whitespace-only cwd should be None");
}

#[test]
fn build_subagent_system_prompt_appends_role_when_set() {
    let assignment = SubAgentAssignment::new("p".to_string(), Some("worker".to_string()));
    let prompt = build_subagent_system_prompt(&SubAgentType::General, &assignment);
    assert!(
        prompt.ends_with("You are operating in the role of `worker`."),
        "expected role line at end, got: {}",
        &prompt[prompt.len().saturating_sub(80)..]
    );
}

#[test]
fn build_subagent_system_prompt_skips_role_when_none() {
    let assignment = SubAgentAssignment::new("p".to_string(), None);
    let prompt = build_subagent_system_prompt(&SubAgentType::General, &assignment);
    assert!(!prompt.contains("You are operating in the role of"));
}

#[test]
fn build_subagent_system_prompt_skips_role_when_blank() {
    let assignment = SubAgentAssignment::new("p".to_string(), Some("   ".to_string()));
    let prompt = build_subagent_system_prompt(&SubAgentType::General, &assignment);
    assert!(!prompt.contains("You are operating in the role of"));
}

#[test]
fn subagent_done_sentinel_format_is_well_formed() {
    let res = make_snapshot(SubAgentStatus::Completed);
    let sentinel = subagent_done_sentinel("agent_xyz", &res);
    assert!(sentinel.starts_with("<deepseek:subagent.done>"));
    assert!(sentinel.ends_with("</deepseek:subagent.done>"));

    // The inner JSON parses and carries the expected fields.
    let inner = sentinel
        .trim_start_matches("<deepseek:subagent.done>")
        .trim_end_matches("</deepseek:subagent.done>");
    let parsed: serde_json::Value = serde_json::from_str(inner).expect("inner JSON parses");
    assert_eq!(parsed["agent_id"], "agent_xyz");
    assert_eq!(parsed["status"], "completed");
    assert_eq!(parsed["agent_type"], "general");
}

#[test]
fn subagent_failed_sentinel_format_is_well_formed() {
    let sentinel = subagent_failed_sentinel("agent_zzz", "boom");
    let inner = sentinel
        .trim_start_matches("<deepseek:subagent.done>")
        .trim_end_matches("</deepseek:subagent.done>");
    let parsed: serde_json::Value = serde_json::from_str(inner).expect("inner JSON parses");
    assert_eq!(parsed["agent_id"], "agent_zzz");
    assert_eq!(parsed["status"], "failed");
    assert_eq!(parsed["error"], "boom");
}

#[test]
fn subagent_runtime_default_max_depth_is_three() {
    // Sanity-check the constant — bumping it without a test means stale docs.
    assert_eq!(DEFAULT_MAX_SPAWN_DEPTH, 3);
}

#[test]
fn would_exceed_depth_at_boundary() {
    // depth=2, max=3 → next spawn (depth 3) is allowed (allow-equal).
    // depth=3, max=3 → next spawn (depth 4) exceeds.
    let runtime = stub_runtime();
    let mut at_max = runtime.clone();
    at_max.spawn_depth = 3;
    at_max.max_spawn_depth = 3;
    assert!(
        at_max.would_exceed_depth(),
        "depth 3 + max 3 → next would be 4, exceeds"
    );

    let mut below_max = runtime;
    below_max.spawn_depth = 2;
    below_max.max_spawn_depth = 3;
    assert!(
        !below_max.would_exceed_depth(),
        "depth 2 + max 3 → next is 3, allowed"
    );
}

#[test]
fn child_runtime_increments_depth_and_forces_auto_approve() {
    let mut parent = stub_runtime();
    parent.spawn_depth = 1;
    parent.context.auto_approve = false; // parent in suggest mode
    let child = parent.child_runtime();
    assert_eq!(child.spawn_depth, 2, "child depth = parent + 1");
    assert!(
        child.context.auto_approve,
        "child must auto-approve regardless of parent mode (spawning IS the approval)"
    );
    // Parent mode is unchanged — the override is on the child only.
    assert!(!parent.context.auto_approve);
}

#[test]
fn child_cancellation_cascades_from_parent() {
    let parent = stub_runtime();
    let child = parent.child_runtime();
    assert!(!child.cancel_token.is_cancelled());
    parent.cancel_token.cancel();
    assert!(
        child.cancel_token.is_cancelled(),
        "parent cancel() must propagate to child via child_token()"
    );
}

#[test]
fn mailbox_propagates_through_child_runtime_chain() {
    use crate::tools::subagent::mailbox::Mailbox;
    let parent_token = CancellationToken::new();
    let (mailbox, _rx) = Mailbox::new(parent_token.clone());

    let mut parent = stub_runtime();
    parent.cancel_token = parent_token;
    parent.mailbox = Some(mailbox);

    let child = parent.child_runtime();
    let grandchild = child.child_runtime();
    assert!(parent.mailbox.is_some());
    assert!(child.mailbox.is_some(), "child inherits parent mailbox");
    assert!(
        grandchild.mailbox.is_some(),
        "grandchild inherits via the cloned Arc inside Mailbox"
    );
}

#[tokio::test]
async fn mailbox_close_as_cancel_propagates_to_grandchild_runtime() {
    use crate::tools::subagent::mailbox::Mailbox;
    let parent_token = CancellationToken::new();
    let (mailbox, _rx) = Mailbox::new(parent_token.clone());

    let mut parent = stub_runtime();
    parent.cancel_token = parent_token;
    parent.mailbox = Some(mailbox.clone());

    let child = parent.child_runtime();
    let grandchild = child.child_runtime();
    assert!(!grandchild.cancel_token.is_cancelled());

    // Close the mailbox via *any* clone — the original or the one stored on
    // the runtime. Cancellation must reach all the way to the grandchild.
    mailbox.close();
    assert!(parent.cancel_token.is_cancelled());
    assert!(child.cancel_token.is_cancelled());
    assert!(
        grandchild.cancel_token.is_cancelled(),
        "close-as-cancel must propagate across max_spawn_depth=3"
    );
}

#[tokio::test]
async fn mailbox_orders_messages_from_parent_and_child_runtimes() {
    use crate::tools::subagent::mailbox::{Mailbox, MailboxMessage};
    let parent_token = CancellationToken::new();
    let (mailbox, mut rx) = Mailbox::new(parent_token.clone());

    let mut parent = stub_runtime();
    parent.cancel_token = parent_token;
    parent.mailbox = Some(mailbox);
    let child = parent.child_runtime();

    // Interleave sends from both runtimes; sequence numbers stay monotonic.
    parent
        .mailbox
        .as_ref()
        .unwrap()
        .send(MailboxMessage::progress("parent_a", "step 1"));
    child
        .mailbox
        .as_ref()
        .unwrap()
        .send(MailboxMessage::progress("child_b", "step 1"));
    parent
        .mailbox
        .as_ref()
        .unwrap()
        .send(MailboxMessage::progress("parent_a", "step 2"));

    let drained = rx.drain();
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0].seq, 1);
    assert_eq!(drained[1].seq, 2);
    assert_eq!(drained[2].seq, 3);
    // Verify ordering is preserved across publishers.
    match (
        &drained[0].message,
        &drained[1].message,
        &drained[2].message,
    ) {
        (
            MailboxMessage::Progress { agent_id: a, .. },
            MailboxMessage::Progress { agent_id: b, .. },
            MailboxMessage::Progress { agent_id: c, .. },
        ) => {
            assert_eq!(a, "parent_a");
            assert_eq!(b, "child_b");
            assert_eq!(c, "parent_a");
        }
        other => panic!("unexpected message order: {other:?}"),
    }
}

#[test]
fn persisted_empty_allowed_tools_loads_as_full_inheritance() {
    // Backward-compat: a v0.6.5 session that persisted with an empty Vec
    // (or a v0.6.6 session with no narrowing) should load as None on
    // restart, meaning full inheritance.
    let dir = tempdir().unwrap();
    let state_path = dir.path().join("subagents.v1.json");
    let payload = serde_json::json!({
        "schema_version": SUBAGENT_STATE_SCHEMA_VERSION,
        "agents": [{
            "id": "agent_test",
            "agent_type": "general",
            "prompt": "p",
            "assignment": { "objective": "p" },
            "status": "Completed",
            "result": null,
            "steps_taken": 0,
            "duration_ms": 0,
            "allowed_tools": [],
            "updated_at_ms": 0
        }]
    });
    std::fs::write(&state_path, payload.to_string()).unwrap();

    let mut manager = SubAgentManager::new(dir.path().to_path_buf(), 5).with_state_path(state_path);
    manager.load_state().expect("load should succeed");
    let agent = manager.agents.get("agent_test").expect("loaded agent");
    assert!(
        agent.allowed_tools.is_none(),
        "empty Vec on disk → None (full inheritance)"
    );
}

#[test]
fn persisted_non_empty_allowed_tools_loads_as_narrow() {
    // Backward-compat the other way: a v0.6.5 session that persisted with
    // an explicit narrow list keeps that list on reload.
    let dir = tempdir().unwrap();
    let state_path = dir.path().join("subagents.v1.json");
    let payload = serde_json::json!({
        "schema_version": SUBAGENT_STATE_SCHEMA_VERSION,
        "agents": [{
            "id": "agent_narrow",
            "agent_type": "custom",
            "prompt": "p",
            "assignment": { "objective": "p" },
            "status": "Completed",
            "result": null,
            "steps_taken": 0,
            "duration_ms": 0,
            "allowed_tools": ["read_file", "list_dir"],
            "updated_at_ms": 0
        }]
    });
    std::fs::write(&state_path, payload.to_string()).unwrap();

    let mut manager = SubAgentManager::new(dir.path().to_path_buf(), 5).with_state_path(state_path);
    manager.load_state().expect("load should succeed");
    let agent = manager.agents.get("agent_narrow").expect("loaded agent");
    assert_eq!(
        agent.allowed_tools.as_deref(),
        Some(&["read_file".to_string(), "list_dir".to_string()][..]),
        "non-empty Vec → Some(list), narrow scope preserved"
    );
}

/// Build a minimal `SubAgentRuntime` for tests that exercise pure runtime
/// helpers (depth, cancellation, child_runtime). Doesn't construct a real
/// HTTP client — calls that hit `runtime.client` would fail, but the
/// helpers we test here don't.
fn stub_runtime() -> SubAgentRuntime {
    use tokio_util::sync::CancellationToken;

    let workspace = std::env::temp_dir().join("deepseek-test-stub");
    let context = ToolContext::new(workspace.clone());
    SubAgentRuntime {
        client: stub_client(),
        model: "deepseek-v4-flash".to_string(),
        role_models: std::collections::HashMap::new(),
        context,
        allow_shell: true,
        event_tx: None,
        manager: new_shared_subagent_manager(workspace, 5),
        spawn_depth: 0,
        max_spawn_depth: DEFAULT_MAX_SPAWN_DEPTH,
        cancel_token: CancellationToken::new(),
        mailbox: None,
    }
}

/// A minimal stub client. Test helpers below only ever check struct fields
/// (depth, cancel_token, context); they don't call the network. We need a
/// *some* `DeepSeekClient` because `SubAgentRuntime.client` isn't
/// `Option<...>`. `Config::default()` is enough — `DeepSeekClient::new`
/// only validates that an API key field exists, not that the key works.
fn stub_client() -> DeepSeekClient {
    let config = crate::config::Config {
        api_key: Some("test-key".to_string()),
        ..crate::config::Config::default()
    };
    DeepSeekClient::new(&config).expect("stub client should construct")
}
