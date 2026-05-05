//! 自动总结 —— 任务完成后自动提取关键经验。
//!
//! # 触发时机
//!
//! - `edit_file` / `write_file` / `apply_patch` 成功 → 提取文件修改经验
//! - `exec_shell` 成功且有输出 → 提取命令执行经验
//! - Review 工具完成 → 提取审查经验
//! - Chat 消息完成后 → 提取本轮对话关键经验
//!
//! # 提取策略
//!
//! 不是每个工具调用都生成经验，只在有明确"问题-方案"模式时生成。

use serde_json::Value;

use super::{Experience, Outcome};

/// 尝试从工具调用中提取经验。
///
/// 返回 `Some(Experience)` 当工具调用有明确的"问题-方案"模式，
/// 否则返回 `None`。
#[must_use]
pub fn try_extract(
    tool_name: &str,
    tool_input: &Value,
    tool_output: &str,
    project: &str,
) -> Option<Experience> {
    match tool_name {
        "edit_file" => extract_edit_experience(tool_input, tool_output, project),
        "write_file" => extract_write_experience(tool_input, tool_output, project),
        "apply_patch" => extract_patch_experience(tool_input, tool_output, project),
        "exec_shell" => extract_shell_experience(tool_input, tool_output, project),
        _ => None,
    }
}

/// 从 `edit_file` 提取经验。
fn extract_edit_experience(input: &Value, _output: &str, project: &str) -> Option<Experience> {
    let path = input.get("path")?.as_str()?;
    let search = input.get("search")?.as_str()?;
    let replace = input.get("replace")?.as_str()?;

    // 只对非平凡的修改生成经验（替换内容有意义）
    if search.len() < 10 && replace.len() < 10 {
        return None;
    }

    let title = format!("编辑文件: {}", path);
    let context = format!("需要修改文件 {} 中的内容", path);
    let action = format!("将 `{}` 替换为 `{}`", truncate(search, 60), truncate(replace, 60));
    let result = "文件修改成功".to_string();
    let tags = vec!["edit".to_string(), "file".to_string()];

    Some(Experience::new(
        title, context, action, result, tags, project.to_string(), Outcome::Success, 0.8,
    ))
}

/// 从 `write_file` 提取经验。
fn extract_write_experience(input: &Value, _output: &str, project: &str) -> Option<Experience> {
    let path = input.get("path")?.as_str()?;
    let content = input.get("content").and_then(|v| v.as_str())?;

    if content.len() < 20 {
        return None;
    }

    let title = format!("写入文件: {}", path);
    let context = format!("创建或覆盖文件 {}", path);
    let action = format!("写入 {} 字节内容", content.len());
    let result = "文件写入成功".to_string();
    let tags = vec!["write".to_string(), "file".to_string()];

    Some(Experience::new(
        title, context, action, result, tags, project.to_string(), Outcome::Success, 0.7,
    ))
}

/// 从 `apply_patch` 提取经验。
fn extract_patch_experience(input: &Value, _output: &str, project: &str) -> Option<Experience> {
    let diff = input.get("diff").and_then(|v| v.as_str())?;

    if diff.len() < 30 {
        return None;
    }

    // 从 diff 中提取文件名
    let files: Vec<&str> = diff
        .lines()
        .filter(|l| l.starts_with("--- ") || l.starts_with("+++ "))
        .filter_map(|l| {
            let path = l.trim_start_matches("--- ").trim_start_matches("+++ ");
            if path != "/dev/null" {
                Some(path.trim_start_matches("a/").trim_start_matches("b/"))
            } else {
                None
            }
        })
        .collect();

    let title = if files.is_empty() {
        "应用补丁".to_string()
    } else {
        format!("补丁: {}", files.join(", "))
    };
    let context = "需要应用代码修改".to_string();
    let action = format!("应用 {} 行补丁", diff.lines().count());
    let result = "补丁应用成功".to_string();
    let tags = vec!["patch".to_string(), "edit".to_string()];

    Some(Experience::new(
        title, context, action, result, tags, project.to_string(), Outcome::Success, 0.85,
    ))
}

