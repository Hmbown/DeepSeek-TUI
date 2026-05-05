#![allow(dead_code)]
//! Obscura browser tool — wraps the obscura headless browser CLI.
//!
//! Obscura (https://github.com/h4ckf0r0day/obscura) is a pure-Rust
//! headless browser engine with V8 JavaScript, CDP support, and built-in
//! anti-detection (stealth mode). At 30 MB memory / 70 MB binary, it's
//! ~10× lighter than headless Chrome.
//!
//! This module wraps obscura's CLI commands into a ToolSpec for
//! fast JS-rendered page data extraction, parallel scraping, stealth
//! mode (anti-fingerprinting + tracker blocking), and CDP server launch.
//!
//! # Complementary to agent-browser
//!
//! - **obscura**: fast fetch, stealth scrape, parallel extraction
//! - **agent-browser** (browser tool): interactive clicks, form fills, screenshots
//!
//! Use obscura when you need stealth, speed, or batch extraction.
//! Use agent-browser when you need interactive UI automation.
//!
//! # Detection
//!
//! The tool is only registered when `obscura` is found on PATH.
//! Call `ObscuraTool::is_available()` to check at registration time.

use std::process::Command;
use std::sync::OnceLock;

// ── Availability check ──────────────────────────────────────────────────────

static OBSCURA_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Returns `true` if `obscura` is installed and on PATH.
/// Result is cached after the first call.
#[must_use]
pub fn is_obscura_available() -> bool {
    *OBSCURA_AVAILABLE.get_or_init(|| {
        Command::new("which")
            .arg("obscura")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

// ── Command runner ──────────────────────────────────────────────────────────

fn run_obscura(args: &[&str]) -> Result<(String, String), String> {
    let output = Command::new("obscura")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run obscura: {e}. Is it installed? Grab from https://github.com/h4ckf0r0day/obscura/releases"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let err_msg = if stderr.is_empty() { &stdout } else { &stderr };
        return Err(format!("obscura error (exit {}): {}", output.status, err_msg.trim()));
    }

    Ok((stdout, stderr))
}

// ── ToolSpec ────────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool wrapping the obscura headless browser CLI.
pub struct ObscuraTool;

impl ObscuraTool {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolSpec for ObscuraTool {
    fn name(&self) -> &'static str {
        "obscura"
    }

    fn description(&self) -> &'static str {
        "Fast, stealthy headless browser for page data extraction. Actions: fetch (JS-rendered page with optional stealth mode), scrape (parallel multi-URL extraction with concurrency), serve (start CDP WebSocket server for Puppeteer/Playwright). 30 MB memory, instant startup, built-in anti-fingerprinting + 3,520-domain tracker blocking. Use for: JS-rendered pages, anti-bot sites requiring stealth, batch URL scraping, DOM→Markdown conversion. NOT for: interactive clicks/form fills (use browser tool)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["fetch", "scrape", "serve"],
                    "description": "Obscura action: fetch (single page), scrape (parallel multi-URL), serve (start CDP server)"
                },
                "url": {
                    "type": "string",
                    "description": "URL to fetch (for fetch action)"
                },
                "urls": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "URLs to scrape in parallel (for scrape action)"
                },
                "dump": {
                    "type": "string",
                    "enum": ["text", "html", "links"],
                    "description": "Output format for fetch (default: text)"
                },
                "eval": {
                    "type": "string",
                    "description": "JavaScript expression to evaluate on the page (e.g., 'document.title', 'document.querySelectorAll(\"h1\").length')"
                },
                "stealth": {
                    "type": "boolean",
                    "description": "Enable stealth mode — anti-fingerprinting + tracker blocking (default: false). Use for anti-bot sites."
                },
                "wait_until": {
                    "type": "string",
                    "enum": ["load", "domcontentloaded", "networkidle0"],
                    "description": "When to consider the page loaded (default: load)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Max navigation time in seconds (default: fetch=30, scrape=60)"
                },
                "concurrency": {
                    "type": "integer",
                    "description": "Parallel workers for scrape (default: 10)"
                },
                "port": {
                    "type": "integer",
                    "description": "CDP WebSocket port (for serve action, default: 9222)"
                }
            },
            "required": ["action"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::ReadOnly]
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
            "fetch" => {
                let url = get_str(&input, "url")?;
                let dump = input.get("dump").and_then(|v| v.as_str()).unwrap_or("text");
                let eval_js = get_str(&input, "eval").ok();
                let stealth = input.get("stealth").and_then(|v| v.as_bool()).unwrap_or(false);
                let wait_until = input.get("wait_until").and_then(|v| v.as_str()).unwrap_or("load");
                let timeout = input.get("timeout").and_then(|v| v.as_u64());

                let mut args: Vec<String> = vec![
                    "fetch".into(), url.clone(),
                    "--dump".into(), dump.to_string(),
                    "--wait-until".into(), wait_until.to_string(),
                ];
                if stealth { args.push("--stealth".into()); }
                if let Some(t) = timeout {
                    args.push("--timeout".into());
                    args.push(t.to_string());
                }

                let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                let (stdout, stderr) = run_obscura(&args_refs)
                    .map_err(|e| ToolError::execution_failed(e))?;

                // If eval was requested, run it separately (obscura's --eval prints to stderr)
                let eval_result = if let Some(ref js) = eval_js {
                    let mut eval_args: Vec<String> = vec![
                        "fetch".into(), url.clone(),
                        "--eval".into(), js.clone(),
                        "--quiet".into(),
                    ];
                    if stealth { eval_args.push("--stealth".into()); }
                    if let Some(t) = timeout {
                        eval_args.push("--timeout".into());
                        eval_args.push(t.to_string());
                    }
                    let eval_args_refs: Vec<&str> = eval_args.iter().map(String::as_str).collect();
                    run_obscura(&eval_args_refs).ok().map(|(out, _)| out.trim().to_string())
                } else {
                    None
                };

                let content = clean_obscura_output(&stdout, &stderr);
                let content_preview = truncate_for_display(&content, 4000);

                let mut result = format!(
                    "Fetched {url}\nDump format: {dump}\nStealth: {stealth}\nWait: {wait_until}\n\n{content_preview}"
                );

                if let Some(ref ev) = eval_result {
                    result.push_str(&format!("\n\n── eval result ──\n{ev}"));
                }

                if content.len() > 4000 {
                    result.push_str(&format!("\n\n[…truncated from {} total chars]", content.len()));
                }

                Ok(ToolResult::success(result))
            }

            "scrape" => {
                let urls: Vec<String> = input
                    .get("urls")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| ToolError::invalid_input("Missing 'urls' array for scrape"))?
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();

                if urls.is_empty() {
                    return Err(ToolError::invalid_input("'urls' must contain at least one URL"));
                }

                let eval_js = get_str(&input, "eval").ok();
                let concurrency = input.get("concurrency").and_then(|v| v.as_u64()).unwrap_or(10);
                let timeout = input.get("timeout").and_then(|v| v.as_u64()).unwrap_or(60);

                let mut args: Vec<String> = vec![
                    "scrape".into(),
                    "--concurrency".into(), concurrency.to_string(),
                    "--timeout".into(), timeout.to_string(),
                    "--format".into(), "json".into(),
                ];
                if let Some(ref js) = eval_js {
                    args.push("--eval".into());
                    args.push(js.clone());
                }
                for url in &urls {
                    args.push(url.clone());
                }

                let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                let (stdout, stderr) = run_obscura(&args_refs)
                    .map_err(|e| ToolError::execution_failed(e))?;

                let content = clean_obscura_output(&stdout, &stderr);

                // Try to parse JSON for structured summary
                let summary = if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                    if let Some(arr) = parsed.as_array() {
                        let count = arr.len();
                        let samples: Vec<String> = arr.iter().take(3).map(|v| {
                            let url = v.get("url").and_then(|u| u.as_str()).unwrap_or("?");
                            let val = v.get("result").or_else(|| v.get("value"))
                                .and_then(|r| r.as_str())
                                .unwrap_or("(no result)");
                            let preview = if val.len() > 120 {
                                format!("{}...", &val[..120])
                            } else {
                                val.to_string()
                            };
                            format!("  {url}: {preview}")
                        }).collect();

                        let mut s = format!(
                            "Scraped {count} URLs (concurrency: {concurrency}, timeout: {timeout}s):\n{}",
                            samples.join("\n")
                        );
                        if count > 3 {
                            s.push_str(&format!("\n  ... and {} more", count - 3));
                        }
                        s
                    } else {
                        format!("Scraped {} URLs\n{}", urls.len(), truncate_for_display(&content, 3000))
                    }
                } else {
                    format!("Scraped {} URLs\n{}", urls.len(), truncate_for_display(&content, 3000))
                };

                Ok(ToolResult::success(summary))
            }

            "serve" => {
                let port = input.get("port").and_then(|v| v.as_u64()).unwrap_or(9222);
                let stealth = input.get("stealth").and_then(|v| v.as_bool()).unwrap_or(false);

                let mut args: Vec<String> = vec![
                    "serve".into(),
                    "--port".into(), port.to_string(),
                ];
                if stealth { args.push("--stealth".into()); }

                // Start obscura serve as a background process
                let args_refs: Vec<String> = args.clone();

                let mut child = std::process::Command::new("obscura")
                    .args(&args_refs)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| ToolError::execution_failed(format!("Failed to start obscura serve: {e}")))?;

                // Give it a moment to start
                std::thread::sleep(std::time::Duration::from_millis(1500));

                // Check if still running
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let stderr_bytes = child.stderr.as_mut()
                            .and_then(|s| {
                                use std::io::Read;
                                let mut buf = Vec::new();
                                s.read_to_end(&mut buf).ok()?;
                                Some(String::from_utf8_lossy(&buf).to_string())
                            })
                            .unwrap_or_default();
                        return Err(ToolError::execution_failed(format!(
                            "obscura serve exited early (status: {status}): {stderr_bytes}"
                        )));
                    }
                    Ok(None) => {
                        // Still running — success. Detach the child.
                        let pid = child.id();
                        // Forget the child handle so it keeps running
                        std::mem::forget(child);

                        let stealth_note = if stealth { " (stealth mode)" } else { "" };
                        Ok(ToolResult::success(format!(
                            "Obscura CDP server started{stealth_note}\n  WebSocket: ws://127.0.0.1:{port}/devtools/browser\n  PID: {pid}\n  Connect with: puppeteer.connect({{ browserWSEndpoint: 'ws://127.0.0.1:{port}/devtools/browser' }})\n  Or via agent-browser: agent-browser --cdp {port} open <url>"
                        )))
                    }
                    Err(e) => {
                        Err(ToolError::execution_failed(format!("Failed to check obscura serve status: {e}")))
                    }
                }
            }

            _ => Err(ToolError::invalid_input(format!(
                "Unknown action '{action}'"
            ))),
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn get_str(input: &Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| ToolError::invalid_input(format!("Missing '{key}'")))
}

