//! Non-blocking startup update check.
//!
//! On TUI launch, spawns a background task that queries the GitHub Releases API
//! for the latest version. If a newer version exists, a notification is sent to
//! the UI via the event channel. The check is throttled to at most once per 24h
//! using a local cache file at `~/.deepseek/update_check.json`.
//!
//! This module intentionally:
//! - Never blocks the TUI startup path
//! - Silently swallows all errors (network down, rate-limited, etc.)
//! - Respects user opt-out via `[updates] check_on_startup = false`

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How often to check for updates (24 hours).
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Timeout for the GitHub API request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// GitHub API endpoint for the latest release.
const RELEASES_URL: &str = "https://api.github.com/repos/Hmbown/DeepSeek-TUI/releases/latest";

/// Result of an update check.
#[derive(Debug, Clone)]
pub struct UpdateAvailable {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
}

impl UpdateAvailable {
    /// Format as a user-facing notification string.
    pub fn notification_message(&self) -> String {
        format!(
            "🆕 Update available: {} → {} — run `deepseek update` to upgrade",
            self.current_version, self.latest_version
        )
    }
}

/// Cached state of the last update check.
#[derive(Debug, Serialize, Deserialize)]
struct UpdateCheckCache {
    /// Unix timestamp of last successful check.
    last_check_at: i64,
    /// Latest version found at that time.
    latest_version: String,
    /// Whether user was already notified for this version.
    #[serde(default)]
    notified: bool,
}

/// Spawn a non-blocking update check task.
///
/// Returns a `tokio::task::JoinHandle` that resolves to `Some(UpdateAvailable)`
/// if a new version is found, or `None` if already up-to-date / check skipped.
///
/// The caller should `.await` this in a `select!` or poll it non-blockingly
/// to avoid delaying startup.
pub fn spawn_update_check() -> tokio::task::JoinHandle<Option<UpdateAvailable>> {
    tokio::task::spawn(async move { check_for_update().await })
}

async fn check_for_update() -> Option<UpdateAvailable> {
    let cache_path = cache_file_path()?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    // Check throttle: skip if last check was less than CHECK_INTERVAL_SECS ago
    if let Some(cache) = load_cache(&cache_path) {
        let now = chrono::Utc::now().timestamp();
        if (now - cache.last_check_at) < CHECK_INTERVAL_SECS as i64 {
            // Within cooldown — still report if there's an unacknowledged newer version
            if !cache.notified && is_newer(&cache.latest_version, &current_version) {
                return Some(UpdateAvailable {
                    current_version,
                    latest_version: cache.latest_version,
                    release_url: format!(
                        "https://github.com/Hmbown/DeepSeek-TUI/releases/latest"
                    ),
                });
            }
            return None;
        }
    }

    // Fetch latest release from GitHub
    let latest = fetch_latest_version().await?;
    let latest_tag = latest.tag_name.trim_start_matches('v').to_string();

    // Save cache
    let is_new = is_newer(&latest_tag, &current_version);
    save_cache(
        &cache_path,
        &UpdateCheckCache {
            last_check_at: chrono::Utc::now().timestamp(),
            latest_version: latest_tag.clone(),
            notified: !is_new, // If not new, mark as already notified
        },
    );

    if is_new {
        Some(UpdateAvailable {
            current_version,
            latest_version: latest_tag,
            release_url: latest
                .html_url
                .unwrap_or_else(|| "https://github.com/Hmbown/DeepSeek-TUI/releases/latest".to_string()),
        })
    } else {
        None
    }
}

/// Compare semver strings: returns true if `latest` is strictly newer than `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<(u32, u32, u32)> {
        let v = v.trim_start_matches('v');
        let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        if parts.is_empty() {
            return None;
        }
        // Pad with zeros: "0.9" → (0, 9, 0), "1" → (1, 0, 0)
        Some((
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        ))
    };

    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

fn cache_file_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = home.join(".deepseek");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("update_check.json"))
}

fn load_cache(path: &PathBuf) -> Option<UpdateCheckCache> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cache(path: &PathBuf, cache: &UpdateCheckCache) {
    if let Ok(content) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(path, content);
    }
}

/// Mark the current version as notified so we don't show the banner again.
pub fn mark_notified(version: &str) {
    if let Some(cache_path) = cache_file_path() {
        if let Some(mut cache) = load_cache(&cache_path) {
            if cache.latest_version == version {
                cache.notified = true;
                save_cache(&cache_path, &cache);
            }
        }
    }
}

// === GitHub API types ===

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: Option<String>,
}

async fn fetch_latest_version() -> Option<GitHubRelease> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent("deepseek-tui-update-check")
        .build()
        .ok()?;

    let response = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    response.json::<GitHubRelease>().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_works() {
        assert!(is_newer("0.8.15", "0.8.14"));
        assert!(is_newer("0.9.0", "0.8.14"));
        assert!(is_newer("1.0.0", "0.8.14"));
        assert!(!is_newer("0.8.14", "0.8.14"));
        assert!(!is_newer("0.8.13", "0.8.14"));
        assert!(!is_newer("0.7.0", "0.8.14"));
    }

    #[test]
    fn version_with_v_prefix() {
        assert!(is_newer("v0.8.15", "0.8.14"));
        assert!(is_newer("v1.0.0", "v0.8.14"));
    }

    #[test]
    fn version_with_two_parts() {
        // Two-part versions should be zero-padded: "0.9" → (0, 9, 0)
        assert!(is_newer("0.9", "0.8.14"));
        assert!(is_newer("1.0", "0.8.14"));
        assert!(!is_newer("0.8", "0.8.14"));
    }

    #[test]
    fn version_with_one_part() {
        // Single-part versions: "1" → (1, 0, 0)
        assert!(is_newer("1", "0.8.14"));
        assert!(!is_newer("0", "0.8.14"));
    }

    #[test]
    fn notification_message_format() {
        let update = UpdateAvailable {
            current_version: "0.8.14".to_string(),
            latest_version: "0.9.0".to_string(),
            release_url: "https://github.com/Hmbown/DeepSeek-TUI/releases/tag/v0.9.0".to_string(),
        };
        let msg = update.notification_message();
        assert!(msg.contains("0.8.14"));
        assert!(msg.contains("0.9.0"));
        assert!(msg.contains("deepseek update"));
    }
}
