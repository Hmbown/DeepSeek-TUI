//! Output truncation and summarization helpers for shell tools.

/// Maximum output size before truncation (30KB like Claude Code).
const MAX_OUTPUT_SIZE: usize = 30_000;
/// Limits for summary strings in tool metadata.
const SUMMARY_MAX_LINES: usize = 3;
const SUMMARY_MAX_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TruncationMeta {
    pub(crate) original_len: usize,
    pub(crate) omitted: usize,
    pub(crate) truncated: bool,
}

pub(crate) fn truncate_with_meta(output: &str) -> (String, TruncationMeta) {
    let original_len = output.len();
    if original_len <= MAX_OUTPUT_SIZE {
        return (
            output.to_string(),
            TruncationMeta {
                original_len,
                omitted: 0,
                truncated: false,
            },
        );
    }

    let cut_index = char_boundary_at_or_before(output, MAX_OUTPUT_SIZE);
    let truncated = &output[..cut_index];
    let omitted = original_len.saturating_sub(cut_index);
    let note =
        format!("...\n\n[Output truncated at {MAX_OUTPUT_SIZE} bytes. {omitted} bytes omitted.]");

    (
        format!("{truncated}{note}"),
        TruncationMeta {
            original_len,
            omitted,
            truncated: true,
        },
    )
}

fn char_boundary_at_or_before(text: &str, max_bytes: usize) -> usize {
    if max_bytes >= text.len() {
        return text.len();
    }

    let mut last_end = 0usize;
    for (idx, ch) in text.char_indices() {
        let end = idx.saturating_add(ch.len_utf8());
        if end > max_bytes {
            break;
        }
        last_end = end;
    }

    last_end.min(text.len())
}

fn strip_truncation_note(text: &str) -> &str {
    text.split_once("\n\n[Output truncated at")
        .map_or(text, |(prefix, _)| prefix)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut end = text.len();
    for (count, (idx, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            end = idx;
            break;
        }
    }

    format!("{}...", &text[..end])
}

pub(crate) fn summarize_output(text: &str) -> String {
    let stripped = strip_truncation_note(text);
    let summary = stripped
        .lines()
        .take(SUMMARY_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if summary.is_empty() {
        String::new()
    } else {
        truncate_chars(&summary, SUMMARY_MAX_CHARS)
    }
}

/// Truncate output to `MAX_OUTPUT_SIZE`.
pub(crate) fn truncate_output(output: &str) -> String {
    truncate_with_meta(output).0
}
