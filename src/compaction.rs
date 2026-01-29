//! Context compaction for long conversations.

#![allow(dead_code)]

use anyhow::Result;
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use crate::client::DeepSeekClient;
use crate::llm_client::LlmClient;
use crate::logging;
use crate::models::{
    CacheControl, ContentBlock, Message, MessageRequest, SystemBlock, SystemPrompt,
};

/// Configuration for conversation compaction behavior.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    pub enabled: bool,
    pub token_threshold: usize,
    pub message_threshold: usize,
    pub model: String,
    pub cache_summary: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token_threshold: 50000,
            message_threshold: 50,
            model: "deepseek-reasoner".to_string(),
            cache_summary: true,
        }
    }
}

const KEEP_RECENT_MESSAGES: usize = 4;
const RECENT_WORKING_SET_WINDOW: usize = 12;
const MAX_WORKING_SET_PATHS: usize = 24;
const MIN_SUMMARIZE_MESSAGES: usize = 6;

#[derive(Debug, Clone, Default)]
struct CompactionPlan {
    pinned_indices: BTreeSet<usize>,
    summarize_indices: Vec<usize>,
    working_set_paths: HashSet<String>,
}

fn path_regex() -> &'static Regex {
    static PATH_RE: OnceLock<Regex> = OnceLock::new();
    PATH_RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            (?:
                (?P<root>
                    Cargo\.toml|
                    Cargo\.lock|
                    README\.md|
                    CHANGELOG\.md|
                    AGENTS\.md|
                    config\.example\.toml
                )
            )
            |
            (?P<path>
                (?:[A-Za-z0-9._-]+/)+
                [A-Za-z0-9._-]+
                \.(?:rs|toml|md|json|ya?ml|txt|lock)
            )
        ",
        )
        .expect("path regex is valid")
    })
}

fn normalize_path_candidate(candidate: &str, workspace: Option<&Path>) -> Option<String> {
    if candidate.is_empty() {
        return None;
    }

    let cleaned = candidate.replace('\\', "/");
    let mut path = PathBuf::from(cleaned);

    if path.is_absolute() {
        let ws = workspace?;
        if let Ok(stripped) = path.strip_prefix(ws) {
            path = stripped.to_path_buf();
        } else {
            return None;
        }
    }

    let rel = path.to_string_lossy().trim_start_matches("./").to_string();
    if rel.is_empty() || rel.contains("..") {
        return None;
    }

    if let Some(ws) = workspace {
        let repo_path = ws.join(&rel);
        if repo_path.exists() || looks_repo_relative(&rel) {
            return Some(rel);
        }
        return None;
    }

    if looks_repo_relative(&rel) {
        return Some(rel);
    }

    None
}

fn looks_repo_relative(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml"
            | "Cargo.lock"
            | "README.md"
            | "CHANGELOG.md"
            | "AGENTS.md"
            | "config.example.toml"
    ) || path.starts_with("src/")
        || path.starts_with("tests/")
        || path.starts_with("docs/")
        || path.starts_with("examples/")
        || path.starts_with("benches/")
        || path.starts_with("crates/")
        || path.starts_with(".github/")
        || (path.contains('/') && path.rsplit('.').next().is_some())
}

fn extract_paths_from_text(text: &str, workspace: Option<&Path>) -> Vec<String> {
    path_regex()
        .captures_iter(text)
        .filter_map(|caps| {
            let candidate = caps
                .name("path")
                .or_else(|| caps.name("root"))
                .map(|m| m.as_str())?;
            normalize_path_candidate(candidate, workspace)
        })
        .collect()
}

fn extract_paths_from_tool_input(
    input: &serde_json::Value,
    workspace: Option<&Path>,
) -> Vec<String> {
    let mut out = Vec::new();
    let Some(obj) = input.as_object() else {
        return out;
    };

    for key in ["path", "file", "target", "cwd"] {
        if let Some(val) = obj.get(key).and_then(serde_json::Value::as_str)
            && let Some(path) = normalize_path_candidate(val, workspace)
        {
            out.push(path);
        }
    }

    for key in ["paths", "files", "targets"] {
        if let Some(vals) = obj.get(key).and_then(serde_json::Value::as_array) {
            for val in vals {
                if let Some(s) = val.as_str()
                    && let Some(path) = normalize_path_candidate(s, workspace)
                {
                    out.push(path);
                }
            }
        }
    }

    out
}