/// 从 `exec_shell` 提取经验。
fn extract_shell_experience(input: &Value, output: &str, project: &str) -> Option<Experience> {
    let command = input.get("command").and_then(|v| v.as_str())?;

    // 只对构建/测试/安装等有意义命令生成经验
    let significant = command.contains("cargo build")
        || command.contains("cargo test")
        || command.contains("npm install")
        || command.contains("git commit")
        || command.contains("make")
        || command.contains("pip install");

    if !significant || output.len() < 10 {
        return None;
    }

    // 判断是否成功（输出中不包含 error/fail）
    let success = !output.to_lowercase().contains("error")
        && !output.to_lowercase().contains("failed")
        && !output.to_lowercase().contains("aborting");

    let outcome = if success {
        Outcome::Success
    } else {
        Outcome::Failure
    };

    let outcome_str = if success { "成功" } else { "失败" };
    let title = format!("{}: {}", outcome_str, truncate(command, 50));
    let context = format!("执行命令 `{}`", command);
    let action = format!("运行 {}", command);
    let result = truncate(output, 100);
    let tags = vec!["shell".to_string(), "command".to_string()];

    Some(Experience::new(
        title, context, action, result, tags, project.to_string(), outcome, 0.75,
    ))
}

/// 截断字符串到最大长度。
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max.saturating_sub(3)).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_edit_short_change_returns_none() {
        let input = json!({"path": "foo.rs", "search": "a", "replace": "b"});
        let result = extract_edit_experience(&input, "ok", "test");
        assert!(result.is_none());
    }

    #[test]
    fn extract_edit_long_change_returns_experience() {
        let input = json!({
            "path": "src/main.rs",
            "search": "旧的错误处理方式导致崩溃",
            "replace": "新的错误处理使用 Result 类型"
        });
        let result = extract_edit_experience(&input, "ok", "test-project");
        assert!(result.is_some());
        let exp = result.unwrap();
        assert!(exp.title.contains("src/main.rs"));
        assert_eq!(exp.project, "test-project");
    }

    #[test]
    fn extract_shell_build_success() {
        let input = json!({"command": "cargo build --release"});
        let output = "Compiling deepseek-tui v0.8.11\nFinished dev profile";
        let result = extract_shell_experience(&input, output, "test");
        assert!(result.is_some());
        assert_eq!(result.unwrap().outcome, Outcome::Success);
    }

    #[test]
    fn extract_shell_trivial_command_returns_none() {
        let input = json!({"command": "ls -la"});
        let result = extract_shell_experience(&input, "file1\nfile2\n", "test");
        assert!(result.is_none());
    }

    #[test]
    fn try_extract_dispatches_by_tool_name() {
        let input = json!({"path": "x.rs", "content": "fn main() {}\n// 超过20字节的内容"});
        let result = try_extract("write_file", &input, "ok", "proj");
        assert!(result.is_some());
    }

    #[test]
    fn try_extract_unknown_tool_returns_none() {
        let result = try_extract("web_search", &json!({}), "ok", "proj");
        assert!(result.is_none());
    }

    // ── apply_patch ───────────────────────────────────────

    #[test]
    fn extract_patch_short_diff_returns_none() {
        let input = json!({"diff": "small change"});
        let result = extract_patch_experience(&input, "ok", "test");
        assert!(result.is_none());
    }

    #[test]
    fn extract_patch_with_files() {
        let diff = [
            "--- a/src/main.rs",
            "+++ b/src/main.rs",
            "@@ -1,5 +1,7 @@",
            " old line",
            "+new line",
            "--- a/src/lib.rs",
            "+++ b/src/lib.rs",
        ].join("\n");
        let input = json!({"diff": diff});
        let result = extract_patch_experience(&input, "ok", "test-proj");
        assert!(result.is_some());
        let exp = result.unwrap();
        assert!(exp.title.contains("src/main.rs") || exp.title.contains("src/lib.rs"));
        assert_eq!(exp.project, "test-proj");
        assert_eq!(exp.outcome, Outcome::Success);
    }

    #[test]
    fn extract_patch_with_new_file() {
        let diff = [
            "--- /dev/null",
            "+++ b/src/new.rs",
            "@@ -0,0 +1,3 @@",
            "+fn new_func() {}",
        ].join("\n");
        let input = json!({"diff": diff});
        let result = extract_patch_experience(&input, "ok", "proj");
        assert!(result.is_some());
        // /dev/null should be excluded from file list
        assert!(!result.unwrap().title.contains("/dev/null"));
    }

    // ── write_file ────────────────────────────────────────

    #[test]
    fn extract_write_file_short_content_returns_none() {
        let input = json!({"path": "f.rs", "content": "short"});
        let result = extract_write_experience(&input, "ok", "test");
        assert!(result.is_none());
    }

    #[test]
    fn extract_write_file_missing_content_returns_none() {
        let input = json!({"path": "f.rs"});
        let result = extract_write_experience(&input, "ok", "test");
        assert!(result.is_none());
    }

    #[test]
    fn extract_write_file_long_content_returns_experience() {
        let input = json!({"path": "/etc/config.toml", "content": "key = 'value'\n# 超过20字节的配置文件内容"});
        let result = extract_write_experience(&input, "ok", "my-project");
        assert!(result.is_some());
        let exp = result.unwrap();
        assert!(exp.title.contains("/etc/config.toml"));
        assert_eq!(exp.project, "my-project");
    }

    // ── exec_shell ─────────────────────────────────────────

    #[test]
    fn extract_shell_failure_detection_contains_error() {
        let input = json!({"command": "cargo build"});
        let output = "error[E0308]: mismatched types\n --> src/main.rs:12:5\n\nFor more information about this error, try `rustc --explain E0308`.";
        let result = extract_shell_experience(&input, output, "test");
        assert!(result.is_some());
        assert_eq!(result.unwrap().outcome, Outcome::Failure);
    }

    #[test]
    fn extract_shell_failure_detection_contains_failed() {
        let input = json!({"command": "cargo test"});
        let output = "test result: FAILED. 10 passed; 1 failed; 0 ignored";
        let result = extract_shell_experience(&input, output, "test");
        assert!(result.is_some());
        assert_eq!(result.unwrap().outcome, Outcome::Failure);
    }

    #[test]
    fn extract_shell_short_output_returns_none() {
        let input = json!({"command": "cargo build"});
        let output = "ok";
        let result = extract_shell_experience(&input, output, "test");
        assert!(result.is_none());
    }

    #[test]
    fn extract_shell_missing_command_returns_none() {
        let input = json!({"not_command": true});
        let result = extract_shell_experience(&input, "some output here that is long enough", "test");
        assert!(result.is_none());
    }

    // ── truncate ───────────────────────────────────────────

    #[test]
    fn truncate_empty_string_stays_empty() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length_unchanged() {
        assert_eq!(truncate("12345", 5), "12345");
    }

    #[test]
    fn truncate_long_string_gets_ellipsis() {
        let result = truncate("一二三四五六七八九十", 8);
        assert!(result.ends_with("..."));
        // 8 chars - 3 for "..." = 5 chars visible
        assert_eq!(result.chars().count(), 8);
    }

    #[test]
    fn truncate_zero_max_returns_ellipsis() {
        let result = truncate("hello world", 0);
        // max=0 → saturating_sub(3)=0 → empty prefix + "..."
        assert_eq!(result, "...");
    }

    #[test]
    fn truncate_small_max_still_works() {
        // max=3, saturating_sub(3) = 0, so visible part is empty + "..."
        let result = truncate("hello", 3);
        assert_eq!(result, "...");
    }

    #[test]
    fn truncate_handles_chinese_characters() {
        let chinese = "这是一个很长的中文句子用来测试截断功能";
        let result = truncate(chinese, 10);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= 10);
    }
}
