//! Security configuration.

use serde::{Deserialize, Serialize};

/// Protection level for prompt injection defense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionLevel {
    /// No protection (not recommended).
    Off,
    /// Detect and warn but don't block.
    Warn,
    /// Standard protection (default).
    #[default]
    Standard,
    /// Strict mode (recommended for enterprise).
    Strict,
}

/// Security configuration loaded from `[security]` in config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Overall injection protection level.
    #[serde(default)]
    pub injection_protection: ProtectionLevel,

    /// Threat score threshold for warning (0.0 - 1.0).
    #[serde(default = "default_warn_threshold")]
    pub threat_warn_threshold: f64,

    /// Threat score threshold for blocking (0.0 - 1.0).
    #[serde(default = "default_block_threshold")]
    pub threat_block_threshold: f64,

    /// Enable sensitive information leak detection.
    #[serde(default = "default_true")]
    pub sensitive_leak_detection: bool,

    /// Redact detected secrets in tool outputs shown to the model.
    #[serde(default = "default_true")]
    pub redact_in_output: bool,

    /// Enable runtime behavior monitoring.
    #[serde(default = "default_true")]
    pub runtime_monitor: bool,

    /// Max behavioral anomalies per turn before halting.
    #[serde(default = "default_max_anomalies")]
    pub max_anomalies_per_turn: u32,

    /// Wrap external content with boundary markers.
    #[serde(default = "default_true")]
    pub content_boundary_markers: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            injection_protection: ProtectionLevel::Standard,
            threat_warn_threshold: default_warn_threshold(),
            threat_block_threshold: default_block_threshold(),
            sensitive_leak_detection: true,
            redact_in_output: true,
            runtime_monitor: true,
            max_anomalies_per_turn: default_max_anomalies(),
            content_boundary_markers: true,
        }
    }
}

impl SecurityConfig {
    /// Whether any protection is active.
    pub fn is_active(&self) -> bool {
        self.injection_protection != ProtectionLevel::Off
    }

    /// Whether to block on threat detection (vs just warn).
    pub fn should_block(&self) -> bool {
        matches!(
            self.injection_protection,
            ProtectionLevel::Standard | ProtectionLevel::Strict
        )
    }
}

fn default_warn_threshold() -> f64 {
    0.3
}
fn default_block_threshold() -> f64 {
    0.7
}
fn default_true() -> bool {
    true
}
fn default_max_anomalies() -> u32 {
    5
}
