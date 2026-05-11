#![allow(clippy::items_after_test_module)]

//! Debug commands: tokens, cost, system, context, undo, retry

use std::time::Instant;

use super::CommandResult;
use crate::client::{
    CacheWarmupKey, PromptInspection, PromptLayerInspection, inspect_prompt_for_request,
};
use crate::compaction::estimate_input_tokens_conservative;
use crate::localization::{Locale, MessageId, tr};
use crate::models::{ContentBlock, MessageRequest, SystemPrompt, context_window_for_model};
use crate::tui::app::{App, AppAction, TurnCacheRecord};
use crate::tui::history::HistoryCell;

fn token_count(value: Option<u32>, locale: Locale) -> String {
    value.map_or_else(
        || tr(locale, MessageId::CmdTokensNotReported).to_string(),
        |tokens| tokens.to_string(),
    )
}

fn active_context_summary(app: &App, locale: Locale) -> String {
    let estimated =
        estimate_input_tokens_conservative(&app.api_messages, app.system_prompt.as_ref());
    match context_window_for_model(&app.model) {
        Some(window) => {
            let used = estimated.min(window as usize);
            let percent = (used as f64 / f64::from(window) * 100.0).clamp(0.0, 100.0);
            tr(locale, MessageId::CmdTokensContextWithWindow)
                .replace("{used}", &used.to_string())
                .replace("{window}", &window.to_string())
                .replace("{percent}", &format!("{percent:.1}"))
        }
        None => tr(locale, MessageId::CmdTokensContextUnknownWindow)
            .replace("{estimated}", &estimated.to_string()),
    }
}

fn cache_summary(app: &App, locale: Locale) -> String {
    match (
        app.session.last_prompt_cache_hit_tokens,
        app.session.last_prompt_cache_miss_tokens,
    ) {
        (Some(hit), Some(miss)) => tr(locale, MessageId::CmdTokensCacheBoth)
            .replace("{hit}", &hit.to_string())
            .replace("{miss}", &miss.to_string()),
        (Some(hit), None) => {
            tr(locale, MessageId::CmdTokensCacheHitOnly).replace("{hit}", &hit.to_string())
        }
        (None, Some(miss)) => {
            tr(locale, MessageId::CmdTokensCacheMissOnly).replace("{miss}", &miss.to_string())
        }
        (None, None) => tr(locale, MessageId::CmdTokensNotReported).to_string(),
    }
}

/// Show token usage for session
pub fn tokens(app: &mut App) -> CommandResult {
    let locale = app.ui_locale;
    let message_count = app.api_messages.len();
    let chat_count = app.history.len();

    let report = tr(locale, MessageId::CmdTokensReport)
        .replace("{active}", &active_context_summary(app, locale))
        .replace(
            "{input}",
            &token_count(app.session.last_prompt_tokens, locale),
        )
        .replace(
            "{output}",
            &token_count(app.session.last_completion_tokens, locale),
        )
        .replace("{cache}", &cache_summary(app, locale))
        .replace("{total}", &app.session.total_tokens.to_string())
        .replace(
            "{cost}",
            &app.format_cost_amount_precise(app.session_cost_for_currency(app.cost_currency)),
        )
        .replace("{api_messages}", &message_count.to_string())
        .replace("{chat_messages}", &chat_count.to_string())
        .replace("{model}", &app.model);
    CommandResult::message(report)
}

/// Show session cost breakdown
pub fn cost(app: &mut App) -> CommandResult {
    let report = tr(app.ui_locale, MessageId::CmdCostReport).replace(
        "{cost}",
        &app.format_cost_amount_precise(app.session_cost_for_currency(app.cost_currency)),
    );
    CommandResult::message(report)
}

/// Show current system prompt
pub fn system_prompt(app: &mut App) -> CommandResult {
    let prompt_text = match &app.system_prompt {
        Some(SystemPrompt::Text(text)) => text.clone(),
        Some(SystemPrompt::Blocks(blocks)) => blocks
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n"),
        None => "(no system prompt)".to_string(),
    };

    // Truncate if too long
    let display = if prompt_text.len() > 500 {
        // Find a valid UTF-8 char boundary at or before byte 500
        let truncate_at = prompt_text
            .char_indices()
            .take_while(|(i, _)| *i <= 500)
            .last()
            .map_or(0, |(i, _)| i);
        format!(
            "{}...\n\n(truncated, {} chars total)",
            &prompt_text[..truncate_at],
            prompt_text.len()
        )
    } else {
        prompt_text
    };

    CommandResult::message(format!(
        "System Prompt ({} mode):\n─────────────────────────────\n{}",
        app.mode.label(),
        display
    ))
}

/// Show context window usage
pub fn context(_app: &mut App) -> CommandResult {
    CommandResult::action(AppAction::OpenContextInspector)
}

/// Show per-turn DeepSeek prefix-cache telemetry for the last N turns (#263).
///
/// `arg` is parsed as a count override (default 10, capped at the ring size).
/// Renders a fixed-width table the user can paste into a bug report.
pub fn cache(app: &mut App, arg: Option<&str>) -> CommandResult {
    let arg = arg.map(str::trim).filter(|s| !s.is_empty());
    if let Some(inspect_arg) = arg.and_then(|a| a.strip_prefix("inspect")) {
        let flags = inspect_arg.trim();
        let verbose = flags.contains("--verbose");
        let json_mode = flags.contains("--json");
        return CommandResult::message(format_cache_inspect(app, verbose, json_mode));
    }
    if matches!(arg, Some("warmup")) {
        return CommandResult::action(AppAction::CacheWarmup);
    }

    let want = arg.and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);
    let cap = app.session.turn_cache_history.len();
    let count = want
        .min(cap)
        .min(crate::tui::app::App::TURN_CACHE_HISTORY_CAP);

    if cap == 0 {
        return CommandResult::message(tr(app.ui_locale, MessageId::CmdCacheNoData));
    }

    CommandResult::message(format_cache_history(app, count, app.ui_locale))
}

fn format_cache_inspect(app: &mut App, verbose: bool, json_mode: bool) -> String {
    let reasoning_effort = if app.reasoning_effort == crate::tui::app::ReasoningEffort::Auto {
        app.last_effective_reasoning_effort
            .and_then(crate::tui::app::ReasoningEffort::api_value)
            .map(str::to_string)
    } else {
        app.reasoning_effort.api_value().map(str::to_string)
    };
    let request = MessageRequest {
        model: app.model.clone(),
        messages: app.api_messages.clone(),
        max_tokens: 0,
        system: app.system_prompt.clone(),
        tools: app.session.last_tool_catalog.clone(),
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort,
        stream: Some(true),
        temperature: None,
        top_p: None,
    };
    let inspection = inspect_prompt_for_request(&request);

    let previous = app.session.last_cache_inspection.as_ref();

    // Compute warmup key from the current inspection.
    let provider_str = format!("{:?}", app.api_provider);
    let base_url = app.session.last_base_url.clone().unwrap_or_default();
    let current_warmup_key =
        CacheWarmupKey::from_inspection(&provider_str, &app.model, &base_url, &inspection);

    if json_mode {
        let json = serde_json::to_string_pretty(&inspection)
            .unwrap_or_else(|_| "{\"error\": \"serialization failed\"}".to_string());
        app.session.last_cache_inspection = Some(inspection);
        app.session.last_warmup_key = Some(current_warmup_key);
        return json;
    }

    let mut out = String::new();
    out.push_str("Cache Inspect\n");
    out.push_str("Full prompt text is not printed. Hashes are SHA-256 of each rendered layer.\n");
    out.push_str(&format!(
        "Base static prefix hash: {}\n",
        inspection.base_static_prefix_hash
    ));
    out.push_str(&format!(
        "Full request prefix hash: {}\n",
        inspection.full_request_prefix_hash
    ));
    out.push_str(&format!(
        "Tool catalog hash: {}\n",
        if inspection.tool_catalog_hash.is_empty() {
            "(no tools registered)".to_string()
        } else {
            inspection.tool_catalog_hash.clone()
        }
    ));
    out.push_str(&format!(
        "Stable prefix hash (excl. tools): {}\n",
        inspection.stable_prefix_hash
    ));
    out.push_str(&format_static_prefix_status(previous, &inspection));
    out.push_str(&format_first_divergence(previous, &inspection));
    out.push('\n');

    // Estimate total tokens across all layers.
    let total_tokens: usize = inspection.layers.iter().map(|l| l.token_estimate).sum();
    out.push_str(&format!("Estimated total tokens: ~{total_tokens}\n\n"));

    for layer in &inspection.layers {
        let mut line = format!(
            "{}: {}, chars={}, bytes={}, ~{}tok, hash={}\n",
            layer.name,
            layer.stability.label(),
            layer.char_len,
            layer.byte_len,
            layer.token_estimate,
            layer.sha256
        );
        if let Some(tool_result) = &layer.tool_result {
            let trimmed = line.trim_end_matches('\n').to_string();
            line = format!(
                "{trimmed}, orig_chars={}, sent_chars={}, truncated={}, dedup={}\n",
                tool_result.original_chars,
                tool_result.sent_chars,
                tool_result.truncated,
                tool_result.deduplicated
            );
        }
        if let Some(turn_meta) = &layer.turn_meta {
            let trimmed = line.trim_end_matches('\n').to_string();
            line = format!(
                "{trimmed}, meta_orig={}, meta_sent={}, meta_dedup={}, meta_hash={}\n",
                turn_meta.original_chars,
                turn_meta.sent_chars,
                turn_meta.deduplicated,
                turn_meta.sha256
            );
        }
        out.push_str(&line);
    }

    // Verbose mode: show layer-by-layer diff with previous inspection.
    if verbose {
        out.push_str("\n── Verbose diff ──\n");
        if let Some(prev) = previous {
            out.push_str(&format_verbose_diff(prev, &inspection));
        } else {
            out.push_str("No previous inspection to compare against.\n");
        }
    }

    // Warmup status.
    out.push('\n');
    out.push_str(&format_warmup_status(
        app.session.last_warmup_key.as_ref(),
        &current_warmup_key,
    ));

    app.session.last_cache_inspection = Some(inspection);
    app.session.last_warmup_key = Some(current_warmup_key);
    out
}

