//! Plugin discovery: load a plugin from a directory, resolving its manifest,
//! auto-discovering components, and expanding path variables.

use crate::manifest::parse::{load_manifest, validate_manifest};
use crate::manifest::{
    ComponentPath, HookComponent, McpServerComponent, McpServerConfig, PluginManifest,
};
use crate::{Plugin, ResolvedMcpServer, ResolvedSkill};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during plugin loading.
#[derive(Debug, Error)]
pub enum PluginLoadError {
    #[error("plugin root does not exist: {path}", path = .0.display())]
    RootNotFound(PathBuf),

    #[error("plugin root is not a directory: {path}", path = .0.display())]
    RootNotDir(PathBuf),

    #[error("failed to read plugin manifest: {0}")]
    ManifestRead(#[source] anyhow::Error),

    #[error("failed to validate plugin manifest: {0}")]
    ManifestValidation(String),

    #[error("failed to read hook config: {0}")]
    HookConfigRead(#[source] anyhow::Error),

    #[error("failed to read MCP config: {0}")]
    McpConfigRead(#[source] anyhow::Error),

    #[error("failed to discover skills: {0}")]
    SkillDiscovery(String),

    #[error("plugin dependency '{dep}' not found")]
    DependencyNotFound { dep: String },
}

/// Result of loading a plugin from a directory.
#[derive(Debug)]
pub enum PluginLoadResult {
    /// Plugin loaded successfully.
    Loaded {
        plugin: Plugin,
        /// Warnings encountered during load (e.g. missing optional components).
        warnings: Vec<String>,
    },
    /// Plugin directory is empty or has no loadable components.
    Empty(PathBuf),
}

/// State machine for loading a plugin from a directory on disk.
#[derive(Debug)]
pub struct PluginDiscovery {
    /// Absolute path to the plugin root directory.
    root: PathBuf,
    /// Absolute path to the plugin's persistent data directory.
    data_dir: PathBuf,
    /// Parsed manifest (may be None if auto-discovering).
    manifest: Option<PluginManifest>,
}

impl PluginDiscovery {
    /// Begin loading a plugin from the given directory.
    ///
    /// `data_dir` is the persistent data directory for this plugin (survives
    /// updates). Typically `~/.deepseek/plugins/data/<plugin-name>/`.
    pub fn new(root: PathBuf, data_dir: PathBuf) -> Result<Self, PluginLoadError> {
        if !root.exists() {
            return Err(PluginLoadError::RootNotFound(root));
        }
        if !root.is_dir() {
            return Err(PluginLoadError::RootNotDir(root));
        }
        Ok(Self {
            root,
            data_dir,
            manifest: None,
        })
    }

    /// Load and validate the manifest.
    ///
    /// Returns `Ok(self)` for chaining. If no manifest is found, auto-discovery
    /// mode is activated (plugin name derived from directory basename).
    pub fn load_manifest(mut self) -> Result<Self, PluginLoadError> {
        self.manifest =
            load_manifest(&self.root).map_err(PluginLoadError::ManifestRead)?;

        if let Some(ref manifest) = self.manifest {
            if let Err(err) = validate_manifest(manifest) {
                return Err(PluginLoadError::ManifestValidation(err.to_string()));
            }
        }

        Ok(self)
    }

    /// Resolve all components and return a fully-loaded [`Plugin`].
    pub fn resolve(self) -> PluginLoadResult {
        let mut warnings: Vec<String> = Vec::new();

        // Determine the plugin name.
        let (name, description, version, repository) = self.resolve_metadata();

        // Discover skills.
        let skills = self.discover_skills(&mut warnings);

        // Discover hooks.
        let hooks_file = self.discover_hooks(&mut warnings);

        // Discover MCP servers.
        let mcp_servers = self.discover_mcp_servers(&mut warnings);

        // If we found nothing at all, report as empty.
        if skills.is_empty() && hooks_file.is_none() && mcp_servers.is_empty() {
            if self.manifest.is_none() {
                return PluginLoadResult::Empty(self.root);
            }
            // Manifest-only plugins are valid (might just provide hooks or
            // be a meta-package).
        }

        PluginLoadResult::Loaded {
            plugin: Plugin {
                name,
                description,
                version,
                repository,
                root: self.root,
                data_dir: self.data_dir,
                skills,
                hooks_file,
                mcp_servers,
            },
            warnings,
        }
    }

    // ── Metadata resolution ──

    fn resolve_metadata(&self) -> (String, String, Option<String>, Option<String>) {
        if let Some(ref manifest) = self.manifest {
            let name = manifest.name.clone();
            let description = manifest
                .description
                .clone()
                .unwrap_or_default();
            let version = manifest.version.clone();
            let repository = manifest.repository.clone();
            return (name, description, version, repository);
        }

        // Auto-discovery: derive name from directory basename.
        let name = self
            .root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        (name, String::new(), None, None)
    }

    // ── Skill discovery ──

    fn discover_skills(&self, _warnings: &mut Vec<String>) -> Vec<ResolvedSkill> {
        let skill_dirs = self.skill_dirs();

        let mut seen = std::collections::HashSet::new();
        let mut skills = Vec::new();

        for relative_dir in &skill_dirs {
            let abs_dir = self.root.join(
                relative_dir
                    .trim_start_matches("./")
                    .trim_start_matches('/'),
            );

            if !abs_dir.exists() || !abs_dir.is_dir() {
                continue;
            }

            // Walk the skill directory looking for `<name>/SKILL.md`.
            if let Ok(entries) = fs::read_dir(&abs_dir) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if !entry_path.is_dir() {
                        continue;
                    }

                    // Skip hidden directories.
                    if entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with('.'))
                    {
                        continue;
                    }

                    let skill_md = entry_path.join("SKILL.md");
                    if !skill_md.exists() {
                        continue;
                    }

                    // Extract name from SKILL.md frontmatter or directory name.
                    let skill_name = match self.read_skill_name(&skill_md) {
                        Ok(name) => name,
                        Err(_) => {
                            entry_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string()
                        }
                    };

                    if seen.insert(skill_name.clone()) {
                        skills.push(ResolvedSkill {
                            name: skill_name,
                            path: skill_md,
                        });
                    }
                }
            }
        }

        skills
    }

