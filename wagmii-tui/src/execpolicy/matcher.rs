//! Command matching helpers for execpolicy rules.

use regex::Regex;

/// Normalize a command string by shlex parsing and re-joining tokens.
pub fn normalize_command(command: &str) -> String {
    if let Some(tokens) = shlex::split(command) {
        tokens.join(" ")
    } else {
        command
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
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
}
