//! Agent profile presets — pre-configured agent behaviors.
//!
//! Each profile bundles a role, model, thinking depth, tool set,
//! and posture prompt so agents can be spawned with a single
//! profile name instead of configuring each parameter individually.
//!
//! # Built-in profiles
//!
//! - **code-reviewer**: thorough code review with security focus
//! - **architect**: system design and architecture planning
//! - **debugger**: root-cause analysis and bug hunting
//! - **documenter**: writes clear, comprehensive documentation
//! - **security-auditor**: security-focused code audit
//! - **performance-engineer**: identifies and fixes performance issues

use serde::{Deserialize, Serialize};

use crate::llm::unified::ThinkingMode;
use crate::tools::subagent::SubAgentType;

// ── Profile ──────────────────────────────────────────────────────────────────

/// A predefined agent profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Human-readable name (e.g., "code-reviewer").
    pub name: String,
    /// Short description for display.
    pub description: String,
    /// The sub-agent type this profile maps to.
    pub agent_type: SubAgentType,
    /// Recommended model.
    pub model: String,
    /// Thinking depth for the recommended model.
    pub thinking: ThinkingMode,
    /// Core instructions injected into the system prompt.
    pub posture_prompt: String,
    /// Additional tools beyond the agent type's defaults.
    #[serde(default)]
    pub extra_tools: Vec<String>,
}

impl AgentProfile {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        agent_type: SubAgentType,
        model: impl Into<String>,
        thinking: ThinkingMode,
        posture_prompt: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            agent_type,
            model: model.into(),
            thinking,
            posture_prompt: posture_prompt.into(),
            extra_tools: Vec::new(),
        }
    }
}

// ── Profile registry ─────────────────────────────────────────────────────────

/// Registry of built-in and user-defined agent profiles.
#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    profiles: Vec<AgentProfile>,
}

