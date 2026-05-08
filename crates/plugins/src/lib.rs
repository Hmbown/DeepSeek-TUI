//! Plugin manifest, discovery, and lifecycle for DeepSeek TUI.
//!
//! A plugin is a self-contained directory that extends DeepSeek TUI with
//! skills, hooks, and MCP servers. Plugins are installed from git repos,
//! local directories, or npm packages, cached under `~/.deepseek/plugins/`,
//! and enabled/disabled per scope (user, project).
//!
//! # Manifest
//!
//! A plugin MAY ship a manifest at `.deepseek-plugin/plugin.json`. When
//! present, its `name` is the unique plugin identifier. When absent,
//! components are auto-discovered from the conventional directory layout
//! and the plugin name is derived from the install directory name (or git
//! repo name).
//!
//! For interop with the broader Claude Code ecosystem, the loader also
//! reads `.claude-plugin/plugin.json` as a secondary manifest source,
//! falling back to auto-discovery when neither is found.
//!
//! # Directory layout (conventional, auto-discovered)
//!
//! ```text
//! my-plugin/
//! ├── .deepseek-plugin/
//! │   └── plugin.json          # Manifest (optional)
//! ├── skills/                  # Skills (<name>/SKILL.md)
//! ├── hooks/
//! │   └── hooks.json           # Hook definitions
//! ├── .mcp.json                # MCP server definitions
//! └── scripts/                 # Utility scripts
//! ```
//!
//! # Environment variables
//!
//! - `DEEPSEEK_PLUGIN_ROOT` — absolute path to the plugin install directory.
//! - `DEEPSEEK_PLUGIN_DATA` — persistent data directory that survives updates.

mod manifest;
mod discovery;
pub mod manager;

pub use manifest::{
    Author, HookConfig, HookDefinition, HookEntry, McpServerConfig, PluginManifest,
    UserConfigField,
};
pub use discovery::{PluginDiscovery, PluginLoadError, PluginLoadResult};

use std::path::PathBuf;

/// A fully-resolved plugin ready for integration into the runtime.
///
/// Everything in this struct has been validated during load — paths are
/// absolute, manifest schema checks passed, and component directories
/// exist on disk.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Unique plugin identifier (from manifest `name` or derived from
    /// install-directory basename).
    pub name: String,
    /// Human-readable plugin description.
    pub description: String,
    /// Semantic version from manifest, if declared.
    pub version: Option<String>,
    /// Source repository URL, if declared.
    pub repository: Option<String>,
    /// Absolute path to the plugin's install directory.
    pub root: PathBuf,
    /// Absolute path to the plugin's persistent data directory.
    pub data_dir: PathBuf,
    /// Discovered skills: list of `<name>/SKILL.md` directories.
    pub skills: Vec<ResolvedSkill>,
    /// Hook definition file path, if present.
    pub hooks_file: Option<PathBuf>,
    /// Parsed MCP server configurations, with `${...}` variables expanded.
    pub mcp_servers: Vec<ResolvedMcpServer>,
}

/// A skill resolved from a plugin.
#[derive(Debug, Clone)]
pub struct ResolvedSkill {
    /// Skill name (from SKILL.md frontmatter `name` or directory basename).
    pub name: String,
    /// Absolute path to the SKILL.md file.
    pub path: PathBuf,
}

/// An MCP server entry with paths expanded.
#[derive(Debug, Clone)]
pub struct ResolvedMcpServer {
    /// Server name (key in mcpServers map).
    pub name: String,
    /// Command to launch the server.
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Environment variables to set.
    pub env: Vec<(String, String)>,
}
