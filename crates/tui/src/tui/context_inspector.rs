//! Compact session context inspector.

use std::collections::HashSet;
use std::fmt::Write;

use crate::compaction::estimate_input_tokens_conservative;
use crate::models::{DEFAULT_CONTEXT_WINDOW_TOKENS, context_window_for_model};
use crate::session_manager::SessionContextReference;
use crate::tui::app::{App, ToolDetailRecord};
use crate::tui::file_mention::ContextReferenceSource;
use crate::utils::estimate_message_chars;

const CONTEXT_WARNING_THRESHOLD_PERCENT: f64 = 85.0;
const CONTEXT_CRITICAL_THRESHOLD_PERCENT: f64 = 95.0;
const MAX_REFERENCE_ROWS: usize = 12;
const MAX_TOOL_ROWS: usize = 8;

#[must_use]
pub fn build_context_inspector_text(app: &App) -> String {
    let mut out = String::new();
    let usage = context_usage(app);
    let status = context_status(usage.2);

    let _ = writeln!(out, "Session Context");
    let _ = writeln!(out, "---------------");
    let _ = writeln!(out, "Model: {}", app.model);
    let _ = writeln!(out, "Workspace: {}", app.workspace.display());
    if let Some(session_id) = app.current_session_id.as_deref() {
        let _ = writeln!(out, "Session: {}", session_id);
    }
    let (used, max, percent) = usage;
    let _ = writeln!(
        out,
        "Context: {status} - ~{used}/{max} tokens ({percent:.1}%)"
    );
    let _ = writeln!(
        out,
        "Transcript: {} cells, {} API messages",
        app.history.len(),
        app.api_messages.len()
    );
    let _ = writeln!(
        out,
        "Workspace status: {}",
        app.workspace_context
            .as_deref()
            .unwrap_or("not sampled yet")
    );

    let _ = writeln!(out);
    push_references(&mut out, &app.session_context_references);
    let _ = writeln!(out);
    push_tools(&mut out, app);

    out
}

fn context_usage(app: &App) -> (usize, u32, f64) {
    let max = context_window_for_model(&app.model).unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS);
    let estimated =
        estimate_input_tokens_conservative(&app.api_messages, app.system_prompt.as_ref());
    let total_chars = estimate_message_chars(&app.api_messages);
    let used = estimated.max(total_chars / 4);
    let percent = ((used as f64 / f64::from(max)) * 100.0).clamp(0.0, 100.0);
    (used, max, percent)
}

fn context_status(percent: f64) -> &'static str {
    if percent >= CONTEXT_CRITICAL_THRESHOLD_PERCENT {
        "critical"
    } else if percent >= CONTEXT_WARNING_THRESHOLD_PERCENT {
        "high"
    } else {
        "ok"
    }
}

