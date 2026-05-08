//! Claude Code config auto-import (#453).
//!
//! At session start, if a `.claude/settings.json` or `CLAUDE.md` exists in
//! the project, this module parses the Claude Code configuration and imports
//! compatible settings into DeepSeek-TUI's own config files.
//!
//! ## What gets imported
//!
//! | Claude Code source | DeepSeek-TUI target | Description |
//! |---|---|:---|
//! | `.claude/settings.json` → `mcpServers` | `mcp.json` → `servers` | MCP server definitions |
//! | `.claude/settings.json` → `autoApprove` | (logged for reference) | Tool auto-approve lists |
//!
//! ## Merge strategy
//!
//! - Servers from Claude Code that do **not** already exist in `mcp.json`
//!   are appended.
//! - Servers with the same name as an existing entry are **skipped** to
//!   avoid overwriting user edits. The diff report notes each skip.
//! - The user's `mcp.json` is never modified without reporting exactly
//!   what would change (the import is a dry-run + apply cycle gated on
//!   a CLI flag or implicit auto-import at boot).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::mcp;

// ============================================================================
// Claude Code settings.json format
// ============================================================================

/// Top-level `.claude/settings.json` shape.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClaudeSettings {
    /// MCP server definitions. Claude Code uses the `mcpServers` key
    /// (camelCase). Serde `alias` handles the overlap with DeepSeek's
    /// own `McpConfig` which also accepts `mcpServers`.
    #[serde(default, alias = "mcp_servers")]
    pub mcpServers: HashMap<String, ClaudeMcpServer>,

    /// Allow-list of projects (unused here, preserved for forward compat).
    #[serde(default)]
    pub allowedTools: Option<AllowedToolsConfig>,

    /// Additional arbitrary keys preserved for round-trip safety.
    #[serde(flatten)]
    pub extras: HashMap<String, serde_json::Value>,
}

/// A single MCP server definition from Claude Code's settings.
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeMcpServer {
    /// Shell command to launch the server process (e.g. `npx`, `node`,
    /// `uvx`, `python`).
    #[serde(default)]
    pub command: Option<String>,

    /// Arguments passed to `command`.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables injected into the server process.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// When `true`, the server is disabled and will not be launched.
    #[serde(default)]
    pub disabled: bool,

    /// URL for SSE-based MCP servers (Claude Code supports streamable HTTP).
    #[serde(default)]
    pub url: Option<String>,

    /// List of tool names that can be invoked without user approval.
    #[serde(default)]
    pub autoApprove: Vec<String>,

    /// Additional arbitrary keys.
    #[serde(flatten)]
    pub extras: HashMap<String, serde_json::Value>,
}

/// Claude Code's `allowedTools` configuration block.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AllowedToolsConfig {
    /// Tools that can run without confirmation.
    #[serde(default)]
    pub tools: Vec<String>,

    /// Whether to disable the approval system entirely.
    #[serde(default)]
    pub disableApproval: Option<bool>,
}

// ============================================================================
// Candidate files to scan
// ============================================================================

/// File names we check for Claude Code config, in priority order.
const CLAUDE_CONFIG_CANDIDATES: &[&str] = &[".claude/settings.json"];

/// Maximum file size we'll attempt to parse (1 MiB).
const MAX_SETTINGS_BYTES: u64 = 1_048_576;

// ============================================================================
// Detection
// ============================================================================

/// Find the first `.claude/settings.json` that exists in the workspace
/// hierarchy. Returns `None` if no candidate is found.
pub fn detect_claude_config(workspace: &Path) -> Option<PathBuf> {
    for rel in CLAUDE_CONFIG_CANDIDATES {
        let path = workspace.join(rel);
        if path.exists() && path.is_file() {
            return Some(path.to_owned());
        }
    }
    // Also check parent directories for monorepo root settings
    let mut current = workspace.parent();
    while let Some(parent) = current {
        for rel in CLAUDE_CONFIG_CANDIDATES {
            let path = parent.join(rel);
            if path.exists() && path.is_file() {
                return Some(path);
            }
        }
        current = parent.parent();
    }
    None
}

// ============================================================================
// Parsing
// ============================================================================

