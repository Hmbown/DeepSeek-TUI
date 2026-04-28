//! Shared error taxonomy across client, tools, runtime, and UI.
use std::fmt;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Unified envelope used when crossing subsystem boundaries.
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

    /// Recoverable internal error — stream stalls, transient retries, generic
    /// engine errors that the user can resolve by retrying. Severity is
    /// `Warning` so the UI surfaces it in amber rather than red.
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCategory::Internal,
            ErrorSeverity::Warning,
            true,
            "transient",
            message,
        )
    }

    /// Non-recoverable internal error — missing client, spawn failure, etc.
    /// Flips the session into offline mode.
    #[must_use]
    pub fn fatal(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCategory::Internal,
            ErrorSeverity::Error,
            false,
            "fatal",
            message,
        )
    }

    /// Authentication failure — fatal and blocks the session.
    #[must_use]
    pub fn fatal_auth(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCategory::Authentication,
            ErrorSeverity::Critical,
            false,
            "auth_fatal",
            message,
        )
    }

    /// Context length / overflow — invalid input, recoverable via /compact.
    #[must_use]
    pub fn context_overflow(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCategory::InvalidInput,
            ErrorSeverity::Error,
            true,
            "context_overflow",
            message,
        )
    }

    /// Recoverable network / transport hiccup.
    #[must_use]
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCategory::Network,
            ErrorSeverity::Warning,
            true,
            "network_transient",
            message,
        )
    }

    /// Tool execution failure.
    #[must_use]
    pub fn tool(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCategory::Tool,
            ErrorSeverity::Error,
            true,
            "tool_failed",
            message,
        )
    }

    /// Build an envelope by classifying a raw error message string. Used at
    /// boundaries where the underlying error type was already stringified.
    #[must_use]
    pub fn classify(message: impl Into<String>, recoverable: bool) -> Self {
        let message = message.into();
        let category = classify_error_message(&message);
        let severity = match category {
            ErrorCategory::Authentication => ErrorSeverity::Critical,
            ErrorCategory::RateLimit | ErrorCategory::Timeout | ErrorCategory::Network => {
                ErrorSeverity::Warning
            }
            ErrorCategory::InvalidInput | ErrorCategory::Authorization | ErrorCategory::Parse => {
                ErrorSeverity::Error
            }
            ErrorCategory::Tool | ErrorCategory::State | ErrorCategory::Internal => {
                if recoverable {
                    ErrorSeverity::Warning
                } else {
                    ErrorSeverity::Error
                }
            }
        };
        Self::new(
            category,
            severity,
            recoverable,
            category.to_string(),
            message,
        )
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
    if lower.contains("network")
        || lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("temporarily unavailable")
        || lower.contains(" 502 ")
        || lower.contains(" 503 ")
        || lower.contains(" 504 ")
        || lower.starts_with("502 ")
        || lower.starts_with("503 ")
        || lower.starts_with("504 ")
        || lower.ends_with(" 502")
        || lower.ends_with(" 503")
        || lower.ends_with(" 504")
        || lower == "502"
        || lower == "503"
        || lower == "504"
    {
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

/// Stream‑level error discriminated by origin.
///
/// Each variant maps to an `ErrorCategory` so the UI can render
/// stream‑specific icons or formatting. Wired into engine.rs at the three
/// stream guard sites (chunk timeout, max-bytes overflow, max-duration).
#[derive(Debug, Clone)]
pub enum StreamError {
    /// Stream stalled — no chunk received within the idle timeout.
    Stall { timeout_secs: u64 },
    /// Stream exceeded content size limit.
    Overflow { limit_bytes: usize },
    /// Stream exceeded wall‑clock duration limit.
    DurationLimit { limit_secs: u64 },
}

impl StreamError {
    /// Convert directly into an `ErrorEnvelope` for emission on the engine
    /// event channel. Stalls are warning-severity and recoverable; size and
    /// duration limits are errors (the user must restart the turn).
    #[must_use]
    pub fn into_envelope(self) -> ErrorEnvelope {
        match self {
            Self::Stall { timeout_secs } => ErrorEnvelope::new(
                ErrorCategory::Timeout,
                ErrorSeverity::Warning,
                true,
                "stream_stall",
                format!("Stream stalled: no data received for {timeout_secs}s, closing stream"),
            ),
            Self::Overflow { limit_bytes } => ErrorEnvelope::new(
                ErrorCategory::Internal,
                ErrorSeverity::Error,
                true,
                "stream_overflow",
                format!("Stream exceeded maximum content size of {limit_bytes} bytes, closing"),
            ),
            Self::DurationLimit { limit_secs } => ErrorEnvelope::new(
                ErrorCategory::Timeout,
                ErrorSeverity::Error,
                true,
                "stream_duration_limit",
                format!("Stream exceeded maximum duration of {limit_secs}s, closing"),
            ),
        }
    }
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stall { timeout_secs } => {
                write!(f, "Stream stalled after {timeout_secs}s idle")
            }
            Self::Overflow { limit_bytes } => {
                write!(f, "Stream exceeded {limit_bytes} bytes limit")
            }
            Self::DurationLimit { limit_secs } => {
                write!(f, "Stream exceeded {limit_secs}s duration limit")
            }
        }
    }
}

impl std::error::Error for StreamError {}
