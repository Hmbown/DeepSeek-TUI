//! Shared error taxonomy across client, tools, runtime, and UI.
use std::fmt;
use std::time::Duration;

use crate::llm_client::LlmError;
use crate::tools::spec::ToolError;

/// Broad category for typed error handling and policy decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Network,
    Authentication,
    Authorization,
    RateLimit,
    Timeout,
    InvalidInput,
    Parse,
    Tool,
    State,
    Internal,
}

/// Severity hint for UI and logs.
///
/// Adopted in `From<LlmError>` / `From<ToolError>` and exercised by tests, but
/// not yet read by an audit-log writer or TUI severity colorer. Pending #66
/// follow-up; the type is stable.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Unified envelope used when crossing subsystem boundaries.
///
/// Constructed by the `From<LlmError>` / `From<ToolError>` impls below and
/// validated by tests, but the engine still emits errors as `(String, bool)`
/// pairs on the event channel. Pending #66 follow-up.
#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ErrorEnvelope {
    pub category: ErrorCategory,
    pub severity: ErrorSeverity,
    pub recoverable: bool,
    pub code: String,
    pub message: String,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Network => "network",
            Self::Authentication => "authentication",
            Self::Authorization => "authorization",
            Self::RateLimit => "rate_limit",
            Self::Timeout => "timeout",
            Self::InvalidInput => "invalid_input",
            Self::Parse => "parse",
            Self::Tool => "tool",
            Self::State => "state",
            Self::Internal => "internal",
        };
        f.write_str(label)
    }
}

impl fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
        };
        f.write_str(label)
    }
}

impl fmt::Display for ErrorEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.severity, self.code, self.message)
    }
}

impl std::error::Error for ErrorEnvelope {}

