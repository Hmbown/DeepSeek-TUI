use super::*;

use crate::tools::spec::ToolContext;
use serde_json::{Value, json};
use tempfile::tempdir;

fn echo_command(message: &str) -> String {
    format!("echo {message}")
}

fn sleep_command(seconds: u64) -> String {
    #[cfg(windows)]
    {
        let ping_count = seconds.saturating_add(1);
        let ps_path = r#"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"#;
        format!(
            "\"{ps_path}\" -NoProfile -Command \"Start-Sleep -Seconds {seconds}\" || ping 127.0.0.1 -n {ping_count} > NUL"
        )
    }
    #[cfg(not(windows))]
    {
        format!("sleep {seconds}")
    }
}

fn sleep_then_echo_command(seconds: u64, message: &str) -> String {
    #[cfg(windows)]
    {
        let ping_count = seconds.saturating_add(1);
        let ps_path = r#"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"#;
        format!(
            "\"{ps_path}\" -NoProfile -Command \"Start-Sleep -Seconds {seconds}; Write-Output {message}\" || (ping 127.0.0.1 -n {ping_count} > NUL && echo {message})"
        )
    }
    #[cfg(not(windows))]
    {
        format!("sleep {seconds} && echo {message}")
    }
}

fn echo_stdin_command() -> String {
    #[cfg(windows)]
    {
        "more".to_string()
    }
    #[cfg(not(windows))]
    {
        "cat".to_string()
    }
}

#[test]
fn test_sync_execution() {
    let tmp = tempdir().expect("tempdir");
    let mut manager = ShellManager::new(tmp.path().to_path_buf());

    let result = manager
        .execute(&echo_command("hello"), None, 5000, false)
        .expect("execute");

    assert_eq!(result.status, ShellStatus::Completed);
    assert!(result.stdout.contains("hello"));
    assert!(result.task_id.is_none());
}

#[test]
fn test_background_execution() {
    let tmp = tempdir().expect("tempdir");
    let mut manager = ShellManager::new(tmp.path().to_path_buf());

    let result = manager
        .execute(&sleep_then_echo_command(1, "done"), None, 5000, true)
        .expect("execute");

    assert_eq!(result.status, ShellStatus::Running);
    assert!(result.task_id.is_some());

    let task_id = result
        .task_id
        .expect("background execution should return task_id");

    // Wait for completion
    let final_result = manager
        .get_output(&task_id, true, 5000)
        .expect("get_output");

    assert_eq!(final_result.status, ShellStatus::Completed);
    assert!(final_result.stdout.contains("done"));
}

#[test]
fn test_timeout() {
    let tmp = tempdir().expect("tempdir");
    let mut manager = ShellManager::new(tmp.path().to_path_buf());

    let result = manager
        .execute(&sleep_command(10), None, 1000, false)
        .expect("execute");

    assert_eq!(result.status, ShellStatus::TimedOut);
}

#[test]
fn test_kill() {
    let tmp = tempdir().expect("tempdir");
    let mut manager = ShellManager::new(tmp.path().to_path_buf());

    let result = manager
        .execute(&sleep_command(60), None, 5000, true)
        .expect("execute");

    let task_id = result
        .task_id
        .expect("background execution should return task_id");

    // Kill it
    let killed = manager.kill(&task_id).expect("kill");
    assert_eq!(killed.status, ShellStatus::Killed);
}

#[test]
fn test_write_stdin_streams_output() {
    let tmp = tempdir().expect("tempdir");
    let mut manager = ShellManager::new(tmp.path().to_path_buf());

    let result = manager
        .execute_with_options(&echo_stdin_command(), None, 5000, true, None, false, None)
        .expect("execute");

    let task_id = result
        .task_id
        .expect("background execution should return task_id");

    manager
        .write_stdin(&task_id, "hello\n", true)
        .expect("write stdin");

    let delta = manager
        .get_output_delta(&task_id, true, 5000)
        .expect("get_output_delta");

    assert!(delta.result.stdout.contains("hello"));

    let delta2 = manager
        .get_output_delta(&task_id, false, 0)
        .expect("get_output_delta");
    assert!(delta2.result.stdout.is_empty());
}

#[test]
fn test_output_truncation() {
    let long_output = "x".repeat(50_000);
    let truncated = truncate_output(&long_output);

    assert!(truncated.len() < long_output.len());
    assert!(truncated.contains("truncated"));
}

#[test]
fn test_truncate_with_meta_reports_omission_counts() {
    let long_output = format!("line1\nline2\n{}", "x".repeat(60_000));
    let (truncated, meta) = truncate_with_meta(&long_output);

    assert!(meta.truncated);
    assert!(meta.original_len >= long_output.len());
    assert!(meta.omitted > 0);
    assert!(truncated.contains("bytes omitted"));
}

#[test]
fn test_summarize_output_strips_truncation_note() {
    let long_output = "x".repeat(60_000);
    let truncated = truncate_output(&long_output);
    let summary = summarize_output(&truncated);
    assert!(!summary.contains("Output truncated at"));
}

#[tokio::test]
async fn test_exec_shell_metadata_includes_summaries() {
    let tmp = tempdir().expect("tempdir");
    let ctx = ToolContext::new(tmp.path());
    let tool = ExecShellTool;

    let result = tool
        .execute(json!({"command": echo_command("hello")}), &ctx)
        .await
        .expect("execute");
    assert!(result.success);

    let meta = result.metadata.expect("metadata");
    let summary = meta
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    assert!(summary.contains("hello"));
    assert!(meta.get("stdout_len").is_some());
    assert!(meta.get("stdout_truncated").is_some());
}
