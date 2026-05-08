//! Plugin manifest schema — compatible with Claude Code's `plugin.json` format
//! for ecosystem interop, with DeepSeek-specific extensions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The top-level plugin manifest, loaded from `.deepseek-plugin/plugin.json`
/// or `.claude-plugin/plugin.json`.
///
/// Only `name` is required. All other fields are optional and have sensible
/// defaults (auto-discovery from directory layout).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// Unique plugin identifier (kebab-case, no spaces). Required.
    pub name: String,

    /// Optional semantic version.
    #[serde(default)]
    pub version: Option<String>,

    /// Brief description of what the plugin does.
    #[serde(default)]
    pub description: Option<String>,

    /// Author information.
    #[serde(default)]
    pub author: Option<Author>,

    /// Documentation URL.
    #[serde(default)]
    pub homepage: Option<String>,

    /// Source code URL.
    #[serde(default)]
    pub repository: Option<String>,

    /// License identifier (e.g. "MIT", "Apache-2.0").
    #[serde(default)]
    pub license: Option<String>,

    /// Discovery keywords.
    #[serde(default)]
    pub keywords: Option<Vec<String>>,

    // ── Component paths ──
    /// Custom skill directories containing `<name>/SKILL.md`.
    /// String = single path, array = multiple paths. Default: `./skills/`.
    #[serde(default)]
    pub skills: Option<ComponentPath>,

    /// Custom hook config file path(s) or inline hook config.
    /// String = single path, array = multiple paths,
    /// object = inline hook definition.
    #[serde(default)]
    pub hooks: Option<HookComponent>,

    /// MCP server definitions. String = path to `.mcp.json`,
    /// object = inline MCP config.
    #[serde(default)]
    pub mcp_servers: Option<McpServerComponent>,

    /// Dependencies on other plugins.
    #[serde(default)]
    pub dependencies: Option<Vec<PluginDependency>>,

    /// User-configurable values prompted at enable time.
    #[serde(default)]
    pub user_config: Option<BTreeMap<String, UserConfigField>>,

    /// Catch-all for forward compatibility with new fields.
    #[serde(flatten)]
    pub extras: BTreeMap<String, serde_json::Value>,
}

/// Author metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub name: Option<String>,
    pub email: Option<String>,
    pub url: Option<String>,
}

/// A component path can be a single string or an array of strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComponentPath {
    Single(String),
    Multiple(Vec<String>),
}

impl ComponentPath {
    /// Return all paths as a flat list.
    pub fn as_paths(&self) -> Vec<&str> {
        match self {
            Self::Single(s) => vec![s.as_str()],
            Self::Multiple(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// Hook configuration: can be a path string, an array of paths, or an
/// inline hook definition object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HookComponent {
    Path(String),
    Paths(Vec<String>),
    Inline(HookConfig),
}

/// MCP server configuration: can be a path string to `.mcp.json`, or an
/// inline MCP config object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerComponent {
    Path(String),
    Inline(BTreeMap<String, McpServerConfig>),
}

/// A single MCP server definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// The command to launch the server.
    pub command: String,

    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set for the server process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Hook configuration file contents.
///
/// Matches Claude Code's `hooks.json` format for interop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfig {
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,

    /// Map of event name → list of hook entries.
    pub hooks: BTreeMap<String, Vec<HookEntry>>,
}

/// A hook entry that fires on a specific event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEntry {
    /// Tool name matcher pattern (e.g. "Bash|Read|Write").
    #[serde(default)]
    pub matcher: Option<String>,

    /// One or more hook actions to execute.
    pub hooks: Vec<HookDefinition>,
}

/// A single hook action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDefinition {
    /// Hook type: "command" (shell), "http" (webhook), or "prompt" (LLM eval).
    #[serde(rename = "type")]
    pub hook_type: String,

    /// Shell command to execute (for "command" type).
    #[serde(default)]
    pub command: Option<String>,

    /// URL to POST (for "http" type).
    #[serde(default)]
    pub url: Option<String>,

    /// LLM prompt (for "prompt" type).
    #[serde(default)]
    pub prompt: Option<String>,
}

/// A dependency on another plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PluginDependency {
    /// Just the plugin name.
    Name(String),
    /// Plugin name with optional version constraint.
    Named {
        name: String,
        #[serde(default)]
        version: Option<String>,
    },
}

impl PluginDependency {
    /// Extract the plugin name regardless of variant.
    pub fn plugin_name(&self) -> &str {
        match self {
            Self::Name(name) => name.as_str(),
            Self::Named { name, .. } => name.as_str(),
        }
    }
}

/// A user-configurable field declared in `userConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfigField {
    /// Field type: string, number, boolean, directory, file.
    #[serde(rename = "type")]
    pub field_type: String,

    /// Label shown in the configuration dialog.
    pub title: String,

    /// Help text shown beneath the field.
    pub description: String,

    /// Whether the value is sensitive (stored in keyring).
    #[serde(default)]
    pub sensitive: Option<bool>,

    /// Whether the field is required.
    #[serde(default)]
    pub required: Option<bool>,

    /// Default value.
    #[serde(default)]
    pub default: Option<serde_json::Value>,

    /// Min/max for number type.
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

/// Re-export the manifest parsing API.
pub mod parse {
    use super::PluginManifest;
    use anyhow::{Context, Result};
    use std::fs;
    use std::path::Path;

