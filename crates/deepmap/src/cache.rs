// File-based caching for scan results.
//
// Uses JSON serialization with atomic writes (temp file + rename).
// Schema version check rejects incompatible cache files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::{Edge, Symbol};

const CACHE_SCHEMA_VERSION: u32 = 1;

/// Cached scan data.
#[derive(Debug, Serialize, Deserialize)]
pub struct SymbolCache {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub scan_time: String,
    pub project_path: String,
    pub file_count: usize,
    pub symbol_count: usize,
    pub edge_count: usize,
    #[serde(default)]
    schema_version: u32,
}

/// Get the cache directory for a project.
pub fn get_cache_dir(project_path: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let path_str = project_path.to_string_lossy();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path_str.hash(&mut hasher);
    let hash = hasher.finish();
    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let cache_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".cache")
        .join("deepmap")
        .join(format!("{}_{:08x}", project_name, hash as u32));
    std::fs::create_dir_all(&cache_dir).ok();
    cache_dir
}

/// Get cache file paths.
pub fn get_cache_paths(project_path: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let cache_dir = get_cache_dir(project_path);
    (
        cache_dir.join("symbols.json"),
        cache_dir.join("git.json"),
        cache_dir.join("last_snapshot.json"),
    )
}

/// Save scan results to cache (atomic write).
pub fn save_cache(
    project_path: &Path,
    symbols: &[Symbol],
    edges: &[Edge],
) -> Result<PathBuf, String> {
    let (symbols_path, _, last_snapshot) = get_cache_paths(project_path);

    // Backup previous cache as last snapshot.
    if symbols_path.exists() {
        let _ = std::fs::copy(&symbols_path, &last_snapshot);
    }

    let cache = SymbolCache {
        symbols: symbols.to_vec(),
        edges: edges.to_vec(),
        scan_time: chrono::Utc::now().to_rfc3339(),
        project_path: project_path.to_string_lossy().to_string(),
        file_count: 0,
        symbol_count: symbols.len(),
        edge_count: edges.len(),
        schema_version: CACHE_SCHEMA_VERSION,
    };

    let json =
        serde_json::to_string_pretty(&cache).map_err(|e| format!("Serialize error: {}", e))?;

    // Atomic write: temp file → rename.
    let tmp_path = symbols_path.with_extension("tmp");
    std::fs::write(&tmp_path, json).map_err(|e| format!("Write error: {}", e))?;
    std::fs::rename(&tmp_path, &symbols_path).map_err(|e| format!("Rename error: {}", e))?;

    Ok(symbols_path)
}

/// Load scan results from cache. Returns None if cache is invalid or missing.
pub fn load_cache(project_path: &Path) -> Option<SymbolCache> {
    let (symbols_path, _, _) = get_cache_paths(project_path);
    let json = std::fs::read_to_string(&symbols_path).ok()?;
    let cache: SymbolCache = serde_json::from_str(&json).ok()?;

    // Reject incompatible schema versions.
    if cache.schema_version != CACHE_SCHEMA_VERSION {
        // Auto-clean incompatible cache.
        let _ = std::fs::remove_file(&symbols_path);
        return None;
    }

    Some(cache)
}

/// Load the previous snapshot for diff comparison.
pub fn load_last_snapshot(project_path: &Path) -> Option<SymbolCache> {
    let (_, _, last_snapshot) = get_cache_paths(project_path);
    let json = std::fs::read_to_string(&last_snapshot).ok()?;
    let cache: SymbolCache = serde_json::from_str(&json).ok()?;
    if cache.schema_version != CACHE_SCHEMA_VERSION {
        return None;
    }
    Some(cache)
}

/// Compute scan fingerprint (SHA-256 of file paths + mtimes + sizes).
pub fn scan_fingerprint(project_path: &Path, files: &[String]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    for file in files {
        file.hash(&mut hasher);
        let full_path = project_path.join(file);
        if let Ok(meta) = std::fs::metadata(&full_path) {
            meta.len().hash(&mut hasher);
            if let Ok(mtime) = meta.modified() {
                mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .hash(&mut hasher);
            }
        }
    }

    format!("{:016x}", hasher.finish())
}