    /// Resolve the list of skill directories from manifest or default.
    fn skill_dirs(&self) -> Vec<String> {
        if let Some(ref manifest) = self.manifest {
            match &manifest.skills {
                Some(ComponentPath::Single(path)) => return vec![path.clone()],
                Some(ComponentPath::Multiple(paths)) => return paths.clone(),
                None => {}
            }
        }
        // Default: `./skills/`
        vec!["./skills/".to_string()]
    }

    /// Read the `name` field from a SKILL.md frontmatter block.
    fn read_skill_name(&self, path: &Path) -> Result<String, ()> {
        let content = fs::read_to_string(path).map_err(|_| ())?;
        let trimmed = content.trim_start();
        if !trimmed.starts_with("---") {
            return Err(());
        }
        let after_open = &trimmed[3..];
        let close = after_open.find("---").ok_or(())?;
        let frontmatter = &after_open[..close];

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once(':') {
                if key.trim().eq_ignore_ascii_case("name") {
                    let name = value.trim().trim_matches('"').trim_matches('\'');
                    if !name.is_empty() {
                        return Ok(name.to_string());
                    }
                }
            }
        }

        // Fall back to extracting from the first `# Heading`.
        for line in content.lines() {
            let line = line.trim();
            if let Some(heading) = line.strip_prefix("# ") {
                if !heading.is_empty() {
                    return Ok(heading.to_string());
                }
            }
        }