fn format_warmup_status(last_warmup: Option<&CacheWarmupKey>, current: &CacheWarmupKey) -> String {
    match last_warmup {
        None => format!(
            "Warmup status: no previous warmup (current key: {}…)\n",
            current.hash_short()
        ),
        Some(prev) if prev == current => format!(
            "Warmup status: valid (key {}… matches)\n",
            current.hash_short()
        ),
        Some(prev) => {
            let mut reasons = Vec::new();
            if prev.provider != current.provider {
                reasons.push(format!("provider {}→{}", prev.provider, current.provider));
            }
            if prev.model != current.model {
                reasons.push(format!("model {}→{}", prev.model, current.model));
            }
            if prev.base_url != current.base_url {
                reasons.push(format!("base_url {}→{}", prev.base_url, current.base_url));
            }
            if prev.static_prefix_hash != current.static_prefix_hash {
                reasons.push("static prefix changed".to_string());
            }
            if prev.tool_catalog_hash != current.tool_catalog_hash {
                reasons.push("tool catalog changed".to_string());
            }
            format!(
                "Warmup status: INVALID (key {}… → {}…, {})\n",
                prev.hash_short(),
                current.hash_short(),
                reasons.join(", ")
            )
        }
    }
}

fn format_static_prefix_status(
    previous: Option<&PromptInspection>,
    current: &PromptInspection,
) -> String {
    let Some(previous) = previous else {
        return "Static base prefix stability: no previous request\n".to_string();
    };
    if previous.base_static_prefix_hash == current.base_static_prefix_hash {
        return "Static base prefix stability: OK\n".to_string();
    }

    let changed = changed_static_layers(previous, current);
    if changed.is_empty() {
        "Static base prefix stability: WARNING (base hash changed)\n".to_string()
    } else {
        format!(
            "Static base prefix stability: WARNING changed layers: {}\n",
            changed.join(", ")
        )
    }
}

fn format_first_divergence(
    previous: Option<&PromptInspection>,
    current: &PromptInspection,
) -> String {
    let Some(previous) = previous else {
        return "First divergence from previous request: unavailable\n".to_string();
    };
    let max_len = previous.layers.len().max(current.layers.len());
    for index in 0..max_len {
        match (previous.layers.get(index), current.layers.get(index)) {
            (Some(prev), Some(curr)) if prev.name == curr.name && prev.sha256 == curr.sha256 => {}
            (Some(prev), Some(curr)) if prev.name == curr.name => {
                let cause = infer_divergence_cause(&curr.name, prev, curr);
                return format!(
                    "First divergence from previous request: {}{}\n",
                    curr.name, cause
                );
            }
            (Some(_), Some(curr)) => {
                return format!("First divergence from previous request: {}\n", curr.name);
            }
            (None, Some(curr)) => {
                return format!("First divergence from previous request: {}\n", curr.name);
            }
            (Some(prev), None) => {
                return format!(
                    "First divergence from previous request: {} removed\n",
                    prev.name
                );
            }
            (None, None) => break,
        }
    }
    "First divergence from previous request: none\n".to_string()
}

/// Infer a human-readable cause for why a layer diverged from the previous request.
fn infer_divergence_cause(
    layer_name: &str,
    prev: &PromptLayerInspection,
    curr: &PromptLayerInspection,
) -> String {
    // Size-based hints for common divergence patterns.
    let char_delta = curr.char_len as i64 - prev.char_len as i64;

    let hint = match layer_name {
        "Tool schema" => {
            if char_delta > 0 {
                " (possible cause: tool added or schema expanded)"
            } else if char_delta < 0 {
                " (possible cause: tool removed or schema shrunk)"
            } else {
                " (possible cause: tool order or schema content changed)"
            }
        }
        "Project context pack" => {
            if char_delta.abs() > 100 {
                " (possible cause: files added/removed from workspace)"
            } else {
                " (possible cause: file content or structure changed)"
            }
        }
        "Project context" => " (possible cause: CLAUDE.md or AGENTS.md changed)",
        "Skills" => {
            if char_delta > 0 {
                " (possible cause: new skill installed)"
            } else if char_delta < 0 {
                " (possible cause: skill removed)"
            } else {
                " (possible cause: skill content changed)"
            }
        }
        "Environment" => " (possible cause: environment info updated)",
        "Configured instructions" => " (possible cause: instructions file changed)",
        "Global system prefix" => " (possible cause: mode prompt or base policy changed)",
        "User memory" => " (possible cause: memory edited via /memory or # quick-add)",
        "Current session goal" => " (possible cause: goal updated)",
        "Previous session handoff" => " (possible cause: compaction rewrote handoff)",
        "Context management" | "Compact template" => " (unexpected: compile-time constant changed)",
        name if name.starts_with("Message #") => {
            if char_delta.abs() > 500 {
                " (possible cause: large tool result or message change)"
            } else {
                " (possible cause: message content changed)"
            }
        }
        "User task" => " (expected: new user message each turn)",
        _ => "",
    };

    if hint.is_empty() && curr.tool_result.as_ref().is_some_and(|t| t.deduplicated) {
        " (tool result deduplicated)".to_string()
    } else if hint.is_empty() && curr.turn_meta.as_ref().is_some_and(|t| t.deduplicated) {
        " (turn meta deduplicated)".to_string()
    } else {
        hint.to_string()
    }
}

/// Compare layer metadata between two inspections and produce a human-readable diff.
/// Does NOT print full prompt text — only names, hashes, sizes, and stability labels.
fn format_verbose_diff(prev: &PromptInspection, curr: &PromptInspection) -> String {
    let mut out = String::new();
    let max_len = prev.layers.len().max(curr.layers.len());

    for index in 0..max_len {
        match (prev.layers.get(index), curr.layers.get(index)) {
            (Some(p), Some(c)) if p == c => {
                out.push_str(&format!("  [{}] {} — unchanged\n", index, c.name));
            }
            (Some(p), Some(c)) => {
                out.push_str(&format!("  [{}] {} — CHANGED\n", index, c.name));
                if p.name != c.name {
                    out.push_str(&format!("    name:     {} → {}\n", p.name, c.name));
                }
                if p.sha256 != c.sha256 {
                    out.push_str(&format!(
                        "    hash:     {}…{} → {}…{}\n",
                        &p.sha256[..8],
                        &p.sha256[p.sha256.len() - 8..],
                        &c.sha256[..8],
                        &c.sha256[c.sha256.len() - 8..],
                    ));
                }
                if p.stability != c.stability {
                    out.push_str(&format!(
                        "    stability: {} → {}\n",
                        p.stability.label(),
                        c.stability.label()
                    ));
                }
                if p.char_len != c.char_len {
                    out.push_str(&format!(
                        "    chars:    {} → {} ({:+})\n",
                        p.char_len,
                        c.char_len,
                        c.char_len as i64 - p.char_len as i64
                    ));
                }
                if p.byte_len != c.byte_len {
                    out.push_str(&format!(
                        "    bytes:    {} → {} ({:+})\n",
                        p.byte_len,
                        c.byte_len,
                        c.byte_len as i64 - p.byte_len as i64
                    ));
                }
                if p.token_estimate != c.token_estimate {
                    out.push_str(&format!(
                        "    tokens:   ~{} → ~{} ({:+})\n",
                        p.token_estimate,
                        c.token_estimate,
                        c.token_estimate as i64 - p.token_estimate as i64
                    ));
                }
                let cause = infer_divergence_cause(&c.name, p, c);
                if !cause.is_empty() {
                    out.push_str(&format!("    cause:{}\n", cause));
                }
            }
            (Some(p), None) => {
                out.push_str(&format!(
                    "  [{}] {} — REMOVED (was {} chars, {} bytes, ~{}tok)\n",
                    index, p.name, p.char_len, p.byte_len, p.token_estimate
                ));
            }
            (None, Some(c)) => {
                out.push_str(&format!(
                    "  [{}] {} — ADDED ({} chars, {} bytes, ~{}tok, {})\n",
                    index,
                    c.name,
                    c.char_len,
                    c.byte_len,
                    c.token_estimate,
                    c.stability.label()
                ));
            }
            (None, None) => break,
        }
    }
    out
}

fn changed_static_layers(previous: &PromptInspection, current: &PromptInspection) -> Vec<String> {
    current
        .layers
        .iter()
        .filter(|layer| layer.stability.label() == "static")
        .filter(|layer| {
            previous
                .layers
                .iter()
                .find(|previous_layer| previous_layer.name == layer.name)
                .is_none_or(|previous_layer| previous_layer.sha256 != layer.sha256)
        })
        .map(|layer| layer.name.clone())
        .collect()
}

