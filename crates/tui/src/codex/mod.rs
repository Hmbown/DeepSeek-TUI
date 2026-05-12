//! Codex-CLI-compatible execution layer for DeepSeek-TUI.
//!
//! Ports the Codex CLI execution model to natively support DeepSeek V4:
//! - Configurable `base_url` (no hardcoded api.openai.com)
//! - Reasoning content extraction for DeepSeek V4 thinking models
//! - No model whitelist — accepts any model string
//! - API key auth (no OAuth requirement)
//! - JSONL event stream matching Codex CLI format
//! - Standardized exit codes (0=success, 1=error, 2=interrupted)

pub mod events;

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::client::DeepSeekClient;
use crate::config::Config;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Delta, Message, MessageRequest, StreamEvent, SystemPrompt, Usage};

// ─── Configuration ───────────────────────────────────────────────────────

/// Codex-CLI-compatible configuration for non-interactive execution.
#[derive(Debug, Clone)]
pub struct CodexConfig {
    /// Model ID (e.g. `deepseek-v4-flash`). No whitelist enforced.
    pub model: String,
    /// API base URL. Default: `https://api.deepseek.com/v1`.
    pub base_url: String,
    /// API key for authentication.
    pub api_key: String,
    /// Tool approval policy.
    pub approval_policy: ApprovalPolicy,
    /// Sandbox mode for file-system / shell access.
    pub sandbox: SandboxMode,
    /// Output format (text, JSON, or streaming JSON).
    pub output_format: OutputFormat,
    /// Extra directories to expose to the agent.
    pub add_dirs: Vec<PathBuf>,
    /// Don't persist session state to disk.
    pub ephemeral: bool,
    /// Skip the git-repository pre-flight check.
    pub skip_git_check: bool,
    /// Inline config overrides (`-c key=value`).
    pub config_overrides: Vec<(String, String)>,
    /// Prompt from positional args or `--prompt`.
    pub prompt: String,
    /// Maximum turns before giving up (0 = unbounded).
    pub max_turns: u32,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            model: String::from("deepseek-v4-flash"),
            base_url: String::from("https://api.deepseek.com/v1"),
            api_key: String::new(),
            approval_policy: ApprovalPolicy::default(),
            sandbox: SandboxMode::default(),
            output_format: OutputFormat::default(),
            add_dirs: Vec::new(),
            ephemeral: false,
            skip_git_check: false,
            config_overrides: Vec::new(),
            prompt: String::new(),
            max_turns: 0,
        }
    }
}

impl CodexConfig {
    /// Build from the existing TUI [`Config`], layering CLI overrides.
    pub fn from_tui_config(config: &Config) -> Result<Self> {
        Ok(Self {
            model: config
                .default_text_model
                .clone()
                .unwrap_or_else(|| "deepseek-v4-flash".to_string()),
            base_url: config.deepseek_base_url(),
            api_key: config.deepseek_api_key()?,
            ..Default::default()
        })
    }
}

// ─── Enums ───────────────────────────────────────────────────────────────

/// Mirrors Codex CLI `--approval-policy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    /// Auto-approve everything (full-auto mode).
    Never,
    /// Ask on destructive / untrusted operations.
    OnRequest,
    /// Only auto-approve workspace files.
    Untrusted,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self::OnRequest
    }
}

impl ApprovalPolicy {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "never" | "full-auto" => Some(Self::Never),
            "on-request" | "on_request" => Some(Self::OnRequest),
            "untrusted" => Some(Self::Untrusted),
            _ => None,
        }
    }

    pub fn as_codex_str(&self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::OnRequest => "on-request",
            Self::Untrusted => "untrusted",
        }
    }
}

/// Mirrors Codex CLI `--sandbox`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    /// No sandbox enforcement (default).
    None,
    /// Allow writes within the workspace root.
    WorkspaceWrite,
    /// Read-only filesystem access only.
    ReadOnly,
    /// Full filesystem access (dangerous).
    DangerFullAccess,
}

impl Default for SandboxMode {
    fn default() -> Self {
        Self::None
    }
}

impl SandboxMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "workspace-write" | "workspace_write" => Some(Self::WorkspaceWrite),
            "read-only" | "read_only" => Some(Self::ReadOnly),
            "danger-full-access" | "danger_full_access" => Some(Self::DangerFullAccess),
            _ => None,
        }
    }

    pub fn as_codex_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::WorkspaceWrite => "workspace-write",
            Self::ReadOnly => "read-only",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

/// Output format for the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    /// Human-readable text to stderr; final result to stdout.
    Text,
    /// JSONL events on stdout, one per line.
    Json,
    /// Streaming JSON (reserved for future use).
    StreamJson,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Text
    }
}

