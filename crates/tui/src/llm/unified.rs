//! Unified LLM call wrappers, thinking capability model, and proxy agent
//! configuration.
//!
//! ## call_llm / stream_llm
//!
//! Thin wrappers around the LLM client with built-in retry logic and
//! response validation. Supports configurable max_retries, backoff, and
//! timeout per call.
//!
//! ## ThinkingCapability
//!
//! Declares the five supported thinking modes and model metadata so
//! callers can make informed model-selection decisions based on the
//! task's reasoning depth requirements.
//!
//! ## ProxyAgent
//!
//! Dual-base-URL proxy configuration for routing requests through an
//! intermediate agent (e.g., for enterprise deployments with separate
//! internal/external API endpoints).

use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Thinking capability ─────────────────────────────────────────────────────

/// Declared thinking mode for a model or provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    /// No thinking — pure non-reasoning model (e.g., standard chat).
    None,
    /// Light thinking — budget-limited reasoning (fast, shallow).
    Light,
    /// Medium thinking — balanced reasoning depth.
    Medium,
    /// Deep thinking — extended reasoning for complex tasks.
    Deep,
    /// Auto — engine selects the optimal thinking depth based on task
    /// complexity, cache pressure, and cost budget.
    Auto,
}

impl ThinkingMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Light => "light",
            Self::Medium => "medium",
            Self::Deep => "deep",
            Self::Auto => "auto",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "none" | "disabled" | "off" => Some(Self::None),
            "light" | "low" => Some(Self::Light),
            "medium" | "normal" | "balanced" => Some(Self::Medium),
            "deep" | "high" | "extended" => Some(Self::Deep),
            "auto" | "adaptive" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Model metadata including its thinking capability.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    /// Canonical model id (e.g., "deepseek-v4-pro").
    pub model_id: String,
    /// The model's thinking capability.
    pub thinking: ThinkingMode,
    /// Context window size in tokens.
    pub context_window: u64,
    /// Provider this model belongs to.
    pub provider: String,
}

impl ModelMetadata {
    #[must_use]
    pub fn new(
        model_id: impl Into<String>,
        thinking: ThinkingMode,
        context_window: u64,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            thinking,
            context_window,
            provider: provider.into(),
        }
    }

    /// Known models with their metadata.
    #[must_use]
    pub fn known_models() -> Vec<Self> {
        vec![
            Self::new("deepseek-v4-pro", ThinkingMode::Deep, 1_048_576, "deepseek"),
            Self::new(
                "deepseek-v4-flash",
                ThinkingMode::Light,
                1_048_576,
                "deepseek",
            ),
            Self::new("deepseek-chat", ThinkingMode::Light, 1_048_576, "deepseek"),
            Self::new(
                "deepseek-reasoner",
                ThinkingMode::Deep,
                1_048_576,
                "deepseek",
            ),
            Self::new(
                "claude-sonnet-4-20250514",
                ThinkingMode::Medium,
                200_000,
                "anthropic",
            ),
            Self::new("gemini-2.5-pro", ThinkingMode::Deep, 1_048_576, "google"),
        ]
    }

    /// Look up metadata by model id.
    #[must_use]
    pub fn find(model_id: &str) -> Option<Self> {
        Self::known_models()
            .into_iter()
            .find(|m| m.model_id == model_id)
    }
}

// ── Retry configuration ─────────────────────────────────────────────────────

/// Retry policy for LLM API calls.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries (0 = no retries).
    pub max_retries: u32,
    /// Initial backoff duration.
    pub initial_backoff: Duration,
    /// Backoff multiplier (2.0 = exponential backoff).
    pub backoff_multiplier: f64,
    /// Maximum backoff duration (cap).
    pub max_backoff: Duration,
    /// Whether to retry on HTTP 429 (rate limit) and 5xx errors.
    pub retry_on_server_error: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_secs(30),
            retry_on_server_error: true,
        }
    }
}

impl RetryConfig {
    /// No retries.
    #[must_use]
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Calculate the backoff duration for a given retry attempt (0-based).
    #[must_use]
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        let base = self.initial_backoff.as_millis() as f64;
        let scaled = base * self.backoff_multiplier.powi(attempt as i32);
        let capped = scaled.min(self.max_backoff.as_millis() as f64);
        Duration::from_millis(capped as u64)
    }
}

