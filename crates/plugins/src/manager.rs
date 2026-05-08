//! Plugin manager: installation, caching, and lifecycle.
//!
//! Plugins are installed into `~/.deepseek/plugins/cache/<name>/<version>/`.
//! Each version is a complete copy of the plugin directory. When a plugin
//! updates, the old version directory remains for 7 days (grace period for
//! running sessions) then is cleaned up on next startup.

use crate::discovery::{PluginDiscovery, PluginLoadError, PluginLoadResult};
use crate::manifest::parse::load_manifest;
use crate::Plugin;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Default paths ──

/// Root directory for all plugin storage.
#[must_use]
pub fn default_plugin_root() -> PathBuf {
    dirs::home_dir().map_or_else(
        || PathBuf::from("/tmp/deepseek/plugins"),
        |p| p.join(".deepseek").join("plugins"),
    )
}

/// Cache directory where plugin versions are stored.
#[must_use]
pub fn default_plugin_cache_dir() -> PathBuf {
    default_plugin_root().join("cache")
}

/// Persistent data directory for plugin state that survives updates.
#[must_use]
pub fn default_plugin_data_dir() -> PathBuf {
    default_plugin_root().join("data")
}

// ── Errors ──

/// Errors that can occur during plugin management operations.
#[derive(Debug, thiserror::Error)]
pub enum PluginManagerError {
    #[error("plugin '{0}' is not installed")]
    NotInstalled(String),

    #[error("plugin '{0}' is already installed (version {1})")]
    AlreadyInstalled(String, String),

    #[error("failed to clone git repository '{url}': {reason}")]
    GitCloneFailed { url: String, reason: String },

    #[error("npm install failed for '{package}': {reason}")]
    NpmInstallFailed { package: String, reason: String },

    #[error("invalid plugin source: {0}")]
    InvalidSource(String),

    #[error("plugin directory not found at {path}", path = .0.display())]
    DirectoryNotFound(PathBuf),

