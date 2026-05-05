//! JSON-based locale loader for extended translations.
//!
//! Provides an additional translation source alongside the Rust-coded
//! functions in `localization.rs`. JSON files live in `crates/tui/locales/`
//! and are compiled into the binary via `include_str!()`.
//!
//! The `tr()` function in `localization.rs` checks this store first; if
//! a key is not found, it falls back to the Rust-coded function.
//!
//! JSON key format: `"section.name"` (e.g. `"config_sections.model"`,
//! `"errors.invalid_locale"`, `"settings_descriptions.auto_compact"`).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

use super::localization::Locale;

/// Global JSON locale store: Locale → flat key → string.
static JSON_LOCALE_STORE: OnceLock<HashMap<Locale, HashMap<String, &'static str>>> =
    OnceLock::new();

/// Initialize the JSON locale store.
///
/// Called once at startup. Compiles JSON translation files into the binary
/// and leaks them into `&'static str` references for the lifetime of the
/// process. This is fine because the files are small and loaded once.
fn init_json_store() -> HashMap<Locale, HashMap<String, &'static str>> {
    let mut store = HashMap::new();

    let locale_files: &[(Locale, &str)] = &[
        (Locale::ZhHans, include_str!("../locales/zh-Hans.json")),
        (Locale::ZhHant, include_str!("../locales/zh-Hant.json")),
    ];

    for (locale, json_str) in locale_files {
        let parsed: Value = serde_json::from_str(json_str)
            .unwrap_or_else(|_| panic!("Failed to parse JSON locale for {:?}", locale));

        let mut flat_map: HashMap<String, &'static str> = HashMap::new();

        // Flatten nested sections into dot-separated keys
        flatten_json(&parsed, String::new(), &mut flat_map);

        store.insert(*locale, flat_map);
    }

    store
}

/// Recursively flatten a JSON object into dot-separated keys.
///
/// Example:
///   { "config_sections": { "model": "模型" } }
///   → "config_sections.model" → "模型"
fn flatten_json(value: &Value, prefix: String, output: &mut HashMap<String, &'static str>) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                let new_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                flatten_json(val, new_key, output);
            }
        }
        Value::String(s) => {
            // Leak the string into a &'static str (one-time allocation, fine for startup)
            let leaked: &'static str = Box::leak(s.clone().into_boxed_str());
            output.insert(prefix, leaked);
        }
        _ => {
            // Skip non-string values (version, locale_tag, display_name)
        }
    }
}

/// Ensure the JSON locale store is initialized.
fn ensure_store() -> &'static HashMap<Locale, HashMap<String, &'static str>> {
    JSON_LOCALE_STORE.get_or_init(init_json_store)
}

/// Look up a translation from JSON files.
///
/// Returns `Some(&'static str)` if the key exists for the given locale,
/// or `None` if not found.
///
/// Keys use dot notation, e.g. `"config_sections.model"` for config section
/// labels, `"errors.invalid_locale"` for error messages.
pub fn tr_json(locale: Locale, key: &str) -> Option<&'static str> {
    let store = ensure_store();
    store
        .get(&locale)
        .and_then(|map| map.get(key).copied())
        .or_else(|| {
            // Fall back to Simplified Chinese if Traditional Chinese is missing a key
            if locale == Locale::ZhHant {
                store
                    .get(&Locale::ZhHans)
                    .and_then(|map| map.get(key).copied())
            } else {
                None
            }
        })
}

/// Get a settings description from JSON, falling back to English.
pub fn tr_settings_desc(locale: Locale, key: &str) -> Option<&'static str> {
    tr_json(locale, &format!("settings_descriptions.{}", key))
}

/// Get an error message from JSON, falling back to English.
#[allow(dead_code)]
pub fn tr_error(locale: Locale, key: &str) -> Option<&'static str> {
    tr_json(locale, &format!("errors.{}", key))
}

/// Get a UI label from JSON.
pub fn tr_ui_label(locale: Locale, key: &str) -> Option<&'static str> {
    tr_json(locale, &format!("ui_labels.{}", key))
}

/// Get a config section label from JSON.
pub fn tr_config_section(locale: Locale, section: &str) -> Option<&'static str> {
    tr_json(locale, &format!("config_sections.{}", section))
}

/// Get a config key display name from JSON.
pub fn tr_config_key(locale: Locale, key: &str) -> Option<&'static str> {
    tr_json(locale, &format!("config_keys.{}", key))
}

/// Get a tool name translation from JSON.
///
/// Maps internal tool names like `edit_file` to localized display names.
/// Falls back to the raw tool name if no translation exists.
pub fn tr_tool_name(locale: Locale, tool_name: &str) -> String {
    tr_json(locale, &format!("tool_names.{}", tool_name))
        .map(str::to_string)
        .unwrap_or_else(|| tool_name.to_string())
}

/// Get an impact summary translation from JSON.
///
/// Keys like `"safe"`, `"reads"`, `"file_write"`, etc. return the
/// localized impact description. Falls back to the key name.
#[allow(dead_code)]
pub fn tr_impact_summary(locale: Locale, key: &str) -> Option<&'static str> {
    tr_json(locale, &format!("impact_summaries.{}", key))
}

/// Get an elevation label translation from JSON.
///
/// Keys like `"tool"`, `"cmd"`, `"reason"`, etc. return the localized
/// elevation dialog label. Falls back to the key name.
pub fn tr_elevation_label(locale: Locale, key: &str) -> Option<&'static str> {
    tr_json(locale, &format!("elevation_labels.{}", key))
}

/// Get a shell control label translation from JSON.
///
/// Keys like `"title"`, `"body"`, `"option_bg"`, etc. return the
/// localized shell control dialog label.
#[allow(dead_code)]
pub fn tr_shell_control(locale: Locale, key: &str) -> Option<&'static str> {
    tr_json(locale, &format!("shell_control.{}", key))
}