impl ProfileRegistry {
    /// Create a registry pre-populated with built-in profiles.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut reg = Self::default();
        reg.add(AgentProfile::new(
            "code-reviewer",
            "Thorough code review with security focus",
            SubAgentType::Review,
            "deepseek-v4-flash",
            ThinkingMode::Medium,
            "You are a meticulous code reviewer. For each file:\n\
             1. Identify bugs and logic errors\n\
             2. Flag security vulnerabilities\n\
             3. Suggest improvements for readability and maintainability\n\
             4. Check adherence to project conventions\n\
             Be specific — cite line numbers and suggest concrete fixes.",
        ));
        reg.add(AgentProfile::new(
            "architect",
            "System design and architecture planning",
            SubAgentType::Plan,
            "deepseek-v4-pro",
            ThinkingMode::Deep,
            "You are a systems architect. Think in terms of:\n\
             - Component boundaries and interfaces\n\
             - Data flow and state management\n\
             - Scalability and fault tolerance\n\
             - Trade-offs between simplicity and flexibility\n\
             Produce clear architecture diagrams (ASCII art) and document decisions.",
        ));
        reg.add(AgentProfile::new(
            "debugger",
            "Root-cause analysis and bug hunting",
            SubAgentType::Explore,
            "deepseek-v4-pro",
            ThinkingMode::Deep,
            "You are a senior debugger. Your process:\n\
             1. Reproduce the issue from the description\n\
             2. Trace the call path from entry to failure\n\
             3. Identify the root cause (not just the symptom)\n\
             4. Propose a minimal, safe fix\n\
             Use log analysis, stack traces, and git blame to narrow the search.",
        ));
        reg.add(AgentProfile::new(
            "documenter",
            "Writes clear, comprehensive documentation",
            SubAgentType::Implementer,
            "deepseek-v4-flash",
            ThinkingMode::Light,
            "You are a technical writer. Produce documentation that:\n\
             - Explains WHY, not just WHAT\n\
             - Includes concrete examples\n\
             - Uses consistent terminology\n\
             - Is structured for skimming (headings, bullets, code blocks)\n\
             Target audience: experienced developers new to this codebase.",
        ));
        reg.add(AgentProfile::new(
            "security-auditor",
            "Security-focused code audit",
            SubAgentType::Review,
            "deepseek-v4-pro",
            ThinkingMode::Deep,
            "You are a security auditor. Check for:\n\
             - OWASP Top 10 vulnerabilities\n\
             - Injection attacks (SQL, command, template)\n\
             - Authentication and authorization bypasses\n\
             - Secret leakage (API keys, tokens in code)\n\
             - Unsafe deserialization and input validation gaps\n\
             Rate each finding: Critical / High / Medium / Low.",
        ));
        reg.add(AgentProfile::new(
            "performance-engineer",
            "Identifies and fixes performance issues",
            SubAgentType::Explore,
            "deepseek-v4-pro",
            ThinkingMode::Medium,
            "You are a performance engineer. Analyze for:\n\
             - Algorithmic complexity hotspots\n\
             - Memory allocation patterns\n\
             - I/O bottlenecks (disk, network, database)\n\
             - Caching opportunities\n\
             - Concurrency and lock contention\n\
             Provide before/after benchmarks where possible.",
        ));
        reg
    }

    /// Add a profile.
    pub fn add(&mut self, profile: AgentProfile) {
        self.profiles.push(profile);
    }

    /// Find a profile by name (case-insensitive).
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&AgentProfile> {
        let lower = name.to_lowercase();
        self.profiles
            .iter()
            .find(|p| p.name.to_lowercase() == lower)
    }

    /// List all profile names.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.profiles.iter().map(|p| p.name.as_str()).collect()
    }

    /// Number of registered profiles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Whether no profiles are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Import agency-agent profiles from a directory of `.md` files.
    ///
    /// Scans the given directory (recursively one level into subdirectories)
    /// for `.md` files in the [agency-agents](https://github.com/msitarzewski/agency-agents)
    /// format. Each file must have YAML frontmatter with `name`, `description`,
    /// and optional `color`/`vibe` fields, followed by a Markdown body.
    ///
    /// Division → SubAgentType mapping:
    /// - engineering/testing → Implementer or Verifier
    /// - design/product → Implementer
    /// - project-management/strategy → Plan
    /// - marketing/sales/support/finance → General
    ///
    /// Returns the number of profiles successfully imported.
    pub fn import_agency_agents(&mut self, dir: &std::path::Path) -> std::io::Result<usize> {
        if !dir.is_dir() {
            return Ok(0);
        }

        let mut imported = 0usize;
        let mut entries: Vec<std::path::PathBuf> = Vec::new();

        // Collect .md files from dir + immediate subdirs
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Recurse one level into division directories
                    if let Ok(sub) = std::fs::read_dir(&path) {
                        for e in sub.flatten() {
                            let p = e.path();
                            if p.extension().map_or(false, |ext| ext == "md") {
                                entries.push(p);
                            }
                        }
                    }
                } else if path.extension().map_or(false, |ext| ext == "md") {
                    entries.push(path);
                }
            }
        }

        for path in &entries {
            if let Ok(profile) = parse_agency_agent_file(path) {
                let name = profile.name.clone();
                // Skip if already registered
                if self.find(&name).is_some() {
                    tracing::debug!(name = %name, "agency-agent profile already registered, skipping");
                    continue;
                }
                self.add(profile);
                imported += 1;
            }
        }

        tracing::info!(imported, dir = %dir.display(), "imported agency-agent profiles");
        Ok(imported)
    }
}

// ── Agency-agent YAML frontmatter parser ─────────────────────────────────────

/// Parsed frontmatter from an agency-agent .md file.
struct AgencyAgentFrontmatter {
    name: String,
    description: String,
}

