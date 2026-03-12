//! Lightweight audit logging for sensitive operations.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::{Value, json};

/// Append an audit event to `~/.deepseek/audit.log`.
///
/// This helper is best-effort by design: callers should not fail critical flows
/// if audit persistence fails.
pub fn log_sensitive_event(event: &str, details: Value) {
    if let Err(err) = append_event(event, details) {
        crate::logging::warn(format!("audit log write failed: {err}"));
    }
}

fn append_event(event: &str, details: Value) -> anyhow::Result<()> {
    let path = default_audit_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let record = json!({
        "ts": Utc::now().to_rfc3339(),
        "event": event,
        "details": details,
    });
    writeln!(file, "{}", serde_json::to_string(&record)?)?;
    Ok(())
}

fn default_audit_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("home directory not found"))?;
    Ok(home.join(".deepseek").join("audit.log"))
}
