//! `/queen` slash command — 虫后记忆系统开关。
//!
//! - `/queen`         — 显示当前状态（开/关、经验数、目录路径）
//! - `/queen on`      — 启动虫后记忆系统
//! - `/queen off`     — 关闭虫后记忆系统
//! - `/queen status`  — 显示详细状态

use std::sync::Arc;

use super::CommandResult;
use crate::tui::app::App;
use crate::queen::Queen;

const QUEEN_USAGE: &str = "/queen [on|off|status]";

fn queen_help() -> String {
    format!(
        "管理虫后记忆系统（Queen Memory System）。\n\n\
         用法: {QUEEN_USAGE}\n\n\
         子命令:\n\
           /queen          显示当前状态\n\
           /queen status   显示详细状态（开/关、经验数、目录路径）\n\
           /queen on       启动虫后记忆系统\n\
           /queen off      关闭虫后记忆系统\n\n\
         虫后系统会在任务完成后自动提取经验，并在未来的对话中注入相关记忆。"
    )
}

/// Determine the base directory for queen storage.
fn queen_base_dir(app: &App) -> std::path::PathBuf {
    if app.workspace.join(".deepseek").exists() {
        app.workspace.join(".deepseek")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home).join(".deepseek")
    }
}

/// Build a status report from the current queen state.
fn status_report(app: &App) -> String {
    let queen_dir = queen_base_dir(app).join(crate::queen::QUEEN_DIR_NAME);
    let enabled = app.queen.is_some();

    let dir_status = if queen_dir.exists() {
        format!("✓ 目录存在: {}", queen_dir.display())
    } else {
        format!("✗ 目录不存在: {}", queen_dir.display())
    };

    let exp_count = match &app.queen {
        Some(queen) => match queen.lock() {
            Ok(guard) => format!("经验数: {}", guard.experience_count()),
            Err(_) => "经验数: (无法获取锁)".to_string(),
        },
        None => "经验数: —".to_string(),
    };

    if enabled {
        format!(
            "虫后记忆系统: **已开启** 🐛\n\
             {}\n\
             {}\n\n\
             使用 `/queen off` 关闭。",
            dir_status, exp_count,
        )
    } else {
        format!(
            "虫后记忆系统: **已关闭**\n\
             {}\n\n\
             使用 `/queen on` 开启。",
            dir_status,
        )
    }
}