fn format_cache_history(app: &App, count: usize, locale: Locale) -> String {
    let total = app.session.turn_cache_history.len();
    let start = total.saturating_sub(count);
    let rows: Vec<&TurnCacheRecord> = app.session.turn_cache_history.iter().skip(start).collect();

    let mut totals_input: u64 = 0;
    let mut totals_hit: u64 = 0;
    let mut totals_miss: u64 = 0;
    let mut header = tr(locale, MessageId::CmdCacheHeader)
        .replace("{count}", &rows.len().to_string())
        .replace("{total}", &total.to_string())
        .replace("{model}", &app.model);
    header.push_str(&"─".repeat(76));
    header.push('\n');
    header.push_str("turn   in    out   hit   miss   replay   ratio   age\n");
    header.push_str(&"─".repeat(76));
    header.push('\n');

    let now = Instant::now();
    let mut body = String::new();
    let absolute_start = total.saturating_sub(rows.len());
    for (i, rec) in rows.iter().enumerate() {
        let turn_index = absolute_start + i + 1;
        totals_input += u64::from(rec.input_tokens);

        let replay_cell = rec
            .reasoning_replay_tokens
            .map_or_else(|| "—".to_string(), |t| t.to_string());
        let age = humanize_age(now.saturating_duration_since(rec.recorded_at));

        // No cache telemetry → render `—` everywhere and don't pollute totals
        // with inferred zeros. Some providers (and some routes inside DeepSeek)
        // skip the cache fields; including a synthesized 0/N for those turns
        // would make every aggregate ratio look broken.
        let Some(hit) = rec.cache_hit_tokens else {
            body.push_str(&format!(
                "{turn:>4}  {input:>5}  {output:>5}  {hit:>5}  {miss:>5}  {replay:>6}   {ratio:>6}   {age}\n",
                turn = turn_index,
                input = rec.input_tokens,
                output = rec.output_tokens,
                hit = "—",
                miss = "—",
                replay = replay_cell,
                ratio = "—",
                age = age,
            ));
            continue;
        };

        let miss_reported = rec.cache_miss_tokens;
        let miss = miss_reported.unwrap_or_else(|| rec.input_tokens.saturating_sub(hit));
        let accounted = u64::from(hit) + u64::from(miss);
        let ratio = if accounted == 0 {
            "    —".to_string()
        } else {
            format!("{:>5.1}%", 100.0 * f64::from(hit) / accounted as f64)
        };
        totals_hit += u64::from(hit);
        totals_miss += u64::from(miss);

        let miss_cell = match miss_reported {
            Some(_) => format!("{miss}"),
            None => format!("{miss}*"),
        };

        body.push_str(&format!(
            "{turn:>4}  {input:>5}  {output:>5}  {hit:>5}  {miss:>5}  {replay:>6}   {ratio}   {age}\n",
            turn = turn_index,
            input = rec.input_tokens,
            output = rec.output_tokens,
            hit = hit,
            miss = miss_cell,
            replay = replay_cell,
            ratio = ratio,
            age = age,
        ));
    }

    let totals_accounted = totals_hit + totals_miss;
    let avg_ratio = if totals_accounted == 0 {
        "—".to_string()
    } else {
        format!(
            "{:.1}%",
            100.0 * totals_hit as f64 / totals_accounted as f64
        )
    };

    let mut footer = String::new();
    footer.push_str(&"─".repeat(76));
    footer.push('\n');
    footer.push_str(
        &tr(locale, MessageId::CmdCacheTotals)
            .replace("{sum_in}", &totals_input.to_string())
            .replace("{sum_hit}", &totals_hit.to_string())
            .replace("{sum_miss}", &totals_miss.to_string())
            .replace("{avg}", &avg_ratio),
    );
    footer.push_str(tr(locale, MessageId::CmdCacheFootnote));
    footer.push_str(tr(locale, MessageId::CmdCacheAdvice));

    format!("{header}{body}{footer}")
}