/// Parse a `.claude/settings.json` file into structured data.
pub fn parse_claude_settings(path: &Path) -> Result<ClaudeSettings> {
    // Size check
    let meta = fs::metadata(path).with_context(|| {
        format!("Failed to stat Claude config at {}", path.display())
    })?;
    if meta.len() > MAX_SETTINGS_BYTES {
        anyhow::bail!(
            "Claude config at {} is too large ({} bytes, max {})",
            path.display(),
            meta.len(),
            MAX_SETTINGS_BYTES,
        );
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read Claude config {}", path.display()))?;
    let settings: ClaudeSettings = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse Claude config {}", path.display()))?;
    Ok(settings)
}

// ============================================================================
// Import report
// ============================================================================

/// Outcome for a single MCP server during import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerImportAction {
    /// Server was added to `mcp.json`.
    Added,
    /// Server with the same name already exists and was skipped.
    SkippedAlreadyExists,
    /// Server already exists and was skipped (the existing config is
    /// non-trivially different so we defer to the user).
    SkippedConflict,
}

/// Full report of what happened during an import run.
#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    /// Path to the Claude Code settings file that was loaded.
    pub claude_source: Option<PathBuf>,

    /// Path to the DeepSeek MCP config file that was modified.
    pub mcp_config_path: Option<PathBuf>,

    /// Per-server import outcomes.
    pub servers: Vec<ImportServerEntry>,

    /// Any auto-approve tool lists found.
    pub auto_approve_tools: Vec<String>,

    /// Warnings encountered during import.
    pub warnings: Vec<String>,
}

/// One server entry in the import report.
#[derive(Debug, Clone)]
pub struct ImportServerEntry {
    /// Server name in the config.
    pub name: String,

    /// What happened.
    pub action: ServerImportAction,

    /// The `command` from Claude Code (for display).
    pub command: String,

    /// Arguments (for display).
    pub args: Vec<String>,
}

// ============================================================================
// MCP server import logic
// ============================================================================

/// Import MCP servers from `.claude/settings.json` into the DeepSeek MCP
/// config (`mcp.json`).
///
/// Returns a report of what happened (added, skipped, conflicts) without
/// modifying the config file — callers write the merged config explicitly.
pub fn import_mcp_servers(
    claude_settings: &ClaudeSettings,
    existing_mcp: &mcp::McpConfig,
) -> ImportReport {
    let mut report = ImportReport::default();
    let mut merged_servers = existing_mcp.servers.clone();

    for (name, claude_server) in &claude_settings.mcpServers {
        let command = claude_server
            .command
            .clone()
            .unwrap_or_else(|| claude_server.url.clone().unwrap_or_default());
        let args = claude_server.args.clone();

        // Collect auto-approve tools
        for tool in &claude_server.autoApprove {
            report
                .auto_approve_tools
                .push(format!("{}:{}", name, tool));
        }

        if let Some(existing) = existing_mcp.servers.get(name) {
            // Server exists in the DeepSeek config already.
            // Check if the command/URL matches — if so, it's the same server
            // (user already imported or configured manually) → skip silently.
            let existing_command = existing
                .command
                .clone()
                .unwrap_or_else(|| existing.url.clone().unwrap_or_default());
            if existing_command == command && existing.args == args {
                report.servers.push(ImportServerEntry {
                    name: name.clone(),
                    action: ServerImportAction::SkippedAlreadyExists,
                    command,
                    args,
                });
            } else {
                // Existing config differs → report as conflict, do not overwrite.
                report.servers.push(ImportServerEntry {
                    name: name.clone(),
                    action: ServerImportAction::SkippedConflict,
                    command,
                    args,
                });
                report.warnings.push(format!(
                    "MCP server '{name}' already exists in mcp.json with different \
                     settings. Remove it first to re-import from Claude Code."
                ));
            }
        } else {
            // New server — add it.
            let de_server = mcp::McpServerConfig {
                command: claude_server.command.clone(),
                args: claude_server.args.clone(),
                env: claude_server.env.clone(),
                url: claude_server.url.clone(),
                connect_timeout: None,
                execute_timeout: None,
                read_timeout: None,
                disabled: claude_server.disabled,
                enabled: !claude_server.disabled,
                required: false,
                enabled_tools: claude_server.autoApprove.clone(),
                disabled_tools: Vec::new(),
            };
            merged_servers.insert(name.clone(), de_server);
            report.servers.push(ImportServerEntry {
                name: name.clone(),
                action: ServerImportAction::Added,
                command,
                args,
            });
        }
    }

    report
}