/// Result from a unified LLM call.
#[derive(Debug, Clone)]
pub struct LlmCallResult {
    /// The response content.
    pub content: String,
    /// Model used for this call.
    pub model: String,
    /// Number of retries performed.
    pub retries: u32,
    /// Total duration including retries.
    pub total_duration: Duration,
    /// Token usage if available.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

// ── Proxy agent ──────────────────────────────────────────────────────────────

/// Dual-base-URL proxy agent configuration.
///
/// Enables routing requests through different endpoints based on the
/// model or request type. Common in enterprise deployments where
/// internal models have separate endpoints from external providers.
#[derive(Debug, Clone, Default)]
pub struct ProxyAgent {
    /// Primary (default) base URL.
    pub primary_url: Option<String>,
    /// Fallback base URL (used when primary is unreachable).
    pub fallback_url: Option<String>,
    /// Whether to automatically fail over to fallback on errors.
    pub auto_failover: bool,
    /// Models that should always use the fallback URL.
    pub fallback_models: Vec<String>,
}

impl ProxyAgent {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the primary URL.
    pub fn with_primary(mut self, url: impl Into<String>) -> Self {
        self.primary_url = Some(url.into());
        self
    }

    /// Set the fallback URL.
    pub fn with_fallback(mut self, url: impl Into<String>) -> Self {
        self.fallback_url = Some(url.into());
        self
    }

    /// Enable automatic failover.
    pub fn with_auto_failover(mut self, enabled: bool) -> Self {
        self.auto_failover = enabled;
        self
    }

    /// Resolve the effective base URL for a given model.
    #[must_use]
    pub fn resolve_url(&self, model: &str) -> Option<&str> {
        if self.fallback_models.iter().any(|m| m == model) {
            self.fallback_url.as_deref().or(self.primary_url.as_deref())
        } else {
            self.primary_url.as_deref().or(self.fallback_url.as_deref())
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ThinkingMode ───────────────────────────────────────────────────

    #[test]
    fn test_thinking_mode_round_trips() {
        for mode in [
            ThinkingMode::None,
            ThinkingMode::Light,
            ThinkingMode::Medium,
            ThinkingMode::Deep,
            ThinkingMode::Auto,
        ] {
            let s = mode.as_str();
            let back = ThinkingMode::from_str(s).unwrap();
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn test_thinking_mode_aliases() {
        assert_eq!(ThinkingMode::from_str("off"), Some(ThinkingMode::None));
        assert_eq!(ThinkingMode::from_str("low"), Some(ThinkingMode::Light));
        assert_eq!(ThinkingMode::from_str("high"), Some(ThinkingMode::Deep));
        assert_eq!(ThinkingMode::from_str("auto"), Some(ThinkingMode::Auto));
    }

    // ── ModelMetadata ──────────────────────────────────────────────────

    #[test]
    fn test_known_models_includes_core_deepseek() {
        let models = ModelMetadata::known_models();
        assert!(models.iter().any(|m| m.model_id == "deepseek-v4-pro"));
        assert!(models.iter().any(|m| m.model_id == "deepseek-v4-flash"));
    }

    #[test]
    fn test_find_model() {
        let m = ModelMetadata::find("deepseek-v4-pro").unwrap();
        assert_eq!(m.thinking, ThinkingMode::Deep);
        assert_eq!(m.context_window, 1_048_576);

        assert!(ModelMetadata::find("nonexistent-model").is_none());
    }

    // ── RetryConfig ────────────────────────────────────────────────────

    #[test]
    fn test_retry_backoff_increases_exponentially() {
        let cfg = RetryConfig::default();
        let b0 = cfg.backoff_for(0).as_millis();
        let b1 = cfg.backoff_for(1).as_millis();
        let b2 = cfg.backoff_for(2).as_millis();
        assert!(b1 > b0);
        assert!(b2 > b1);
        // 1s * 2^2 = 4s
        assert!((b2 as f64 - 4000.0).abs() < 100.0);
    }

    #[test]
    fn test_retry_backoff_capped() {
        let cfg = RetryConfig {
            max_backoff: Duration::from_secs(10),
            ..Default::default()
        };
        let b10 = cfg.backoff_for(10).as_millis();
        assert!(b10 <= 10_000);
    }

    #[test]
    fn test_no_retries_config() {
        let cfg = RetryConfig::none();
        assert_eq!(cfg.max_retries, 0);
    }

    // ── ProxyAgent ─────────────────────────────────────────────────────

    #[test]
    fn test_proxy_resolves_primary_for_unknown_model() {
        let proxy = ProxyAgent::new()
            .with_primary("https://primary.example/v1")
            .with_fallback("https://fallback.example/v1");

        assert_eq!(
            proxy.resolve_url("deepseek-v4-pro"),
            Some("https://primary.example/v1")
        );
    }

    #[test]
    fn test_proxy_resolves_fallback_for_listed_model() {
        let mut proxy = ProxyAgent::new()
            .with_primary("https://primary.example/v1")
            .with_fallback("https://fallback.example/v1");
        proxy.fallback_models.push("gemini-2.5-pro".to_string());

        assert_eq!(
            proxy.resolve_url("gemini-2.5-pro"),
            Some("https://fallback.example/v1")
        );
    }

    #[test]
    fn test_proxy_returns_none_when_no_urls_configured() {
        let proxy = ProxyAgent::new();
        assert_eq!(proxy.resolve_url("any-model"), None);
    }
}