/// Parse a single agency-agent .md file into an AgentProfile.
fn parse_agency_agent_file(path: &std::path::Path) -> Result<AgentProfile, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    let (frontmatter, body) = parse_yaml_frontmatter(&raw)
        .ok_or_else(|| format!("No YAML frontmatter found in {}", path.display()))?;

    let name = frontmatter.name;
    let description = frontmatter.description;

    // Derive SubAgentType from directory or content keywords
    let agent_type = infer_agent_type(path, &name, &body);

    // Use the body as the posture prompt
    let posture_prompt = format_agency_posture(&name, &body);

    Ok(AgentProfile::new(
        slugify(&name),
        description,
        agent_type,
        "deepseek-v4-flash",
        ThinkingMode::Medium,
        posture_prompt,
    ))
}

/// Parse YAML frontmatter delimited by `---`.
/// Returns (frontmatter_fields, body_after_frontmatter).
fn parse_yaml_frontmatter(content: &str) -> Option<(AgencyAgentFrontmatter, String)> {
    let mut lines = content.lines();
    let first = lines.next()?;
    if first.trim() != "---" {
        return None;
    }

    let mut name = String::new();
    let mut description = String::new();

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key.trim() {
                "name" => name = value.to_string(),
                "description" => description = value.to_string(),
                _ => {}
            }
        }
    }

    if name.is_empty() {
        return None;
    }
    if description.is_empty() {
        description = format!("Agency agent: {name}");
    }

    let body = lines.collect::<Vec<&str>>().join("\n");

    Some((AgencyAgentFrontmatter { name, description }, body))
}

/// Infer SubAgentType from directory path, agent name, and body keywords.
fn infer_agent_type(path: &std::path::Path, _name: &str, body: &str) -> SubAgentType {
    let path_str = path.to_string_lossy().to_lowercase();
    let body_lower = body.to_lowercase();

    // Check path context
    if path_str.contains("testing") || path_str.contains("qa") {
        return SubAgentType::Verifier;
    }
    if path_str.contains("design") || path_str.contains("engineering") {
        return SubAgentType::Implementer;
    }
    if path_str.contains("plan") || path_str.contains("strategy") || path_str.contains("product") {
        return SubAgentType::Plan;
    }
    if path_str.contains("review") {
        return SubAgentType::Review;
    }

    // Check content keywords
    if body_lower.contains("review") || body_lower.contains("audit") {
        return SubAgentType::Review;
    }
    if body_lower.contains("test") || body_lower.contains("verify") || body_lower.contains("qa") {
        return SubAgentType::Verifier;
    }
    if body_lower.contains("plan") || body_lower.contains("architecture") || body_lower.contains("strategy") {
        return SubAgentType::Plan;
    }
    if body_lower.contains("implement") || body_lower.contains("build") || body_lower.contains("develop") || body_lower.contains("code") {
        return SubAgentType::Implementer;
    }

    SubAgentType::General
}

/// Convert agent name to a URL-safe slug for profile lookup.
fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}

