// TODO(integrate): Wire into engine/UI — tracked as future work
#![allow(dead_code)]

//! Shared error taxonomy across client, tools, runtime, and UI.
//!
//! Not yet wired into consumers; will be adopted incrementally.
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
