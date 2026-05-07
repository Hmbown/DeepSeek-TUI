//! Input Sanitizer — wraps external content with boundary markers and source labels.

use std::fmt;

/// Source type for content being injected into the LLM context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentSource {
    /// User's direct typed input (trusted).
    UserInput,
    /// File content from read_file tool.
    FileContent { path: String },
    /// Shell command output.
    ShellOutput { command: String },
    /// Web page content from fetch_url.
    WebContent { url: String },
    /// Web search results.
    WebSearchResult { query: String },
    /// Git history, diff, or commit messages.
    GitContent { ref_type: String },
    /// MCP tool result.
    McpToolResult { server: String, tool: String },
    /// Sub-agent returned result.
    SubAgentResult { agent_id: String },
    /// LSP diagnostic output.
    LspDiagnostic { language: String },
    /// Skill instruction content.
    SkillInstruction { name: String, trusted: bool },
    /// Project documentation (AGENTS.md, etc.).
    ProjectDoc { filename: String },
}

impl ContentSource {
    /// Human-readable label for the source.
    pub fn label(&self) -> String {
        match self {
            Self::UserInput => "user_input".to_string(),
            Self::FileContent { path } => format!("file:{path}"),
            Self::ShellOutput { command } => {
                let short = if command.len() > 40 {
                    format!("{}...", &command[..40])
                } else {
                    command.clone()
                };
                format!("shell:{short}")
            }
            Self::WebContent { url } => format!("web:{url}"),
            Self::WebSearchResult { query } => format!("search:{query}"),
            Self::GitContent { ref_type } => format!("git:{ref_type}"),
            Self::McpToolResult { server, tool } => format!("mcp:{server}/{tool}"),
            Self::SubAgentResult { agent_id } => format!("subagent:{agent_id}"),
            Self::LspDiagnostic { language } => format!("lsp:{language}"),
            Self::SkillInstruction { name, .. } => format!("skill:{name}"),
            Self::ProjectDoc { filename } => format!("project_doc:{filename}"),
        }
    }

    /// Trust level multiplier for threat scoring.
    /// Lower = more trusted, higher = less trusted.
    pub fn trust_multiplier(&self) -> f64 {
        match self {
            Self::UserInput => 0.1,                              // Highly trusted
            Self::SkillInstruction { trusted: true, .. } => 0.3, // Trusted skill
            Self::LspDiagnostic { .. } => 0.3,                   // Machine-generated
            Self::FileContent { .. } => 0.6,                     // Workspace file
            Self::ShellOutput { .. } => 0.7,                     // Command output
            Self::GitContent { .. } => 0.7,                      // Git content
            Self::ProjectDoc { .. } => 0.7,                      // Project docs
            Self::SubAgentResult { .. } => 0.8,                  // Sub-agent
            Self::McpToolResult { .. } => 0.8,                   // External tool
            Self::SkillInstruction { trusted: false, .. } => 0.9, // Untrusted skill
            Self::WebContent { .. } => 1.0,                      // Internet (least trusted)
            Self::WebSearchResult { .. } => 1.0,                 // Internet
        }
    }

    /// Whether this source should have boundary markers applied.
    pub fn needs_boundary(&self) -> bool {
        !matches!(self, Self::UserInput)
    }
}

impl fmt::Display for ContentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Sanitize external content by wrapping with boundary markers.
///
/// Returns the content wrapped in `<external_content>` tags when protection is active.
/// User input is passed through unchanged.
pub fn sanitize_external_content(content: &str, source: &ContentSource) -> String {
    if !source.needs_boundary() {
        return content.to_string();
    }

    let label = source.label();
    // Use XML-style tags that the model can recognize as data boundaries
    format!(
        "<external_content source=\"{label}\">\n{content}\n</external_content>"
    )
}

/// Strip boundary markers from content (for display to user).
pub fn strip_boundary_markers(content: &str) -> String {
    let mut result = content.to_string();
    // Remove opening tags
    while let Some(start) = result.find("<external_content") {
        if let Some(end) = result[start..].find('>') {
            result.replace_range(start..start + end + 1, "");
        } else {
            break;
        }
    }
    // Remove closing tags
    result = result.replace("</external_content>", "");
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_input_is_not_wrapped() {
        let content = "explain this code";
        let result = sanitize_external_content(content, &ContentSource::UserInput);
        assert_eq!(result, content);
    }

    #[test]
    fn file_content_is_wrapped() {
        let content = "fn main() {}";
        let source = ContentSource::FileContent {
            path: "src/main.rs".to_string(),
        };
        let result = sanitize_external_content(content, &source);
        assert!(result.contains("<external_content source=\"file:src/main.rs\">"));
        assert!(result.contains("fn main() {}"));
        assert!(result.contains("</external_content>"));
    }

    #[test]
    fn web_content_has_lowest_trust() {
        let web = ContentSource::WebContent {
            url: "https://example.com".to_string(),
        };
        let file = ContentSource::FileContent {
            path: "test.rs".to_string(),
        };
        assert!(web.trust_multiplier() > file.trust_multiplier());
    }

    #[test]
    fn strip_markers_restores_original() {
        let original = "hello world";
        let wrapped = sanitize_external_content(
            original,
            &ContentSource::ShellOutput {
                command: "echo hello".to_string(),
            },
        );
        let stripped = strip_boundary_markers(&wrapped);
        assert_eq!(stripped, original);
    }
}