impl ErrorEnvelope {
    #[allow(dead_code)]
    #[must_use]
    pub fn new(
        category: ErrorCategory,
        severity: ErrorSeverity,
        recoverable: bool,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            category,
            severity,
            recoverable,
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<LlmError> for ErrorEnvelope {
    fn from(value: LlmError) -> Self {
        match value {
            LlmError::RateLimited { message, .. } => Self::new(
                ErrorCategory::RateLimit,
                ErrorSeverity::Warning,
                true,
                "llm_rate_limited",
                message,
            ),
            LlmError::ServerError { status, message } => Self::new(
                ErrorCategory::Internal,
                ErrorSeverity::Error,
                true,
                format!("llm_server_{status}"),
                message,
            ),
            LlmError::NetworkError(message) => Self::new(
                ErrorCategory::Network,
                ErrorSeverity::Error,
                true,
                "llm_network_error",
                message,
            ),
            LlmError::Timeout(duration) => Self::new(
                ErrorCategory::Timeout,
                ErrorSeverity::Warning,
                true,
                "llm_timeout",
                format!("Request timed out after {duration:?}"),
            ),
            LlmError::AuthenticationError(message) => Self::new(
                ErrorCategory::Authentication,
                ErrorSeverity::Critical,
                false,
                "llm_auth_error",
                message,
            ),
            LlmError::InvalidRequest { message, .. } => Self::new(
                ErrorCategory::InvalidInput,
                ErrorSeverity::Error,
                false,
                "llm_invalid_request",
                message,
            ),
            LlmError::ModelError(message) => Self::new(
                ErrorCategory::InvalidInput,
                ErrorSeverity::Error,
                false,
                "llm_model_error",
                message,
            ),
            LlmError::ContentPolicyError(message) => Self::new(
                ErrorCategory::Authorization,
                ErrorSeverity::Error,
                false,
                "llm_content_policy",
                message,
            ),
            LlmError::ParseError(message) => Self::new(
                ErrorCategory::Parse,
                ErrorSeverity::Error,
                false,
                "llm_parse_error",
                message,
            ),
            LlmError::ContextLengthError(message) => Self::new(
                ErrorCategory::InvalidInput,
                ErrorSeverity::Error,
                false,
                "llm_context_length",
                message,
            ),
            LlmError::Other(message) => Self::new(
                ErrorCategory::Internal,
                ErrorSeverity::Error,
                true,
                "llm_other",
                message,
            ),
        }
    }
}

/// Classify an error message string into an ErrorCategory.
///
/// Uses heuristic keyword matching on the lowercased message.
/// This is a replacement for ad-hoc string matching in callers.
#[must_use]
pub fn classify_error_message(message: &str) -> ErrorCategory {
    let lower = message.to_lowercase();

    if lower.contains("maximum context length")
        || lower.contains("context length")
        || lower.contains("context_length")
        || lower.contains("prompt is too long")
        || (lower.contains("requested") && lower.contains("tokens") && lower.contains("maximum"))
        || lower.contains("context window")
    {
        return ErrorCategory::InvalidInput;
    }
    if lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("429")
        || lower.contains("quota")
    {
        return ErrorCategory::RateLimit;
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return ErrorCategory::Timeout;
    }
    if lower.contains("auth") || lower.contains("unauthorized") || lower.contains("api key") {
        return ErrorCategory::Authentication;
    }
    if lower.contains("permission") || lower.contains("forbidden") || lower.contains("denied") {
        return ErrorCategory::Authorization;
    }
    if lower.contains("network") || lower.contains("connection") || lower.contains("dns") {
        return ErrorCategory::Network;
    }
    if lower.contains("parse") || lower.contains("syntax") || lower.contains("malformed") {
        return ErrorCategory::Parse;
    }
    if lower.contains("not found")
        || lower.contains("unavailable")
        || lower.contains("not available")
    {
        return ErrorCategory::State;
    }
    if lower.contains("tool") {
        return ErrorCategory::Tool;
    }

    ErrorCategory::Internal
}

impl From<ToolError> for ErrorEnvelope {
    fn from(value: ToolError) -> Self {
        match value {
            ToolError::InvalidInput { message } => Self::new(
                ErrorCategory::InvalidInput,
                ErrorSeverity::Error,
                false,
                "tool_invalid_input",
                message,
            ),
            ToolError::MissingField { field } => Self::new(
                ErrorCategory::InvalidInput,
                ErrorSeverity::Error,
                false,
                "tool_missing_field",
                format!("Missing required field: {field}"),
            ),
            ToolError::PathEscape { path } => Self::new(
                ErrorCategory::Authorization,
                ErrorSeverity::Error,
                false,
                "tool_path_escape",
                format!("Path escapes workspace: {}", path.display()),
            ),
            ToolError::ExecutionFailed { message } => Self::new(
                ErrorCategory::Tool,
                ErrorSeverity::Error,
                true,
                "tool_execution_failed",
                message,
            ),
            ToolError::Timeout { seconds } => Self::new(
                ErrorCategory::Timeout,
                ErrorSeverity::Warning,
                true,
                "tool_timeout",
                format!("Tool timed out after {seconds}s"),
            ),
            ToolError::NotAvailable { message } => Self::new(
                ErrorCategory::State,
                ErrorSeverity::Error,
                false,
                "tool_not_available",
                message,
            ),
            ToolError::PermissionDenied { message } => Self::new(
                ErrorCategory::Authorization,
                ErrorSeverity::Error,
                false,
                "tool_permission_denied",
                message,
            ),
        }
    }
}

/// Client‑side error wrapper surfaced to the UI.
///
/// Carries a full `ErrorEnvelope` so the TUI can render category‑specific
/// styling instead of a generic `Event::Error { message, recoverable }`.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ClientError {
    /// Transport / HTTP / auth error from the LLM provider.
    Provider {
        envelope: ErrorEnvelope,
        /// When true the engine should attempt a retry.
        retryable: bool,
    },
    /// Error originating from the stream (SSE / chunk decode / protocol).
    Stream {
        envelope: ErrorEnvelope,
        retryable: bool,
    },
    /// Generic internal error that doesn't fit a provider taxonomy.
    Internal { envelope: ErrorEnvelope },
}

#[allow(dead_code)]
impl ClientError {
    /// Unwrap the inner envelope regardless of variant.
    #[must_use]
    pub fn envelope(&self) -> &ErrorEnvelope {
        match self {
            Self::Provider { envelope, .. }
            | Self::Stream { envelope, .. }
            | Self::Internal { envelope } => envelope,
        }
    }