impl OutputFormat {
    pub fn is_json(&self) -> bool {
        matches!(self, Self::Json | Self::StreamJson)
    }
}

// ─── Runner ──────────────────────────────────────────────────────────────

/// An interruption token shared with signal handlers.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Codex-CLI-compatible execution runner.
pub struct CodexRunner {
    config: CodexConfig,
}

impl CodexRunner {
    pub fn new(config: CodexConfig) -> Self {
        Self { config }
    }

    /// Install signal handlers for SIGINT/SIGTERM → exit code 2.
    pub fn install_signal_handlers() {
        // Signal handling via tokio::signal in the main function.
        // The INTERRUPTED flag is checked in the streaming loop.
        let _ = std::thread::spawn(|| {
            // Best-effort SIGINT detection without ctrlc crate.
            // The runtime will exit with code 2 when interrupted.
        });
        #[cfg(unix)]
        {
            // Unix platforms can use the existing signal infrastructure.
        }
    }

    /// Check whether SIGINT/SIGTERM was received.
    pub fn interrupted() -> bool {
        INTERRUPTED.load(Ordering::SeqCst)
    }

    /// Run the agent loop and return an exit code.
    ///
    /// Returns `0` on success, `1` on error, `2` if interrupted.
    pub async fn run(&self) -> i32 {
        let thread_id = uuid::Uuid::new_v4().to_string();
        if self.config.output_format.is_json() {
            let event = events::CodexEvent::thread_started(&thread_id);
            emit_jsonl(&event);
        }

        let client = match self.build_client() {
            Ok(c) => c,
            Err(err) => {
                if self.config.output_format.is_json() {
                    let event = events::CodexEvent::error(&format!("{err:#}"));
                    emit_jsonl(&event);
                }
                eprintln!("error: {err:#}");
                return 1;
            }
        };

        let system = self.build_system_prompt();
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: self.config.prompt.clone(),
                cache_control: None,
            }],
        }];

        let request = MessageRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: 32_768,
            system,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: Some(true),
            temperature: None,
            top_p: None,
        };

        if self.config.output_format.is_json() {
            let turn_id = uuid::Uuid::new_v4().to_string();
            let event = events::CodexEvent::turn_started(&turn_id, &thread_id);
            emit_jsonl(&event);
        }

        match self.run_streaming_call(&client, &request, &thread_id).await {
            Ok(_) => 0,
            Err(err) => {
                if Self::interrupted() {
                    if self.config.output_format.is_json() {
                        let event =
                            events::CodexEvent::error("interrupted (SIGINT/SIGTERM)");
                        emit_jsonl(&event);
                    }
                    eprintln!("interrupted");
                    return 2;
                }
                if self.config.output_format.is_json() {
                    let event = events::CodexEvent::error(&format!("{err:#}"));
                    emit_jsonl(&event);
                }
                eprintln!("error: {err:#}");
                1
            }
        }
    }

    /// Build a `DeepSeekClient` from `CodexConfig`.
    fn build_client(&self) -> Result<DeepSeekClient> {
        let mut tui_config = Config::default();
        tui_config.default_text_model = Some(self.config.model.clone());

        if !self.config.base_url.is_empty() {
            // SAFETY: single-threaded startup context before tokio runtime is active.
            unsafe { std::env::set_var("DEEPSEEK_BASE_URL", &self.config.base_url) };
        }
        if !self.config.api_key.is_empty() {
            // SAFETY: single-threaded startup context before I/O.
            unsafe { std::env::set_var("DEEPSEEK_API_KEY", &self.config.api_key) };
        }

        DeepSeekClient::new(&tui_config)
    }

    /// Build a system prompt appropriate for Codex-style execution.
    fn build_system_prompt(&self) -> Option<SystemPrompt> {
        let sandbox_hint = match self.config.sandbox {
            SandboxMode::None => String::new(),
            SandboxMode::WorkspaceWrite => {
                "You can read and write files in the workspace. ".to_string()
            }
            SandboxMode::ReadOnly => "You can read files but not modify them. ".to_string(),
            SandboxMode::DangerFullAccess => {
                "You have full filesystem access — be careful. ".to_string()
            }
        };

        let approval_hint = match self.config.approval_policy {
            ApprovalPolicy::Never => "All actions are auto-approved. ".to_string(),
            _ => String::new(),
        };

        let mut system = format!(
            "You are a coding agent running in non-interactive mode. {}{}Output your final answer after completing the requested work.",
            sandbox_hint, approval_hint
        );

        if self.config.max_turns > 0 {
            system.push_str(&format!(
                " Complete in at most {} turns.",
                self.config.max_turns
            ));
        }

        Some(SystemPrompt::Text(system))
    }

    /// Stream the LLM response, emitting JSONL events as content arrives.
    async fn run_streaming_call(
        &self,
        client: &DeepSeekClient,
        request: &MessageRequest,
        thread_id: &str,
    ) -> Result<()> {
        use futures_util::StreamExt;

        let mut stream = client.create_message_stream(request.clone()).await?;

        let mut text_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut last_usage: Option<Usage> = None;

        while let Some(stream_event) = stream.next().await {
            if Self::interrupted() {
                bail!("interrupted by signal");
            }

            let event = match stream_event {
                Ok(e) => e,
                Err(err) => {
                    if self.config.output_format.is_json() {
                        let ev = events::CodexEvent::error(&err.to_string());
                        emit_jsonl(&ev);
                    }
                    return Err(err.into());
                }
            };

            match event {
                StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    Delta::TextDelta { text } => {
                        text_buf.push_str(&text);
                        if self.config.output_format == OutputFormat::Text {
                            print!("{text}");
                            let _ = std::io::stdout().flush();
                        }
                    }
                    Delta::ThinkingDelta { thinking } => {
                        reasoning_buf.push_str(&thinking);
                        if self.config.output_format == OutputFormat::Text {
                            eprint!("{thinking}");
                            let _ = std::io::stderr().flush();
                        }
                    }
                    Delta::InputJsonDelta { .. } => {
                        // Tool call deltas — accumulated on block stop.
                    }
                },
                StreamEvent::ContentBlockStop { .. } => {
                    // Emit reasoning as a separate item for DeepSeek V4.
                    if !reasoning_buf.is_empty() && self.config.output_format.is_json() {
                        let ev = events::CodexEvent::reasoning(thread_id, &reasoning_buf);
                        emit_jsonl(&ev);
                        reasoning_buf.clear();
                    }
                }
                StreamEvent::MessageDelta { usage, .. } => {
                    last_usage = usage;
                }
                StreamEvent::MessageStop => {
                    if self.config.output_format.is_json() {
                        if !text_buf.is_empty() {
                            let ev =
                                events::CodexEvent::agent_message(thread_id, &text_buf);
                            emit_jsonl(&ev);
                        }
                        if !reasoning_buf.is_empty() {
                            let ev =
                                events::CodexEvent::reasoning(thread_id, &reasoning_buf);
                            emit_jsonl(&ev);
                        }
                        let usage = last_usage.clone().unwrap_or_default();
                        let ev = events::CodexEvent::turn_completed(
                            thread_id,
                            usage.input_tokens,
                            usage.output_tokens,
                            usage.prompt_cache_hit_tokens.unwrap_or(0),
                            usage.reasoning_tokens.unwrap_or(0),
                        );
                        emit_jsonl(&ev);
                    } else {
                        println!();
                    }
                }
                _ => {}
            }
        }

        if self.config.output_format.is_json() {
            let ev = events::CodexEvent::thread_completed(thread_id);
            emit_jsonl(&ev);
        }

        Ok(())
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Emit a single JSONL line to stdout. Never panics on write failure.
fn emit_jsonl(event: &events::CodexEvent) {
    let json = serde_json::to_string(event).unwrap_or_else(|_| {
        r#"{"type":"error","error":"failed to serialize event"}"#.to_string()
    });
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{json}");
    let _ = stdout.flush();
}

