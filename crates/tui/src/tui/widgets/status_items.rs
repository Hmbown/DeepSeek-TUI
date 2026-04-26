//! Configurable status-line items (#95).
//!
//! `StatusItem` is the canonical enum of footer/header chips the user can
//! show or hide via the `/statusline` picker. The footer renderer reads from
//! [`crate::settings::Settings::status_items`] (a `Vec<String>` of variant
//! IDs) and uses [`StatusItem`] to compose the right-hand chip cluster — so
//! toggling persists across sessions without changing the widget API.
//!
//! `Mode` and `Model` are always-on: they live in the footer's left status
//! line and aren't part of the configurable cluster, so the footer can
//! never become anonymous about which agent / which model is talking.

use std::fmt;

/// One configurable item that can appear on the footer/header.
///
/// `Mode` and `Model` are intentionally listed here — they appear in the
/// picker as locked rows so users can see them — but the footer renders
/// them unconditionally (they belong to the left status line, not the
/// auxiliary chip cluster).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusItem {
    /// Current TUI mode chip (agent / yolo / plan). Always-on.
    Mode,
    /// Current model id (e.g. `deepseek-v4-pro`). Always-on.
    Model,
    /// Active coherence intervention chip (refresh / verify / reset).
    Coherence,
    /// In-flight sub-agent count (`N agents`).
    Agents,
    /// `rsn <tokens>` — replayed `reasoning_content` size.
    ReasoningReplay,
    /// `cache <pct>%` — prompt-cache hit rate.
    CacheHitRate,
    /// `$X.YZ` — running session cost.
    SessionCost,
    /// Last prompt token count (`in <N>`).
    LastPromptTokens,
    /// Context-window utilisation percentage (`<N>%`).
    ContextPercent,
    /// Current git branch (`⎇ main`). Read directly from `.git/HEAD`.
    GitBranch,
    /// Workspace directory basename (`~/proj`).
    WorkspacePath,
    /// API rate-limit budget remaining. Stub today — surfaces nothing
    /// when the engine doesn't expose it.
    RateLimitRemaining,
}