/// Render an [`ImportReport`] as a human-readable diff string suitable for
/// display before the TUI starts.
pub fn render_import_diff(report: &ImportReport) -> String {
    use std::fmt::Write;

    if report.servers.is_empty() && report.auto_approve_tools.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let _ = writeln!(out, "── Claude Code Import ──────────────────────────────");

    if let Some(source) = &report.claude_source {
        let _ = writeln!(out, "  Source: {}", source.display());
    }
    if let Some(mcp_path) = &report.mcp_config_path {
        let _ = writeln!(out, "  Target: {}", mcp_path.display());
    }

    // Group servers by action
    let added: Vec<_> = report
        .servers
        .iter()
        .filter(|s| s.action == ServerImportAction::Added)
        .collect();
    let skipped: Vec<_> = report
        .servers
        .iter()
        .filter(|s| s.action == ServerImportAction::SkippedAlreadyExists)
        .collect();
    let conflicts: Vec<_> = report
        .servers
        .iter()
        .filter(|s| s.action == ServerImportAction::SkippedConflict)
        .collect();

    if !added.is_empty() {
        let _ = writeln!(out, "\n  MCP servers added ({}):", added.len());
        for entry in &added {
            let cmd = if entry.args.is_empty() {
                entry.command.clone()
            } else {
                format!("{} {}", entry.command, entry.args.join(" "))
            };
            let _ = writeln!(out, "    + {}  ({})", entry.name, cmd);
        }
    }

    if !skipped.is_empty() {
        let _ = writeln!(
            out,
            "\n  MCP servers already configured ({}):",
            skipped.len()
        );
        for entry in &skipped {
            let _ = writeln!(out, "    ~ {}  (identical, skipped)", entry.name);
        }
    }

    if !conflicts.is_empty() {
        let _ = writeln!(
            out,
            "\n  MCP servers with conflicting config ({}):",
            conflicts.len()
        );
        for entry in &conflicts {
            let _ = writeln!(
                out,
                "    ! {}  (different settings exist, skipped)",
                entry.name
            );
        }
    }

    if !report.auto_approve_tools.is_empty() {
        let _ = writeln!(
            out,
            "\n  Auto-approve tools noted ({}):",
            report.auto_approve_tools.len()
        );
        for tool in &report.auto_approve_tools {
            let _ = writeln!(out, "    • {}  (mapped to enabled_tools)", tool);
        }
    }

    if !report.warnings.is_empty() {
        let _ = writeln!(out);
        for warn in &report.warnings {
            let _ = writeln!(out, "  ⚠ {warn}");
        }
    }

    let _ = writeln!(
        out,
        "──────────────────────────────────────────────────"
    );
    out
}

// ============================================================================
// High-level entry point
// ============================================================================

/// Results from a full import run.
#[derive(Debug)]
pub struct ImportResult {
    /// Human-readable diff to show the user.
    pub diff: String,

    /// Whether any files were modified.
    pub modified: bool,

    /// The report for programmatic inspection.
    pub report: ImportReport,

    /// The merged MCP config (caller should save if `modified` is true).
    pub merged_mcp: Option<mcp::McpConfig>,
}