        Err(())
    }

    // ── Hook discovery ──

    fn discover_hooks(&self, _warnings: &mut Vec<String>) -> Option<PathBuf> {
        let hook_paths = if let Some(ref manifest) = self.manifest {
            match &manifest.hooks {
                Some(HookComponent::Path(path)) => vec![path.clone()],
                Some(HookComponent::Paths(paths)) => paths.clone(),
                Some(HookComponent::Inline(_)) => {
                    // Inline hooks are handled at integration time — the
                    // HookConfig is read directly from the manifest.
                    return None; // Caller will use manifest.hooks
                }
                None => vec!["./hooks/hooks.json".to_string()],
            }
        } else {
            vec!["./hooks/hooks.json".to_string()]
        };

        for relative in &hook_paths {
            let abs_path = self.root.join(
                relative
                    .trim_start_matches("./")
                    .trim_start_matches('/'),
            );
            if abs_path.exists() {
                return Some(abs_path);
            }
        }

        None
    }

    // ── MCP server discovery ──

    fn discover_mcp_servers(&self, warnings: &mut Vec<String>) -> Vec<ResolvedMcpServer> {
        let mcp_configs: BTreeMap<String, McpServerConfig> = if let Some(ref manifest) =
            self.manifest
        {
            match &manifest.mcp_servers {
                Some(McpServerComponent::Path(path)) => {
                    let abs_path = self.root.join(
                        path.trim_start_matches("./").trim_start_matches('/'),
                    );
                    self.read_mcp_json(&abs_path)
                        .unwrap_or_else(|err| {
                            warnings.push(format!(
                                "failed to read MCP config {}: {}",
                                abs_path.display(),
                                err
                            ));
                            BTreeMap::new()
                        })
                }
                Some(McpServerComponent::Inline(servers)) => servers.clone(),
                None => {
                    // Auto-discover: look for `.mcp.json` at the plugin root.
                    let mcp_json = self.root.join(".mcp.json");
                    if mcp_json.exists() {
                        self.read_mcp_json(&mcp_json)
                            .unwrap_or_else(|err| {
                                warnings.push(format!(
                                    "failed to read .mcp.json: {}",
                                    err
                                ));
                                BTreeMap::new()
                            })
                    } else {
                        BTreeMap::new()
                    }
                }
            }
        } else {
            // No manifest: auto-discover `.mcp.json`.
            let mcp_json = self.root.join(".mcp.json");
            if mcp_json.exists() {
                self.read_mcp_json(&mcp_json)
                    .unwrap_or_else(|err| {
                        warnings.push(format!(
                            "failed to read .mcp.json: {}",
                            err
                        ));
                        BTreeMap::new()
                    })
            } else {
                BTreeMap::new()
            }
        };

        // Expand variables and convert to ResolvedMcpServer.
        mcp_configs
            .into_iter()
            .map(|(name, config)| {
                let plugin_root = self.root.to_string_lossy().to_string();
                let plugin_data = self.data_dir.to_string_lossy().to_string();

                let command = Self::expand_vars(&config.command, &plugin_root, &plugin_data);
                let args: Vec<String> = config
                    .args
                    .iter()
                    .map(|a| Self::expand_vars(a, &plugin_root, &plugin_data))
                    .collect();
                let env: Vec<(String, String)> = config
                    .env
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            Self::expand_vars(v, &plugin_root, &plugin_data),
                        )
                    })
                    .collect();

                ResolvedMcpServer {
                    name,
                    command,
                    args,
                    env,
                }
            })
            .collect()
    }

    /// Read an MCP server configuration from a `.mcp.json` file.
    fn read_mcp_json(
        &self,
        path: &Path,
    ) -> Result<BTreeMap<String, McpServerConfig>, anyhow::Error> {
        let raw = fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;

        #[derive(serde::Deserialize)]
        struct McpJson {
            #[serde(rename = "mcpServers")]
            mcp_servers: BTreeMap<String, McpServerConfig>,
        }

        let parsed: McpJson = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("invalid JSON in {}: {}", path.display(), e))?;

        Ok(parsed.mcp_servers)
    }

    /// Expand `${DEEPSEEK_PLUGIN_ROOT}` and `${DEEPSEEK_PLUGIN_DATA}`
    /// placeholders in strings. Also expands `$ENV_VAR` from the environment.
    fn expand_vars(value: &str, plugin_root: &str, plugin_data: &str) -> String {
        let mut result = value.to_string();

        // Replace named variables.
        result = result.replace("${CLAUDE_PLUGIN_ROOT}", plugin_root);
        result = result.replace("${DEEPSEEK_PLUGIN_ROOT}", plugin_root);
        result = result.replace("${CLAUDE_PLUGIN_DATA}", plugin_data);
        result = result.replace("${DEEPSEEK_PLUGIN_DATA}", plugin_data);

        // Expand simple $ENV_VAR references.
        let re = regex_lite::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
        result = re
            .replace_all(&result, |caps: &regex_lite::Captures| {
                let binding = caps.get(0).unwrap();
                let var_name = binding.as_str();
                std::env::var(var_name).unwrap_or_default()
            })
            .to_string();

        result
    }
}