impl StatusItem {
    /// Stable string id used in `settings.toml`. Round-trips through
    /// [`StatusItem::from_id`].
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            StatusItem::Mode => "mode",
            StatusItem::Model => "model",
            StatusItem::Coherence => "coherence",
            StatusItem::Agents => "agents",
            StatusItem::ReasoningReplay => "reasoning_replay",
            StatusItem::CacheHitRate => "cache_hit_rate",
            StatusItem::SessionCost => "session_cost",
            StatusItem::LastPromptTokens => "last_prompt_tokens",
            StatusItem::ContextPercent => "context_percent",
            StatusItem::GitBranch => "git_branch",
            StatusItem::WorkspacePath => "workspace_path",
            StatusItem::RateLimitRemaining => "rate_limit_remaining",
        }
    }

    /// Human-readable label shown in the picker.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            StatusItem::Mode => "Mode",
            StatusItem::Model => "Model",
            StatusItem::Coherence => "Coherence",
            StatusItem::Agents => "Sub-agents",
            StatusItem::ReasoningReplay => "Reasoning replay",
            StatusItem::CacheHitRate => "Cache hit rate",
            StatusItem::SessionCost => "Session cost",
            StatusItem::LastPromptTokens => "Last prompt tokens",
            StatusItem::ContextPercent => "Context %",
            StatusItem::GitBranch => "Git branch",
            StatusItem::WorkspacePath => "Workspace path",
            StatusItem::RateLimitRemaining => "Rate-limit remaining",
        }
    }

    /// One-line description shown alongside the label.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            StatusItem::Mode => "agent / yolo / plan (always on)",
            StatusItem::Model => "active model id (always on)",
            StatusItem::Coherence => "active intervention (refresh / verify)",
            StatusItem::Agents => "in-flight sub-agent count",
            StatusItem::ReasoningReplay => "replayed reasoning_content size",
            StatusItem::CacheHitRate => "prompt-cache hit percentage",
            StatusItem::SessionCost => "running USD cost for this session",
            StatusItem::LastPromptTokens => "tokens sent on the last prompt",
            StatusItem::ContextPercent => "context-window utilisation",
            StatusItem::GitBranch => "current git branch",
            StatusItem::WorkspacePath => "workspace directory",
            StatusItem::RateLimitRemaining => "remaining rate-limit budget",
        }
    }

    /// Whether the picker should treat this item as always-on (locked).
    /// Locked items can't be toggled off — `Mode` and `Model` carry
    /// information no terminal user wants to lose silently.
    #[must_use]
    pub fn always_on(self) -> bool {
        matches!(self, StatusItem::Mode | StatusItem::Model)
    }

    /// Parse a stable id back into a variant. Unknown ids return `None` so
    /// callers can drop forward-compat entries without crashing.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().iter().copied().find(|item| item.id() == id)
    }

    /// Every variant in canonical picker order.
    #[must_use]
    pub fn all() -> &'static [StatusItem] {
        &[
            StatusItem::Mode,
            StatusItem::Model,
            StatusItem::ContextPercent,
            StatusItem::SessionCost,
            StatusItem::LastPromptTokens,
            StatusItem::CacheHitRate,
            StatusItem::ReasoningReplay,
            StatusItem::Coherence,
            StatusItem::Agents,
            StatusItem::GitBranch,
            StatusItem::WorkspacePath,
            StatusItem::RateLimitRemaining,
        ]
    }

    /// Default selection — chosen so an upgrading user sees the same
    /// footer they had before the picker landed (mode/model + coherence,
    /// agents, reasoning replay, cache, cost). New items default off so
    /// the chip cluster doesn't grow without consent.
    #[must_use]
    pub fn defaults() -> Vec<StatusItem> {
        vec![
            StatusItem::Mode,
            StatusItem::Model,
            StatusItem::Coherence,
            StatusItem::Agents,
            StatusItem::ReasoningReplay,
            StatusItem::CacheHitRate,
            StatusItem::SessionCost,
        ]
    }

    /// Return the default selection encoded as the string ids that go in
    /// `settings.toml`.
    #[must_use]
    pub fn default_ids() -> Vec<String> {
        Self::defaults()
            .into_iter()
            .map(|item| item.id().to_string())
            .collect()
    }
}

