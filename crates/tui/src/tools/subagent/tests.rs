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
fn test_render_instruction_template_replaces_columns() {
    let mut values = HashMap::new();
    values.insert("name".to_string(), "alpha".to_string());
    values.insert("owner".to_string(), "hunter".to_string());

    let rendered = render_instruction_template("Inspect {name} for {owner}", &values);
    assert_eq!(rendered, "Inspect alpha for hunter");
}

#[test]
fn test_render_instruction_template_preserves_escaped_braces() {
    let mut values = HashMap::new();
    values.insert("name".to_string(), "alpha".to_string());

    let rendered = render_instruction_template("literal {{x}} and {name}", &values);
    assert_eq!(rendered, "literal {x} and alpha");
}

#[test]
fn test_record_agent_job_result_accepts_first_report_only() {
    let job_id = "job_test_reports";
    clear_agent_job_results(job_id);
    record_agent_job_assignment(job_id, "item-1", "agent_1");

    assert!(record_agent_job_result(
        job_id,
        "item-1",
        json!({"status":"ok"}),
        false,
        Some("agent_1")
    ));
    assert!(!record_agent_job_result(
        job_id,
        "item-1",
        json!({"status":"duplicate"}),
        true,
        Some("agent_1")
    ));

    let report = take_agent_job_result(job_id, "item-1").expect("report should exist");
    assert_eq!(report.result["status"], "ok");
    assert!(!report.stop);
    assert!(take_agent_job_result(job_id, "item-1").is_none());
    clear_agent_job_results(job_id);
}

#[test]
fn test_record_agent_job_result_rejects_wrong_agent_assignment() {
    let job_id = "job_test_reports_wrong_agent";
    clear_agent_job_results(job_id);
    record_agent_job_assignment(job_id, "item-1", "agent_good");

    assert!(!record_agent_job_result(
        job_id,
        "item-1",
        json!({"status":"bad"}),
        false,
        Some("agent_bad")
    ));
    assert!(take_agent_job_result(job_id, "item-1").is_none());
    clear_agent_job_results(job_id);
}

#[test]
fn test_record_agent_job_result_rejects_missing_agent_assignment_context() {
    let job_id = "job_test_reports_missing_agent_context";
    clear_agent_job_results(job_id);
    record_agent_job_assignment(job_id, "item-1", "agent_good");

    assert!(!record_agent_job_result(
        job_id,
        "item-1",
        json!({"status":"bad"}),
        false,
        None
    ));
    assert!(take_agent_job_result(job_id, "item-1").is_none());
    clear_agent_job_results(job_id);
}

#[test]
fn test_validate_output_schema_enforces_required_fields() {
    let schema = json!({
        "type": "object",
        "required": ["status", "score"]
    });
    let ok_payload = json!({"status":"ok","score":1});
    assert!(validate_output_schema(&schema, &ok_payload).is_ok());

    let missing = json!({"status":"ok"});
    let err = validate_output_schema(&schema, &missing).expect_err("missing required field");
    assert!(err.contains("missing required field 'score'"));
}

#[test]
fn test_default_results_csv_path_uses_input_stem() {
    let path = PathBuf::from("/tmp/inventory.csv");
    let output = default_results_csv_path(&path);
    assert_eq!(output, PathBuf::from("/tmp/inventory.results.csv"));
}

#[test]
fn test_parse_csv_concurrency_prefers_max_concurrency() {
    let input = json!({
        "max_workers": 3,
        "max_concurrency": 9
    });
    assert_eq!(parse_csv_concurrency(&input), 9);
}

#[test]
fn test_load_csv_rows_uses_id_column_and_row_fallback() {
    let tmp = tempdir().expect("tempdir");
    let csv_path = tmp.path().join("rows.csv");
    std::fs::write(&csv_path, "id,name\nalpha,First\n,Second\n").expect("write csv");

    let rows = load_csv_rows(&csv_path, Some("id")).expect("load rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].item_id, "alpha");
    assert_eq!(rows[1].item_id, "row-2");
    assert_eq!(
        rows[1].values.get("name").map(String::as_str),
        Some("Second")
    );
}

#[test]
fn test_load_csv_rows_dedupes_item_ids() {
    let tmp = tempdir().expect("tempdir");
    let csv_path = tmp.path().join("rows.csv");
    std::fs::write(&csv_path, "id,name\nfoo,First\nfoo,Second\n").expect("write csv");

    let rows = load_csv_rows(&csv_path, Some("id")).expect("load rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].item_id, "foo");
    assert_eq!(rows[1].item_id, "foo-2");
}

#[test]
fn test_load_csv_rows_rejects_duplicate_headers() {
    let tmp = tempdir().expect("tempdir");
    let csv_path = tmp.path().join("rows.csv");
    std::fs::write(&csv_path, "id,id\nfoo,bar\n").expect("write csv");

    let err = load_csv_rows(&csv_path, Some("id")).expect_err("duplicate headers should fail");
    assert!(err.to_string().contains("duplicate header"));
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
fn test_allowed_tools_shell_filter() {
    let tools = build_allowed_tools(&SubAgentType::General, None, false).unwrap();
    assert!(!tools.contains(&"exec_shell".to_string()));
    assert!(!tools.contains(&"exec_shell_wait".to_string()));
    assert!(!tools.contains(&"exec_shell_interact".to_string()));
    assert!(!tools.contains(&"exec_wait".to_string()));
    assert!(!tools.contains(&"exec_interact".to_string()));
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
        vec!["read_file".to_string(), "grep_files".to_string()]
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
    let context = ToolContext::new(tmp.path().to_path_buf());
    let registry = SubAgentToolRegistry::new(
        context,
        vec!["read_file".to_string(), "missing_tool".to_string()],
        false,
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
        vec!["read_file".to_string()],
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
        vec!["read_file".to_string()],
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
        vec!["read_file".to_string()],
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
        vec!["read_file".to_string()],
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
        vec!["read_file".to_string()],
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
        vec!["read_file".to_string()],
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
        vec!["read_file".to_string()],
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