fn humanize_age(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::models::{ContentBlock, Message, SystemBlock, Tool};
    use crate::tui::app::{App, TuiOptions};
    use crate::tui::history::{GenericToolCell, ToolCell, ToolStatus};
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: PathBuf::from("/tmp/test-workspace"),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("/tmp/test-skills"),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());
        app.ui_locale = crate::localization::Locale::En;
        app.api_provider = crate::config::ApiProvider::Deepseek;
        app
    }

    fn test_tool(name: &str) -> Tool {
        Tool {
            tool_type: Some("function".to_string()),
            name: name.to_string(),
            description: format!("Test tool {name}"),
            input_schema: serde_json::json!({"type": "object"}),
            allowed_callers: Some(vec!["direct".to_string()]),
            defer_loading: None,
            input_examples: None,
            strict: None,
            cache_control: None,
        }
    }

    #[test]
    fn test_tokens_shows_usage_info() {
        let mut app = create_test_app();
        app.session.total_tokens = 1234;
        app.session.session_cost = 0.05;
        app.session.last_prompt_tokens = Some(100);
        app.session.last_completion_tokens = Some(25);
        app.session.last_prompt_cache_hit_tokens = Some(70);
        app.session.last_prompt_cache_miss_tokens = Some(30);
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "test".to_string(),
                cache_control: None,
            }],
        });
        app.history.push(HistoryCell::User {
            content: "test".to_string(),
        });

        let result = tokens(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Token Usage"));
        assert!(msg.contains("Active context:"));
        assert!(msg.contains("Last API input:"));
        assert!(msg.contains("Last API output:"));
        assert!(msg.contains("Cache hit/miss:"));
        assert!(msg.contains("70 hit / 30 miss"));
        assert!(msg.contains("Cumulative tokens:"));
        assert!(msg.contains("Approx session cost:"));
        assert!(msg.contains("API messages:"));
        assert!(msg.contains("Chat messages:"));
        assert!(msg.contains("Model:"));
    }

    #[test]
    fn test_cost_shows_spending_info() {
        let mut app = create_test_app();
        app.session.session_cost = 0.1234;
        let result = cost(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Session Cost"));
        assert!(msg.contains("Approx total spent:"));
        assert!(msg.contains("approximate"));
        assert!(msg.contains("$0.1234"));
    }

    #[test]
    fn test_system_prompt_displays_text() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Test system prompt".to_string()));
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("System Prompt"));
        assert!(msg.contains("Test system prompt"));
    }

    #[test]
    fn test_system_prompt_displays_blocks() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Blocks(vec![
            SystemBlock {
                block_type: "text".to_string(),
                text: "Block 1".to_string(),
                cache_control: None,
            },
            SystemBlock {
                block_type: "text".to_string(),
                text: "Block 2".to_string(),
                cache_control: None,
            },
        ]));
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("System Prompt"));
        assert!(msg.contains("Block 1"));
        assert!(msg.contains("Block 2"));
    }

    #[test]
    fn test_system_prompt_none() {
        let mut app = create_test_app();
        app.system_prompt = None;
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("(no system prompt)"));
    }

    #[test]
    fn test_system_prompt_truncates_long_text() {
        let mut app = create_test_app();
        let long_text = "x".repeat(600);
        app.system_prompt = Some(SystemPrompt::Text(long_text));
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("..."));
        assert!(msg.contains("chars total"));
    }

    #[test]
    fn cache_command_reports_no_data_before_first_turn() {
        let mut app = create_test_app();
        let result = cache(&mut app, None);
        let msg = result.message.expect("cache produces a message");
        assert!(msg.contains("no turns recorded yet"), "got: {msg}");
    }

    #[test]
    fn cache_inspect_reports_hashes_without_prompt_text() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n<project_instructions source=\"AGENTS.md\">\nSECRET_PROJECT_RULE\n</project_instructions>"
                .to_string(),
        ));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "SECRET_USER_TASK".to_string(),
                cache_control: None,
            }],
        });

        let result = cache(&mut app, Some("inspect"));
        let msg = result.message.expect("inspect output");

        assert!(msg.contains("Cache Inspect"));
        assert!(msg.contains("Base static prefix hash:"));
        assert!(msg.contains("Full request prefix hash:"));
        assert!(msg.contains("Static base prefix stability: no previous request"));
        assert!(msg.contains("First divergence from previous request: unavailable"));
        assert!(msg.contains("Global system prefix: static"));
        assert!(msg.contains("Project context: static"));
        assert!(msg.contains("User task: dynamic"));
        assert!(!msg.contains("SECRET_PROJECT_RULE"));
        assert!(!msg.contains("SECRET_USER_TASK"));
    }

    #[test]
    fn cache_inspect_reports_divergence_from_previous_request() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Environment\n\n- shell: powershell".to_string(),
        ));
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![crate::models::ContentBlock::Text {
                text: "Prior answer".to_string(),
                cache_control: None,
            }],
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![crate::models::ContentBlock::Text {
                text: "First task".to_string(),
                cache_control: None,
            }],
        });

        let first = cache(&mut app, Some("inspect"))
            .message
            .expect("first inspect output");
        assert!(first.contains("Static base prefix stability: no previous request"));

        if let Some(last) = app.api_messages.last_mut()
            && let Some(crate::models::ContentBlock::Text { text, .. }) = last.content.first_mut()
        {
            *text = "Second task".to_string();
        }

        let second = cache(&mut app, Some("inspect"))
            .message
            .expect("second inspect output");
        assert!(second.contains("Static base prefix stability: OK"));
        assert!(second.contains("First divergence from previous request: User task"));
        assert!(second.contains("Message #1 assistant: history"));
    }

    #[test]
    fn cache_inspect_displays_tool_result_budget_metadata() {
        let mut app = create_test_app();
        let long_output = format!("{}{}", "A".repeat(7_000), "Z".repeat(7_000));
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tool-1".to_string(),
                name: "shell_command".to_string(),
                input: serde_json::json!({"command": "cargo test"}),
                caller: None,
            }],
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: long_output.clone(),
                is_error: None,
                content_blocks: None,
            }],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tool-2".to_string(),
                name: "shell_command".to_string(),
                input: serde_json::json!({"command": "cargo test"}),
                caller: None,
            }],
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-2".to_string(),
                content: long_output,
                is_error: None,
                content_blocks: None,
            }],
        });

        let result = cache(&mut app, Some("inspect"));
        let msg = result.message.expect("inspect output");

        assert!(msg.contains("orig_chars=14000"), "got: {msg}");
        assert!(msg.contains("truncated=true"), "got: {msg}");
        assert!(msg.contains("dedup=false"), "got: {msg}");
        assert!(msg.contains("dedup=true"), "got: {msg}");
    }

    #[test]
    fn cache_inspect_displays_turn_meta_dedup_metadata() {
        let mut app = create_test_app();
        let turn_meta = format!(
            "<turn_meta>\nCurrent local date: 2026-05-09\n{}\n</turn_meta>",
            "Working set: src/lib.rs\n".repeat(20)
        );
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: turn_meta.clone(),
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: "first task".to_string(),
                    cache_control: None,
                },
            ],
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: turn_meta,
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: "second task".to_string(),
                    cache_control: None,
                },
            ],
        });

        let result = cache(&mut app, Some("inspect"));
        let msg = result.message.expect("inspect output");

        assert!(msg.contains("meta_orig="), "got: {msg}");
        assert!(msg.contains("meta_sent="), "got: {msg}");
        assert!(msg.contains("meta_dedup=false"), "got: {msg}");
        assert!(msg.contains("meta_dedup=true"), "got: {msg}");
        assert!(msg.contains("meta_hash="), "got: {msg}");
        assert!(!msg.contains("Working set: src/lib.rs"), "got: {msg}");
    }

    #[test]
    fn cache_command_renders_recorded_turns_with_ratio() {
        let mut app = create_test_app();
        let now = Instant::now();
        // Three turns: 75% hit, 50% hit, miss-only (provider didn't report hit).
        app.push_turn_cache_record(TurnCacheRecord {
            input_tokens: 4_000,
            output_tokens: 200,
            cache_hit_tokens: Some(3_000),
            cache_miss_tokens: Some(1_000),
            reasoning_replay_tokens: None,
            recorded_at: now,
        });
        app.push_turn_cache_record(TurnCacheRecord {
            input_tokens: 6_000,
            output_tokens: 250,
            cache_hit_tokens: Some(3_000),
            cache_miss_tokens: Some(3_000),
            reasoning_replay_tokens: Some(150),
            recorded_at: now,
        });
        // Turn 3: hit reported but provider didn't report miss separately —
        // infer miss = input − hit and mark with `*`.
        app.push_turn_cache_record(TurnCacheRecord {
            input_tokens: 5_000,
            output_tokens: 100,
            cache_hit_tokens: Some(2_500),
            cache_miss_tokens: None,
            reasoning_replay_tokens: None,
            recorded_at: now,
        });
        // Turn 4: no telemetry at all — must not pollute aggregate ratios.
        app.push_turn_cache_record(TurnCacheRecord {
            input_tokens: 1_000,
            output_tokens: 50,
            cache_hit_tokens: None,
            cache_miss_tokens: None,
            reasoning_replay_tokens: None,
            recorded_at: now,
        });

        let result = cache(&mut app, None);
        let msg = result.message.expect("cache produces a message");

        // Header reflects total rows and model.
        assert!(msg.contains("last 4 of 4 turn(s)"), "got: {msg}");
        // Per-turn ratios are rendered.
        assert!(msg.contains("75.0%"), "got: {msg}");
        assert!(msg.contains("50.0%"), "got: {msg}");
        // Turn 3: hit=2500, inferred miss=2500 → 50.0% with `*`-marked miss.
        assert!(msg.contains("2500*"), "got: {msg}");
        // Turn 4 (no telemetry) shows em-dashes and is excluded from totals.
        // Aggregate over turns 1-3: hit=8500, miss=6500 → 56.7%.
        assert!(msg.contains("avg hit ratio: 56.7%"), "got: {msg}");
        // Footer guidance is present.
        assert!(msg.contains("70%"), "got: {msg}");
    }

    #[test]
    fn cache_command_count_argument_clamps_to_history() {
        let mut app = create_test_app();
        for _ in 0..3 {
            app.push_turn_cache_record(TurnCacheRecord {
                input_tokens: 1_000,
                output_tokens: 100,
                cache_hit_tokens: Some(500),
                cache_miss_tokens: Some(500),
                reasoning_replay_tokens: None,
                recorded_at: Instant::now(),
            });
        }
        let result = cache(&mut app, Some("100"));
        let msg = result.message.expect("cache produces a message");
        // Asked for 100 turns, only 3 exist — should report "last 3 of 3".
        assert!(msg.contains("last 3 of 3 turn(s)"), "got: {msg}");
    }

    #[test]
    fn turn_cache_history_is_capped_at_50() {
        let mut app = create_test_app();
        for i in 0..(crate::tui::app::App::TURN_CACHE_HISTORY_CAP + 12) {
            app.push_turn_cache_record(TurnCacheRecord {
                input_tokens: i as u32,
                output_tokens: 1,
                cache_hit_tokens: Some(i as u32),
                cache_miss_tokens: Some(0),
                reasoning_replay_tokens: None,
                recorded_at: Instant::now(),
            });
        }
        assert_eq!(
            app.session.turn_cache_history.len(),
            crate::tui::app::App::TURN_CACHE_HISTORY_CAP
        );
        // Oldest record was evicted; newest record is still at the back.
        assert_eq!(
            app.session.turn_cache_history.back().unwrap().input_tokens,
            (crate::tui::app::App::TURN_CACHE_HISTORY_CAP + 11) as u32
        );
    }

    #[test]
    fn test_context_shows_usage_stats() {
        let mut app = create_test_app();
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "Hello".to_string(),
                cache_control: None,
            }],
        });
        app.history.push(HistoryCell::User {
            content: "Hello".to_string(),
        });

        let result = context(&mut app);
        assert!(matches!(
            result.action,
            Some(AppAction::OpenContextInspector)
        ));
        assert!(result.message.is_none());
    }

    #[test]
    fn test_undo_conversation_removes_last_exchange() {
        let mut app = create_test_app();
        app.history.push(HistoryCell::User {
            content: "Hello".to_string(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "Hi".to_string(),
            streaming: false,
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![],
        });

        let initial_history_len = app.history.len();
        let initial_api_len = app.api_messages.len();
        let result = undo_conversation(&mut app);

        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Removed"));
        assert!(app.history.len() < initial_history_len);
        assert!(app.api_messages.len() < initial_api_len);
    }

    #[test]
    fn test_undo_conversation_nothing_to_undo() {
        let mut app = create_test_app();
        // Clear any default history
        app.history.clear();
        app.api_messages.clear();
        let result = undo_conversation(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Nothing to undo") || msg.contains("Removed"));
    }

    #[test]
    fn test_retry_with_previous_message() {
        let mut app = create_test_app();
        app.history.push(HistoryCell::User {
            content: "Test message".to_string(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "Response".to_string(),
            streaming: false,
        });

        let result = retry(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Retrying"));
        assert!(msg.contains("Test message"));
        assert!(matches!(result.action, Some(AppAction::SendMessage(_))));
    }

    #[test]
    fn test_retry_no_previous_message() {
        let mut app = create_test_app();
        let result = retry(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("No previous request to retry"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_retry_truncates_long_input() {
        let mut app = create_test_app();
        let long_input = "x".repeat(100);
        app.history.push(HistoryCell::User {
            content: long_input.clone(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "Response".to_string(),
            streaming: false,
        });

        let result = retry(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Retrying"));
        assert!(msg.contains("..."));
    }

    #[test]
    fn test_patch_undo_requests_session_resync_after_restore() {
        use crate::snapshot::SnapshotRepo;
        use crate::test_support::lock_test_env;
        use std::sync::MutexGuard;
        use tempfile::tempdir;

        struct HomeGuard {
            prev: Option<std::ffi::OsString>,
            _lock: MutexGuard<'static, ()>,
        }

        impl Drop for HomeGuard {
            fn drop(&mut self) {
                // SAFETY: process-wide lock still held.
                unsafe {
                    match self.prev.take() {
                        Some(v) => std::env::set_var("HOME", v),
                        None => std::env::remove_var("HOME"),
                    }
                }
            }
        }

        fn scoped_home(home: &std::path::Path) -> HomeGuard {
            let lock = lock_test_env();
            let prev = std::env::var_os("HOME");
            // SAFETY: serialized by the global env lock.
            unsafe {
                std::env::set_var("HOME", home);
            }
            HomeGuard { prev, _lock: lock }
        }

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _guard = scoped_home(tmp.path());

        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        std::fs::write(workspace.join("a.txt"), b"original").unwrap();
        repo.snapshot("pre-turn:1").unwrap();
        std::fs::write(workspace.join("a.txt"), b"modified").unwrap();
        repo.snapshot("post-turn:1").unwrap();

        let mut app = create_test_app();
        app.workspace = workspace.clone();
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "please edit a.txt".to_string(),
                cache_control: None,
            }],
        });

        let result = patch_undo(&mut app);

        assert!(!result.is_error);
        assert!(matches!(
            result.action,
            Some(AppAction::SyncSession {
                ref messages,
                ref workspace,
                ..
            }) if messages == &app.api_messages && workspace == &app.workspace
        ));
    }

    #[test]
    fn test_patch_undo_walks_back_to_older_snapshot_on_repeat() {
        use crate::snapshot::SnapshotRepo;
        use crate::test_support::lock_test_env;
        use std::sync::MutexGuard;
        use tempfile::tempdir;

        struct HomeGuard {
            prev: Option<std::ffi::OsString>,
            _lock: MutexGuard<'static, ()>,
        }

        impl Drop for HomeGuard {
            fn drop(&mut self) {
                // SAFETY: process-wide lock still held.
                unsafe {
                    match self.prev.take() {
                        Some(v) => std::env::set_var("HOME", v),
                        None => std::env::remove_var("HOME"),
                    }
                }
            }
        }

        fn scoped_home(home: &std::path::Path) -> HomeGuard {
            let lock = lock_test_env();
            let prev = std::env::var_os("HOME");
            // SAFETY: serialized by the global env lock.
            unsafe {
                std::env::set_var("HOME", home);
            }
            HomeGuard { prev, _lock: lock }
        }

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _guard = scoped_home(tmp.path());

        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        let file = workspace.join("a.txt");
        std::fs::write(&file, b"zero").unwrap();
        repo.snapshot("tool:first").unwrap();
        std::fs::write(&file, b"one").unwrap();
        repo.snapshot("tool:second").unwrap();
        std::fs::write(&file, b"two").unwrap();

        let mut app = create_test_app();
        app.workspace = workspace.clone();

        let first = patch_undo(&mut app);
        assert!(!first.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one");

        let second = patch_undo(&mut app);
        assert!(!second.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "zero");
    }

    #[test]
    fn test_patch_undo_prunes_tool_turn_context() {
        use crate::snapshot::SnapshotRepo;
        use crate::test_support::lock_test_env;
        use std::sync::MutexGuard;
        use tempfile::tempdir;

        struct HomeGuard {
            prev: Option<std::ffi::OsString>,
            _lock: MutexGuard<'static, ()>,
        }

        impl Drop for HomeGuard {
            fn drop(&mut self) {
                // SAFETY: process-wide lock still held.
                unsafe {
                    match self.prev.take() {
                        Some(v) => std::env::set_var("HOME", v),
                        None => std::env::remove_var("HOME"),
                    }
                }
            }
        }

        fn scoped_home(home: &std::path::Path) -> HomeGuard {
            let lock = lock_test_env();
            let prev = std::env::var_os("HOME");
            // SAFETY: serialized by the global env lock.
            unsafe {
                std::env::set_var("HOME", home);
            }
            HomeGuard { prev, _lock: lock }
        }

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _guard = scoped_home(tmp.path());

        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        let file = workspace.join("a.txt");
        std::fs::write(&file, b"alpha").unwrap();
        repo.snapshot("tool:call-1").unwrap();
        std::fs::write(&file, b"alpha-fixed").unwrap();

        let mut app = create_test_app();
        app.workspace = workspace.clone();
        app.history.push(HistoryCell::User {
            content: "please edit a.txt".to_string(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "I will update the file.".to_string(),
            streaming: false,
        });
        app.history
            .push(HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
                name: "write_file".to_string(),
                status: ToolStatus::Success,
                input_summary: Some("a.txt".to_string()),
                output: Some("updated".to_string()),
                prompts: None,
                spillover_path: None,
                output_summary: None,
                is_diff: false,
            })));
        app.history.push(HistoryCell::Assistant {
            content: "Done, file is fixed now.".to_string(),
            streaming: false,
        });
        app.tool_cells.insert("call-1".to_string(), 2);

        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "please edit a.txt".to_string(),
                cache_control: None,
            }],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "I will update the file.".to_string(),
                    cache_control: None,
                },
                ContentBlock::ToolUse {
                    id: "call-1".to_string(),
                    name: "write_file".to_string(),
                    input: serde_json::json!({"path": "a.txt"}),
                    caller: None,
                },
            ],
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call-1".to_string(),
                content: "updated".to_string(),
                is_error: None,
                content_blocks: None,
            }],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "Done, file is fixed now.".to_string(),
                cache_control: None,
            }],
        });

        let result = patch_undo(&mut app);

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha");
        assert_eq!(app.history.len(), 3);
        assert!(matches!(
            app.history.last(),
            Some(HistoryCell::System { content }) if content.contains("/undo reverted workspace")
        ));
        assert_eq!(app.api_messages.len(), 2);
        assert!(matches!(
            &app.api_messages[0].content[0],
            ContentBlock::Text { text, .. } if text == "please edit a.txt"
        ));
        assert_eq!(app.api_messages[1].content.len(), 1);
        assert!(matches!(
            &app.api_messages[1].content[0],
            ContentBlock::Text { text, .. } if text == "I will update the file."
        ));
    }

    #[test]
    fn test_patch_undo_prunes_pre_turn_context() {
        use crate::snapshot::SnapshotRepo;
        use crate::test_support::lock_test_env;
        use std::sync::MutexGuard;
        use tempfile::tempdir;

        struct HomeGuard {
            prev: Option<std::ffi::OsString>,
            _lock: MutexGuard<'static, ()>,
        }

        impl Drop for HomeGuard {
            fn drop(&mut self) {
                // SAFETY: process-wide lock still held.
                unsafe {
                    match self.prev.take() {
                        Some(v) => std::env::set_var("HOME", v),
                        None => std::env::remove_var("HOME"),
                    }
                }
            }
        }

        fn scoped_home(home: &std::path::Path) -> HomeGuard {
            let lock = lock_test_env();
            let prev = std::env::var_os("HOME");
            // SAFETY: serialized by the global env lock.
            unsafe {
                std::env::set_var("HOME", home);
            }
            HomeGuard { prev, _lock: lock }
        }

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _guard = scoped_home(tmp.path());

        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        let file = workspace.join("a.txt");
        std::fs::write(&file, b"alpha").unwrap();
        repo.snapshot("pre-turn:1").unwrap();
        std::fs::write(&file, b"alpha-fixed").unwrap();

        let mut app = create_test_app();
        app.workspace = workspace.clone();
        app.history.push(HistoryCell::User {
            content: "please edit a.txt".to_string(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "Done, file is fixed now.".to_string(),
            streaming: false,
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "please edit a.txt".to_string(),
                cache_control: None,
            }],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "Done, file is fixed now.".to_string(),
                cache_control: None,
            }],
        });

        let result = patch_undo(&mut app);

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha");
        assert_eq!(app.history.len(), 1);
        assert!(matches!(
            app.history.last(),
            Some(HistoryCell::System { content }) if content.contains("/undo reverted workspace")
        ));
        assert!(app.api_messages.is_empty());
    }

    #[test]
    fn test_prune_undone_tool_context_preserves_prior_tool_pairs() {
        let mut app = create_test_app();
        app.history.push(HistoryCell::User {
            content: "edit two files".to_string(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "I will update both files.".to_string(),
            streaming: false,
        });
        app.history
            .push(HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
                name: "write_file".to_string(),
                status: ToolStatus::Success,
                input_summary: Some("a.txt".to_string()),
                output: Some("updated a".to_string()),
                prompts: None,
                spillover_path: None,
                output_summary: None,
                is_diff: false,
            })));
        app.history
            .push(HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
                name: "write_file".to_string(),
                status: ToolStatus::Success,
                input_summary: Some("b.txt".to_string()),
                output: Some("updated b".to_string()),
                prompts: None,
                spillover_path: None,
                output_summary: None,
                is_diff: false,
            })));
        app.history.push(HistoryCell::Assistant {
            content: "Done.".to_string(),
            streaming: false,
        });
        app.tool_cells.insert("call-a".to_string(), 2);
        app.tool_cells.insert("call-b".to_string(), 3);

        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "edit two files".to_string(),
                cache_control: None,
            }],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "I will update both files.".to_string(),
                    cache_control: None,
                },
                ContentBlock::ToolUse {
                    id: "call-a".to_string(),
                    name: "write_file".to_string(),
                    input: serde_json::json!({"path": "a.txt"}),
                    caller: None,
                },
                ContentBlock::ToolUse {
                    id: "call-b".to_string(),
                    name: "write_file".to_string(),
                    input: serde_json::json!({"path": "b.txt"}),
                    caller: None,
                },
            ],
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call-a".to_string(),
                content: "updated a".to_string(),
                is_error: None,
                content_blocks: None,
            }],
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call-b".to_string(),
                content: "updated b".to_string(),
                is_error: None,
                content_blocks: None,
            }],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "Done.".to_string(),
                cache_control: None,
            }],
        });

        prune_undone_tool_context(&mut app, "call-b");

        assert_eq!(app.history.len(), 3);
        assert_eq!(app.api_messages.len(), 3);
        assert!(matches!(
            &app.api_messages[1].content[..],
            [
                ContentBlock::Text { .. },
                ContentBlock::ToolUse { id, .. }
            ] if id == "call-a"
        ));
        assert!(matches!(
            &app.api_messages[2].content[0],
            ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call-a"
        ));
    }

    #[test]
    fn cache_inspect_layer_hashes_are_stable_across_calls() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Environment\n\n- shell: powershell".to_string(),
        ));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "stable task".to_string(),
                cache_control: None,
            }],
        });

        let first = cache(&mut app, Some("inspect"))
            .message
            .expect("first inspect");
        // Clear last_cache_inspection so we rebuild from scratch.
        app.session.last_cache_inspection = None;
        let second = cache(&mut app, Some("inspect"))
            .message
            .expect("second inspect");

        // Extract hashes from both outputs and compare.
        let extract_hashes = |s: &str| -> Vec<String> {
            s.lines()
                .filter(|l| l.contains("hash="))
                .filter_map(|l| l.split("hash=").nth(1).map(str::to_string))
                .collect()
        };
        let h1 = extract_hashes(&first);
        let h2 = extract_hashes(&second);
        assert_eq!(h1, h2, "hashes must be stable across identical calls");
    }

    #[test]
    fn cache_inspect_json_mode_roundtrips() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "json test".to_string(),
                cache_control: None,
            }],
        });

        let result = cache(&mut app, Some("inspect --json"));
        let msg = result.message.expect("json output");

        // Must be valid JSON.
        let parsed: serde_json::Value =
            serde_json::from_str(&msg).expect("output must be valid JSON");

        // Must contain the expected top-level keys.
        assert!(parsed.get("base_static_prefix_hash").is_some());
        assert!(parsed.get("full_request_prefix_hash").is_some());
        assert!(parsed.get("layers").is_some());
        let layers = parsed["layers"].as_array().expect("layers is array");
        assert!(!layers.is_empty());

        // Each layer must have byte_len and token_estimate.
        for layer in layers {
            assert!(layer.get("byte_len").is_some());
            assert!(layer.get("token_estimate").is_some());
            assert!(layer.get("char_len").is_some());
            assert!(layer.get("sha256").is_some());
            assert!(layer.get("name").is_some());
            assert!(layer.get("stability").is_some());
        }
    }

    #[test]
    fn cache_inspect_verbose_shows_diff_on_change() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Environment\n\n- shell: powershell".to_string(),
        ));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "first task".to_string(),
                cache_control: None,
            }],
        });

        // First call — no previous inspection to diff against.
        let first = cache(&mut app, Some("inspect --verbose"))
            .message
            .expect("first verbose");
        assert!(
            first.contains("No previous inspection to compare against"),
            "got: {first}"
        );

        // Change the user message.
        if let Some(last) = app.api_messages.last_mut()
            && let Some(ContentBlock::Text { text, .. }) = last.content.first_mut()
        {
            *text = "second task".to_string();
        }

        // Second call — should show a diff.
        let second = cache(&mut app, Some("inspect --verbose"))
            .message
            .expect("second verbose");
        assert!(second.contains("── Verbose diff ──"), "got: {second}");
        assert!(
            second.contains("CHANGED") || second.contains("ADDED") || second.contains("REMOVED"),
            "expected at least one diff marker, got: {second}"
        );
    }

    #[test]
    fn cache_inspect_default_mode_has_no_diff_output() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        // Change message.
        if let Some(last) = app.api_messages.last_mut()
            && let Some(ContentBlock::Text { text, .. }) = last.content.first_mut()
        {
            *text = "task changed".to_string();
        }
        let second = cache(&mut app, Some("inspect"))
            .message
            .expect("second default");

        // Default mode must NOT contain verbose diff markers.
        assert!(
            !second.contains("── Verbose diff ──"),
            "default mode should not show verbose diff"
        );
        assert!(
            !second.contains("CHANGED"),
            "default mode should not show CHANGED markers"
        );
    }

    #[test]
    fn cache_inspect_divergence_cause_inference() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Environment\n\n- shell: powershell".to_string(),
        ));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "first task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        // Change user message — should trigger cause inference.
        if let Some(last) = app.api_messages.last_mut()
            && let Some(ContentBlock::Text { text, .. }) = last.content.first_mut()
        {
            *text = "second task".to_string();
        }
        let second = cache(&mut app, Some("inspect"))
            .message
            .expect("second inspect");

        // "User task" layer should show expected cause.
        assert!(
            second.contains("(expected: new user message each turn)"),
            "got: {second}"
        );
    }

    #[test]
    fn cache_inspect_byte_len_and_token_estimate_present() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "test".to_string(),
                cache_control: None,
            }],
        });

        let result = cache(&mut app, Some("inspect"))
            .message
            .expect("inspect output");

        // Every layer line must show bytes= and ~Ntok.
        for line in result.lines() {
            if line.contains("chars=") {
                assert!(line.contains("bytes="), "missing bytes in: {line}");
                assert!(
                    line.contains("tok,") || line.contains("tok\n") || line.ends_with("tok"),
                    "missing token_estimate in: {line}"
                );
            }
        }
        // Must show estimated total tokens.
        assert!(result.contains("Estimated total tokens:"), "got: {result}");
    }

    // ── Phase 7: CacheWarmupKey tests ──

    #[test]
    fn warmup_key_is_stable_across_repeated_inspect() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        // Run inspect again with the same state.
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_eq!(
            key1, key2,
            "warmup key must be stable across repeated inspect"
        );
    }

    #[test]
    fn warmup_key_changes_when_model_changes() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        app.model = "deepseek-v3".to_string();
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_ne!(key1, key2, "warmup key must change when model changes");
        assert_eq!(key1.provider, key2.provider);
        assert_ne!(key1.model, key2.model);
    }

    #[test]
    fn warmup_key_changes_when_provider_changes() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        app.api_provider = crate::config::ApiProvider::DeepseekCN;
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_ne!(key1, key2, "warmup key must change when provider changes");
        assert_ne!(key1.provider, key2.provider);
    }

    #[test]
    fn warmup_key_changes_when_static_prefix_changes() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy v1".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        // Change the system prompt (static layer).
        app.system_prompt = Some(SystemPrompt::Text("Base policy v2".to_string()));
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_ne!(
            key1, key2,
            "warmup key must change when static prefix changes"
        );
    }

    #[test]
    fn warmup_key_does_not_change_when_user_message_changes() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "first task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        // Change user message (dynamic layer).
        if let Some(last) = app.api_messages.last_mut()
            && let Some(ContentBlock::Text { text, .. }) = last.content.first_mut()
        {
            *text = "second task".to_string();
        }
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_eq!(
            key1, key2,
            "warmup key must not change when user message changes"
        );
    }

    #[test]
    fn warmup_key_changes_when_skills_change() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Skills\n\n- skill-a".to_string(),
        ));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        // Change skills content (static layer).
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Skills\n\n- skill-a\n- skill-b".to_string(),
        ));
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_ne!(key1, key2, "warmup key must change when skills change");
    }

    #[test]
    fn warmup_status_displayed_in_inspect_output() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        // First inspect — no previous warmup.
        let first = cache(&mut app, Some("inspect"))
            .message
            .expect("first inspect");
        assert!(
            first.contains("Warmup status: no previous warmup"),
            "got: {first}"
        );

        // Second inspect — same state, warmup key should match.
        let second = cache(&mut app, Some("inspect"))
            .message
            .expect("second inspect");
        assert!(second.contains("Warmup status: valid"), "got: {second}");
    }

    #[test]
    fn warmup_key_in_json_output() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect --json"));
        // JSON mode should also store the warmup key.
        assert!(
            app.session.last_warmup_key.is_some(),
            "warmup key must be stored after JSON inspect"
        );
    }

    #[test]
    fn cache_inspect_shows_tool_catalog_hash_when_available() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });
        app.session.last_tool_catalog = Some(vec![test_tool("read_file")]);

        let result = cache(&mut app, Some("inspect"))
            .message
            .expect("inspect output");
        assert!(
            result.contains("Tool catalog hash: ") && !result.contains("(no tools registered)"),
            "got: {result}"
        );
    }

    #[test]
    fn cache_inspect_shows_no_tools_when_hash_unavailable() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });
        // No tool catalog hash stored.
        app.session.last_tool_catalog_hash = None;

        let result = cache(&mut app, Some("inspect"))
            .message
            .expect("inspect output");
        assert!(
            result.contains("Tool catalog hash: (no tools registered)"),
            "got: {result}"
        );
    }

    #[test]
    fn cache_inspect_shows_stable_prefix_hash() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let result = cache(&mut app, Some("inspect"))
            .message
            .expect("inspect output");
        assert!(
            result.contains("Stable prefix hash (excl. tools):"),
            "got: {result}"
        );
    }

    #[test]
    fn cache_inspect_json_includes_tool_catalog_hash() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });
        app.session.last_tool_catalog = Some(vec![test_tool("read_file")]);

        let result = cache(&mut app, Some("inspect --json"))
            .message
            .expect("json output");
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        assert_eq!(parsed["tool_catalog_hash"].as_str().unwrap().len(), 64);
        assert!(
            parsed["stable_prefix_hash"].as_str().is_some(),
            "stable_prefix_hash must be present in JSON output"
        );
    }

    #[test]
    fn tool_catalog_hash_changes_when_stored_hash_changes() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        app.session.last_tool_catalog = Some(vec![test_tool("alpha")]);
        let _ = cache(&mut app, Some("inspect")).message.expect("first");
        let first_key = app.session.last_warmup_key.clone().expect("first key");

        app.session.last_tool_catalog = Some(vec![test_tool("beta")]);
        let second = cache(&mut app, Some("inspect")).message.expect("second");
        let second_key = app.session.last_warmup_key.clone().expect("second key");
        assert!(second.contains("Tool catalog hash: "));
        assert_ne!(first_key.tool_catalog_hash, second_key.tool_catalog_hash);
    }

    #[test]
    fn warmup_key_has_all_required_fields() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });
        app.session.last_tool_catalog = Some(vec![test_tool("read_file")]);
        app.session.last_base_url = Some("https://api.deepseek.com".to_string());

        let _ = cache(&mut app, Some("inspect"));
        let key = app.session.last_warmup_key.clone().expect("key");

        assert!(!key.provider.is_empty(), "provider must be set");
        assert!(!key.model.is_empty(), "model must be set");
        assert_eq!(key.base_url, "https://api.deepseek.com");
        assert!(
            !key.static_prefix_hash.is_empty(),
            "static_prefix_hash must be set"
        );
        assert_eq!(key.tool_catalog_hash.len(), 64);
        assert!(key.project_pack_hash.is_empty());
        assert!(key.skills_hash.is_empty());
    }

    #[test]
    fn warmup_key_changes_when_base_url_changes() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });
        app.session.last_base_url = Some("https://api.deepseek.com".to_string());

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        app.session.last_base_url = Some("https://custom.endpoint.com".to_string());
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_ne!(key1, key2, "warmup key must change when base_url changes");
        assert_ne!(key1.base_url, key2.base_url);
    }

    #[test]
    fn warmup_key_changes_when_tool_catalog_hash_changes() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });
        app.session.last_tool_catalog = Some(vec![test_tool("alpha")]);

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");

        app.session.last_tool_catalog = Some(vec![test_tool("beta")]);
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");

        assert_ne!(
            key1, key2,
            "warmup key must change when tool_catalog_hash changes"
        );
    }

    #[test]
    fn warmup_key_changes_when_project_pack_or_skills_change() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Project Context Pack\n\n<project_context_pack>\n{\"files\":[\"a.rs\"]}\n</project_context_pack>\n\n## Skills\n\n- rust: code"
                .to_string(),
        ));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });

        let _ = cache(&mut app, Some("inspect"));
        let key1 = app.session.last_warmup_key.clone().expect("key1");
        assert_eq!(key1.project_pack_hash.len(), 64);
        assert_eq!(key1.skills_hash.len(), 64);

        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Project Context Pack\n\n<project_context_pack>\n{\"files\":[\"b.rs\"]}\n</project_context_pack>\n\n## Skills\n\n- rust: code"
                .to_string(),
        ));
        let _ = cache(&mut app, Some("inspect"));
        let key2 = app.session.last_warmup_key.clone().expect("key2");
        assert_ne!(key1.project_pack_hash, key2.project_pack_hash);

        app.system_prompt = Some(SystemPrompt::Text(
            "Base policy\n\n## Project Context Pack\n\n<project_context_pack>\n{\"files\":[\"b.rs\"]}\n</project_context_pack>\n\n## Skills\n\n- go: code"
                .to_string(),
        ));
        let _ = cache(&mut app, Some("inspect"));
        let key3 = app.session.last_warmup_key.clone().expect("key3");
        assert_ne!(key2.skills_hash, key3.skills_hash);
    }

    #[test]
    fn warmup_key_does_not_depend_on_last_cache_inspection() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Base policy".to_string()));
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "task".to_string(),
                cache_control: None,
            }],
        });
        app.session.last_tool_catalog = Some(vec![test_tool("read_file")]);
        app.session.last_base_url = Some("https://api.deepseek.com".to_string());

        // Run inspect first to populate last_cache_inspection.
        let _ = cache(&mut app, Some("inspect"));
        let key_with_inspect = app.session.last_warmup_key.clone().expect("key");

        // Clear last_cache_inspection but keep other state.
        app.session.last_cache_inspection = None;

        // Run inspect again — key should be the same even without prior inspection.
        let _ = cache(&mut app, Some("inspect"));
        let key_without_inspect = app.session.last_warmup_key.clone().expect("key2");

        assert_eq!(
            key_with_inspect, key_without_inspect,
            "warmup key must not depend on last_cache_inspection"
        );
    }
}