/// Format the posture prompt from the agency-agent body content.
fn format_agency_posture(name: &str, body: &str) -> String {
    // Clean up the body — remove leading `#` heading and trim
    let cleaned = body
        .lines()
        .skip_while(|l| l.starts_with('#'))
        .collect::<Vec<&str>>()
        .join("\n")
        .trim()
        .to_string();

    format!(
        "## Agent Profile: {name}\n\n\
         You are **{name}**.\n\n\
         {cleaned}\n\n\
         ---\n\
         Follow the core mission and critical rules above. \
         Produce the technical deliverables expected of this role. \
         Be specific, concrete, and deliverable-focused."
    )
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins_include_all_six() {
        let reg = ProfileRegistry::with_builtins();
        assert_eq!(reg.len(), 6);
        assert!(reg.find("code-reviewer").is_some());
        assert!(reg.find("architect").is_some());
        assert!(reg.find("debugger").is_some());
        assert!(reg.find("documenter").is_some());
        assert!(reg.find("security-auditor").is_some());
        assert!(reg.find("performance-engineer").is_some());
    }

    #[test]
    fn test_find_case_insensitive() {
        let reg = ProfileRegistry::with_builtins();
        assert!(reg.find("CODE-REVIEWER").is_some());
        assert!(reg.find("Architect").is_some());
    }

    #[test]
    fn test_find_unknown_returns_none() {
        let reg = ProfileRegistry::with_builtins();
        assert!(reg.find("nonexistent").is_none());
    }

    #[test]
    fn test_profiles_have_distinct_postures() {
        let reg = ProfileRegistry::with_builtins();
        let reviewer = reg.find("code-reviewer").unwrap();
        let debugger = reg.find("debugger").unwrap();
        assert_ne!(reviewer.posture_prompt, debugger.posture_prompt);
    }

    #[test]
    fn test_names_returns_all() {
        let reg = ProfileRegistry::with_builtins();
        let names = reg.names();
        assert_eq!(names.len(), 6);
    }

    #[test]
    fn test_custom_profile() {
        let mut reg = ProfileRegistry::default();
        reg.add(AgentProfile::new(
            "my-custom",
            "Custom profile for testing",
            SubAgentType::General,
            "deepseek-v4-flash",
            ThinkingMode::Light,
            "Be concise.",
        ));
        assert_eq!(reg.len(), 1);
        assert!(reg.find("my-custom").is_some());
    }

    #[test]
    fn test_parse_yaml_frontmatter() {
        let content = "---\nname: Frontend Developer\ndescription: Expert frontend developer\ncolor: cyan\n---\n\n# Heading\nBody content here.";
        let (fm, body) = parse_yaml_frontmatter(content).unwrap();
        assert_eq!(fm.name, "Frontend Developer");
        assert_eq!(fm.description, "Expert frontend developer");
        assert!(body.contains("Body content"));
    }

    #[test]
    fn test_parse_no_frontmatter() {
        assert!(parse_yaml_frontmatter("Just text, no frontmatter").is_none());
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Frontend Developer"), "frontend-developer");
        assert_eq!(slugify("Backend Architect"), "backend-architect");
        assert_eq!(slugify("AI Engineer"), "ai-engineer");
    }

    #[test]
    fn test_infer_agent_type_from_path() {
        use std::path::Path;
        assert_eq!(
            infer_agent_type(Path::new("engineering/agent.md"), "", ""),
            SubAgentType::Implementer
        );
        assert_eq!(
            infer_agent_type(Path::new("testing/agent.md"), "", ""),
            SubAgentType::Verifier
        );
        assert_eq!(
            infer_agent_type(Path::new("strategy/agent.md"), "", ""),
            SubAgentType::Plan
        );
    }

    #[test]
    fn test_infer_agent_type_from_body() {
        use std::path::Path;
        assert_eq!(
            infer_agent_type(Path::new("unknown/agent.md"), "", "review the code and audit security"),
            SubAgentType::Review
        );
        assert_eq!(
            infer_agent_type(Path::new("unknown/agent.md"), "", "test and verify functionality"),
            SubAgentType::Verifier
        );
    }

    #[test]
    fn test_import_agency_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let eng_dir = tmp.path().join("engineering");
        std::fs::create_dir_all(&eng_dir).unwrap();

        let agent_md = "---\nname: Test Engineer\ndescription: A test engineer for QA\n---\n\n## Identity\nYou are a test engineer.\n\n## Core Mission\nTest everything.\n\n## Critical Rules\nAlways verify.";
        std::fs::write(eng_dir.join("engineering-test-engineer.md"), agent_md).unwrap();

        let mut reg = ProfileRegistry::default();
        let count = reg.import_agency_agents(tmp.path()).unwrap();
        assert!(count >= 1);
        assert!(reg.find("test-engineer").is_some());

        let profile = reg.find("test-engineer").unwrap();
        assert!(profile.posture_prompt.contains("Test Engineer"));
        assert!(profile.posture_prompt.contains("Test everything"));
    }

    #[test]
    fn test_import_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reg = ProfileRegistry::default();
        let count = reg.import_agency_agents(tmp.path()).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_format_agency_posture_includes_name() {
        let result = format_agency_posture("Code Reviewer", "# heading\n\nYou review code.\n\nCheck security.");
        assert!(result.contains("Code Reviewer"));
        assert!(result.contains("You review code."));
        assert!(result.contains("Check security."));
    }
}