fn message_text(msg: &Message) -> String {
    let mut text = String::new();
    for block in &msg.content {
        match block {
            ContentBlock::Text { text: t, .. } => {
                let _ = writeln!(text, "{t}");
            }
            ContentBlock::Thinking { .. } => {}
            ContentBlock::ToolUse { name, input, .. } => {
                let _ = writeln!(text, "[tool_use:{name}] {input}");
            }
            ContentBlock::ToolResult { content, .. } => {
                let _ = writeln!(text, "{content}");
            }
        }
    }
    text
}

fn extract_paths_from_message(message: &Message, workspace: Option<&Path>) -> Vec<String> {
    let mut paths = Vec::new();
    for block in &message.content {
        let candidates = match block {
            ContentBlock::Text { text, .. } => extract_paths_from_text(text, workspace),
            ContentBlock::ToolResult { content, .. } => extract_paths_from_text(content, workspace),
            ContentBlock::ToolUse { input, .. } => extract_paths_from_tool_input(input, workspace),
            ContentBlock::Thinking { .. } => Vec::new(),
        };
        paths.extend(candidates);
    }
    paths
}

fn derive_working_set_paths(
    messages: &[Message],
    workspace: Option<&Path>,
    seed_indices: &[usize],
) -> HashSet<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut seeds: Vec<usize> = seed_indices
        .iter()
        .copied()
        .filter(|idx| *idx < messages.len())
        .collect();
    seeds.sort_unstable_by(|a, b| b.cmp(a));

    for idx in seeds {
        for candidate in extract_paths_from_message(&messages[idx], workspace) {
            if seen.insert(candidate.clone()) {
                paths.push(candidate);
                if paths.len() >= MAX_WORKING_SET_PATHS {
                    return paths.into_iter().collect();
                }
            }
        }
    }

    for msg in messages.iter().rev().take(RECENT_WORKING_SET_WINDOW) {
        for candidate in extract_paths_from_message(msg, workspace) {
            if seen.insert(candidate.clone()) {
                paths.push(candidate);
                if paths.len() >= MAX_WORKING_SET_PATHS {
                    return paths.into_iter().collect();
                }
            }
        }
    }

    paths.into_iter().collect()
}

fn should_pin_message(text: &str, working_set_paths: &HashSet<String>) -> bool {
    let lower = text.to_lowercase();

    let mentions_working_set = working_set_paths.iter().any(|p| text.contains(p));
    if mentions_working_set {
        return true;
    }

    let error_markers = [
        "error:",
        "error ",
        "failed",
        "panic",
        "traceback",
        "stack trace",
        "assertion failed",
        "test failed",
    ];
    if error_markers.iter().any(|m| lower.contains(m)) {
        return true;
    }

    let patch_markers = [
        "diff --git",
        "+++ b/",
        "--- a/",
        "*** begin patch",
        "*** update file:",
        "*** add file:",
        "*** delete file:",
        "```diff",
        "apply_patch",
    ];
    patch_markers.iter().any(|m| lower.contains(m))
}