/// Remove last message pair (user + assistant).
///
/// This is the old `/undo` behaviour — it removes the most recent
/// user+assistant conversation pair from history and API messages.
/// The new `/undo` first tries to revert workspace files via
/// [`patch_undo`]; if no snapshots are available it falls back to
/// this function.
pub fn undo_conversation(app: &mut App) -> CommandResult {
    // Remove from display history (up to the last user message)
    let mut removed_count = 0;
    while !app.history.is_empty() {
        let last_is_user = matches!(app.history.last(), Some(HistoryCell::User { .. }));
        app.pop_history();
        removed_count += 1;
        if last_is_user {
            break;
        }
    }

    // Remove from API messages
    while let Some(last) = app.api_messages.last() {
        if last.role == "user" {
            app.api_messages.pop();
            break;
        }
        app.api_messages.pop();
    }

    if removed_count > 0 {
        // Keep tool/index mappings consistent after truncation.
        app.tool_cells.clear();
        app.tool_details_by_cell.clear();
        app.exploring_entries.clear();
        app.ignored_tool_calls.clear();
        app.mark_history_updated();
        CommandResult::message(format!("Removed {removed_count} message(s)"))
    } else {
        CommandResult::message("Nothing to undo")
    }
}

fn prune_undone_tool_context(app: &mut App, tool_id: &str) {
    if let Some(history_idx) = app.tool_cells.get(tool_id).copied() {
        app.truncate_history_to(history_idx);
    }

    let Some((msg_idx, block_idx)) =
        app.api_messages
            .iter()
            .enumerate()
            .find_map(|(msg_idx, msg)| {
                msg.content
                    .iter()
                    .position(
                        |block| matches!(block, ContentBlock::ToolUse { id, .. } if id == tool_id),
                    )
                    .map(|block_idx| (msg_idx, block_idx))
            })
    else {
        return;
    };

    let kept_blocks = app.api_messages[msg_idx].content[..block_idx].to_vec();
    let kept_tool_ids: std::collections::HashSet<String> = kept_blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();

    if kept_blocks.is_empty() {
        app.api_messages.truncate(msg_idx);
        return;
    }
    let preserved_tool_results: Vec<_> =
        app.api_messages
            .iter()
            .skip(msg_idx + 1)
            .take_while(|msg| {
                msg.role == "user"
                    && !msg.content.is_empty()
                    && msg
                        .content
                        .iter()
                        .all(|block| tool_result_id(block).is_some())
            })
            .filter(|msg| {
                msg.role == "user"
                    && !msg.content.is_empty()
                    && msg.content.iter().all(|block| {
                        tool_result_id(block).is_some_and(|id| kept_tool_ids.contains(id))
                    })
            })
            .cloned()
            .collect();
    app.api_messages.truncate(msg_idx + 1);
    app.api_messages[msg_idx].content = kept_blocks;
    app.api_messages.extend(preserved_tool_results);
}

