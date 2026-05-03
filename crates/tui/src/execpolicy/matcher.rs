//! Command matching helpers for execpolicy rules.

use regex::Regex;

/// Normalize a command string by shlex parsing and re-joining tokens.
///
/// Strips heredoc bodies first (#419) so a command like
/// `cat <<EOF > file.txt\nbody\nEOF` collapses to `cat > file.txt`
/// before pattern matching. Without this, an `auto_allow` pattern
/// of `cat > file.txt` would fail to match because shlex would
/// tokenize the body lines into the command.
pub fn normalize_command(command: &str) -> String {
    let stripped = strip_heredoc_bodies(command);
    if let Some(tokens) = shlex::split(&stripped) {
        tokens.join(" ")
    } else {
        stripped
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Strip heredoc bodies from a multi-line command string.
///
/// Recognises the common forms:
///
/// * `<<DELIM` — body until line equal to `DELIM`.
/// * `<<-DELIM` — body until line equal to `DELIM` (tabs stripped
///   in real shell; we keep the delimiter match the same).
/// * `<<'DELIM'` / `<<"DELIM"` — quoted delimiter; quotes peeled
///   for the closing match.
///
/// The here-string operator `<<<` is intentionally not stripped —
/// its body is the next token on the same line, not separate lines,
/// and shlex tokenizes it correctly.
fn strip_heredoc_bodies(command: &str) -> String {
    if !command.contains("<<") {
        return command.to_string();
    }
    // Sidestep the here-string operator (`<<<`) by replacing it
    // with a placeholder before running the heredoc regex, then
    // restoring it after. Rust's `regex` crate doesn't support
    // lookbehind, so we can't write "match `<<` only when not
    // preceded by `<`" directly; this preprocessing achieves the
    // same outcome.
    const HERESTRING_PLACEHOLDER: &str = "\u{0001}HERESTRING\u{0001}";
    let command_owned: String = command.replace("<<<", HERESTRING_PLACEHOLDER);
    let command: &str = &command_owned;

    // Lazy-init the heredoc-start regex. Allows whitespace / `-`
    // between `<<` and the delimiter, accepts optional `'` / `"`
    // around the delimiter name. The delimiter is a typical
    // shell identifier (alphanumeric + underscore).
    static HEREDOC_RE_INIT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = HEREDOC_RE_INIT.get_or_init(|| {
        Regex::new(r#"<<-?\s*(?:['"]?)([A-Za-z_][A-Za-z0-9_]*)(?:['"]?)"#)
            .expect("heredoc regex compiles")
    });

    let mut out = String::with_capacity(command.len());
    let mut lines = command.lines();
    while let Some(line) = lines.next() {
        // Detect heredoc on this line, capture the delimiter, and
        // strip the `<<DELIM` operator from the line so downstream
        // tokenizers don't see it in the pattern. A single line can
        // have multiple heredocs (rare but legal: `cmd <<A <<B`);
        // we strip every match on the line and consume until the
        // *last* delimiter (the matching shell behavior is to stack
        // them, but for pattern-match purposes they all collapse).
        let mut delim: Option<String> = None;
        let mut redacted = line.to_string();
        for cap in re.captures_iter(line) {
            // Strip the entire `<<DELIM` text from the line.
            let whole = cap.get(0).map_or("", |m| m.as_str());
            redacted = redacted.replace(whole, "");
            // Track the last-seen delimiter for body consumption.
            delim = cap.get(1).map(|m| m.as_str().to_string());
        }
        // Trim any double-spaces left after stripping.
        let cleaned = redacted
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&cleaned);
        out.push('\n');
        if let Some(d) = delim {
            // Skip body lines until we hit the matching delimiter.
            for body_line in lines.by_ref() {
                if body_line.trim() == d {
                    break;
                }
            }
        }
    }
    // Restore the here-string operator we hid before regex matching.
    out.replace(HERESTRING_PLACEHOLDER, "<<<")
}

/// Return true if the pattern matches the command.
///
/// Patterns support `*` wildcards that match any substring.
pub fn pattern_matches(pattern: &str, command: &str) -> bool {
    let pattern = normalize_command(pattern);
    let command = normalize_command(command);

    if pattern == "*" {
        return true;
    }

    let escaped = regex::escape(&pattern).replace("\\*", ".*");
    let Ok(re) = Regex::new(&format!("^{escaped}$")) else {
        return false;
    };
    re.is_match(&command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_command() {
        assert_eq!(normalize_command("git   status"), "git status");
        assert_eq!(
            normalize_command("git \"log --oneline\""),
            "git log --oneline"
        );
    }

    #[test]
    fn test_pattern_matches() {
        assert!(pattern_matches("git status", "git status"));
        assert!(pattern_matches("git log *", "git log --oneline"));
        assert!(pattern_matches("cargo *", "cargo test --all"));
        assert!(!pattern_matches("git push --force", "git push origin main"));
    }

    #[test]
    fn strip_heredoc_strips_simple_body() {
        let cmd = "cat <<EOF > file.txt\nhello\nworld\nEOF";
        let stripped = super::strip_heredoc_bodies(cmd);
        // Body lines `hello` and `world` are gone; the delimiter
        // `EOF` line is also consumed.
        assert!(!stripped.contains("hello"));
        assert!(!stripped.contains("world"));
        // The redirect target survives.
        assert!(stripped.contains("> file.txt"));
    }

    #[test]
    fn strip_heredoc_handles_dash_form() {
        // `<<-EOF` strips leading tabs in a real shell; for our
        // matching purposes we still want the delimiter consumed.
        let cmd = "cat <<-EOF > file.txt\n\tbody\nEOF";
        let stripped = super::strip_heredoc_bodies(cmd);
        assert!(!stripped.contains("body"));
        assert!(stripped.contains("> file.txt"));
    }

    #[test]
    fn strip_heredoc_handles_quoted_delimiter() {
        let cmd = "cat <<'END_OF_FILE' > out\nliteral $vars\nEND_OF_FILE";
        let stripped = super::strip_heredoc_bodies(cmd);
        assert!(!stripped.contains("literal $vars"));
        assert!(stripped.contains("> out"));
    }

    #[test]
    fn strip_heredoc_leaves_non_heredoc_commands_intact() {
        let cmd = "echo hello && ls";
        // Early-return path: no `<<` in the input, so the original
        // string flows through unchanged (no trailing newline added).
        assert_eq!(super::strip_heredoc_bodies(cmd), "echo hello && ls");
    }

    #[test]
    fn strip_heredoc_does_not_touch_here_string_operator() {
        // `<<<` is here-string; the body is on the same line.
        // shlex handles it fine — we shouldn't try to strip
        // anything because there's no body following on later lines.
        let cmd = "grep foo <<< \"some text\"";
        let stripped = super::strip_heredoc_bodies(cmd);
        // Output keeps the `<<<` — content not stripped.
        assert!(stripped.contains("<<<"));
        assert!(stripped.contains("some text"));
    }

    #[test]
    fn normalize_command_strips_heredoc_for_pattern_matching() {
        // The end-to-end goal: a user's `auto_allow = ["cat > file.txt"]`
        // pattern matches the heredoc form too.
        let normalized = normalize_command("cat <<EOF > file.txt\nbody\nEOF");
        assert!(pattern_matches("cat > file.txt", &normalized));
    }
}