// We use a lightweight regex for variable expansion.
// `regex_lite` is a subset of `regex` without the binary-size hit.
// If not available, fall back to manual string replacement.
mod regex_lite {
    pub struct Regex {
        #[allow(dead_code)]
        pattern: String,
    }

    impl Regex {
        pub fn new(pattern: &str) -> Result<Self, String> {
            Ok(Self {
                pattern: pattern.to_string(),
            })
        }

        pub fn replace_all(
            &self,
            text: &str,
            mut replacer: impl FnMut(&Captures) -> String,
        ) -> String {
            let mut result = String::with_capacity(text.len());
            let mut last_end = 0;
            let bytes = text.as_bytes();
            let mut i = 0;

            while i < bytes.len() {
                // Look for `${`
                if i + 1 < bytes.len()
                    && bytes[i] == b'$'
                    && bytes[i + 1] == b'{'
                {
                    let start = i;
                    i += 2;
                    let var_start = i;
                    while i < bytes.len() && bytes[i] != b'}' {
                        if !(bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                            break;
                        }
                        i += 1;
                    }
                    if i < bytes.len() && bytes[i] == b'}' {
                        let var_name =
                            std::str::from_utf8(&bytes[var_start..i]).unwrap_or("");
                        let before = std::str::from_utf8(&bytes[last_end..start]).unwrap_or("");
                        result.push_str(before);

                        let caps = Captures {
                            groups: vec![Some(var_name.to_string())],
                        };
                        result.push_str(&replacer(&caps));
                        last_end = i + 1;
                    }
                }
                i += 1;
            }

            if last_end < bytes.len() {
                let tail = std::str::from_utf8(&bytes[last_end..]).unwrap_or("");
                result.push_str(tail);
            }

            result
        }
    }

    pub struct Captures {
        groups: Vec<Option<String>>,
    }

    impl Captures {
        pub fn get(&self, index: usize) -> Option<Match> {
            self.groups.get(index).and_then(|g| {
                g.as_ref().map(|s| Match {
                    text: s.clone(),
                })
            })
        }
    }

    pub struct Match {
        pub text: String,
    }

    impl Match {
        pub fn as_str(&self) -> &str {
            &self.text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
    }

    #[test]
    fn expand_variables() {
        let result = PluginDiscovery::expand_vars(
            "${DEEPSEEK_PLUGIN_ROOT}/start.mjs",
            "/home/user/.deepseek/plugins/ctx",
            "/home/user/.deepseek/plugins/data/ctx",
        );
        assert_eq!(result, "/home/user/.deepseek/plugins/ctx/start.mjs");

        // CLAUDE_ prefixed vars also work for interop.
        let result = PluginDiscovery::expand_vars(
            "${CLAUDE_PLUGIN_ROOT}/bin/tool",
            "/home/user/.deepseek/plugins/ctx",
            "/home/user/.deepseek/plugins/data/ctx",
        );
        assert_eq!(result, "/home/user/.deepseek/plugins/ctx/bin/tool");
    }

