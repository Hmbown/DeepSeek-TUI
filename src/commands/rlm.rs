//! RLM commands for the TUI (load/status/repl/save-session).

use std::fs;
use std::path::{Path, PathBuf};

use crate::rlm::{context_id_from_path, unique_context_id};
use crate::tui::app::{App, AppMode};

use super::CommandResult;

const DEFAULT_CHUNK_SIZE: usize = 2000;
const DEFAULT_CHUNK_OVERLAP: usize = 200;

pub fn welcome_message() -> String {
    [
        "DeepSeek RLM / Aleph Sandbox",
        "Commands: /rlm, /aleph, /load <file>, /repl, /status, /save-session",
        "Press Tab to exit RLM mode",
        "Use /repl to toggle expression mode (chat is the default)",
        "Tip: /load @path forces workspace-relative paths (e.g. @docs/rlm-paper.txt)",
        "",
        "Expressions:",
        "  len(ctx)",
        "  search(\"pattern\")",
        "  lines(1, 20)",
        "  chunk(2000, 200)",
        "  chunk_sections(20000)",
        "  chunk_auto(20000)",
        "  vars(), get(\"name\"), set(\"name\", \"value\")",
        "",
        "Tip: rlm_query auto_chunks runs the same question over chunk_auto slices.",
        "Tip: /save-session <path> persists the current RLM session.",
    ]
    .join("\n")
}

pub fn overview_message() -> String {
    [
        "RLM / Aleph Quickstart",
        "Use /rlm or /aleph to enter external-memory mode.",
        "Use /load @path to load a file into the RLM context store.",
        "Use /status to list contexts and usage totals.",
        "Use /repl to toggle expression mode (chat is default).",
        "Tip: rlm_query auto_chunks runs the same question over chunk_auto slices.",
    ]
    .join("\n")
}

pub fn enter(app: &mut App) -> CommandResult {
    if app.mode != AppMode::Rlm {
        app.set_mode(AppMode::Rlm);
    }
    app.rlm_repl_active = false;
    CommandResult::message(overview_message())
}

pub fn repl(app: &mut App) -> CommandResult {
    if app.mode != AppMode::Rlm {
        app.set_mode(AppMode::Rlm);
    }
    if app.rlm_repl_active {
        app.rlm_repl_active = false;
        return CommandResult::message("Exited RLM REPL mode. Chat is active.");
    }
    app.rlm_repl_active = true;
    CommandResult::message(welcome_message())
}

pub fn status(app: &mut App) -> CommandResult {
    let session = match app.rlm_session.lock() {
        Ok(session) => session,
        Err(_) => return CommandResult::error("Failed to access RLM session"),
    };

    if session.contexts.is_empty() {
        return CommandResult::message("No RLM contexts loaded. Use /load <path>.");
    }

    let mut lines = Vec::new();
    lines.push("RLM Session".to_string());
    lines.push(format!("Active context: {}", session.active_context));
    lines.push(format!("Loaded contexts: {}", session.contexts.len()));
    lines.push(format!(
        "Queries: {} | Input tokens: {} | Output tokens: {}",
        session.usage.queries, session.usage.input_tokens, session.usage.output_tokens
    ));

    let mut ids: Vec<_> = session.contexts.keys().collect();
    ids.sort();
    for id in ids {
        if let Some(ctx) = session.contexts.get(id) {
            let source = ctx
                .source_path
                .as_ref()
                .map(|s| format!(" (source: {s})"))
                .unwrap_or_default();
            let chunk_count = ctx.chunk(DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP).len();
            let section_count = ctx.chunk_sections(20_000).len();
            lines.push(format!(
                "- {id}: {} lines, {} chars, {} chunks, {} sections{source}",
                ctx.line_count, ctx.char_count, chunk_count, section_count
            ));
            if !ctx.variables.is_empty() {
                lines.push(format!("  variables: {}", ctx.variables.len()));
            }
        }
    }

    CommandResult::message(lines.join("\n"))
}

pub fn load(app: &mut App, path: Option<&str>) -> CommandResult {
    let Some(raw) = path else {
        return CommandResult::error("Usage: /load <path>");
    };

    let resolved = match resolve_path(app, raw) {
        Ok(path) => path,
        Err(err) => return CommandResult::error(err),
    };

    let mut session = match app.rlm_session.lock() {
        Ok(session) => session,
        Err(_) => return CommandResult::error("Failed to access RLM session"),
    };

    let base_id = context_id_from_path(&resolved);
    let id = unique_context_id(&session, &base_id);
    let (line_count, char_count) = match session.load_file(&id, &resolved) {
        Ok(stats) => stats,
        Err(err) => {
            return CommandResult::error(format!("Failed to load {}: {err}", resolved.display()));
        }
    };

    CommandResult::message(format!(
        "Loaded {} ({} lines, {} chars)",
        resolved.display(),
        line_count,
        char_count
    ))
}

pub fn save_session(app: &mut App, path: Option<&str>) -> CommandResult {
    let save_path = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("rlm_session_{timestamp}.json"))
    };

    let parent_dir = save_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf);
    if let Some(dir) = parent_dir
        && let Err(err) = fs::create_dir_all(&dir)
    {
        return CommandResult::error(format!(
            "Failed to create directory {}: {err}",
            dir.display()
        ));
    }

    let session = match app.rlm_session.lock() {
        Ok(session) => session,
        Err(_) => return CommandResult::error("Failed to access RLM session"),
    };
    let json = match serde_json::to_string_pretty(&*session) {
        Ok(json) => json,
        Err(err) => return CommandResult::error(format!("Failed to serialize session: {err}")),
    };

    match fs::write(&save_path, json) {
        Ok(()) => CommandResult::message(format!("RLM session saved to {}", save_path.display())),
        Err(err) => CommandResult::error(format!("Failed to save session: {err}")),
    }
}

fn resolve_path(app: &App, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    let (raw, force_workspace) = if let Some(stripped) = raw.strip_prefix('@') {
        (stripped.trim(), true)
    } else {
        (raw, false)
    };
    if raw.is_empty() {
        return Err("Usage: /load <path> (use @ for workspace-relative paths)".to_string());
    }

    let candidate = if force_workspace {
        app.workspace.join(raw.trim_start_matches(['/', '\\']))
    } else if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        app.workspace.join(raw)
    };
    let canonical = candidate.canonicalize().map_err(|err| {
        let mut message = format!("Failed to resolve path {}: {err}", candidate.display());
        if !force_workspace {
            message.push_str("\nTip: use /load @path to resolve relative to the workspace.");
        }
        message
    })?;
    let workspace_root = app
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| app.workspace.clone());
    if !app.trust_mode && !canonical.starts_with(&workspace_root) {
        return Err("Path is outside workspace. Use /trust to allow access.".to_string());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use std::fs;

    fn make_app(workspace: PathBuf) -> App {
        let options = TuiOptions {
            model: "test-model".to_string(),
            workspace,
            allow_shell: false,
            use_alt_screen: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn resolve_path_with_at_prefix_uses_workspace_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let docs_dir = tmp.path().join("docs");
        fs::create_dir_all(&docs_dir).expect("create docs dir");
        let file_path = docs_dir.join("rlm-paper.txt");
        fs::write(&file_path, "hello").expect("write file");

        let app = make_app(tmp.path().to_path_buf());
        let resolved = resolve_path(&app, "@/docs/rlm-paper.txt").expect("resolve path with @");
        assert_eq!(resolved, file_path.canonicalize().expect("canonicalize"));
    }
}
