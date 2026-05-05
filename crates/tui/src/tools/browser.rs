#![allow(dead_code)]
//! Browser automation tool — wraps the agent-browser CLI for web interaction.
//!
//! agent-browser (https://github.com/vercel-labs/agent-browser) is a
//! pure-Rust browser automation CLI that controls Chrome via CDP. It
//! provides accessibility-tree snapshots with compact `@eN` refs that
//! allow AI agents to interact with pages in ~200–400 tokens instead of
//! parsing raw HTML.
//!
//! This module wraps agent-browser's most common commands into a ToolSpec
//! so the model gets type-safe parameters, automatic `--json` flag injection,
//! and structured output. For advanced operations not covered here, the
//! model falls back to `exec_shell agent-browser ...`.
//!
//! # Detection
//!
//! The tool is only registered when `agent-browser` is found on PATH.
//! Call `BrowserTool::is_available()` to check at registration time.
//!
//! # Architecture
//!
//! ```text
//! Model → browser { action: "snapshot", … }
//!          → std::process::Command("agent-browser", ["snapshot", "-i", "--json"])
//!          → parse JSON stdout
//!          → return structured ToolResult
//! ```

use std::process::Command;
use std::sync::OnceLock;

// ── Availability check ──────────────────────────────────────────────────────

/// Cached result of `which agent-browser`. Checked once per process.
static AGENT_BROWSER_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Returns `true` if `agent-browser` is installed and on PATH.
/// Result is cached after the first call.
#[must_use]
pub fn is_agent_browser_available() -> bool {
    *AGENT_BROWSER_AVAILABLE.get_or_init(|| {
        Command::new("which")
            .arg("agent-browser")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

// ── Command runner ──────────────────────────────────────────────────────────

/// Run an agent-browser command and return (stdout, stderr).
/// All commands get `--json` appended for machine parsing.
fn run_agent_browser(args: &[&str]) -> Result<(String, String), String> {
    let mut cmd = Command::new("agent-browser");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg("--json");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run agent-browser: {e}. Is it installed? Run: npm i -g agent-browser && agent-browser install"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let err_msg = if stderr.is_empty() { &stdout } else { &stderr };
        return Err(format!("agent-browser error (exit {}): {}", output.status, err_msg.trim()));
    }

    Ok((stdout, stderr))
}

/// Run agent-browser without --json (for screenshot, etc.).
fn run_agent_browser_raw(args: &[&str]) -> Result<(String, String), String> {
    let mut cmd = Command::new("agent-browser");
    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run agent-browser: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let err_msg = if stderr.is_empty() { &stdout } else { &stderr };
        return Err(format!("agent-browser error (exit {}): {}", output.status, err_msg.trim()));
    }

    Ok((stdout, stderr))
}

// ── ToolSpec ────────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool wrapping agent-browser CLI commands.
pub struct BrowserTool;

impl BrowserTool {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolSpec for BrowserTool {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn description(&self) -> &'static str {
        "Control a real Chrome browser via agent-browser CLI. Actions: navigate (open a URL), snapshot (get accessibility tree with @eN refs), click, fill, type_text, screenshot, get_text, get_url, get_title, wait, evaluate (run JS), press_key, scroll, close. The core workflow: navigate → snapshot → click/fill → snapshot. For advanced commands (tabs, network, auth, profiles), use exec_shell with agent-browser directly."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "navigate", "snapshot", "click", "fill", "type_text",
                        "screenshot", "get_text", "get_url", "get_title",
                        "wait", "evaluate", "press_key", "scroll", "close"
                    ],
                    "description": "Browser action to perform"
                },
                "url": {
                    "type": "string",
                    "description": "URL to navigate to (for navigate action)"
                },
                "target": {
                    "type": "string",
                    "description": "Element ref (@e3), CSS selector (#submit), or semantic locator. For click, fill, type_text, get_text, scroll."
                },
                "text": {
                    "type": "string",
                    "description": "Text to fill or type (for fill, type_text actions)"
                },
                "path": {
                    "type": "string",
                    "description": "File path for screenshot (optional; defaults to temp file)"
                },
                "interactive": {
                    "type": "boolean",
                    "description": "Only show interactive elements in snapshot (default: true)"
                },
                "compact": {
                    "type": "boolean",
                    "description": "Compact snapshot — remove empty structural nodes (default: true)"
                },
                "full_page": {
                    "type": "boolean",
                    "description": "Full-page screenshot (default: false)"
                },
                "annotate": {
                    "type": "boolean",
                    "description": "Annotate screenshot with numbered element labels (default: false)"
                },
                "wait_target": {
                    "type": "string",
                    "description": "What to wait for: element ref (@e3), text on page, URL pattern (glob), load state (networkidle/domcontentloaded/load), JS condition, or milliseconds (e.g. '2000')"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Wait timeout in milliseconds (default: 25000)"
                },
                "js": {
                    "type": "string",
                    "description": "JavaScript to evaluate (for evaluate action)"
                },
                "key": {
                    "type": "string",
                    "description": "Key to press (for press_key: Enter, Tab, Escape, Control+a, etc.)"
                },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"],
                    "description": "Scroll direction"
                },
                "amount": {
                    "type": "integer",
                    "description": "Scroll amount in pixels (default: 300)"
                },
                "value": {
                    "type": "string",
                    "description": "Value to select in dropdown (for select action)"
                }
            },
            "required": ["action"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'action'"))?;

        match action {
            "navigate" => {
                let url = get_str(&input, "url")?;
                let (stdout, _) = run_agent_browser(&["open", &url])
                    .map_err(|e| ToolError::execution_failed(e))?;
                Ok(ToolResult::success(format_navigate(&stdout, &url)))
            }

            "snapshot" => {
                let interactive = input.get("interactive").and_then(|v| v.as_bool()).unwrap_or(true);
                let compact = input.get("compact").and_then(|v| v.as_bool()).unwrap_or(true);

                let mut args: Vec<String> = vec!["snapshot".into()];
                if interactive { args.push("-i".into()); }
                if compact { args.push("-c".into()); }
                let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

                let (stdout, _) = run_agent_browser(&args_refs)
                    .map_err(|e| ToolError::execution_failed(e))?;

                let formatted = format_snapshot(&stdout);
                Ok(ToolResult::success(formatted))
            }

            "click" => {
                let target = get_str(&input, "target")?;
                let (stdout, _) = run_agent_browser(&["click", &target])
                    .map_err(|e| ToolError::execution_failed(e))?;
                Ok(ToolResult::success(format_click(&stdout, &target)))
            }

            "fill" => {
                let target = get_str(&input, "target")?;
                let text = get_str(&input, "text")?;
                let _ = run_agent_browser(&["fill", &target, &text])
                    .map_err(|e| ToolError::execution_failed(e))?;
                Ok(ToolResult::success(format!("Filled {} with text ({} chars)", target, text.len())))
            }

            "type_text" => {
                let target = get_str(&input, "target")?;
                let text = get_str(&input, "text")?;
                let _ = run_agent_browser(&["type", &target, &text])
                    .map_err(|e| ToolError::execution_failed(e))?;
                Ok(ToolResult::success(format!("Typed into {} ({} chars)", target, text.len())))
            }

            "screenshot" => {
                let path = get_str(&input, "path").ok();
                let full_page = input.get("full_page").and_then(|v| v.as_bool()).unwrap_or(false);
                let annotate = input.get("annotate").and_then(|v| v.as_bool()).unwrap_or(false);

                let mut args: Vec<String> = vec!["screenshot".into()];
                if full_page { args.push("--full".into()); }
                if annotate { args.push("--annotate".into()); }
                if let Some(ref p) = path { args.push(p.clone()); }
                let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

                // Screenshot doesn't use --json; captures raw output
                let (stdout, stderr) = run_agent_browser_raw(&args_refs)
                    .map_err(|e| ToolError::execution_failed(e))?;

                let output_path = extract_screenshot_path(&stdout, &stderr)
                    .unwrap_or_else(|| path.unwrap_or_else(|| "screenshot.png".into()));

                let annotated = if annotate { " (annotated)" } else { "" };
                Ok(ToolResult::success(format!(
                    "Screenshot saved{annotated}: {output_path}"
                )))
            }

            "get_text" => {
                let target = get_str(&input, "target")?;
                let (stdout, _) = run_agent_browser(&["get", "text", &target])
                    .map_err(|e| ToolError::execution_failed(e))?;
                Ok(ToolResult::success(format_get_text(&stdout, &target)))
            }

            "get_url" => {
                let (stdout, _) = run_agent_browser(&["get", "url"])
                    .map_err(|e| ToolError::execution_failed(e))?;
                let url = try_parse_json_string(&stdout, "url").unwrap_or_else(|| stdout.trim().to_string());
                Ok(ToolResult::success(format!("Current URL: {url}")))
            }

            "get_title" => {
                let (stdout, _) = run_agent_browser(&["get", "title"])
                    .map_err(|e| ToolError::execution_failed(e))?;
                let title = try_parse_json_string(&stdout, "title").unwrap_or_else(|| stdout.trim().to_string());
                Ok(ToolResult::success(format!("Page title: {title}")))
            }

            "wait" => {
                let target = get_str(&input, "wait_target")?;
                let timeout = input.get("timeout_ms").and_then(|v| v.as_u64());

                // Determine the right wait command
                let (stdout, _) = if target.parse::<u64>().is_ok() {
                    // Numeric → wait milliseconds
                    run_agent_browser(&["wait", &target])
                } else if target.starts_with('@') {
                    // Ref → wait for element
                    run_agent_browser(&["wait", &target])
                } else if target.starts_with("**") || target.contains("://") || target.contains('/') {
                    // URL pattern → wait --url
                    run_agent_browser(&["wait", "--url", &target])
                } else if target == "networkidle" || target == "domcontentloaded" || target == "load" {
                    // Load state → wait --load
                    run_agent_browser(&["wait", "--load", &target])
                } else if target.starts_with("window.") || target.contains("===") || target.contains("!==") {
                    // JS condition → wait --fn
                    run_agent_browser(&["wait", "--fn", &target])
                } else {
                    // Text → wait --text
                    run_agent_browser(&["wait", "--text", &target])
                }.map_err(|e| ToolError::execution_failed(e))?;

                let timeout_note = timeout.map(|t| format!(" (timeout: {t}ms)")).unwrap_or_default();
                Ok(ToolResult::success(format!("Wait complete for '{target}'{timeout_note}")))
            }

            "evaluate" => {
                let js = get_str(&input, "js")?;
                // Use --stdin for heredoc-style eval to avoid shell escaping issues
                let mut child = std::process::Command::new("agent-browser")
                    .args(["eval", "--stdin", "--json"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| ToolError::execution_failed(format!("Failed to spawn agent-browser: {e}")))?;

                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(js.as_bytes());
                }

                let output = child
                    .wait_with_output()
                    .map_err(|e| ToolError::execution_failed(format!("agent-browser eval failed: {e}")))?;

                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if !output.status.success() {
                    let err = if stderr.is_empty() { &stdout } else { &stderr };
                    return Err(ToolError::execution_failed(format!("Eval error: {}", err.trim())));
                }

                // Parse JSON result
                let result_text = try_parse_json_string(&stdout, "result")
                    .or_else(|| try_parse_json_string(&stdout, "value"))
                    .unwrap_or_else(|| stdout.trim().to_string());

                Ok(ToolResult::success(format!("Eval result:\n{result_text}")))
            }

            "press_key" => {
                let key = get_str(&input, "key")?;
                let _ = run_agent_browser(&["press", &key])
                    .map_err(|e| ToolError::execution_failed(e))?;
                Ok(ToolResult::success(format!("Pressed key: {key}")))
            }

            "scroll" => {
                let direction = get_str(&input, "direction").unwrap_or_else(|_| "down".into());
                let amount = input.get("amount").and_then(|v| v.as_u64()).unwrap_or(300);
                let target = get_str(&input, "target").ok();

                let amount_str = amount.to_string();
                let mut all_args: Vec<String> = vec!["scroll".into(), direction.clone(), amount_str];
                if let Some(ref t) = target {
                    all_args.push("--selector".into());
                    all_args.push(t.clone());
                }
                let args_refs: Vec<&str> = all_args.iter().map(String::as_str).collect();

                let _ = run_agent_browser(&args_refs)
                    .map_err(|e| ToolError::execution_failed(e))?;
                let target_note = target.map(|t| format!(" on {t}")).unwrap_or_default();
                Ok(ToolResult::success(format!("Scrolled {direction} by {amount}px{target_note}")))
            }

            "close" => {
                let (_stdout, _) = run_agent_browser_raw(&["close"])
                    .map_err(|e| ToolError::execution_failed(e))?;
                Ok(ToolResult::success("Browser closed"))
            }

            _ => Err(ToolError::invalid_input(format!(
                "Unknown action '{action}'"
            ))),
        }
    }
}

