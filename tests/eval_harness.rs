//! Integration tests for the offline evaluation harness.

use std::fs;

#[path = "../src/eval.rs"]
mod eval;

use eval::{EvalHarness, EvalHarnessConfig, ScenarioStepKind};

#[test]
fn runs_offline_tool_loop_successfully() {
    let harness = EvalHarness::default();
    let run = harness.run().expect("eval harness run should succeed");
    assert_eq!(
        ScenarioStepKind::parse("patch"),
        Some(ScenarioStepKind::ApplyPatch)
    );

    assert!(run.metrics.success, "expected success metrics: {run:#?}");
    assert_eq!(run.metrics.tool_errors, 0);
    assert_eq!(run.metrics.steps, 6);
    assert!(run.metrics.duration.as_millis() > 0);
    assert!(!run.scenario_name.is_empty());
    assert!(run.workspace_summary.file_count >= 3);

    for kind in [
        ScenarioStepKind::List,
        ScenarioStepKind::Read,
        ScenarioStepKind::Search,
        ScenarioStepKind::Edit,
        ScenarioStepKind::ApplyPatch,
        ScenarioStepKind::ExecShell,
    ] {
        let stats = run
            .metrics
            .per_tool
            .get(&kind)
            .expect("missing per-tool stats");
        assert_eq!(stats.invocations, 1, "unexpected invocations for {kind:?}");
        assert_eq!(stats.errors, 0, "unexpected errors for {kind:?}");
        assert!(stats.total_duration.as_nanos() > 0);
    }

    let notes_path = run.workspace_root().join("notes.txt");
    let notes = fs::read_to_string(&notes_path).expect("notes.txt should exist");
    assert!(notes.contains("edited = true"));
    assert!(notes.contains("todo: offline metrics (patched)"));

    let report = run.to_report();
    assert_eq!(report.metrics.success, run.metrics.success);
}

#[test]
fn records_tool_errors_when_step_fails() {
    let config = EvalHarnessConfig {
        fail_step: Some(ScenarioStepKind::ApplyPatch),
        ..EvalHarnessConfig::default()
    };
    let harness = EvalHarness::new(config);

    let run = harness
        .run()
        .expect("eval harness should return metrics even when a step fails");

    assert!(!run.metrics.success);
    assert!(run.metrics.tool_errors >= 1);

    let patch_stats = run
        .metrics
        .per_tool
        .get(&ScenarioStepKind::ApplyPatch)
        .expect("missing apply_patch stats");
    assert_eq!(patch_stats.invocations, 1);
    assert_eq!(patch_stats.errors, 1);

    let patch_step = run
        .steps
        .iter()
        .find(|step| step.kind == ScenarioStepKind::ApplyPatch)
        .expect("missing apply_patch step");
    assert!(!patch_step.success);
    assert!(patch_step.error.as_deref().is_some_and(|e| !e.is_empty()));
}

#[test]
fn validation_can_fail_without_tool_errors() {
    let config = EvalHarnessConfig {
        shell_expect_token: "definitely-not-in-output".to_string(),
        ..EvalHarnessConfig::default()
    };
    let harness = EvalHarness::new(config);

    let run = harness.run().expect("eval harness run should complete");

    assert_eq!(run.metrics.tool_errors, 0);
    assert!(
        !run.metrics.success,
        "validation should fail due to shell token"
    );
}