fn plan_compaction(
    messages: &[Message],
    workspace: Option<&Path>,
    keep_recent: usize,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> CompactionPlan {
    let mut pinned_indices: BTreeSet<usize> = BTreeSet::new();
    let len = messages.len();
    if len == 0 {
        return CompactionPlan::default();
    }

    // Always pin the tail of the conversation to preserve immediate context.
    let recent_start = len.saturating_sub(keep_recent);
    pinned_indices.extend(recent_start..len);

    // Derive a repo-aware working set from recent messages/tool calls and
    // merge it with any externally provided working-set paths.
    let seed_indices = external_pins.unwrap_or(&[]);
    let mut working_set_paths = derive_working_set_paths(messages, workspace, seed_indices);
    if let Some(paths) = external_working_set_paths {
        for path in paths {
            if let Some(normalized) = normalize_path_candidate(path, workspace) {
                let _ = working_set_paths.insert(normalized);
            }
        }
    }

    for (idx, msg) in messages.iter().enumerate() {
        if pinned_indices.contains(&idx) {
            continue;
        }
        let text = message_text(msg);
        if should_pin_message(&text, &working_set_paths) {
            pinned_indices.insert(idx);
        }
    }

    // External pins are authoritative and should be preserved even if they
    // were not detected by the heuristics above.
    if let Some(pins) = external_pins {
        pinned_indices.extend(pins.iter().copied().filter(|idx| *idx < len));
    }

    // Ensure tool result messages are not kept without their corresponding tool call.
    enforce_tool_call_pairs(messages, &mut pinned_indices);

    let summarize_indices = (0..len)
        .filter(|idx| !pinned_indices.contains(idx))
        .collect();

    CompactionPlan {
        pinned_indices,
        summarize_indices,
        working_set_paths,
    }
}

fn enforce_tool_call_pairs(messages: &[Message], pinned_indices: &mut BTreeSet<usize>) {
    if pinned_indices.is_empty() {
        return;
    }

    // Build maps: tool_id → message index across ALL messages (not just pinned).
    let mut call_id_to_idx: HashMap<String, usize> = HashMap::new();
    let mut result_id_to_idx: HashMap<String, usize> = HashMap::new();

    for (idx, msg) in messages.iter().enumerate() {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    call_id_to_idx.insert(id.clone(), idx);
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    result_id_to_idx.insert(tool_use_id.clone(), idx);
                }
                _ => {}
            }
        }
    }

    // Fixpoint loop: re-check until stable.
    // Newly pinned messages may introduce new pair requirements;
    // removed messages may orphan their counterparts.
    // Track permanently removed indices so they cannot be re-added
    // by a counterpart in a later iteration (prevents oscillation).
    let mut permanently_removed: HashSet<usize> = HashSet::new();

    let max_iters = messages.len().max(10);
    let mut converged = false;
    for _ in 0..max_iters {
        let mut to_add = Vec::new();
        let mut to_remove = Vec::new();

        let snapshot: Vec<usize> = pinned_indices.iter().copied().collect();

        for idx in snapshot {
            let msg = &messages[idx];
            for block in &msg.content {
                match block {
                    // Pinned result → its call must also be pinned (or remove result)
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        match call_id_to_idx.get(tool_use_id) {
                            Some(&call_idx) if !permanently_removed.contains(&call_idx) => {
                                to_add.push(call_idx);
                            }
                            _ => {
                                to_remove.push(idx);
                            }
                        }
                    }
                    // Pinned call → its result must also be pinned (or remove call)
                    ContentBlock::ToolUse { id, .. } => match result_id_to_idx.get(id) {
                        Some(&result_idx) if !permanently_removed.contains(&result_idx) => {
                            to_add.push(result_idx);
                        }
                        _ => {
                            to_remove.push(idx);
                        }
                    },
                    _ => {}
                }
            }
        }

        // Removals take priority: if a message is both needed and orphaned,
        // remove it now; the fixpoint loop will cascade the orphaning.
        let remove_set: HashSet<usize> = to_remove.iter().copied().collect();
        let mut changed = false;
        for idx in to_add {
            if !remove_set.contains(&idx) && pinned_indices.insert(idx) {
                changed = true;
            }
        }
        for idx in to_remove {
            if pinned_indices.remove(&idx) {
                permanently_removed.insert(idx);
                changed = true;
            }
        }

        if !changed {
            converged = true;
            break;
        }
    }
    if !converged {
        logging::warn(format!(
            "enforce_tool_call_pairs did not converge after {max_iters} iterations \
             ({} messages, {} pinned)",
            messages.len(),
            pinned_indices.len()
        ));
    }
}

fn estimate_tokens_for_message(message: &Message) -> usize {
    message
        .content
        .iter()
        .map(|c| match c {
            ContentBlock::Text { text, .. } => text.len() / 4,
            ContentBlock::Thinking { thinking } => thinking.len() / 4,
            ContentBlock::ToolUse { input, .. } => serde_json::to_string(input)
                .map(|s| s.len() / 4)
                .unwrap_or(100),
            ContentBlock::ToolResult { content, .. } => content.len() / 4,
        })
        .sum::<usize>()
}

