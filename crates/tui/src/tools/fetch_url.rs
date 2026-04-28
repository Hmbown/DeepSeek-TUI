//! Direct-fetch HTTP tool. Complements `web_search` for cases where the user
//! already knows the URL — a known repo, a blog post, a spec page — and
//! search is overkill or actively unhelpful.
//!
//! Returns a structured `{url, status, content_type, content, truncated}`
//! payload. HTML responses are stripped to readable text by default
//! (`format = "markdown"`); pass `format = "raw"` to keep the bytes intact
//! when the model wants to do its own parsing.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, optional_u64,
};
use crate::network_policy::{Decision, host_from_url};
use async_trait::async_trait;
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::OnceLock;
use std::time::Duration;

const DEFAULT_MAX_BYTES: u64 = 1_000_000;
const HARD_MAX_BYTES: u64 = 10 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const HARD_MAX_TIMEOUT_MS: u64 = 60_000;
const MAX_REDIRECTS: usize = 5;
const USER_AGENT: &str =
    "Mozilla/5.0 (compatible; deepseek-tui/0.5; +https://github.com/Hmbown/DeepSeek-TUI)";

static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
static STYLE_RE: OnceLock<Regex> = OnceLock::new();
static TAG_RE: OnceLock<Regex> = OnceLock::new();
static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

fn script_re() -> &'static Regex {
    SCRIPT_RE.get_or_init(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("script re"))
}
fn style_re() -> &'static Regex {
    STYLE_RE.get_or_init(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("style re"))
}
fn tag_re() -> &'static Regex {
    TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("tag re"))
}
fn whitespace_re() -> &'static Regex {
    WHITESPACE_RE.get_or_init(|| Regex::new(r"\s+").expect("ws re"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Markdown,
    Raw,
}

impl Format {
    fn parse(value: Option<&str>) -> Result<Self, ToolError> {
        match value
            .unwrap_or("markdown")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "text" | "txt" | "plain" => Ok(Self::Text),
            "markdown" | "md" => Ok(Self::Markdown),
            "raw" | "html" | "bytes" => Ok(Self::Raw),
            other => Err(ToolError::invalid_input(format!(
                "unknown format `{other}` (allowed: text, markdown, raw)"
            ))),
        }
    }
}

#[derive(Debug, Serialize)]
struct FetchResponse {
    url: String,
    status: u16,
    content_type: String,
    content: String,
    truncated: bool,
}

pub struct FetchUrlTool;

#[async_trait]
impl ToolSpec for FetchUrlTool {
    fn name(&self) -> &'static str {
        "fetch_url"
    }

    fn description(&self) -> &'static str {
        "Fetch a known URL directly (HTTP GET) and return its content. Use this when the user gives a URL or you already know the canonical link — it's faster and more reliable than web_search for known pages."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Absolute HTTP/HTTPS URL to fetch."
                },
                "format": {
                    "type": "string",
                    "enum": ["text", "markdown", "raw"],
                    "description": "Post-processing for the response body. `markdown` (default) and `text` strip HTML tags to readable text; `raw` returns the body bytes as-is."
                },
                "max_bytes": {
                    "type": "integer",
                    "description": "Truncate response body after this many bytes (default 1,000,000; hard max 10,485,760)."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Request timeout in milliseconds (default 15,000; max 60,000)."
                }
            },
            "required": ["url"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Network]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let url = input
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_input("`url` is required"))?
            .trim()
            .to_string();

        if url.is_empty() {
            return Err(ToolError::invalid_input("`url` cannot be empty"));
        }
        let scheme_ok = url.starts_with("http://") || url.starts_with("https://");
        if !scheme_ok {
            return Err(ToolError::invalid_input(
                "only http:// and https:// URLs are supported",
            ));
        }

        // Per-domain network policy gate (#135). If no policy is attached
        // (e.g. ad-hoc tests), behavior is permissive — match pre-v0.7.0.
        if let Some(decider) = context.network_policy.as_ref()
            && let Some(host) = host_from_url(&url)
        {
            match decider.evaluate(&host, "fetch_url") {
                Decision::Allow => {}
                Decision::Deny => {
                    return Err(ToolError::permission_denied(format!(
                        "network call to '{host}' blocked by network policy"
                    )));
                }
                Decision::Prompt => {
                    return Err(ToolError::permission_denied(format!(
                        "network call to '{host}' requires approval; \
                         re-run after `/network allow {host}` or set network.default = \"allow\" in config"
                    )));
                }
            }
        }

        let format = Format::parse(input.get("format").and_then(Value::as_str))?;
        let max_bytes = optional_u64(&input, "max_bytes", DEFAULT_MAX_BYTES).min(HARD_MAX_BYTES);
        let timeout_ms =
            optional_u64(&input, "timeout_ms", DEFAULT_TIMEOUT_MS).min(HARD_MAX_TIMEOUT_MS);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .build()
            .map_err(|e| {
                ToolError::execution_failed(format!("failed to build HTTP client: {e}"))
            })?;

        let resp = client
            .get(&url)
            .header("Accept", "text/html,text/plain,application/json,*/*;q=0.5")
            .header("Accept-Language", "en-US,en;q=0.5")
            .send()
            .await
            .map_err(|e| ToolError::execution_failed(format!("request failed: {e}")))?;

        let final_url = resp.url().to_string();
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ToolError::execution_failed(format!("failed to read body: {e}")))?;
        let total_bytes = bytes.len() as u64;
        let truncated = total_bytes > max_bytes;
        let usable = if truncated {
            &bytes[..max_bytes as usize]
        } else {
            &bytes[..]
        };

        let body_text = String::from_utf8_lossy(usable).to_string();
        let processed = match format {
            Format::Raw => body_text,
            Format::Text | Format::Markdown => {
                if content_type.contains("text/html") || body_text.contains("<html") {
                    html_to_text(&body_text)
                } else {
                    body_text
                }
            }
        };

        let response = FetchResponse {
            url: final_url,
            status: status.as_u16(),
            content_type,
            content: processed,
            truncated,
        };

        if !status.is_success() {
            // Don't `Err` on 4xx/5xx — the caller often wants to see the body
            // (e.g. a JSON error envelope). Mark the result as a failure so the
            // engine renders it as such.
            return Ok(ToolResult {
                content: serde_json::to_string_pretty(&response).map_err(|e| {
                    ToolError::execution_failed(format!("failed to serialize response: {e}"))
                })?,
                success: false,
                metadata: None,
            });
        }

        ToolResult::json(&response)
            .map_err(|e| ToolError::execution_failed(format!("failed to serialize response: {e}")))
    }
}

