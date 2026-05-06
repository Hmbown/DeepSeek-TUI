//! Sensitive information leak detection.
//!
//! Scans tool outputs and model responses for accidentally exposed secrets.

use std::sync::OnceLock;

use regex::Regex;

/// Type of sensitive information detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretType {
    ApiKey,
    AwsSecret,
    PrivateKey,
    PasswordInUrl,
    JwtToken,
    GenericSecret,
    GithubToken,
    DatabaseUrl,
}

/// A detected leak instance.
#[derive(Debug, Clone)]
pub struct LeakDetection {
    pub secret_type: SecretType,
    pub description: &'static str,
    pub byte_range: std::ops::Range<usize>,
    /// The redacted replacement string.
    pub redacted: String,
}

struct LeakPattern {
    secret_type: SecretType,
    pattern: &'static str,
    description: &'static str,
}

const LEAK_PATTERNS: &[LeakPattern] = &[
    LeakPattern {
        secret_type: SecretType::ApiKey,
        pattern: r"(?i)(sk-|api[_-]?key[=:\s]+)[a-zA-Z0-9]{20,}",
        description: "API key pattern",
    },
    LeakPattern {
        secret_type: SecretType::AwsSecret,
        pattern: r"(?i)aws[_-]?secret[_-]?access[_-]?key[=:\s]+[A-Za-z0-9/+=]{40}",
        description: "AWS secret access key",
    },
    LeakPattern {
        secret_type: SecretType::PrivateKey,
        pattern: r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
        description: "Private key header",
    },
    LeakPattern {
        secret_type: SecretType::PasswordInUrl,
        pattern: r"://[^:]+:[^@\s]{3,}@[^\s]+",
        description: "Password in URL",
    },
    LeakPattern {
        secret_type: SecretType::JwtToken,
        pattern: r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
        description: "JWT token",
    },
    LeakPattern {
        secret_type: SecretType::GithubToken,
        pattern: r"(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}",
        description: "GitHub personal access token",
    },
    LeakPattern {
        secret_type: SecretType::DatabaseUrl,
        pattern: r"(?i)(postgres|mysql|mongodb)://[^:]+:[^@\s]+@",
        description: "Database connection string with credentials",
    },
    LeakPattern {
        secret_type: SecretType::GenericSecret,
        pattern: r#"(?i)(password|passwd|secret|token)\s*[=:]\s*["'][^"']{8,}["']"#,
        description: "Generic secret assignment",
    },
];

struct CompiledLeakPattern {
    secret_type: SecretType,
    regex: Regex,
    description: &'static str,
}

fn compiled_patterns() -> &'static Vec<CompiledLeakPattern> {
    static COMPILED: OnceLock<Vec<CompiledLeakPattern>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        LEAK_PATTERNS
            .iter()
            .filter_map(|p| {
                Regex::new(p.pattern).ok().map(|regex| CompiledLeakPattern {
                    secret_type: p.secret_type.clone(),
                    regex,
                    description: p.description,
                })
            })
            .collect()
    })
}

/// Check content for sensitive information leaks.
pub fn check_sensitive_leak(content: &str) -> Vec<LeakDetection> {
    let patterns = compiled_patterns();
    let mut detections = Vec::new();

    for pattern in patterns {
        for m in pattern.regex.find_iter(content) {
            let matched_text = m.as_str();
            let redacted = redact_value(matched_text, &pattern.secret_type);
            detections.push(LeakDetection {
                secret_type: pattern.secret_type.clone(),
                description: pattern.description,
                byte_range: m.start()..m.end(),
                redacted,
            });
        }
    }

    detections
}

/// Redact content by replacing detected secrets with placeholder.
pub fn redact_secrets(content: &str) -> String {
    let detections = check_sensitive_leak(content);
    if detections.is_empty() {
        return content.to_string();
    }

    let mut result = content.to_string();
    // Process from end to start so byte ranges remain valid
    let mut sorted = detections;
    sorted.sort_by(|a, b| b.byte_range.start.cmp(&a.byte_range.start));

    for detection in sorted {
        result.replace_range(detection.byte_range, &detection.redacted);
    }
    result
}

fn redact_value(value: &str, secret_type: &SecretType) -> String {
    match secret_type {
        SecretType::PrivateKey => "[REDACTED: private key]".to_string(),
        SecretType::PasswordInUrl => {
            // Keep the scheme and host visible
            if let Some(at_pos) = value.rfind('@') {
                let after_at = &value[at_pos..];
                format!("://***:***{after_at}")
            } else {
                "[REDACTED: url with credentials]".to_string()
            }
        }
        _ => {
            // Show first 4 chars + mask the rest
            let visible = value.chars().take(4).collect::<String>();
            format!("{visible}***[REDACTED]")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_api_key() {
        let content = "api_key=sk-1234567890abcdefghijklmnop";
        let leaks = check_sensitive_leak(content);
        assert!(!leaks.is_empty());
        assert_eq!(leaks[0].secret_type, SecretType::ApiKey);
    }

    #[test]
    fn detects_private_key() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...";
        let leaks = check_sensitive_leak(content);
        assert!(!leaks.is_empty());
        assert_eq!(leaks[0].secret_type, SecretType::PrivateKey);
    }

    #[test]
    fn detects_github_token() {
        let content = "export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz1234567890";
        let leaks = check_sensitive_leak(content);
        assert!(!leaks.is_empty());
        assert_eq!(leaks[0].secret_type, SecretType::GithubToken);
    }

    #[test]
    fn no_false_positive_on_normal_code() {
        let content = r#"
fn main() {
    let port = 8080;
    let host = "localhost";
    println!("Listening on {}:{}", host, port);
}
"#;
        let leaks = check_sensitive_leak(content);
        assert!(leaks.is_empty());
    }

    #[test]
    fn redaction_preserves_structure() {
        let content = "my key is sk-abcdefghijklmnopqrstuvwxyz123456";
        let redacted = redact_secrets(content);
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz123456"));
        assert!(redacted.contains("[REDACTED]"));
    }
}