fn prune_undone_turn_context(app: &mut App) {
    if let Some(history_idx) = app
        .history
        .iter()
        .rposition(|cell| matches!(cell, HistoryCell::User { .. }))
    {
        app.truncate_history_to(history_idx);
    }

    if let Some(api_idx) = app.api_messages.iter().rposition(|msg| msg.role == "user") {
        app.api_messages.truncate(api_idx);
    }
}

fn tool_result_id(block: &ContentBlock) -> Option<&String> {
    match block {
        ContentBlock::ToolResult { tool_use_id, .. }
        | ContentBlock::ToolSearchToolResult { tool_use_id, .. }
        | ContentBlock::CodeExecutionToolResult { tool_use_id, .. } => Some(tool_use_id),
        _ => None,
    }
}

/// Revert the most recent write tool (apply_patch/edit_file/write_file) or turn.
///
/// Opens the side-git snapshot repo and finds the most recent snapshot,
/// preferring per-tool snapshots (`tool:*`) over pre-turn snapshots
/// (`pre-turn:*`). Restores files from that snapshot and shows a diff
/// summary. Falls back to conversation undo when no snapshots exist.
///
/// Posts a `HistoryCell::System` entry so the user can see what was
/// reverted in the transcript.
pub fn patch_undo(app: &mut App) -> CommandResult {
    let workspace = app.workspace.clone();

    let repo = match crate::snapshot::SnapshotRepo::open_or_init(&workspace) {
        Ok(r) => r,
        Err(e) => {
            return CommandResult::error(format!(
                "Snapshot repo unavailable for {}: {e}",
                workspace.display(),
            ));
        }
    };

    let snapshots = match repo.list(20) {
        Ok(s) => s,
        Err(e) => {
            return CommandResult::error(format!("Failed to list snapshots: {e}"));
        }
    };

    if snapshots.is_empty() {
        return CommandResult::message("No snapshots found to undo — nothing to revert.");
    }

    // Prefer the newest revertable `tool:` / `pre-turn:` snapshot whose
    // tracked content differs from the current workspace. This lets
    // repeated `/undo` walk back through older snapshots instead of
    // restoring the same no-op target forever.
    let target = snapshots
        .iter()
        .filter(|s| s.label.starts_with("tool:") || s.label.starts_with("pre-turn:"))
        .find(|s| match repo.work_tree_matches_snapshot(&s.id) {
            Ok(matches) => !matches,
            Err(_) => true,
        });

    let Some(target) = target else {
        return CommandResult::message(
            "No older tool or pre-turn snapshots differ from the current workspace — nothing to revert.",
        );
    };

    if let Err(e) = repo.restore(&target.id) {
        return CommandResult::error(format!("Restore failed: {e}"));
    }

    if let Some(tool_id) = target.label.strip_prefix("tool:") {
        prune_undone_tool_context(app, tool_id);
    } else if target.label.starts_with("pre-turn:") {
        prune_undone_turn_context(app);
    }

    // Show diff stat so the user knows what changed.
    let diff_stat = std::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(&workspace)
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        });

    let short = &target.id.as_str()[..target.id.as_str().len().min(8)];
    let summary = match diff_stat {
        Some(ref stat) => {
            format!(
                "Restored snapshot '{}' ({}). Files affected:\n{stat}",
                target.label, short
            )
        }
        None => {
            format!(
                "Restored snapshot '{}' ({}). No diff changes detected.",
                target.label, short
            )
        }
    };

    // Post a system cell so the reverted state is visible in the transcript.
    app.push_history_cell(HistoryCell::System {
        content: format!(
            "/undo reverted workspace to snapshot '{}' ({})",
            target.label, short
        ),
    });

    CommandResult::with_message_and_action(
        summary,
        AppAction::SyncSession {
            session_id: app.current_session_id.clone(),
            messages: app.api_messages.clone(),
            system_prompt: app.system_prompt.clone(),
            model: app.model.clone(),
            workspace: app.workspace.clone(),
        },
    )
}