/// Strip `<script>` / `<style>` blocks, drop remaining tags, and collapse
/// whitespace. Good enough for "let the model read this page" — not a full
/// HTML-to-Markdown converter.
fn html_to_text(html: &str) -> String {
    let no_script = script_re().replace_all(html, "");
    let no_style = style_re().replace_all(&no_script, "");
    let no_tags = tag_re().replace_all(&no_style, " ");
    let decoded = decode_entities(&no_tags);
    whitespace_re()
        .replace_all(&decoded, " ")
        .trim()
        .to_string()
}

/// Decode the handful of HTML entities we expect to hit in stripped text.
/// Pulling in `html-escape` for the long tail isn't worth the dep weight.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::spec::ToolContext;
    use std::path::PathBuf;

    fn ctx() -> ToolContext {
        ToolContext::new(PathBuf::from("."))
    }

    #[test]
    fn html_to_text_strips_scripts_styles_and_tags() {
        let html = r#"
            <html>
              <head>
                <style>body { color: red; }</style>
                <script>alert("nope");</script>
              </head>
              <body>
                <h1>Hello &amp; welcome</h1>
                <p>This is <b>important</b>.</p>
              </body>
            </html>
        "#;
        let text = html_to_text(html);
        assert!(text.contains("Hello & welcome"));
        assert!(text.contains("This is important"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color: red"));
    }

    #[test]
    fn format_parse_accepts_aliases_and_rejects_unknown() {
        assert_eq!(Format::parse(Some("markdown")).unwrap(), Format::Markdown);
        assert_eq!(Format::parse(Some("MD")).unwrap(), Format::Markdown);
        assert_eq!(Format::parse(Some("text")).unwrap(), Format::Text);
        assert_eq!(Format::parse(Some("raw")).unwrap(), Format::Raw);
        assert_eq!(Format::parse(None).unwrap(), Format::Markdown);
        assert!(Format::parse(Some("yaml")).is_err());
    }

    #[tokio::test]
    async fn rejects_non_http_schemes() {
        let tool = FetchUrlTool;
        let res = tool
            .execute(json!({"url": "file:///etc/passwd"}), &ctx())
            .await;
        let err = res.unwrap_err();
        assert!(format!("{err:?}").contains("http"));
    }

    #[tokio::test]
    async fn rejects_empty_url() {
        let tool = FetchUrlTool;
        let res = tool.execute(json!({"url": "   "}), &ctx()).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn rejects_missing_url() {
        let tool = FetchUrlTool;
        let res = tool.execute(json!({}), &ctx()).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn network_policy_denies_blocked_host() {
        use crate::network_policy::{Decision, NetworkPolicy, NetworkPolicyDecider};
        let policy = NetworkPolicy {
            default: Decision::Deny.into(),
            allow: vec!["api.deepseek.com".to_string()],
            deny: vec![],
            audit: false,
        };
        let decider = NetworkPolicyDecider::new(policy, None);
        let ctx = ToolContext::new(PathBuf::from(".")).with_network_policy(decider);
        let tool = FetchUrlTool;
        let res = tool
            .execute(json!({"url": "https://example.com/foo"}), &ctx)
            .await;
        let err = res.expect_err("blocked host should fail");
        assert!(format!("{err}").contains("blocked"));
    }
}