// ── Output formatters ───────────────────────────────────────────────────────

fn format_navigate(stdout: &str, url: &str) -> String {
    let title = try_parse_json_string(stdout, "title").unwrap_or_default();
    if title.is_empty() {
        format!("Navigated to {url}")
    } else {
        format!("Navigated to {url} — {title}")
    }
}

fn format_snapshot(stdout: &str) -> String {
    // Try to parse the full JSON output; fall back to raw text
    if let Ok(parsed) = serde_json::from_str::<Value>(stdout) {
        let page_title = parsed
            .get("data").or_else(|| parsed.get("snapshot"))
            .and_then(|d| d.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let page_url = parsed
            .get("data").or_else(|| parsed.get("snapshot"))
            .and_then(|d| d.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tree = parsed
            .get("data").or_else(|| parsed.get("snapshot"))
            .and_then(|d| d.get("snapshot"))
            .and_then(|v| v.as_str());

        // Count refs (agent-browser format: @e1, @e2, ...)
        let ref_count = tree.map(|t| t.matches("@e").count()).unwrap_or(0);

        let mut out = String::new();
        if !page_title.is_empty() {
            out.push_str(&format!("Page: {page_title}\n"));
        }
        if !page_url.is_empty() {
            out.push_str(&format!("URL: {page_url}\n"));
        }
        if ref_count > 0 {
            out.push_str(&format!("\n{ref_count} interactive elements:\n"));
        }
        if let Some(t) = tree {
            out.push_str(t);
        } else {
            out.push_str(stdout.trim());
        }
        out
    } else {
        format!("Snapshot:\n{}", stdout.trim())
    }
}

fn format_click(stdout: &str, target: &str) -> String {
    let element = try_parse_json_string(stdout, "element")
        .or_else(|| try_parse_json_string(stdout, "clicked"));
    match element {
        Some(el) => format!("Clicked: {el}"),
        None => {
            let success = stdout.contains("success") || stdout.contains("\"ok\"");
            if success {
                format!("Clicked {target}")
            } else {
                format!("Clicked {target}: {}", stdout.trim())
            }
        }
    }
}

fn format_get_text(stdout: &str, target: &str) -> String {
    let text = try_parse_json_string(stdout, "text")
        .or_else(|| try_parse_json_string(stdout, "value"))
        .unwrap_or_else(|| stdout.trim().to_string());
    if text.is_empty() {
        format!("{target}: (empty)")
    } else {
        format!("{target}:\n{text}")
    }
}

fn extract_screenshot_path(stdout: &str, _stderr: &str) -> Option<String> {
    // Try parsing JSON first: check data.path, path, screenshot (string or nested)
    if let Ok(parsed) = serde_json::from_str::<Value>(stdout)
        && parsed.is_object()
    {
        // Check data.path (nested)
        if let Some(path) = parsed.get("data").and_then(|d| d.get("path")).and_then(|v| v.as_str()) {
            return Some(path.to_string());
        }
        // Check top-level path
        if let Some(path) = parsed.get("path").and_then(|v| v.as_str()) {
            return Some(path.to_string());
        }
        // Check top-level screenshot
        if let Some(path) = parsed.get("screenshot").and_then(|v| v.as_str()) {
            return Some(path.to_string());
        }
    }
    // Fall back: look for file path pattern in stdout
    for line in stdout.lines() {
        let trimmed = line.trim();
        // Extract just the path portion — strip common prefixes
        for prefix in &["Screenshot saved: ", "Screenshot: ", "Saved: "] {
            if let Some(path) = trimmed.strip_prefix(prefix) {
                let path = path.trim();
                if path.ends_with(".png") || path.ends_with(".jpg") || path.ends_with(".jpeg") {
                    return Some(path.to_string());
                }
            }
        }
        if trimmed.ends_with(".png") || trimmed.ends_with(".jpg") || trimmed.ends_with(".jpeg") {
            return Some(trimmed.to_string());
        }
    }
    None
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn get_str(input: &Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| ToolError::invalid_input(format!("Missing '{key}'")))
}

fn try_parse_json_string(json_str: &str, key: &str) -> Option<String> {
    serde_json::from_str::<Value>(json_str).ok().and_then(|v| {
        // Check both top-level and nested under 'data'
        v.get(key).and_then(|v| v.as_str().map(String::from))
            .or_else(|| {
                v.get("data")
                    .and_then(|d| d.get(key))
                    .and_then(|v| v.as_str().map(String::from))
            })
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_navigate_with_title() {
        let json = r#"{"data":{"title":"Example Domain"}}"#;
        let result = format_navigate(json, "https://example.com");
        assert!(result.contains("Example Domain"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn test_format_navigate_no_title() {
        let result = format_navigate("{}", "https://example.com");
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn test_format_snapshot_parses_refs() {
        let json = r#"{"data":{"title":"Test","url":"https://test.com","snapshot":"@e1 button Submit\n@e2 textbox Email"}}"#;
        let result = format_snapshot(json);
        assert!(result.contains("Test"));
        assert!(result.contains("2 interactive elements"));
        assert!(result.contains("@e1"));
    }

    #[test]
    fn test_extract_screenshot_path_from_json() {
        let json = r#"{"data":{"path":"/tmp/screenshot-abc123.png"}}"#;
        assert_eq!(extract_screenshot_path(json, "").as_deref(), Some("/tmp/screenshot-abc123.png"));
    }

    #[test]
    fn test_extract_screenshot_path_fallback() {
        let stdout = "Screenshot saved: /tmp/page.png\nDone.";
        assert_eq!(extract_screenshot_path(stdout, "").as_deref(), Some("/tmp/page.png"));
    }

    #[test]
    fn test_try_parse_json_string_nested() {
        let json = r#"{"data":{"url":"https://example.com"}}"#;
        assert_eq!(try_parse_json_string(json, "url").as_deref(), Some("https://example.com"));
    }

    #[test]
    fn test_try_parse_json_string_top_level() {
        let json = r#"{"url":"https://example.com"}"#;
        assert_eq!(try_parse_json_string(json, "url").as_deref(), Some("https://example.com"));
    }

    #[test]
    fn test_is_available_cached() {
        // First call caches; second returns same
        let a = is_agent_browser_available();
        let b = is_agent_browser_available();
        assert_eq!(a, b);
    }

    #[test]
    fn test_json_parse_non_json_string() {
        // Verify serde_json doesn't successfully parse arbitrary text
        let s = "Screenshot saved: /tmp/page.png\nDone.";
        let parsed = serde_json::from_str::<serde_json::Value>(s);
        assert!(parsed.is_err(), "Expected parse failure, got: {parsed:?}");
    }
}