/// Run the Claude Code import: detect, parse, merge, and optionally write.
///
/// If `write_changes` is `true`, modifies `mcp.json` on disk. Returns a
/// report of what was found and done.
pub fn run_import(
    workspace: &Path,
    mcp_config_path: &Path,
    write_changes: bool,
) -> ImportResult {
    let mut report = ImportReport::default();

    // Detect Claude Code config
    let claude_source = match detect_claude_config(workspace) {
        Some(path) => {
            report.claude_source = Some(path.clone());
            path
        }
        None => {
            return ImportResult {
                diff: String::new(),
                modified: false,
                report,
                merged_mcp: None,
            };
        }
    };

    report.mcp_config_path = Some(mcp_config_path.to_owned());

    // Parse Claude Code settings
    let claude_settings = match parse_claude_settings(&claude_source) {
        Ok(s) => s,
        Err(e) => {
            report.warnings.push(format!("Failed to parse Claude config: {e}"));
            return ImportResult {
                diff: String::new(),
                modified: false,
                report,
                merged_mcp: None,
            };
        }
    };

    // Exit early if no MCP servers defined in Claude config
    if claude_settings.mcpServers.is_empty() {
        return ImportResult {
            diff: String::new(),
            modified: false,
            report,
            merged_mcp: None,
        };
    }

    // Load existing DeepSeek MCP config
    let existing_mcp = mcp::load_config(mcp_config_path).unwrap_or_default();

    // Compute the import plan
    let mut import_report = import_mcp_servers(&claude_settings, &existing_mcp);
    import_report.claude_source = report.claude_source.clone();
    import_report.mcp_config_path = report.mcp_config_path.clone();
    import_report
        .warnings
        .extend(report.warnings.drain(..));

    // Build merged config
    let merged: mcp::McpConfig = mcp::McpConfig {
        timeouts: existing_mcp.timeouts,
        servers: {
            let mut merged = existing_mcp.servers.clone();
            for entry in &import_report.servers {
                if entry.action != ServerImportAction::Added {
                    continue;
                }
                if let Some(cs) = claude_settings.mcpServers.get(&entry.name) {
                    merged.insert(
                        entry.name.clone(),
                        mcp::McpServerConfig {
                            command: cs.command.clone(),
                            args: cs.args.clone(),
                            env: cs.env.clone(),
                            url: cs.url.clone(),
                            connect_timeout: None,
                            execute_timeout: None,
                            read_timeout: None,
                            disabled: cs.disabled,
                            enabled: !cs.disabled,
                            required: false,
                            enabled_tools: cs.autoApprove.clone(),
                            disabled_tools: Vec::new(),
                        },
                    );
                }
            }
            merged
        },
    };

    let has_additions = import_report
        .servers
        .iter()
        .any(|s| s.action == ServerImportAction::Added);

    // Write the merged config if changes were requested
    if write_changes && has_additions {
        if let Err(e) = mcp::save_config(mcp_config_path, &merged) {
            import_report
                .warnings
                .push(format!("Failed to write merged MCP config: {e}"));
        }
    }

    let diff = render_import_diff(&import_report);

    ImportResult {
        diff,
        modified: has_additions && write_changes,
        report: import_report,
        merged_mcp: Some(merged),
    }
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_claude_settings() -> ClaudeSettings {
        ClaudeSettings {
            mcpServers: {
                let mut m = HashMap::new();
                m.insert(
                    "filesystem".to_string(),
                    ClaudeMcpServer {
                        command: Some("npx".to_string()),
                        args: vec![
                            "-y".to_string(),
                            "@modelcontextprotocol/server-filesystem".to_string(),
                            "/tmp".to_string(),
                        ],
                        env: HashMap::new(),
                        disabled: false,
                        url: None,
                        autoApprove: vec!["read".to_string(), "write".to_string()],
                        extras: HashMap::new(),
                    },
                );
                m.insert(
                    "github".to_string(),
                    ClaudeMcpServer {
                        command: Some("npx".to_string()),
                        args: vec![
                            "-y".to_string(),
                            "@modelcontextprotocol/server-github".to_string(),
                        ],
                        env: {
                            let mut e = HashMap::new();
                            e.insert("GITHUB_TOKEN".to_string(), "ghp_fake".to_string());
                            e
                        },
                        disabled: false,
                        url: None,
                        autoApprove: Vec::new(),
                        extras: HashMap::new(),
                    },
                );
                m.insert(
                    "playwright".to_string(),
                    ClaudeMcpServer {
                        command: Some("npx".to_string()),
                        args: vec![
                            "-y".to_string(),
                            "@playwright/mcp".to_string(),
                        ],
                        env: HashMap::new(),
                        disabled: true,
                        url: None,
                        autoApprove: Vec::new(),
                        extras: HashMap::new(),
                    },
                );
                m
            },
            allowedTools: None,
            extras: HashMap::new(),
        }
    }

    #[test]
    fn test_detect_claude_config_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(detect_claude_config(tmp.path()).is_none());
    }

    #[test]
    fn test_detect_claude_config_found() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        let settings_path = claude_dir.join("settings.json");
        fs::write(&settings_path, "{}").unwrap();

        let found = detect_claude_config(tmp.path());
        assert!(found.is_some());
        assert_eq!(found.unwrap(), settings_path);
    }

    #[test]
    fn test_parse_claude_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        let path = claude_dir.join("settings.json");
        fs::write(
            &path,
            r#"{
                "mcpServers": {
                    "filesystem": {
                        "command": "npx",
                        "args": ["-y", "server-fs", "/tmp"],
                        "autoApprove": ["read"]
                    }
                }
            }"#,
        )
        .unwrap();

        let settings = parse_claude_settings(&path).unwrap();
        assert_eq!(settings.mcpServers.len(), 1);
        let fs = settings.mcpServers.get("filesystem").unwrap();
        assert_eq!(fs.command.as_deref(), Some("npx"));
        assert_eq!(fs.args, vec!["-y", "server-fs", "/tmp"]);
        assert_eq!(fs.autoApprove, vec!["read"]);
    }

    #[test]
    fn test_import_mcp_servers_adds_new() {
        let claude = sample_claude_settings();
        let existing = mcp::McpConfig::default();

        let report = import_mcp_servers(&claude, &existing);

        assert_eq!(report.servers.len(), 3);
        for s in &report.servers {
            assert_eq!(s.action, ServerImportAction::Added);
        }
    }

    #[test]
    fn test_import_mcp_servers_skips_identical() {
        let claude = sample_claude_settings();
        let mut existing = mcp::McpConfig::default();
        existing.servers.insert(
            "filesystem".to_string(),
            mcp::McpServerConfig {
                command: Some("npx".to_string()),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                    "/tmp".to_string(),
                ],
                env: HashMap::new(),
                url: None,
                connect_timeout: None,
                execute_timeout: None,
                read_timeout: None,
                disabled: false,
                enabled: true,
                required: false,
                enabled_tools: vec!["read".to_string(), "write".to_string()],
                disabled_tools: Vec::new(),
            },
        );

        let report = import_mcp_servers(&claude, &existing);

        let fs_entry = report
            .servers
            .iter()
            .find(|s| s.name == "filesystem")
            .unwrap();
        assert_eq!(fs_entry.action, ServerImportAction::SkippedAlreadyExists);

        let gh_entry = report
            .servers
            .iter()
            .find(|s| s.name == "github")
            .unwrap();
        assert_eq!(gh_entry.action, ServerImportAction::Added);
    }

    #[test]
    fn test_import_mcp_servers_conflict() {
        let claude = sample_claude_settings();
        let mut existing = mcp::McpConfig::default();
        existing.servers.insert(
            "filesystem".to_string(),
            mcp::McpServerConfig {
                command: Some("docker".to_string()),
                args: vec!["run".to_string(), "mcp-fs".to_string()],
                env: HashMap::new(),
                url: None,
                connect_timeout: None,
                execute_timeout: None,
                read_timeout: None,
                disabled: false,
                enabled: true,
                required: false,
                enabled_tools: Vec::new(),
                disabled_tools: Vec::new(),
            },
        );

        let report = import_mcp_servers(&claude, &existing);

        let fs_entry = report
            .servers
            .iter()
            .find(|s| s.name == "filesystem")
            .unwrap();
        assert_eq!(fs_entry.action, ServerImportAction::SkippedConflict);
    }

    #[test]
    fn test_render_import_diff_empty() {
        let report = ImportReport::default();
        let diff = render_import_diff(&report);
        assert!(diff.is_empty());
    }

    #[test]
    fn test_render_import_diff_with_additions() {
        let mut report = ImportReport::default();
        report.claude_source = Some(PathBuf::from("/project/.claude/settings.json"));
        report.mcp_config_path = Some(PathBuf::from("/home/user/.deepseek/mcp.json"));
        report.servers.push(ImportServerEntry {
            name: "filesystem".to_string(),
            action: ServerImportAction::Added,
            command: "npx".to_string(),
            args: vec!["-y", "@mcp/fs"].into_iter().map(String::from).collect(),
        });
        report.servers.push(ImportServerEntry {
            name: "github".to_string(),
            action: ServerImportAction::Added,
            command: "npx".to_string(),
            args: vec!["-y", "@mcp/gh"].into_iter().map(String::from).collect(),
        });
        report.servers.push(ImportServerEntry {
            name: "old-server".to_string(),
            action: ServerImportAction::SkippedAlreadyExists,
            command: "node".to_string(),
            args: vec!["server.js".to_string()],
        });
        report
            .auto_approve_tools
            .push("filesystem:read".to_string());
        report
            .auto_approve_tools
            .push("filesystem:write".to_string());

        let diff = render_import_diff(&report);
        assert!(diff.contains("filesystem"));
        assert!(diff.contains("github"));
        assert!(diff.contains("old-server"));
        assert!(diff.contains("auto-approve"));
        assert!(diff.contains("Claude Code Import"));
    }

    #[test]
    fn test_run_import_no_claude_config() {
        let tmp = tempfile::tempdir().unwrap();
        let mcp_path = tmp.path().join("mcp.json");
        let result = run_import(tmp.path(), &mcp_path, true);

        assert!(!result.modified);
        assert!(result.diff.is_empty());
    }

    #[test]
    fn test_run_import_with_config() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{
                "mcpServers": {
                    "filesystem": {
                        "command": "npx",
                        "args": ["-y", "server-fs", "/tmp"]
                    }
                }
            }"#,
        )
        .unwrap();
        let mcp_path = tmp.path().join("mcp.json");

        let result = run_import(tmp.path(), &mcp_path, true);

        assert!(result.modified);
        assert!(result.diff.contains("filesystem"));
        assert!(result.merged_mcp.is_some());
        assert!(result
            .merged_mcp
            .as_ref()
            .unwrap()
            .servers
            .contains_key("filesystem"));

        // Verify the file was actually written
        let saved = mcp::load_config(&mcp_path).unwrap();
        assert!(saved.servers.contains_key("filesystem"));
    }

    #[test]
    fn test_run_import_preserves_existing_mcp_config() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{
                "mcpServers": {
                    "new-server": {
                        "command": "node",
                        "args": ["new.js"]
                    }
                }
            }"#,
        )
        .unwrap();
        let mcp_path = tmp.path().join("mcp.json");

        // Pre-populate mcp.json with an existing server
        let mut existing = mcp::McpConfig::default();
        existing.servers.insert(
            "existing-server".to_string(),
            mcp::McpServerConfig {
                command: Some("node".to_string()),
                args: vec!["existing.js".to_string()],
                env: HashMap::new(),
                url: None,
                connect_timeout: None,
                execute_timeout: None,
                read_timeout: None,
                disabled: false,
                enabled: true,
                required: false,
                enabled_tools: Vec::new(),
                disabled_tools: Vec::new(),
            },
        );
        mcp::save_config(&mcp_path, &existing).unwrap();

        // Run import
        let result = run_import(tmp.path(), &mcp_path, true);

        assert!(result.modified);

        // Saved config should have both servers
        let saved = mcp::load_config(&mcp_path).unwrap();
        assert!(saved.servers.contains_key("existing-server"));
        assert!(saved.servers.contains_key("new-server"));
        // Existing server unchanged
        assert_eq!(
            saved.servers.get("existing-server").unwrap().command,
            Some("node".to_string())
        );
        assert_eq!(
            saved.servers.get("existing-server").unwrap().args,
            vec!["existing.js"]
        );
    }

    #[test]
    fn test_run_import_with_disabled_server() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{
                "mcpServers": {
                    "legacy": {
                        "command": "node",
                        "args": ["old.js"],
                        "disabled": true
                    }
                }
            }"#,
        )
        .unwrap();
        let mcp_path = tmp.path().join("mcp.json");

        let result = run_import(tmp.path(), &mcp_path, true);
        assert!(result.modified);

        let saved = mcp::load_config(&mcp_path).unwrap();
        let server = saved.servers.get("legacy").unwrap();
        assert!(server.disabled);
        assert!(!server.enabled);
    }

    #[test]
    fn test_detect_claude_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        // Put settings.json in a parent directory
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        fs::write(claude_dir.join("settings.json"), "{}").unwrap();

        // Look from a child directory
        let child = tmp.path().join("src").join("deep");
        fs::create_dir_all(&child).unwrap();

        let found = detect_claude_config(&child);
        assert!(found.is_some());
    }
}