/// Strip obscura's ASCII art banner from output.
/// The banner is 15 lines of ASCII art that obscura prints on every command.
fn clean_obscura_output(stdout: &str, _stderr: &str) -> String {
    // obscura prints an ASCII banner before actual output
    // Look for the banner end marker (blank line after the art) and strip
    let lines: Vec<&str> = stdout.lines().collect();
    let mut start_idx = 0;
    for (i, line) in lines.iter().enumerate() {
        // The banner ends at the version line
        if line.contains("Headless Browser v") || line.contains("CDP server:") {
            start_idx = i + 1;
        }
    }
    if start_idx > 0 && start_idx < lines.len() {
        lines[start_idx..].join("\n").trim().to_string()
    } else {
        stdout.trim().to_string()
    }
}

fn truncate_for_display(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        format!("{}...", &text[..max_chars])
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_banner_strips_ascii_art() {
        let output = r#"   ____  _                              
  / __ \| |                             
 | |  | | |__  ___  ___ _   _ _ __ __ _ 
 | |  | | '_ \/ __|/ __| | | | '__/ _` |
 | |__| | |_) \__ \ (__| |_| | | | (_| |
  \____/|_.__/|___/\___|\__,_|_|  \__,_|
                   
  Headless Browser v0.1.0
  CDP server: ws://127.0.0.1:9222/devtools/browser

Actual output here"#;
        let cleaned = clean_obscura_output(output, "");
        assert_eq!(cleaned, "Actual output here");
    }

    #[test]
    fn test_clean_no_banner() {
        let output = "Just some plain text output";
        let cleaned = clean_obscura_output(output, "");
        assert_eq!(cleaned, "Just some plain text output");
    }

    #[test]
    fn test_truncate() {
        let text = "abcdefghij";
        assert_eq!(truncate_for_display(text, 5), "abcde...");
        assert_eq!(truncate_for_display(text, 20), "abcdefghij");
    }

    #[test]
    fn test_is_available_cached() {
        let a = is_obscura_available();
        let b = is_obscura_available();
        assert_eq!(a, b);
    }
}