/// Load the last user message back into the composer for editing.
///
/// Searches `app.history` for the most recent `HistoryCell::User`, copies its
/// content into `app.input`, and positions the cursor at the end so the user
/// can edit and press Enter to resubmit. The original exchange stays visible
/// in the transcript.
pub fn edit(app: &mut App) -> CommandResult {
    let last_user = app.history.iter().rev().find_map(|cell| match cell {
        HistoryCell::User { content } => Some(content.clone()),
        _ => None,
    });

    match last_user {
        Some(content) => {
            app.input = content;
            app.cursor_position = app.input.chars().count();
            app.edit_in_progress = true;
            CommandResult::message(
                "Last message loaded into composer — edit and press Enter to resubmit",
            )
        }
        None => CommandResult::message("No previous message to edit"),
    }
}

/// Show git diff output since session start.
///
/// Runs `git diff --stat` and `git diff --name-only` in the workspace
/// directory. Displays which files have changed and a stat summary. If no
/// changes exist or git fails, returns an appropriate message.
pub fn diff(app: &mut App) -> CommandResult {
    let workspace = app.workspace.clone();

    let name_only_output = std::process::Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(&workspace)
        .output();
    let stat_output = std::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(&workspace)
        .output();

    match (name_only_output, stat_output) {
        (Ok(name_only), Ok(stat)) => {
            let name_stdout = String::from_utf8_lossy(&name_only.stdout);
            let stat_stdout = String::from_utf8_lossy(&stat.stdout);

            if name_stdout.trim().is_empty() {
                return CommandResult::message("No changes since session start");
            }

            let files: Vec<&str> = name_stdout.lines().filter(|l| !l.is_empty()).collect();
            let file_count = files.len();
            let file_list = files.join("\n");

            // Detect rename entries (e.g. "foo -> bar") and exclude them
            // from the file-count header so the user sees only actual
            // modifications.
            let renamed_count = files.iter().filter(|f| f.contains(" -> ")).count();
            let summary = if renamed_count > 0 {
                format!("Changed files ({file_count}, {renamed_count} renamed):\n{file_list}")
            } else {
                format!("Changed files ({file_count}):\n{file_list}")
            };

            let stat_str = stat_stdout.trim();
            let mut message = summary;
            if !stat_str.is_empty() {
                message.push_str("\n\n── Stat ──\n");
                message.push_str(stat_str);
            }
            CommandResult::message(message)
        }
        (Err(e), _) | (_, Err(e)) => {
            CommandResult::message(format!("Git diff failed — is this a git repository?\n{e}"))
        }
    }
}