pub fn estimate_tokens(messages: &[Message]) -> usize {
    // Rough estimate: ~4 chars per token
    messages.iter().map(estimate_tokens_for_message).sum()
}

pub fn should_compact(
    messages: &[Message],
    config: &CompactionConfig,
    workspace: Option<&Path>,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> bool {
    if !config.enabled {
        return false;
    }

    let plan = plan_compaction(
        messages,
        workspace,
        KEEP_RECENT_MESSAGES,
        external_pins,
        external_working_set_paths,
    );
    let pinned_tokens: usize = plan
        .pinned_indices
        .iter()
        .map(|&idx| estimate_tokens_for_message(&messages[idx]))
        .sum();
    let pinned_count = plan.pinned_indices.len();

    let token_estimate: usize = plan
        .summarize_indices
        .iter()
        .map(|&idx| estimate_tokens_for_message(&messages[idx]))
        .sum();
    let message_count = plan.summarize_indices.len();

    // Pinned messages consume part of the budget, so compact earlier when needed.
    let effective_token_threshold = config.token_threshold.saturating_sub(pinned_tokens);
    let effective_message_threshold = config.message_threshold.saturating_sub(pinned_count);

    let enough_unpinned = message_count >= MIN_SUMMARIZE_MESSAGES
        || effective_token_threshold == 0
        || effective_message_threshold == 0;
    if !enough_unpinned {
        return false;
    }

    token_estimate > effective_token_threshold || message_count > effective_message_threshold
}

fn truncate_chars(text: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => &text[..idx],
        None => text,
    }
}

/// Result of a compaction operation with metadata.
#[derive(Debug)]
pub struct CompactionResult {
    /// Compacted messages
    pub messages: Vec<Message>,
    /// Summary system prompt
    pub summary_prompt: Option<SystemPrompt>,
    /// Number of retries used before success
    pub retries_used: u32,
}

/// Check if an error is transient and worth retrying.
fn is_transient_error(e: &anyhow::Error) -> bool {
    let msg = e.to_string().to_lowercase();
    msg.contains("timeout")
        || msg.contains("timed out")
        || msg.contains("connection")
        || msg.contains("rate limit")
        || msg.contains("too many requests")
        || msg.contains("503")
        || msg.contains("502")
        || msg.contains("429")
        || msg.contains("network")
        || msg.contains("temporarily unavailable")
}

/// Compact messages with retry and backoff for transient errors.
///
/// This function wraps `compact_messages` with retry logic to handle
/// transient network errors and rate limits. It uses exponential backoff
/// with delays of 1s, 2s, 4s between retries.
///
/// # Safety
/// - Never panics
/// - Never corrupts the original messages (returns error instead)
/// - Only retries on transient errors (network, rate limit, etc.)
pub async fn compact_messages_safe(
    client: &DeepSeekClient,
    messages: &[Message],
    config: &CompactionConfig,
    workspace: Option<&Path>,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> Result<CompactionResult> {
    const MAX_RETRIES: u32 = 3;
    const BASE_DELAY_MS: u64 = 1000;

    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            // Exponential backoff: 1s, 2s, 4s
            let delay = Duration::from_millis(BASE_DELAY_MS * (1 << (attempt - 1)));
            tokio::time::sleep(delay).await;
        }

        match compact_messages(
            client,
            messages,
            config,
            workspace,
            external_pins,
            external_working_set_paths,
        )
        .await
        {
            Ok((msgs, prompt)) => {
                return Ok(CompactionResult {
                    messages: msgs,
                    summary_prompt: prompt,
                    retries_used: attempt,
                });
            }
            Err(e) => {
                // Only retry on transient errors
                if !is_transient_error(&e) {
                    return Err(e);
                }
                last_error = Some(e);
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("Compaction failed after {MAX_RETRIES} retries")))
}