    #[error("failed to load plugin: {0}")]
    LoadError(#[source] PluginLoadError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Manager ──

/// Manages installed plugins: install, uninstall, enable, disable, list.
///
/// # Plugin cache layout
///
/// ```text
/// ~/.deepseek/plugins/
/// ├── cache/
/// │   └── <plugin-name>/
/// │       └── <version-sha>/
/// │           ├── .deepseek-plugin/plugin.json
/// │           ├── skills/
/// │           ├── hooks/
/// │           └── ...
/// └── data/
///     └── <plugin-name>/
///         └── (persistent state)
/// ```
pub struct PluginManager {
    /// Root of the plugin cache.
    cache_dir: PathBuf,
    /// Root of persistent plugin data.
    data_dir: PathBuf,
}

impl PluginManager {
    /// Create a new plugin manager with default paths.
    pub fn new() -> Self {
        Self {
            cache_dir: default_plugin_cache_dir(),
            data_dir: default_plugin_data_dir(),
        }
    }

    /// Create a manager with custom paths (for testing).
    pub fn with_dirs(cache_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            data_dir,
        }
    }

    // ── Installation ──

    /// Install a plugin from a git repository URL.
    ///
    /// Clones the repo into the cache under `<cache_dir>/<name>/<git_sha>/`.
    /// The plugin name is derived from the manifest (or repo name if no manifest).
    pub fn install_from_git(&self, repo_url: &str) -> Result<Plugin, PluginManagerError> {
        // Determine a temporary directory for the clone.
        let temp_dir = tempfile::TempDir::new().map_err(PluginManagerError::Io)?;

        // Clone the repository.
        let status = Command::new("git")
            .args(["clone", "--depth", "1", repo_url, "."])
            .current_dir(temp_dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
            .map_err(|e| PluginManagerError::GitCloneFailed {
                url: repo_url.to_string(),
                reason: e.to_string(),
            })?;

        if !status.success() {
            return Err(PluginManagerError::GitCloneFailed {
                url: repo_url.to_string(),
                reason: format!("git clone exited with status {status}"),
            });
        }

        // Get the git SHA for versioning.
        let git_sha = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(temp_dir.path())
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        self.install_from_directory_impl(temp_dir.path(), &git_sha)
    }

    /// Install a plugin from a local directory.
    ///
    /// Copies the directory into the cache. The version defaults to "local"
    /// unless overridden.
    pub fn install_from_directory(
        &self,
        source_dir: &Path,
    ) -> Result<Plugin, PluginManagerError> {
        self.install_from_directory_impl(source_dir, "local")
    }

    fn install_from_directory_impl(
        &self,
        source_dir: &Path,
        version: &str,
    ) -> Result<Plugin, PluginManagerError> {
        if !source_dir.is_dir() {
            return Err(PluginManagerError::DirectoryNotFound(source_dir.to_path_buf()));
        }

        // Load the plugin to get its name (and validate it's a real plugin).
        let discovery = PluginDiscovery::new(
            source_dir.to_path_buf(),
            self.data_dir_for("__temp__"),
        )
        .map_err(PluginManagerError::LoadError)?
        .load_manifest()
        .map_err(PluginManagerError::LoadError)?;

        let result = discovery.resolve();
        let plugin = match result {
            PluginLoadResult::Loaded { plugin, .. } => plugin,
            PluginLoadResult::Empty(_) => {
                return Err(PluginManagerError::InvalidSource(
                    "directory contains no loadable plugin components".to_string(),
                ));
            }
        };

        // Check if already installed.
        let dest_dir = self.cache_dir.join(&plugin.name).join(version);
        if dest_dir.exists() {
            return Err(PluginManagerError::AlreadyInstalled(
                plugin.name.clone(),
                version.to_string(),
            ));
        }

        // Copy the plugin to the cache.
        fs::create_dir_all(&dest_dir).map_err(PluginManagerError::Io)?;
        copy_dir_all(source_dir, &dest_dir)?;

        // Create data directory.
        let data_dir = self.data_dir_for(&plugin.name);
        fs::create_dir_all(&data_dir).ok();

        // Reload from the cache directory for accurate paths.
        self.load_plugin(&plugin.name, version)
    }

    /// Install a plugin from an npm package.
    ///
    /// Runs `npm install <package>` in a temp directory, then discovers the
    /// plugin components from the installed package.
    pub fn install_from_npm(&self, package: &str) -> Result<Plugin, PluginManagerError> {
        let temp_dir = tempfile::TempDir::new().map_err(PluginManagerError::Io)?;
        let install_dir = temp_dir.path().join("node_modules").join(
            package
                .strip_prefix('@')
                .unwrap_or(package)
                .replace('/', "-"),
        );

        // Create a minimal package.json for npm install.
        let pkg_json = format!(
            r#"{{"name":"deepseek-plugin-install","private":true,"dependencies":{{"{package}":"*"}}}}"#
        );
        fs::write(temp_dir.path().join("package.json"), pkg_json)
            .map_err(PluginManagerError::Io)?;

        let status = Command::new("npm")
            .args(["install", "--no-save", "--legacy-peer-deps"])
            .current_dir(temp_dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
            .map_err(|e| PluginManagerError::NpmInstallFailed {
                package: package.to_string(),
                reason: e.to_string(),
            })?;

        if !status.success() {
            return Err(PluginManagerError::NpmInstallFailed {
                package: package.to_string(),
                reason: format!("npm install exited with status {status}"),
            });
        }

        // Find the installed package directory.
        // npm may install into a scoped directory or flatten.
        let mut found_dir: Option<PathBuf> = None;
        if install_dir.exists() {
            found_dir = Some(install_dir);
        } else {
            // Search node_modules for a matching directory.
            let nm = temp_dir.path().join("node_modules");
            if let Ok(entries) = fs::read_dir(&nm) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // Check for plugin manifest inside.
                        let manifest_path = path
                            .join(".deepseek-plugin")
                            .join("plugin.json");
                        let claude_manifest = path
                            .join(".claude-plugin")
                            .join("plugin.json");
                        if manifest_path.exists() || claude_manifest.exists() {
                            found_dir = Some(path);
                            break;
                        }
                    }
                }
            }
        }

        let source_dir = found_dir
            .ok_or_else(|| PluginManagerError::NpmInstallFailed {
                package: package.to_string(),
                reason: "could not locate installed plugin directory".to_string(),
            })?;

        // Get npm package version for versioning.
        let version = Command::new("npm")
            .args(["view", package, "version"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "latest".to_string());

        self.install_from_directory_impl(&source_dir, &version)
    }

    // ── Uninstall ──

    /// Uninstall a plugin by name. Removes all cached versions.
    pub fn uninstall(&self, name: &str) -> Result<(), PluginManagerError> {
        let plugin_cache_dir = self.cache_dir.join(name);
        if !plugin_cache_dir.exists() {
            return Err(PluginManagerError::NotInstalled(name.to_string()));
        }

        // Remove all cached versions.
        fs::remove_dir_all(&plugin_cache_dir).map_err(PluginManagerError::Io)?;

        // Remove data directory (keep-data flag would skip this).
        let data_dir = self.data_dir_for(name);
        if data_dir.exists() {
            fs::remove_dir_all(&data_dir).ok();
        }

        Ok(())
    }

    // ── Loading ──

    /// Load a specific installed plugin by name and version.
    ///
    /// If `version` is not provided, loads the latest version (highest
    /// directory name sort-order).
    pub fn load_plugin(
        &self,
        name: &str,
        version: &str,
    ) -> Result<Plugin, PluginManagerError> {
        let plugin_dir = self.cache_dir.join(name).join(version);
        if !plugin_dir.is_dir() {
            return Err(PluginManagerError::NotInstalled(format!(
                "{name}@{version}"
            )));
        }

        let data_dir = self.data_dir_for(name);
        let discovery = PluginDiscovery::new(plugin_dir, data_dir)
            .map_err(PluginManagerError::LoadError)?
            .load_manifest()
            .map_err(PluginManagerError::LoadError)?;

        match discovery.resolve() {
            PluginLoadResult::Loaded { plugin, .. } => Ok(plugin),
            PluginLoadResult::Empty(_) => Err(PluginManagerError::InvalidSource(
                "cached plugin has no loadable components".to_string(),
            )),
        }
    }

    /// Load the latest installed version of a plugin.
    pub fn load_latest(&self, name: &str) -> Result<Plugin, PluginManagerError> {
        let latest = self
            .latest_version(name)?
            .ok_or_else(|| PluginManagerError::NotInstalled(name.to_string()))?;
        self.load_plugin(name, &latest)
    }

    // ── Listing ──

    /// List all installed plugins with their available versions.
    pub fn list_installed(&self) -> Result<Vec<InstalledPluginInfo>, PluginManagerError> {
        let mut plugins = Vec::new();

        if !self.cache_dir.exists() {
            return Ok(plugins);
        }

        for entry in fs::read_dir(&self.cache_dir).map_err(PluginManagerError::Io)? {
            let entry = entry.map_err(PluginManagerError::Io)?;
            if !entry.file_type().map_err(PluginManagerError::Io)?.is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();

            // Collect available versions.
            let mut versions = Vec::new();
            if let Ok(version_entries) = fs::read_dir(entry.path()) {
                for ve in version_entries.flatten() {
                    if ve.file_type().is_ok_and(|t| t.is_dir()) {
                        versions.push(ve.file_name().to_string_lossy().to_string());
                    }
                }
            }
            versions.sort();

            // Try to load metadata from the latest version.
            let (description, repository, manifest_version) = if let Some(latest) = versions.last()
            {
                let plugin_dir = entry.path().join(latest);
                match load_manifest(&plugin_dir) {
                    Ok(Some(manifest)) => (
                        manifest.description.unwrap_or_default(),
                        manifest.repository,
                        manifest.version,
                    ),
                    _ => (String::new(), None, None),
                }
            } else {
                (String::new(), None, None)
            };

            plugins.push(InstalledPluginInfo {
                name,
                versions,
                description,
                repository,
                manifest_version,
            });
        }

        Ok(plugins)
    }

    // ── Helpers ──

    fn latest_version(&self, name: &str) -> Result<Option<String>, PluginManagerError> {
        let plugin_dir = self.cache_dir.join(name);
        if !plugin_dir.is_dir() {
            return Ok(None);
        }

        let mut versions: Vec<String> = Vec::new();
        for entry in fs::read_dir(&plugin_dir).map_err(PluginManagerError::Io)? {
            let entry = entry.map_err(PluginManagerError::Io)?;
            if entry.file_type().map_err(PluginManagerError::Io)?.is_dir() {
                versions.push(entry.file_name().to_string_lossy().to_string());
            }
        }

        if versions.is_empty() {
            return Ok(None);
        }

        // Sort versions — git SHAs sort lexicographically which is fine;
        // semver versions also sort correctly.
        versions.sort();
        Ok(Some(versions.into_iter().last().unwrap()))
    }

    fn data_dir_for(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    /// The cache directory.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// The persistent data directory.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

// ── Info type ──

/// Summary metadata for an installed plugin.
#[derive(Debug, Clone)]
pub struct InstalledPluginInfo {
    /// Plugin name.
    pub name: String,
    /// Available versions (sorted).
    pub versions: Vec<String>,
    /// Description from manifest.
    pub description: String,
    /// Repository URL from manifest.
    pub repository: Option<String>,
    /// Version declared in the manifest (of the latest installed version).
    pub manifest_version: Option<String>,
}

// ── Utility ──

/// Recursively copy a directory tree.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_plugin_dir(name: &str, dest: &Path) {
        let manifest_dir = dest.join(".deepseek-plugin");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("plugin.json"),
            format!(r#"{{"name":"{name}","description":"test plugin"}}"#),
        )
        .unwrap();

        // Add a skill.
        let skill_dir = dest.join("skills").join("hello");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: hello\ndescription: a skill\n---\nbody\n",
        )
        .unwrap();

        // Add an MCP server.
        fs::create_dir_all(dest).unwrap();
        fs::write(
            dest.join(".mcp.json"),
            r#"{"mcpServers":{"test":{"command":"echo","args":["hello"]}}}"#,
        )
        .unwrap();
    }