/// `/queen` command handler.
pub fn queen(app: &mut App, arg: Option<&str>) -> CommandResult {
    let raw = arg.map(str::trim).unwrap_or("");

    match raw {
        // Show status (no arg or "status")
        "" | "status" => CommandResult::message(status_report(app)),

        // Turn queen ON
        "on" | "enable" | "1" | "true" => {
            if app.queen.is_some() {
                return CommandResult::message(
                    "虫后记忆系统已经处于开启状态。使用 `/queen status` 查看详情。",
                );
            }

            let base_dir = queen_base_dir(app);
            match Queen::init(&base_dir) {
                Ok(queen) => {
                    app.queen = Some(Arc::new(std::sync::Mutex::new(queen)));
                    CommandResult::message(format!(
                        "🐛 虫后记忆系统已开启！\n\
                         经验将存储在: {}/{}",
                        base_dir.join(crate::queen::QUEEN_DIR_NAME).display(),
                        crate::queen::EXPERIENCES_DIR,
                    ))
                }
                Err(err) => CommandResult::error(format!("虫后初始化失败: {err}")),
            }
        }

        // Turn queen OFF
        "off" | "disable" | "0" | "false" => {
            if app.queen.is_none() {
                return CommandResult::message(
                    "虫后记忆系统已经处于关闭状态。",
                );
            }

            app.queen = None;
            CommandResult::message(
                "🐛 虫后记忆系统已关闭。\n\
                 已有的经验文件保留在磁盘上，重新开启后可继续使用。\
                 使用 `/queen on` 重新开启。",
            )
        }

        // Help
        "help" => CommandResult::message(queen_help()),

        other => CommandResult::error(format!(
            "未知的 /queen 参数 `{other}`。使用 `/queen on`、`/queen off`、`/queen status` 或 `/queen help`。"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn make_app_no_queen() -> App {
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
        app.model = "deepseek-v4-flash".to_string();
        app.queen = None;
        app
    }

    fn make_app_with_queen() -> App {
        let tmp = tempdir().unwrap();
        let mut app = make_app_no_queen();
        // Create .deepseek dir so queen_base_dir uses workspace dir
        std::fs::create_dir_all(tmp.path().join(".deepseek")).unwrap();
        app.workspace = tmp.path().to_path_buf();
        // Pre-init the queen to simulate enabled state
        let queen = crate::queen::Queen::init(tmp.path()).unwrap();
        app.queen = Some(Arc::new(std::sync::Mutex::new(queen)));
        app
    }

    // ── queen_help ────────────────────────────────────────

    #[test]
    fn queen_help_contains_usage_and_subcommands() {
        let help = queen_help();
        assert!(help.contains("/queen"));
        assert!(help.contains("on"));
        assert!(help.contains("off"));
        assert!(help.contains("status"));
    }

    // ── status_report ─────────────────────────────────────

    #[test]
    fn status_report_disabled_shows_closed_state() {
        let app = make_app_no_queen();
        let report = status_report(&app);
        assert!(report.contains("已关闭"));
        assert!(!report.contains("已开启"));
        // disabled report does not include exp_count line
        assert!(!report.contains("经验数"));
    }

    #[test]
    fn status_report_enabled_shows_open_state_and_count() {
        let app = make_app_with_queen();
        let report = status_report(&app);
        assert!(report.contains("已开启"));
        assert!(report.contains("经验数:"));
        assert!(!report.contains("经验数: —"));
    }

    // ── queen command: on/off/status/help ──────────────────

    #[test]
    fn queen_command_status_on_disabled_queen() {
        let mut app = make_app_no_queen();
        let result = queen(&mut app, Some("status"));
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("已关闭"));
    }

    #[test]
    fn queen_command_status_on_enabled_queen() {
        let mut app = make_app_with_queen();
        let result = queen(&mut app, Some("status"));
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("已开启"));
    }

    #[test]
    fn queen_command_no_arg_shows_status() {
        let mut app = make_app_no_queen();
        let result = queen(&mut app, None);
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        // No arg should behave like "status"
        assert!(msg.contains("已关闭") || msg.contains("已开启"));
    }

    #[test]
    fn queen_command_on_creates_queen() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".deepseek")).unwrap();
        let mut app = make_app_no_queen();
        app.workspace = tmp.path().to_path_buf();

        assert!(app.queen.is_none());
        let result = queen(&mut app, Some("on"));
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("已开启"));
        assert!(app.queen.is_some());
    }

    #[test]
    fn queen_command_on_when_already_enabled() {
        let mut app = make_app_with_queen();
        let result = queen(&mut app, Some("on"));
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("已经处于开启状态"));
    }

    #[test]
    fn queen_command_off_disables_queen() {
        let mut app = make_app_with_queen();
        assert!(app.queen.is_some());

        let result = queen(&mut app, Some("off"));
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("已关闭"));
        assert!(app.queen.is_none());
    }

    #[test]
    fn queen_command_off_when_already_disabled() {
        let mut app = make_app_no_queen();
        assert!(app.queen.is_none());

        let result = queen(&mut app, Some("off"));
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("已经处于关闭状态"));
    }

    #[test]
    fn queen_command_help_subcommand() {
        let mut app = make_app_no_queen();
        let result = queen(&mut app, Some("help"));
        assert!(!result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("/queen"));
        assert!(msg.contains("on"));
        assert!(msg.contains("off"));
    }

    #[test]
    fn queen_command_unknown_arg_returns_error() {
        let mut app = make_app_no_queen();
        let result = queen(&mut app, Some("unknown_cmd_xyz"));
        assert!(result.is_error);
        let msg = result.message.unwrap_or_default();
        assert!(msg.contains("未知"));
    }

    #[test]
    fn queen_command_aliases_on() {
        for alias in &["enable", "1", "true"] {
            let tmp = tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".deepseek")).unwrap();
            let mut app = make_app_no_queen();
            app.workspace = tmp.path().to_path_buf();
            let result = queen(&mut app, Some(alias));
            assert!(!result.is_error, "alias '{alias}' should work");
            assert!(app.queen.is_some(), "alias '{alias}' should enable queen");
        }
    }

    #[test]
    fn queen_command_aliases_off() {
        for alias in &["disable", "0", "false"] {
            let tmp = tempdir().unwrap();
            let q = crate::queen::Queen::init(tmp.path()).unwrap();
            let mut app = make_app_no_queen();
            app.workspace = tmp.path().to_path_buf();
            app.queen = Some(Arc::new(std::sync::Mutex::new(q)));
            let result = queen(&mut app, Some(alias));
            assert!(!result.is_error, "alias '{alias}' should work");
            assert!(app.queen.is_none(), "alias '{alias}' should disable queen");
        }
    }
}