    /// Whether this error is eligible for a transparent retry.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Provider { retryable, .. } | Self::Stream { retryable, .. } => *retryable,
            Self::Internal { .. } => false,
        }
    }

    /// Construct from an `LlmError` with Retry‑After header support.
    pub fn from_llm_error(err: LlmError, retry_after: Option<Duration>) -> Self {
        let retryable = err.is_retryable();
        let envelope: ErrorEnvelope = err.into();
        if retryable {
            let envelope = if let Some(delay) = retry_after {
                ErrorEnvelope {
                    code: format!("{}:retry_after_{}s", envelope.code, delay.as_secs()),
                    ..envelope
                }
            } else {
                envelope
            };
            Self::Provider {
                envelope,
                retryable: true,
            }
        } else {
            Self::Provider {
                envelope,
                retryable: false,
            }
        }
    }

    /// Construct a stream‑level error.
    pub fn stream(message: impl Into<String>, retryable: bool) -> Self {
        let envelope = ErrorEnvelope::new(
            ErrorCategory::Internal,
            ErrorSeverity::Warning,
            retryable,
            "stream_error",
            message,
        );
        Self::Stream {
            envelope,
            retryable,
        }
    }

    /// Construct an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        let envelope = ErrorEnvelope::new(
            ErrorCategory::Internal,
            ErrorSeverity::Error,
            false,
            "internal",
            message,
        );
        Self::Internal { envelope }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.envelope())
    }
}

impl std::error::Error for ClientError {}

/// Stream‑level error discriminated by origin.
///
/// Each variant maps to an `ErrorCategory` so the UI can render
/// stream‑specific icons or formatting.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum StreamError {
    /// Stream stalled — no chunk received within the idle timeout.
    Stall { timeout_secs: u64 },
    /// Chunk decode / JSON parse failure.
    Decode { message: String },
    /// Stream exceeded content size limit.
    Overflow { limit_bytes: usize },
    /// Stream exceeded wall‑clock duration limit.
    DurationLimit { limit_secs: u64 },
    /// Transport error from the underlying SSE connection.
    Transport { message: String },
}

#[allow(dead_code)]
impl StreamError {
    /// Convert into a `ClientError` for emission.
    #[must_use]
    pub fn into_client_error(self) -> ClientError {
        match self {
            Self::Stall { timeout_secs } => {
                ClientError::stream(format!("Stream stalled after {timeout_secs}s idle"), true)
            }
            Self::Decode { message } => {
                ClientError::stream(format!("Stream decode error: {message}"), true)
            }
            Self::Overflow { limit_bytes } => {
                ClientError::stream(format!("Stream exceeded {limit_bytes} bytes limit"), false)
            }
            Self::DurationLimit { limit_secs } => ClientError::stream(
                format!("Stream exceeded {limit_secs}s duration limit"),
                false,
            ),
            Self::Transport { message } => ClientError::stream(message, true),
        }
    }
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stall { timeout_secs } => {
                write!(f, "Stream stalled after {timeout_secs}s idle")
            }
            Self::Decode { message } => write!(f, "Stream decode error: {message}"),
            Self::Overflow { limit_bytes } => {
                write!(f, "Stream exceeded {limit_bytes} bytes limit")
            }
            Self::DurationLimit { limit_secs } => {
                write!(f, "Stream exceeded {limit_secs}s duration limit")
            }
            Self::Transport { message } => write!(f, "Stream transport: {message}"),
        }
    }
}

impl std::error::Error for StreamError {}