    #[test]
    fn minimal_auto_discovery() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("my-plugin");
        fs::create_dir_all(root.join("skills").join("hello")).unwrap();
        write_file(
            &root,
            "skills/hello/SKILL.md",
            "---\nname: hello\ndescription: a test\n---\nbody\n",
        );
        write_file(&root, ".mcp.json", r#"{"mcpServers":{"test-srv":{"command":"echo"}}}"#);
        write_file(
            &root,
            "hooks/hooks.json",
            r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"true"}]}]}}"#,
        );

        let discovery = PluginDiscovery::new(root.clone(), root.join("data"))
            .unwrap()
            .load_manifest()
            .unwrap();

        match discovery.resolve() {
            PluginLoadResult::Loaded { plugin, warnings } => {
                assert_eq!(plugin.name, "my-plugin");
                assert_eq!(plugin.skills.len(), 1);
                assert_eq!(plugin.skills[0].name, "hello");
                assert!(plugin.hooks_file.is_some());
                assert_eq!(plugin.mcp_servers.len(), 1);
                assert_eq!(plugin.mcp_servers[0].name, "test-srv");
                assert_eq!(plugin.mcp_servers[0].command, "echo");
                assert!(warnings.is_empty());
            }
            other => panic!("expected Loaded, got {:?}", other),
        }
    }

    #[test]
    fn manifest_with_inline_mcp() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("inline-plugin");
        fs::create_dir_all(root.join(".deepseek-plugin")).unwrap();
        write_file(
            &root,
            ".deepseek-plugin/plugin.json",
            r#"{
                "name": "inline-test",
                "mcpServers": {
                    "ctx": {
                        "command": "node",
                        "args": ["${DEEPSEEK_PLUGIN_ROOT}/start.mjs"],
                        "env": {"NODE_ENV": "production"}
                    }
                }
            }"#,
        );

        let discovery = PluginDiscovery::new(root.clone(), root.join("data"))
            .unwrap()
            .load_manifest()
            .unwrap();

        match discovery.resolve() {
            PluginLoadResult::Loaded { plugin, .. } => {
                assert_eq!(plugin.name, "inline-test");
                assert_eq!(plugin.mcp_servers.len(), 1);
                assert_eq!(plugin.mcp_servers[0].name, "ctx");
                assert_eq!(plugin.mcp_servers[0].command, "node");
                assert!(plugin.mcp_servers[0].args[0].contains("start.mjs"));
                assert!(plugin.mcp_servers[0].env.iter().any(|(k, v)| k == "NODE_ENV" && v == "production"));
            }
            other => panic!("expected Loaded, got {:?}", other),
        }
    }

    #[test]
    fn claude_interop_manifest() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("claude-plugin");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        write_file(
            &root,
            ".claude-plugin/plugin.json",
            r#"{"name": "from-claude", "skills": "./my-skills/"}"#,
        );
        fs::create_dir_all(root.join("my-skills").join("test")).unwrap();
        write_file(
            &root,
            "my-skills/test/SKILL.md",
            "---\nname: test\ndescription: x\n---\nbody\n",
        );

        let discovery = PluginDiscovery::new(root.clone(), root.join("data"))
            .unwrap()
            .load_manifest()
            .unwrap();

        match discovery.resolve() {
            PluginLoadResult::Loaded { plugin, .. } => {
                assert_eq!(plugin.name, "from-claude");
                assert_eq!(plugin.skills.len(), 1);
                assert_eq!(plugin.skills[0].name, "test");
            }
            other => panic!("expected Loaded, got {:?}", other),
        }
    }

    #[test]
    fn plugin_load_error_not_found() {
        let err = PluginDiscovery::new(
            PathBuf::from("/nonexistent/path"),
            PathBuf::from("/tmp"),
        )
        .unwrap_err();
        assert!(matches!(err, PluginLoadError::RootNotFound(_)));
    }

    #[test]
    fn expand_vars_with_env() {
        // Set a test env var.
        // Safety: test-only environment mutation.
        unsafe { std::env::set_var("TEST_PLUGIN_PORT", "9090"); }
        let result = PluginDiscovery::expand_vars(
            "${DEEPSEEK_PLUGIN_ROOT}/server --port ${TEST_PLUGIN_PORT}",
            "/plugin/root",
            "/plugin/data",
        );
        assert_eq!(result, "/plugin/root/server --port 9090");
    }
}