    #[test]
    fn install_from_directory_and_list() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache");
        let data = tmp.path().join("data");

        // Create a source plugin directory.
        let source = tmp.path().join("my-plugin");
        make_plugin_dir("my-plugin", &source);

        let manager = PluginManager::with_dirs(cache.clone(), data.clone());

        // Install.
        let plugin = manager
            .install_from_directory(&source)
            .expect("install succeeds");
        assert_eq!(plugin.name, "my-plugin");
        assert_eq!(plugin.skills.len(), 1);
        assert_eq!(plugin.skills[0].name, "hello");
        assert_eq!(plugin.mcp_servers.len(), 1);
        assert_eq!(plugin.mcp_servers[0].name, "test");

        // List installed.
        let installed = manager.list_installed().expect("list succeeds");
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "my-plugin");
        assert_eq!(installed[0].versions.len(), 1);

        // Load the installed plugin.
        let loaded = manager.load_latest("my-plugin").expect("load succeeds");
        assert_eq!(loaded.name, "my-plugin");
        assert_eq!(loaded.skills.len(), 1);

        // Uninstall.
        manager.uninstall("my-plugin").expect("uninstall succeeds");
        assert!(manager.list_installed().unwrap().is_empty());
    }

    #[test]
    fn duplicate_install_rejected() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache");
        let data = tmp.path().join("data");
        let source = tmp.path().join("dup-plugin");
        make_plugin_dir("dup-plugin", &source);

        let manager = PluginManager::with_dirs(cache, data);
        manager.install_from_directory(&source).unwrap();

        let err = manager.install_from_directory(&source).unwrap_err();
        assert!(
            matches!(err, PluginManagerError::AlreadyInstalled(..)),
            "expected AlreadyInstalled, got {err:?}"
        );
    }

    #[test]
    fn uninstall_nonexistent_errors() {
        let tmp = TempDir::new().unwrap();
        let manager =
            PluginManager::with_dirs(tmp.path().join("cache"), tmp.path().join("data"));

        let err = manager.uninstall("no-such-plugin").unwrap_err();
        assert!(matches!(err, PluginManagerError::NotInstalled(..)));
    }
}