pub async fn compact_messages(
    client: &DeepSeekClient,
    messages: &[Message],
    config: &CompactionConfig,
    workspace: Option<&Path>,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> Result<(Vec<Message>, Option<SystemPrompt>)> {
    if messages.is_empty() {
        return Ok((Vec::new(), None));
    }

    let plan = plan_compaction(
        messages,
        workspace,
        KEEP_RECENT_MESSAGES,
        external_pins,
        external_working_set_paths,
    );
    if plan.summarize_indices.is_empty() {
        return Ok((messages.to_vec(), None));
    }

    let to_summarize: Vec<Message> = plan
        .summarize_indices
        .iter()
        .map(|&idx| messages[idx].clone())
        .collect();

    // Create a summary of the unpinned portion of the conversation
    let summary = create_summary(client, &to_summarize, &config.model).await?;

    // Build new message list with summary as system block
    let summary_block = SystemBlock {
        block_type: "text".to_string(),
        text: format!(
            "## Conversation Summary\n\nThe following summarizes earlier context that was not pinned to the working set:\n\n{summary}\n\n---\nPinned messages follow:"
        ),
        cache_control: if config.cache_summary {
            Some(CacheControl {
                cache_type: "ephemeral".to_string(),
            })
        } else {
            None
        },
    };

    let pinned_messages = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, msg)| plan.pinned_indices.contains(&idx).then_some(msg.clone()))
        .collect();

    Ok((
        pinned_messages,
        Some(SystemPrompt::Blocks(vec![summary_block])),
    ))
}

async fn create_summary(
    client: &DeepSeekClient,
    messages: &[Message],
    model: &str,
) -> Result<String> {
    // Format messages for summarization
    let mut conversation_text = String::new();
    for msg in messages {
        let role = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        for block in &msg.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    let _ = write!(conversation_text, "{role}: {text}\n\n");
                }
                ContentBlock::ToolUse { name, .. } => {
                    let _ = write!(conversation_text, "{role}: [Used tool: {name}]\n\n");
                }
                ContentBlock::ToolResult { content, .. } => {
                    let snippet = truncate_chars(content, 500);
                    let _ = write!(conversation_text, "Tool result: {}\n\n", snippet);
                }
                ContentBlock::Thinking { .. } => {
                    // Skip thinking blocks in summary
                }
            }
        }
    }

    let request = MessageRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: format!(
                    "Summarize the following conversation in a concise but comprehensive way. \
                     Preserve key information, decisions made, and any important context. \
                     Keep it under 500 words.\n\n---\n\n{conversation_text}"
                ),
                cache_control: None,
            }],
        }],
        max_tokens: 1024,
        system: Some(SystemPrompt::Text(
            "You are a helpful assistant that creates concise conversation summaries.".to_string(),
        )),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: Some(false),
        temperature: Some(0.3),
        top_p: None,
    };

    let response = client.create_message(request).await?;

    // Extract text from response
    let summary = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(summary)
}