/// Retry last request - remove last exchange and re-send the user's message
pub fn retry(app: &mut App) -> CommandResult {
    let last_user_input = app.history.iter().rev().find_map(|cell| match cell {
        HistoryCell::User { content } => Some(content.clone()),
        _ => None,
    });

    match last_user_input {
        Some(input) => {
            undo_conversation(app);
            let display_input = if input.len() > 50 {
                let truncate_at = input
                    .char_indices()
                    .take_while(|(i, _)| *i <= 50)
                    .last()
                    .map_or(0, |(i, _)| i);
                format!("{}...", &input[..truncate_at])
            } else {
                input.clone()
            };
            CommandResult::with_message_and_action(
                format!("Retrying: {display_input}"),
                AppAction::SendMessage(input),
            )
        }
        None => CommandResult::error("No previous request to retry"),
    }
}

/// Show flicker prevention diagnostics for the current terminal.
///
/// Displays terminal capabilities, synchronized update support, and
/// rendering configuration. Useful for debugging flicker issues on
/// Windows terminals.
pub fn flicker(_app: &mut App) -> CommandResult {
    use crate::tui::terminal_caps::TerminalCapabilities;

    let caps = TerminalCapabilities::detect();

    let mut report = String::new();
    report.push_str("Flicker Prevention Diagnostics\n");
    report.push_str("═══════════════════════════════════════════════════════════════\n\n");

    // Terminal detection
    report.push_str("Terminal Detection:\n");
    report.push_str(&format!(
        "  Windows Terminal:    {}\n",
        if caps.is_windows_terminal { "YES" } else { "no" }
    ));
    report.push_str(&format!(
        "  VSCode Terminal:     {}\n",
        if caps.is_vscode_terminal { "YES" } else { "no" }
    ));
    report.push_str(&format!(
        "  ConPTY:              {}\n",
        if caps.is_conpty { "YES" } else { "no" }
    ));

    report.push('\n');

    // Synchronized update support
    report.push_str("DECSET 2026 (Synchronized Output):\n");
    report.push_str(&format!(
        "  Supported:           {}\n",
        if caps.supports_synchronized_update { "YES" } else { "NO" }
    ));
    report.push_str("  Status:              ");
    if caps.supports_synchronized_update {
        report.push_str("Enabled - terminal will buffer frames atomically\n");
    } else {
        report.push_str("Disabled - may cause flicker on resize/focus\n");
    }

    report.push('\n');

    // Frame rate
    report.push_str("Frame Rate Configuration:\n");
    let interval_ms = caps.recommended_frame_interval_ms();
    let fps = 1000.0 / interval_ms as f64;
    report.push_str(&format!(
        "  Frame interval:      {}ms (~{:.0} FPS)\n",
        interval_ms, fps
    ));

    report.push('\n');

    // Anti-flicker measures
    report.push_str("Active Anti-Flicker Measures:\n");
    report.push_str("  [x] DECSET 2026 synchronized updates\n");
    report.push_str("  [x] swap_buffers() instead of clear()\\x1b[2J]\n");
    report.push_str("  [x] Frame rate limiter (120 FPS cap)\n");
    report.push_str("  [x] Resize event coalescing\n");
    report.push_str("  [x] Force-size during resize to prevent stale dimensions\n");

    report.push('\n');

    // Platform-specific notes
    report.push_str("Platform Notes:\n");
    if cfg!(windows) {
        report.push_str("  OS: Windows\n");
        if caps.is_windows_terminal {
            report.push_str("  → Windows Terminal detected: Full VT support\n");
        } else if caps.is_conpty {
            report.push_str("  → ConPTY detected: VT passthrough enabled\n");
        } else {
            report.push_str("  → Legacy console detected: Limited VT support\n");
            report.push_str("  → Consider using Windows Terminal for best experience\n");
        }
    } else if cfg!(target_os = "macos") {
        report.push_str("  OS: macOS\n");
        report.push_str("  → Most macOS terminals support synchronized updates\n");
    } else {
        report.push_str("  OS: Linux/Unix\n");
        report.push_str("  → Most Linux terminals support synchronized updates\n");
    }

    report.push('\n');
    report.push_str("If you experience flicker, check:\n");
    report.push_str("  1. Is your terminal up to date?\n");
    report.push_str("  2. Try enabling low_motion mode: /config set low_motion true\n");
    report.push_str("  3. Check RUST_LOG=render for rendering debug output\n");

    CommandResult::message(report)
}