impl fmt::Display for StatusItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Resolve a list of stable ids (as stored in `settings.toml`) into the
/// matching `StatusItem` variants, preserving order and silently dropping
/// unknown ids. `Mode` and `Model` are forced into the resolved set even
/// when missing from the persisted list — they're always-on.
#[must_use]
pub fn resolve_ids(ids: &[String]) -> Vec<StatusItem> {
    let mut resolved: Vec<StatusItem> = ids.iter().filter_map(|s| StatusItem::from_id(s)).collect();
    for locked in [StatusItem::Mode, StatusItem::Model] {
        if !resolved.contains(&locked) {
            resolved.insert(0, locked);
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_have_unique_ids() {
        let mut ids: Vec<&str> = StatusItem::all().iter().map(|i| i.id()).collect();
        ids.sort_unstable();
        let len = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), len, "duplicate StatusItem ids");
    }

    #[test]
    fn id_round_trips() {
        for item in StatusItem::all() {
            let id = item.id();
            assert_eq!(StatusItem::from_id(id), Some(*item), "id {id} round-trip");
        }
    }

    #[test]
    fn unknown_id_is_none() {
        assert_eq!(StatusItem::from_id("wat"), None);
        assert_eq!(StatusItem::from_id(""), None);
    }

    #[test]
    fn mode_and_model_are_always_on() {
        assert!(StatusItem::Mode.always_on());
        assert!(StatusItem::Model.always_on());
        for item in StatusItem::all() {
            if !matches!(item, StatusItem::Mode | StatusItem::Model) {
                assert!(!item.always_on(), "{} unexpectedly always-on", item.id());
            }
        }
    }

    #[test]
    fn defaults_include_locked_items() {
        let defaults = StatusItem::defaults();
        assert!(defaults.contains(&StatusItem::Mode));
        assert!(defaults.contains(&StatusItem::Model));
    }

    #[test]
    fn defaults_match_pre_picker_footer() {
        // The pre-picker footer surfaced these chips on the right; locking
        // the default to that set means an upgrade doesn't silently lose
        // any signal the user had before.
        let defaults: Vec<&str> = StatusItem::defaults().iter().map(|i| i.id()).collect();
        assert!(defaults.contains(&"mode"));
        assert!(defaults.contains(&"model"));
        assert!(defaults.contains(&"coherence"));
        assert!(defaults.contains(&"agents"));
        assert!(defaults.contains(&"reasoning_replay"));
        assert!(defaults.contains(&"cache_hit_rate"));
        assert!(defaults.contains(&"session_cost"));
    }

    #[test]
    fn resolve_drops_unknown_and_keeps_locked() {
        let ids = vec![
            "session_cost".to_string(),
            "wat".to_string(),
            "agents".to_string(),
        ];
        let resolved = resolve_ids(&ids);
        assert!(resolved.contains(&StatusItem::SessionCost));
        assert!(resolved.contains(&StatusItem::Agents));
        assert!(resolved.contains(&StatusItem::Mode));
        assert!(resolved.contains(&StatusItem::Model));
    }

    #[test]
    fn resolve_empty_yields_locked_only() {
        let resolved = resolve_ids(&[]);
        assert_eq!(resolved.len(), 2);
        assert!(resolved.contains(&StatusItem::Mode));
        assert!(resolved.contains(&StatusItem::Model));
    }

    #[test]
    fn default_ids_round_trip_through_resolve() {
        let ids = StatusItem::default_ids();
        let resolved = resolve_ids(&ids);
        assert_eq!(resolved, StatusItem::defaults());
    }

    #[test]
    fn settings_persistence_round_trips_status_items() {
        // Persistence path: pick a non-default selection, write it
        // through the same Settings codepath the picker uses, reload it,
        // and confirm `resolve_ids` recovers the same StatusItem set.
        // This locks in the contract `/statusline` depends on.
        use crate::settings::Settings;
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg_path = tmp.path().join("config.toml");
        // Marker file: Settings reads DEEPSEEK_CONFIG_PATH and places
        // settings.toml beside it.
        std::fs::write(&cfg_path, "").expect("write config marker");
        let prior = std::env::var("DEEPSEEK_CONFIG_PATH").ok();
        // SAFETY: this test serializes on the env var via the
        // sequential test infra; Rust requires an `unsafe` block for
        // env mutation since 1.85 because it isn't thread-safe across
        // OS APIs. The env is restored at the end of the test.
        unsafe {
            std::env::set_var("DEEPSEEK_CONFIG_PATH", &cfg_path);
        }

        let settings = Settings {
            status_items: vec![
                StatusItem::Mode.id().to_string(),
                StatusItem::Model.id().to_string(),
                StatusItem::ContextPercent.id().to_string(),
                StatusItem::SessionCost.id().to_string(),
            ],
            ..Settings::default()
        };
        settings.save().expect("save");

        let loaded = Settings::load().expect("load");
        let resolved = resolve_ids(&loaded.status_items);
        assert!(resolved.contains(&StatusItem::ContextPercent));
        assert!(resolved.contains(&StatusItem::SessionCost));
        assert!(resolved.contains(&StatusItem::Mode));
        assert!(resolved.contains(&StatusItem::Model));
        assert!(!resolved.contains(&StatusItem::GitBranch));

        // Restore env so other tests aren't disturbed.
        unsafe {
            match prior {
                Some(v) => std::env::set_var("DEEPSEEK_CONFIG_PATH", v),
                None => std::env::remove_var("DEEPSEEK_CONFIG_PATH"),
            }
        }
    }
}