pub fn merge_system_prompts(
    original: Option<&SystemPrompt>,
    summary: Option<SystemPrompt>,
) -> Option<SystemPrompt> {
    match (original, summary) {
        (None, None) => None,
        (Some(orig), None) => Some(orig.clone()),
        (None, Some(sum)) => Some(sum),
        (Some(SystemPrompt::Text(orig_text)), Some(SystemPrompt::Blocks(mut sum_blocks))) => {
            // Prepend original system prompt
            sum_blocks.insert(
                0,
                SystemBlock {
                    block_type: "text".to_string(),
                    text: orig_text.clone(),
                    cache_control: None,
                },
            );
            Some(SystemPrompt::Blocks(sum_blocks))
        }
        (Some(SystemPrompt::Blocks(orig_blocks)), Some(SystemPrompt::Blocks(mut sum_blocks))) => {
            // Prepend original blocks
            for (i, block) in orig_blocks.iter().enumerate() {
                sum_blocks.insert(i, block.clone());
            }
            Some(SystemPrompt::Blocks(sum_blocks))
        }
        (Some(orig), Some(SystemPrompt::Text(sum_text))) => {
            let mut blocks = match orig {
                SystemPrompt::Text(t) => vec![SystemBlock {
                    block_type: "text".to_string(),
                    text: t.clone(),
                    cache_control: None,
                }],
                SystemPrompt::Blocks(b) => b.clone(),
            };
            blocks.push(SystemBlock {
                block_type: "text".to_string(),
                text: sum_text,
                cache_control: None,
            });
            Some(SystemPrompt::Blocks(blocks))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    #[test]
    fn truncate_chars_respects_unicode_boundaries() {
        let text = "abc😀é";
        assert_eq!(truncate_chars(text, 0), "");
        assert_eq!(truncate_chars(text, 1), "a");
        assert_eq!(truncate_chars(text, 3), "abc");
        assert_eq!(truncate_chars(text, 4), "abc😀");
        assert_eq!(truncate_chars(text, 5), "abc😀é");
    }

    #[test]
    fn is_transient_error_detects_network_issues() {
        let timeout_err = anyhow::anyhow!("Connection timeout");
        assert!(is_transient_error(&timeout_err));

        let rate_limit_err = anyhow::anyhow!("429 Too Many Requests");
        assert!(is_transient_error(&rate_limit_err));

        let service_err = anyhow::anyhow!("503 Service Unavailable");
        assert!(is_transient_error(&service_err));

        let network_err = anyhow::anyhow!("network error: connection refused");
        assert!(is_transient_error(&network_err));
    }

    #[test]
    fn is_transient_error_rejects_permanent_errors() {
        let auth_err = anyhow::anyhow!("401 Unauthorized: Invalid API key");
        assert!(!is_transient_error(&auth_err));

        let parse_err = anyhow::anyhow!("Failed to parse JSON response");
        assert!(!is_transient_error(&parse_err));

        let validation_err = anyhow::anyhow!("Invalid request: missing required field");
        assert!(!is_transient_error(&validation_err));
    }

    #[test]
    fn estimate_tokens_empty_messages() {
        let messages: Vec<Message> = vec![];
        assert_eq!(estimate_tokens(&messages), 0);
    }

    #[test]
    fn estimate_tokens_with_text() {
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "Hello, world!".to_string(), // 13 chars = ~3 tokens
                cache_control: None,
            }],
        }];
        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0 && tokens < 10);
    }

    #[test]
    fn should_compact_respects_enabled_flag() {
        let config = CompactionConfig {
            enabled: false,
            ..Default::default()
        };
        // Even with many messages, disabled compaction should return false
        let messages: Vec<Message> = (0..100)
            .map(|_| Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "test".to_string(),
                    cache_control: None,
                }],
            })
            .collect();
        assert!(!should_compact(&messages, &config, None, None, None));
    }

    #[test]
    fn should_compact_respects_message_threshold() {
        let config = CompactionConfig {
            enabled: true,
            token_threshold: 1_000_000, // Very high
            message_threshold: 5,
            ..Default::default()
        };

        // Under threshold
        let few_messages: Vec<Message> = (0..4)
            .map(|_| Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "x".to_string(),
                    cache_control: None,
                }],
            })
            .collect();
        assert!(!should_compact(&few_messages, &config, None, None, None));

        // Over threshold
        let many_messages: Vec<Message> = (0..10)
            .map(|_| Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "x".to_string(),
                    cache_control: None,
                }],
            })
            .collect();
        assert!(should_compact(&many_messages, &config, None, None, None));
    }

    #[test]
    fn plan_compaction_pins_recent_and_working_set_paths() {
        let messages = vec![
            msg("user", "General discussion"),
            msg("assistant", "Unrelated note"),
            msg("user", "Earlier we touched src/core/engine.rs"),
            msg("assistant", "More unrelated chatter"),
            msg("user", "Let's keep working on src/core/engine.rs"),
            msg("assistant", "Tool output mentions src/core/engine.rs too"),
            msg("assistant", "Recent reasoning"),
            msg("user", "Final recent instruction"),
        ];

        let plan = plan_compaction(&messages, None, KEEP_RECENT_MESSAGES, None, None);

        assert!(plan.pinned_indices.contains(&2));
        for idx in 4..messages.len() {
            assert!(plan.pinned_indices.contains(&idx));
        }
        assert!(plan.summarize_indices.contains(&0));
        assert!(plan.summarize_indices.contains(&1));
        assert!(plan.summarize_indices.contains(&3));
    }

    #[test]
    fn plan_compaction_respects_external_pins() {
        let messages = vec![
            msg("user", "noise 0"),
            msg("assistant", "noise 1"),
            msg("user", "noise 2"),
            msg("assistant", "noise 3"),
            msg("user", "recent 4"),
            msg("assistant", "recent 5"),
            msg("assistant", "recent 6"),
            msg("user", "recent 7"),
        ];

        let pins = vec![1usize];
        let plan = plan_compaction(&messages, None, KEEP_RECENT_MESSAGES, Some(&pins), None);

        assert!(plan.pinned_indices.contains(&1));
        assert!(!plan.summarize_indices.contains(&1));
    }

    #[test]
    fn plan_compaction_uses_external_working_set_paths() {
        let mut messages = vec![msg("user", "edit src/core/engine.rs now")];
        messages.extend((1..20).map(|i| msg("assistant", &format!("noise {i}"))));

        let working_set_paths = vec!["src/core/engine.rs".to_string()];
        let plan = plan_compaction(
            &messages,
            None,
            KEEP_RECENT_MESSAGES,
            None,
            Some(&working_set_paths),
        );

        assert!(plan.pinned_indices.contains(&0));
    }

    #[test]
    fn plan_compaction_pins_tool_calls_for_tool_results() {
        let messages = vec![
            msg("user", "noise"),
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path": "src/main.rs"}),
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "ok src/main.rs".to_string(),
                }],
            },
        ];

        let plan = plan_compaction(&messages, None, 1, None, None);
        assert!(plan.pinned_indices.contains(&2));
        assert!(plan.pinned_indices.contains(&1));
    }

    #[test]
    fn should_compact_ignores_fully_pinned_context() {
        let config = CompactionConfig {
            enabled: true,
            token_threshold: 10,
            message_threshold: 2,
            ..Default::default()
        };

        let messages: Vec<Message> = (0..12)
            .map(|_| msg("user", "Work on src/compaction.rs right now"))
            .collect();

        assert!(!should_compact(&messages, &config, None, None, None));
    }

    #[test]
    fn should_compact_counts_only_unpinned_messages() {
        let config = CompactionConfig {
            enabled: true,
            token_threshold: 1_000_000,
            message_threshold: 5,
            ..Default::default()
        };

        let mut messages: Vec<Message> = (0..7)
            .map(|i| msg("user", &format!("noise message {i}")))
            .collect();
        messages.push(msg("user", "Focus on src/core/engine.rs"));
        messages.extend((0..4).map(|i| msg("assistant", &format!("recent {i}"))));

        assert!(should_compact(&messages, &config, None, None, None));
    }

    #[test]
    fn should_compact_when_pins_consume_budget() {
        let config = CompactionConfig {
            enabled: true,
            token_threshold: 50,
            message_threshold: 50,
            ..Default::default()
        };

        let mut messages = vec![msg("user", "noise 0"), msg("assistant", "noise 1")];
        messages.extend((0..4).map(|_| {
            msg(
                "assistant",
                &format!("{} src/core/engine.rs", "x".repeat(400)),
            )
        }));

        // Pinned recent messages exceed the token budget, so unpinned noise should trigger compaction.
        assert!(should_compact(&messages, &config, None, None, None));
    }

    #[test]
    fn enforce_tool_call_pairs_removes_orphaned_tool_call() {
        // An assistant message with a tool call but no matching result anywhere
        // in the history should be removed from the pinned set.
        let messages = vec![
            msg("user", "noise"),
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "orphan-call".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path": "src/main.rs"}),
                }],
            },
            msg("assistant", "recent"),
        ];

        let mut pinned = BTreeSet::from([0, 1, 2]);
        enforce_tool_call_pairs(&messages, &mut pinned);

        // The orphaned tool call message (index 1) should be removed.
        assert!(
            !pinned.contains(&1),
            "orphaned tool call should be removed from pinned set"
        );
        // Other messages stay.
        assert!(pinned.contains(&0));
        assert!(pinned.contains(&2));
    }

    #[test]
    fn enforce_tool_call_pairs_removes_orphaned_tool_result() {
        // A tool result whose call doesn't exist anywhere should be removed.
        let messages = vec![
            msg("user", "noise"),
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "orphan-result".to_string(),
                    content: "ok".to_string(),
                }],
            },
            msg("assistant", "recent"),
        ];

        let mut pinned = BTreeSet::from([0, 1, 2]);
        enforce_tool_call_pairs(&messages, &mut pinned);

        assert!(
            !pinned.contains(&1),
            "orphaned tool result should be removed from pinned set"
        );
        assert!(pinned.contains(&0));
        assert!(pinned.contains(&2));
    }

    #[test]
    fn enforce_tool_call_pairs_preserves_valid_pairs() {
        // A complete call+result pair should remain intact.
        let messages = vec![
            msg("user", "do something"),
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-ok".to_string(),
                    name: "list_dir".to_string(),
                    input: json!({}),
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-ok".to_string(),
                    content: "files here".to_string(),
                }],
            },
            msg("assistant", "done"),
        ];

        let mut pinned = BTreeSet::from([1, 2, 3]);
        enforce_tool_call_pairs(&messages, &mut pinned);

        assert!(pinned.contains(&1), "tool call should stay pinned");
        assert!(pinned.contains(&2), "tool result should stay pinned");
        assert!(pinned.contains(&3));
    }

    #[test]
    fn enforce_tool_call_pairs_pins_transitive_pairs() {
        // If only the result is initially pinned, the call should be pulled in.
        // The call message may also contain another tool call whose result should
        // then be pulled in transitively.
        let messages = vec![
            msg("user", "start"),
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: "t1".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "a.rs"}),
                    },
                    ContentBlock::ToolUse {
                        id: "t2".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "b.rs"}),
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "content of a.rs".to_string(),
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t2".to_string(),
                    content: "content of b.rs".to_string(),
                }],
            },
            msg("assistant", "done"),
        ];

        // Only pin the result for t1 initially.
        let mut pinned = BTreeSet::from([2, 4]);
        enforce_tool_call_pairs(&messages, &mut pinned);

        // The call message (index 1) should be pulled in because t1's result is pinned.
        assert!(
            pinned.contains(&1),
            "call message should be transitively pinned"
        );
        // Since the call message also contains t2, t2's result (index 3) should also be pinned.
        assert!(
            pinned.contains(&3),
            "t2 result should be transitively pinned via the call message"
        );
    }

    #[test]
    fn enforce_tool_call_pairs_cascading_removal() {
        // Removing an orphaned call should cascade to remove its result.
        // Message 1: assistant with t1 (call) — t1 has a result at index 2
        // Message 2: user with t1 (result)
        // Message 3: assistant with t2 (call) — t2 has NO result
        // Message 4: user with t2 result referencing the call
        //
        // If t2 has no result in history, message 3 is removed. That's straightforward.
        // Here we test: if a call message is removed because ONE of its calls is orphaned,
        // the result for the other call also gets removed in subsequent iterations.
        let messages = vec![
            msg("user", "start"),
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: "good".to_string(),
                        name: "read_file".to_string(),
                        input: json!({}),
                    },
                    ContentBlock::ToolUse {
                        id: "orphan".to_string(),
                        name: "shell".to_string(),
                        input: json!({}),
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "good".to_string(),
                    content: "ok".to_string(),
                }],
            },
            // Note: NO result for "orphan" exists anywhere
            msg("assistant", "done"),
        ];

        let mut pinned = BTreeSet::from([1, 2, 3]);
        enforce_tool_call_pairs(&messages, &mut pinned);

        // Message 1 has an orphaned tool call ("orphan"), so it's removed.
        assert!(
            !pinned.contains(&1),
            "message with orphaned call should be removed"
        );
        // Message 2 (result for "good") now has no matching call pinned, so it's also removed.
        assert!(
            !pinned.contains(&2),
            "result whose call was removed should cascade-remove"
        );
        // Message 3 (plain text) stays.
        assert!(pinned.contains(&3));
    }

    #[test]
    fn enforce_tool_call_pairs_converges_long_chain() {
        let mut messages = vec![msg("user", "start")];
        for i in 0..15 {
            messages.push(Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: format!("t{i}"),
                    name: "read_file".to_string(),
                    input: json!({}),
                }],
            });
            messages.push(Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: format!("t{i}"),
                    content: format!("result {i}"),
                }],
            });
        }
        messages.push(msg("assistant", "done"));

        let mut pinned: BTreeSet<usize> = (0..messages.len()).collect();
        enforce_tool_call_pairs(&messages, &mut pinned);

        // All pairs should remain intact (no orphans)
        assert_eq!(pinned.len(), messages.len());
    }
}
