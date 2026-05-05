//! Deterministic JSON argument repair for malformed tool-call inputs.
//!
//! DeepSeek streams `tool_calls.function.arguments` as deltas. Two failure
//! shapes are common: (a) SSE chunk boundary cuts inside a JSON string and
//! reassembly leaves a trailing comma or unclosed brace; (b) some local
//! backends emit literal control characters inside JSON string values.
//!
//! The repair ladder runs five stages before falling back to an empty object:
//!
//!  1. Strict parse — done if it parses.
//!  2. Strip literal control chars inside string values.
//!  3. Strip trailing commas before `}` or `]`.
//!  4. Balance braces/brackets (append closers).
//!  5. Strip excess closers if delta is negative.
//!  6. Fallback: empty object `{}`.

use serde_json::{Map, Value};

/// Maximum raw argument length we'll attempt to repair (1 MiB).
const MAX_ARG_LEN: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ArgRepairError {
    #[error("argument exceeded {0} chars; refusing to repair")]
    TooLarge(usize),
}

/// Repair a raw JSON argument string into a valid `serde_json::Value`.
///
/// Runs the deterministic ladder; on success returns the parsed value.
/// The final fallback is an empty object `{}` so dispatch always proceeds.
pub fn repair(raw: &str) -> Result<Value, ArgRepairError> {
    if raw.len() > MAX_ARG_LEN {
        return Err(ArgRepairError::TooLarge(raw.len()));
    }
    // Stage 1: strict parse
    if let Ok(v) = serde_json::from_str(raw) {
        return Ok(v);
    }
    // Stage 2: strip control chars inside strings
    let mut s = strip_control_chars_in_strings(raw);
    if let Ok(v) = serde_json::from_str(&s) {
        return Ok(v);
    }
    // Stage 3: strip trailing commas
    s = strip_trailing_commas(&s);
    if let Ok(v) = serde_json::from_str(&s) {
        return Ok(v);
    }
    // Stage 4: balance braces
    s = balance_braces(&s, 50);
    if let Ok(v) = serde_json::from_str(&s) {
        return Ok(v);
    }
    // Stage 5: strip excess closers
    s = strip_excess_closers(&s);
    if let Ok(v) = serde_json::from_str(&s) {
        return Ok(v);
    }
    // Fallback: empty object
    Ok(Value::Object(Map::new()))
}

/// Strip ASCII control characters (0x00–0x1F except \t, \n, \r) that appear
/// inside JSON string values. We walk character-by-character tracking whether
/// we're inside a string (between unescaped double-quotes).
fn strip_control_chars_in_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escape = false;
    for ch in s.chars() {
        if escape {
            out.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            out.push(ch);
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            out.push(ch);
            continue;
        }
        if in_string && (ch as u32) < 0x20 && ch != '\t' && ch != '\n' && ch != '\r' {
            // Drop control characters inside strings
            continue;
        }
        out.push(ch);
    }
    out
}

/// Strip trailing commas before `}` or `]`.
fn strip_trailing_commas(s: &str) -> String {
    // Repeatedly replace ",}" and ",]" until stable (handles nested cases).
    let mut out = s.to_string();
    loop {
        let prev = out.clone();
        out = out.replace(",}", "}").replace(",]", "]");
        // Handle trailing comma at end of string
        out = out.trim_end_matches(',').to_string();
        if out == prev {
            break;
        }
    }
    out
}

/// Balance braces and brackets: count `{`/`}` and `[`/`]`, append closers if
/// positive delta (more opens than closes). Caps iterations so a
/// catastrophically broken input doesn't loop forever.
fn balance_braces(s: &str, max_iter: usize) -> String {
    let mut out = s.to_string();
    for _ in 0..max_iter {
        let brace_delta: i32 = out
            .chars()
            .map(|ch| match ch {
                '{' => 1,
                '}' => -1,
                _ => 0,
            })
            .sum();
        let bracket_delta: i32 = out
            .chars()
            .map(|ch| match ch {
                '[' => 1,
                ']' => -1,
                _ => 0,
            })
            .sum();
        if brace_delta <= 0 && bracket_delta <= 0 {
            break;
        }
        // Append needed closers in reverse order (brackets before braces
        // for correct nesting when both are unbalanced).
        for _ in 0..bracket_delta.max(0) {
            out.push(']');
        }
        for _ in 0..brace_delta.max(0) {
            out.push('}');
        }
    }
    out
}

/// Strip excess closers when the delta is negative (more closes than opens).
fn strip_excess_closers(s: &str) -> String {
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                    out.push(ch);
                }
                // else drop excess closer
            }
            ']' => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                    out.push(ch);
                }
            }
            '{' => {
                brace_depth += 1;
                out.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strict_parse_passes_through() {
        let v = repair(r#"{"path": "hello.txt"}"#).unwrap();
        assert_eq!(v, json!({"path": "hello.txt"}));
    }

    #[test]
    fn repairs_trailing_comma() {
        let v = repair(r#"{"path": "hello.txt",}"#).unwrap();
        assert_eq!(v, json!({"path": "hello.txt"}));
    }

    #[test]
    fn repairs_trailing_comma_in_array() {
        let v = repair(r#"["a", "b",]"#).unwrap();
        assert_eq!(v, json!(["a", "b"]));
    }

    #[test]
    fn repairs_missing_close_brace() {
        let v = repair(r#"{"path": "hello.txt""#).unwrap();
        assert_eq!(v, json!({"path": "hello.txt"}));
    }

    #[test]
    fn repairs_missing_close_bracket() {
        let v = repair(r#"["a", "b""#).unwrap();
        assert_eq!(v, json!(["a", "b"]));
    }

    #[test]
    fn strips_embedded_control_chars() {
        // Raw \x0B (vertical tab) inside a string value
        let raw = "{\"key\": \"val\x0Bue\"}";
        let v = repair(raw).unwrap();
        assert_eq!(v, json!({"key": "value"}));
    }

    #[test]
    fn handles_empty_string() {
        let v = repair("").unwrap();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn handles_gibberish() {
        let v = repair("not json at all").unwrap();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn balances_nested_braces() {
        let v = repair(r#"{"outer": {"inner": "val""#).unwrap();
        assert_eq!(v, json!({"outer": {"inner": "val"}}));
    }

    #[test]
    fn strips_excess_closers() {
        let v = repair(r#"{"key": "val"}}"#).unwrap();
        assert_eq!(v, json!({"key": "val"}));
    }

    #[test]
    fn handles_double_encoded_json() {
        // This is a valid JSON string containing a JSON object literal.
        // repair parses it as a string; the engine's existing fallback
        // (parse_tool_input) will unwrap the string and re-parse.
        let v = repair(r#""{\"path\": \"hello.txt\"}""#).unwrap();
        assert_eq!(v, Value::String(r#"{"path": "hello.txt"}"#.to_string()));
    }

    #[test]
    fn oversize_input_rejected() {
        let big = "x".repeat(MAX_ARG_LEN + 1);
        assert!(repair(&big).is_err());
    }

    #[test]
    fn repairs_brace_balance_with_trailing_comma() {
        let v = repair(r#"{"a": 1,"#).unwrap();
        assert_eq!(v, json!({"a": 1}));
    }
}
