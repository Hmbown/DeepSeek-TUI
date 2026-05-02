use serde_json::{Map, Value};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MAX_STRING_VISIBLE: usize = 36;
const MAX_INLINE_STRING_VISIBLE: usize = 24;
const MAX_COLLECTION_ITEMS: usize = 3;
const MAX_NESTED_DEPTH: usize = 2;

pub(crate) fn summarize_value(value: &Value) -> String {
    summarize_value_impl(value, 0, false)
}

pub(crate) fn summarize_inline_value(value: &Value) -> String {
    summarize_value_impl(value, 0, true)
}

pub(crate) fn summarize_value_with_limit(value: &Value, max_visible: usize) -> String {
    summarize_value_limited_impl(value, 0, false, max_visible)
}

pub(crate) fn summarize_inline_value_with_limit(value: &Value, max_visible: usize) -> String {
    summarize_value_limited_impl(value, 0, true, max_visible)
}

fn summarize_value_impl(value: &Value, depth: usize, inline_string: bool) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(num) => num.to_string(),
        Value::String(text) => summarize_string(text, inline_string),
        Value::Array(items) => summarize_array(items, depth),
        Value::Object(map) => summarize_object(map, depth),
    }
}

fn summarize_array(items: &[Value], depth: usize) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    if depth >= MAX_NESTED_DEPTH {
        return format!("[{} items]", items.len());
    }

    let mut parts = items
        .iter()
        .take(MAX_COLLECTION_ITEMS)
        .map(|value| summarize_value_impl(value, depth + 1, true))
        .collect::<Vec<_>>();
    if items.len() > MAX_COLLECTION_ITEMS {
        parts.push("…".to_string());
    }
    truncate_visible(&format!("[{}]", parts.join(", ")), MAX_STRING_VISIBLE)
}

fn summarize_object(map: &Map<String, Value>, depth: usize) -> String {
    if map.is_empty() {
        return "{}".to_string();
    }
    if depth >= MAX_NESTED_DEPTH {
        return format!("{{{} keys}}", map.len());
    }

    let mut parts = map
        .iter()
        .take(MAX_COLLECTION_ITEMS)
        .map(|(key, value)| format!("{key}: {}", summarize_value_impl(value, depth + 1, true)))
        .collect::<Vec<_>>();
    if map.len() > MAX_COLLECTION_ITEMS {
        parts.push("…".to_string());
    }
    truncate_visible(&format!("{{ {} }}", parts.join(", ")), MAX_STRING_VISIBLE)
}

fn summarize_string(text: &str, inline: bool) -> String {
    if inline {
        truncate_visible(text, MAX_INLINE_STRING_VISIBLE)
    } else {
        format!("\"{}\"", truncate_display_string(text))
    }
}

fn truncate_display_string(text: &str) -> String {
    const TRUNCATE_TO: usize = MAX_STRING_VISIBLE - 12;
    if UnicodeWidthStr::width(text) > MAX_STRING_VISIBLE {
        let mut truncated = String::new();
        for ch in text.chars().take(TRUNCATE_TO) {
            truncated.push(ch);
        }
        truncated.push('…');
        truncated
    } else {
        text.to_string()
    }
}

pub(crate) fn truncate_visible_text(text: &str, max_visible: usize) -> String {
    truncate_visible(text, max_visible)
}

fn summarize_value_limited_impl(
    value: &Value,
    depth: usize,
    inline_string: bool,
    max_visible: usize,
) -> String {
    if max_visible == 0 {
        return String::new();
    }

    match value {
        Value::Null => truncate_visible("null", max_visible),
        Value::Bool(flag) => truncate_visible(&flag.to_string(), max_visible),
        Value::Number(num) => truncate_visible(&num.to_string(), max_visible),
        Value::String(text) => summarize_string_limited(text, inline_string, max_visible),
        Value::Array(items) => summarize_array_limited(items, depth, max_visible),
        Value::Object(map) => summarize_object_limited(map, depth, max_visible),
    }
}

fn summarize_array_limited(items: &[Value], depth: usize, max_visible: usize) -> String {
    if items.is_empty() {
        return truncate_visible("[]", max_visible);
    }
    if depth >= MAX_NESTED_DEPTH {
        return truncate_visible(&format!("[{} items]", items.len()), max_visible);
    }

    let entries = items.iter().take(MAX_COLLECTION_ITEMS).collect::<Vec<_>>();
    let hidden = items.len().saturating_sub(entries.len());
    summarize_sequence_limited(
        "[",
        "]",
        ", ",
        entries.len(),
        hidden,
        max_visible,
        |index, budget| summarize_value_limited_impl(entries[index], depth + 1, true, budget),
    )
}