    /// Load a plugin manifest from a plugin root directory.
    ///
    /// Tries `.deepseek-plugin/plugin.json` first, then `.claude-plugin/plugin.json`
    /// for ecosystem interop. Returns `None` if neither file exists.
    pub fn load_manifest(plugin_root: &Path) -> Result<Option<PluginManifest>> {
        let candidates = [
            plugin_root.join(".deepseek-plugin").join("plugin.json"),
            plugin_root.join(".claude-plugin").join("plugin.json"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                let raw = fs::read_to_string(candidate)
                    .with_context(|| format!("failed to read {}", candidate.display()))?;
                let manifest: PluginManifest = serde_json::from_str(&raw)
                    .with_context(|| format!("failed to parse {}", candidate.display()))?;
                return Ok(Some(manifest));
            }
        }

        Ok(None)
    }

    /// Validate a manifest — currently checks that `name` is non-empty and
    /// kebab-case-safe.
    pub fn validate_manifest(manifest: &PluginManifest) -> Result<()> {
        if manifest.name.trim().is_empty() {
            anyhow::bail!("plugin manifest `name` must not be empty");
        }
        if manifest.name.contains(' ') {
            anyhow::bail!("plugin name must not contain spaces (got '{}')", manifest.name);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_manifest() {
        let json = r#"{"name": "my-plugin"}"#;
        let manifest: PluginManifest =
            serde_json::from_str(json).expect("minimal manifest parses");
        assert_eq!(manifest.name, "my-plugin");
        assert!(manifest.version.is_none());
        assert!(manifest.skills.is_none());
    }

    #[test]
    fn parse_full_manifest() {
        let json = r#"{
            "name": "context-mode",
            "version": "1.0.0",
            "description": "Context window optimization",
            "author": { "name": "Dev", "url": "https://example.com" },
            "repository": "https://github.com/mksglu/context-mode",
            "license": "MIT",
            "keywords": ["mcp", "context"],
            "skills": "./skills/",
            "hooks": "./hooks/hooks.json",
            "mcpServers": {
                "context-mode": {
                    "command": "node",
                    "args": ["${DEEPSEEK_PLUGIN_ROOT}/start.mjs"]
                }
            }
        }"#;
        let manifest: PluginManifest = serde_json::from_str(json).expect("full manifest parses");
        assert_eq!(manifest.name, "context-mode");
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert_eq!(
            manifest.description.as_deref(),
            Some("Context window optimization")
        );

        // Verify MCP servers
        match &manifest.mcp_servers {
            Some(McpServerComponent::Inline(servers)) => {
                assert!(servers.contains_key("context-mode"));
                let srv = &servers["context-mode"];
                assert_eq!(srv.command, "node");
                assert_eq!(srv.args, vec!["${DEEPSEEK_PLUGIN_ROOT}/start.mjs"]);
            }
            other => panic!("expected inline MCP servers, got {:?}", other),
        }

        // Verify skills path
        match &manifest.skills {
            Some(ComponentPath::Single(s)) => assert_eq!(s, "./skills/"),
            other => panic!("expected single skills path, got {:?}", other),
        }

        // Verify hooks path
        match &manifest.hooks {
            Some(HookComponent::Path(path)) => assert_eq!(path, "./hooks/hooks.json"),
            other => panic!("expected hooks path, got {:?}", other),
        }
    }

    #[test]
    fn parse_hooks_config_file() {
        let json = r#"{
            "description": "Context-mode hooks",
            "hooks": {
                "PostToolUse": [
                    {
                        "matcher": "Bash|Read|Write|Edit|Glob|Grep|mcp__",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "node \"${CLAUDE_PLUGIN_ROOT}/hooks/posttooluse.mjs\""
                            }
                        ]
                    }
                ],
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "node \"${CLAUDE_PLUGIN_ROOT}/hooks/pretooluse.mjs\""
                            }
                        ]
                    }
                ],
                "PreCompact": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "node \"${CLAUDE_PLUGIN_ROOT}/hooks/precompact.mjs\""
                            }
                        ]
                    }
                ],
                "SessionStart": [
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "node \"${CLAUDE_PLUGIN_ROOT}/hooks/sessionstart.mjs\""
                            }
                        ]
                    }
                ]
            }
        }"#;
        let config: HookConfig = serde_json::from_str(json).expect("hooks config parses");

        assert!(config.hooks.contains_key("PostToolUse"));
        assert!(config.hooks.contains_key("PreToolUse"));
        assert!(config.hooks.contains_key("PreCompact"));
        assert!(config.hooks.contains_key("SessionStart"));

        let post_tool_use = &config.hooks["PostToolUse"];
        assert_eq!(post_tool_use.len(), 1);
        let entry = &post_tool_use[0];
        assert_eq!(
            entry.matcher.as_deref(),
            Some("Bash|Read|Write|Edit|Glob|Grep|mcp__")
        );
        assert_eq!(entry.hooks.len(), 1);
        assert_eq!(entry.hooks[0].hook_type, "command");
        assert!(
            entry.hooks[0]
                .command
                .as_ref()
                .is_some_and(|c| c.contains("posttooluse.mjs"))
        );
    }

    #[test]
    fn component_path_conversion() {
        let single = ComponentPath::Single("./skills/".to_string());
        assert_eq!(single.as_paths(), vec!["./skills/"]);

        let multiple = ComponentPath::Multiple(vec![
            "./skills/".to_string(),
            "./extra/".to_string(),
        ]);
        assert_eq!(multiple.as_paths(), vec!["./skills/", "./extra/"]);
    }
}
