//! LLM Security Module — Prompt Injection Defense & Runtime Protection.
//!
//! This module provides multi-layered defense against prompt injection attacks,
//! sensitive data leakage, and behavioral anomalies in the coding agent context.
//!
//! # Architecture
//!
//! ```text
//! Input Sanitizer → Context Boundary Enforcer → LLM → Output Validator → Runtime Monitor
//! ```
//!
//! # Configuration
//!
//! Controlled via `[security]` in `~/.deepseek/config.toml`.

pub mod boundary;
pub mod config;
pub mod hard_block;
pub mod leak_detector;
pub mod monitor;
pub mod sanitizer;
pub mod threat;

// Re-exports for convenience
pub use config::SecurityConfig;
pub use hard_block::{HardBlockReason, hard_block_check};
pub use leak_detector::{LeakDetection, check_sensitive_leak};
pub use monitor::{MonitorDecision, RuntimeMonitor};
pub use sanitizer::{ContentSource, sanitize_external_content};
pub use threat::{ThreatAction, ThreatAssessment, assess_threat};