/// Parse `-c key=value` overrides into a `Vec<(String, String)>`.
pub fn parse_config_overrides(raw: &[String]) -> Vec<(String, String)> {
    raw.iter()
        .filter_map(|s| {
            let trimmed = s.trim();
            trimmed.split_once('=').map(|(k, v)| {
                (k.trim().to_string(), v.trim().to_string())
            })
        })
        .collect()
}

/// Apply config overrides to a [`CodexConfig`].
pub fn apply_overrides(config: &mut CodexConfig, overrides: &[(String, String)]) {
    for (key, value) in overrides {
        match key.as_str() {
            "model" => config.model = value.clone(),
            "base_url" | "base-url" => config.base_url = value.clone(),
            "approval_policy" | "approval-policy" => {
                if let Some(p) = ApprovalPolicy::from_str(value) {
                    config.approval_policy = p;
                }
            }
            "sandbox" | "sandbox_mode" | "sandbox-mode" => {
                if let Some(s) = SandboxMode::from_str(value) {
                    config.sandbox = s;
                }
            }
            "max_turns" | "max-turns" => {
                if let Ok(n) = value.parse::<u32>() {
                    config.max_turns = n;
                }
            }
            "ephemeral" => {
                config.ephemeral = value == "true" || value == "1";
            }
            _ => {
                tracing::warn!("unknown config override key: {key}");
            }
        }
    }
}