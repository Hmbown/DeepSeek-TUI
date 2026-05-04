//! `/watch` command — start, stop, and list file watchers.
//!
//! Watches a path recursively for file modifications and shows brief
//! notifications in the TUI status bar.

use std::path::PathBuf;
use std::time::Instant;

use crate::commands::CommandResult;
use crate::tui::app::App;
use crate::tui::watch::WatchInstance;

/// Dispatch `/watch` subcommands.
///
/// Usage:
/// - `/watch <path>`        — start watching a file or directory
/// - `/watch stop`          — stop the active watcher
/// - `/watch list`          — show active watchers
pub fn watch_command(app: &mut App, arg: Option<&str>) -> CommandResult {
    let arg = arg.unwrap_or("").trim();

    if arg.is_empty() {
        return show_help();
    }

    match arg {
        "stop" | "kill" | "cancel" => stop_watcher(app),
        "list" | "ls" | "status" => list_watchers(app),
        _ => start_watcher(app, arg),
    }
}

fn show_help() -> CommandResult {
    CommandResult::message(
        "/watch — background file watcher\n\n\
         Usage:\n\
         \x20 /watch <path>     Start watching a file or directory recursively\n\
         \x20 /watch stop       Stop the active watcher\n\
         \x20 /watch list       Show active watchers\n\n\
         When a watched file changes, a notification appears: [watch] <file> changed"
    )
}

fn start_watcher(app: &mut App, path_str: &str) -> CommandResult {
    let raw_path = PathBuf::from(shellexpand::tilde(path_str).as_ref());

    // Resolve relative to workspace if not absolute.
    let path = if raw_path.is_absolute() {
        raw_path
    } else {
        app.workspace.join(&raw_path)
    };

    // Canonicalize to catch nonexistent paths early.
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return CommandResult::error(format!(
                "Cannot access `{}`: {}",
                path.display(),
                e
            ));
        }
    };

    // Store a pre-watch content snapshot for diff display.
    if canonical.is_file() {
        let snapshot = crate::tui::watch::read_snapshot(&canonical);
        app.watch_snapshots.insert(canonical.clone(), snapshot);
    }

    match WatchInstance::start(&canonical) {
        Ok(instance) => {
            let display = canonical.to_string_lossy().to_string();

            // Stop any existing watcher before starting a new one.
            app.watch_instance = Some(instance);
            app.watch_path = Some(canonical);
            app.watch_started_at = Some(Instant::now());

            CommandResult::message(format!(
                "[watch] watching `{display}` — file changes will appear as notifications. /watch stop to stop."
            ))
        }
        Err(e) => CommandResult::error(format!("Failed to start watcher: {e}")),
    }
}

fn stop_watcher(app: &mut App) -> CommandResult {
    if app.watch_instance.is_some() {
        let path = app
            .watch_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        app.watch_instance = None;
        app.watch_path = None;
        app.watch_started_at = None;
        app.watch_snapshots.clear();
        CommandResult::message(format!("[watch] stopped watching `{path}`"))
    } else {
        CommandResult::message("[watch] no active watcher")
    }
}

fn list_watchers(app: &mut App) -> CommandResult {
    if let Some(ref path) = app.watch_path {
        let elapsed = app
            .watch_started_at
            .map(|t| {
                let d = t.elapsed();
                crate::tui::notifications::humanize_duration(d)
            })
            .unwrap_or_else(|| "unknown".to_string());
        CommandResult::message(format!(
            "[watch] watching `{}` (active for {})",
            path.display(),
            elapsed
        ))
    } else {
        CommandResult::message("[watch] no active watcher")
    }
}

/// Poll the active watcher for file-change events and push status messages.
/// Called from the TUI event loop once per iteration.
pub fn poll_watcher(app: &mut App) {
    let Some(ref instance) = app.watch_instance else {
        return;
    };
    let Some(event) = instance.poll() else {
        return;
    };

    let path = event.path;
    let display = path.to_string_lossy();

    // Show a diff if we have a cached snapshot and the file is text.
    let mut detail = String::new();
    if let Some(old_content) = app.watch_snapshots.get(&path) {
        if let Some(old) = old_content {
            if let Some(diff) = crate::tui::watch::show_diff(&path, old) {
                // Keep diff short for the status bar
                let truncated = if diff.len() > 200 {
                    format!("{}...", &diff[..197])
                } else {
                    diff
                };
                detail = format!("\n{}", truncated);
            }
        }
    }

    // Update the snapshot for future diffs.
    let new_snapshot = crate::tui::watch::read_snapshot(&path);
    app.watch_snapshots
        .insert(path.clone(), new_snapshot);

    let msg = if detail.is_empty() {
        format!("[watch] `{display}` changed")
    } else {
        format!("[watch] `{display}` changed:\n{}", detail)
    };

    app.status_message = Some(msg);
    app.needs_redraw = true;
}