fn summarize_object_limited(map: &Map<String, Value>, depth: usize, max_visible: usize) -> String {
    if map.is_empty() {
        return truncate_visible("{}", max_visible);
    }
    if depth >= MAX_NESTED_DEPTH {
        return truncate_visible(&format!("{{{} keys}}", map.len()), max_visible);
    }

    let entries = map
        .iter()
        .take(MAX_COLLECTION_ITEMS)
        .map(|(key, value)| (key.as_str(), value))
        .collect::<Vec<_>>();
    let hidden = map.len().saturating_sub(entries.len());

    summarize_sequence_limited(
        "{ ",
        " }",
        ", ",
        entries.len(),
        hidden,
        max_visible,
        |index, budget| {
            let (key, value) = entries[index];
            let prefix = format!("{key}: ");
            let prefix_width = visible_width(&prefix);
            if prefix_width >= budget {
                return truncate_visible(&prefix, budget);
            }
            let value_budget = budget.saturating_sub(prefix_width);
            let summary = summarize_value_limited_impl(value, depth + 1, true, value_budget);
            format!("{prefix}{summary}")
        },
    )
}

fn summarize_sequence_limited<F>(
    open: &str,
    close: &str,
    separator: &str,
    total_entries: usize,
    hidden_entries: usize,
    max_visible: usize,
    mut render_item: F,
) -> String
where
    F: FnMut(usize, usize) -> String,
{
    let open_width = visible_width(open);
    let close_width = visible_width(close);
    let separator_width = visible_width(separator);
    let ellipsis_width = visible_width("…");

    if max_visible <= open_width + close_width {
        let compact = format!(
            "{}{}",
            open.trim_end_matches(' '),
            close.trim_start_matches(' ')
        );
        return truncate_visible(&compact, max_visible);
    }

    let full_items = (0..total_entries)
        .map(|index| render_item(index, max_visible))
        .collect::<Vec<_>>();
    let full_widths = full_items
        .iter()
        .map(|item| visible_width(item))
        .collect::<Vec<_>>();

    let mut visible_entries = total_entries;
    while visible_entries > 0 {
        let show_ellipsis = hidden_entries > 0 || visible_entries < total_entries;
        let ellipsis_block_width = if show_ellipsis {
            separator_width + ellipsis_width
        } else {
            0
        };
        let fixed_width = open_width
            + close_width
            + separator_width * visible_entries.saturating_sub(1)
            + ellipsis_block_width;
        let min_value_width = full_widths
            .iter()
            .take(visible_entries)
            .filter(|width| **width > 0)
            .count();

        if fixed_width + min_value_width <= max_visible {
            let available_values = max_visible.saturating_sub(fixed_width);
            let budgets =
                allocate_visible_budgets(&full_widths[..visible_entries], available_values);
            let mut out = String::from(open);
            for index in 0..visible_entries {
                if index > 0 {
                    out.push_str(separator);
                }
                let item = if budgets[index] >= full_widths[index] {
                    full_items[index].clone()
                } else {
                    render_item(index, budgets[index])
                };
                out.push_str(&item);
            }
            if show_ellipsis {
                if visible_entries > 0 {
                    out.push_str(separator);
                }
                out.push('…');
            }
            out.push_str(close);
            return truncate_visible(&out, max_visible);
        }

        visible_entries -= 1;
    }

    truncate_visible(&format!("{open}…{close}"), max_visible)
}

fn allocate_visible_budgets(full_widths: &[usize], available: usize) -> Vec<usize> {
    if full_widths.is_empty() {
        return Vec::new();
    }

    let mut budgets = vec![0usize; full_widths.len()];
    let mut remaining_available = available;
    let mut remaining_items = full_widths.len();

    for (index, full_width) in full_widths.iter().enumerate() {
        if remaining_items == 0 {
            break;
        }
        let share = remaining_available / remaining_items;
        let minimum = usize::from(*full_width > 0);
        let budget = (*full_width).min(share.max(minimum));
        budgets[index] = budget;
        remaining_available = remaining_available.saturating_sub(budget);
        remaining_items -= 1;
    }

    for (budget, full_width) in budgets.iter_mut().zip(full_widths.iter()) {
        if remaining_available == 0 {
            break;
        }
        let extra = full_width.saturating_sub(*budget).min(remaining_available);
        *budget += extra;
        remaining_available -= extra;
    }

    budgets
}

fn summarize_string_limited(text: &str, inline: bool, max_visible: usize) -> String {
    if inline {
        truncate_visible(text, max_visible)
    } else if max_visible <= 2 {
        truncate_visible("\"\"", max_visible)
    } else {
        let inner = truncate_visible(text, max_visible.saturating_sub(2));
        format!("\"{inner}\"")
    }
}

fn visible_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn truncate_visible(text: &str, max_visible: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_visible {
        return text.to_string();
    }

    let mut out = String::new();
    let mut width = 0usize;
    let limit = max_visible.saturating_sub(1);
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > limit {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push('…');
    out
}