fn push_references(out: &mut String, references: &[SessionContextReference]) {
    let _ = writeln!(out, "References");
    let _ = writeln!(out, "----------");

    let mut seen = HashSet::new();
    let mut rendered = 0usize;
    for record in references {
        let reference = &record.reference;
        let key = format!(
            "{:?}:{:?}:{}:{}",
            reference.source, reference.kind, reference.target, reference.label
        );
        if !seen.insert(key) {
            continue;
        }
        if rendered >= MAX_REFERENCE_ROWS {
            let remaining = references.len().saturating_sub(rendered);
            if remaining > 0 {
                let _ = writeln!(out, "- ... {remaining} more reference(s)");
            }
            break;
        }

        let prefix = match reference.source {
            ContextReferenceSource::AtMention => "@",
            ContextReferenceSource::Attachment => "/attach ",
        };
        let state = if reference.included {
            if reference.expanded {
                "included"
            } else {
                "attached"
            }
        } else {
            "not included"
        };
        let detail = reference
            .detail
            .as_deref()
            .filter(|detail| !detail.trim().is_empty())
            .map(|detail| format!(" - {detail}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "- [{}] {prefix}{} -> {} ({state}{detail})",
            reference.badge, reference.label, reference.target
        );
        rendered += 1;
    }

    if rendered == 0 {
        let _ = writeln!(
            out,
            "- No file, directory, or media references recorded yet."
        );
    }
}

fn push_tools(out: &mut String, app: &App) {
    let _ = writeln!(out, "Recent Tools");
    let _ = writeln!(out, "------------");

    let mut rows: Vec<(usize, &ToolDetailRecord)> = app
        .tool_details_by_cell
        .iter()
        .map(|(idx, detail)| (*idx, detail))
        .collect();
    rows.sort_by_key(|(idx, _)| std::cmp::Reverse(*idx));

    let mut rendered = 0usize;
    for detail in app.active_tool_details.values() {
        push_tool_row(out, "active", detail);
        rendered += 1;
        if rendered >= MAX_TOOL_ROWS {
            return;
        }
    }
    for (cell_idx, detail) in rows
        .into_iter()
        .take(MAX_TOOL_ROWS.saturating_sub(rendered))
    {
        let location = format!("cell {cell_idx}");
        push_tool_row(out, &location, detail);
        rendered += 1;
    }

    if rendered == 0 {
        let _ = writeln!(out, "- No tool activity recorded yet.");
    } else {
        let _ = writeln!(
            out,
            "- Open the matching card and press Alt+V for full details."
        );
    }
}

fn push_tool_row(out: &mut String, location: &str, detail: &ToolDetailRecord) {
    let output_state = if detail.output.as_deref().is_some_and(|out| !out.is_empty()) {
        "output captured"
    } else {
        "no output yet"
    };
    let _ = writeln!(
        out,
        "- [{}] {} {} ({output_state})",
        location,
        detail.tool_name,
        short_tool_id(&detail.tool_id)
    );
}

fn short_tool_id(id: &str) -> String {
    if id.len() <= 8 {
        id.to_string()
    } else {
        format!("{}...", &id[..8])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::models::{ContentBlock, Message};
    use crate::session_manager::SessionContextReference;
    use crate::tui::app::TuiOptions;
    use crate::tui::file_mention::{
        ContextReference, ContextReferenceKind, ContextReferenceSource,
    };
    use crate::tui::history::HistoryCell;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            TuiOptions {
                model: "unknown-model".to_string(),
                workspace: PathBuf::from("/tmp/project"),
                allow_shell: false,
                use_alt_screen: true,
                use_mouse_capture: false,
                use_bracketed_paste: true,
                max_subagents: 1,
                skills_dir: PathBuf::from("/tmp/skills"),
                memory_path: PathBuf::from("memory.md"),
                notes_path: PathBuf::from("notes.md"),
                mcp_config_path: PathBuf::from("mcp.json"),
                use_memory: false,
                start_in_agent_mode: false,
                skip_onboarding: true,
                yolo: false,
                resume_session_id: None,
            },
            &Config::default(),
        )
    }

    #[test]
    fn inspector_formats_empty_state() {
        let app = test_app();
        let text = build_context_inspector_text(&app);
        assert!(text.contains("Session Context"));
        assert!(text.contains("No file, directory, or media references recorded yet."));
        assert!(text.contains("No tool activity recorded yet."));
    }

    #[test]
    fn inspector_lists_context_references() {
        let mut app = test_app();
        app.history.push(HistoryCell::User {
            content: "read @src/main.rs".to_string(),
        });
        app.session_context_references
            .push(SessionContextReference {
                message_index: 0,
                reference: ContextReference {
                    kind: ContextReferenceKind::File,
                    source: ContextReferenceSource::AtMention,
                    badge: "file".to_string(),
                    label: "src/main.rs".to_string(),
                    target: "/tmp/project/src/main.rs".to_string(),
                    included: true,
                    expanded: true,
                    detail: Some("included".to_string()),
                },
            });

        let text = build_context_inspector_text(&app);
        assert!(text.contains("[file] @src/main.rs -> /tmp/project/src/main.rs"));
    }

    #[test]
    fn inspector_marks_high_context_pressure() {
        let mut app = test_app();
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "x".repeat(4_000_000),
                cache_control: None,
            }],
        });

        let text = build_context_inspector_text(&app);
        assert!(text.contains("Context: critical"), "{text}");
    }
}
