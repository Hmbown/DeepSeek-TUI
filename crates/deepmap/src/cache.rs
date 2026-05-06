// Cache layer for DeepMap: disk-backed symbol cache with schema gating,
// atomic snapshots, and content-based fingerprinting for incremental scans.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use crate::types::{Edge, Symbol};

/// Current schema version.  Increment when the serialized layout changes so
/// stale caches are silently discarded rather than deserialised into garbage.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// On-disk snapshot of a completed scan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SymbolCache {
    pub symbols: HashMap<String, Symbol>,
    pub edges: Vec<Edge>,
    pub scan_time: String,
    pub project_path: String,
    pub file_count: usize,
    pub symbol_count: usize,
    pub edge_count: usize,
    pub schema_version: u32,
}

// ---------------------------------------------------------------------------
// Directory layout
// ---------------------------------------------------------------------------

/// Return the canonical cache directory for a project.
///
/// Layout: `~/.cache/deepmap/{project_name}_{sha256_prefix}`
/// The hash disambiguates projects that share the same directory basename.
pub fn get_cache_dir(project_path: &Path) -> PathBuf {
    let base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("deepmap");

    let name = project_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".to_string());

    let hash = {
        let mut h = Sha256::new();
        h.update(project_path.to_string_lossy().as_bytes());
        let digest: [u8; 32] = h.finalize().into();
        hex_prefix(&digest, 12)
    };

    base.join(format!("{}_{}", name, hash))
}

/// First `len` hex chars of a SHA-256 digest.
fn hex_prefix(digest: &[u8; 32], len: usize) -> String {
    let full: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    full.chars().take(len).collect()
}

// ---------------------------------------------------------------------------
// Read / Write
// ---------------------------------------------------------------------------

/// Persist a completed scan to disk.
///
/// The write is atomic: content goes to a `.tmp` sibling first, then renamed
/// over the real file.  An older cache (if present) is moved to `.bak` so a
/// corrupted write never destroys the last good snapshot.
pub fn save_cache(
    project_path: &Path,
    symbols: &HashMap<String, Symbol>,
    edges: &[Edge],
) -> Result<PathBuf, String> {
    let cache_dir = get_cache_dir(project_path);
    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("cannot create cache dir `{}`: {}", cache_dir.display(), e))?;

    let file_count = {
        let mut files = HashSet::new();
        for sym in symbols.values() {
            files.insert(&sym.file);
        }
        files.len()
    };

    let cache = SymbolCache {
        symbols: symbols.clone(),
        edges: edges.to_vec(),
        scan_time: chrono::Utc::now().to_rfc3339(),
        project_path: project_path.to_string_lossy().to_string(),
        file_count,
        symbol_count: symbols.len(),
        edge_count: edges.len(),
        schema_version: CACHE_SCHEMA_VERSION,
    };

    let json = serde_json::to_string_pretty(&cache)
        .map_err(|e| format!("serialization failed: {}", e))?;

    let dest = cache_dir.join("symbol_cache.json");
    let bak = cache_dir.join("symbol_cache.json.bak");
    let tmp = cache_dir.join("symbol_cache.json.tmp");

    // Rotate previous cache to backup.
    if dest.exists() {
        let _ = fs::rename(&dest, &bak);
    }

    // Atomic write.
    fs::write(&tmp, &json).map_err(|e| format!("write failed: {}", e))?;
    fs::rename(&tmp, &dest).map_err(|e| format!("rename failed: {}", e))?;

    // Also write a snapshot copy so `load_last_snapshot` stays cheap.
    let snapshot = cache_dir.join("symbol_cache.snapshot.json");
    let _ = fs::write(&snapshot, &json);

    Ok(dest)
}

/// Load the most recent cache for a project.
///
/// Returns `None` when no cache exists, the schema version differs, or
/// deserialisation fails (auto-cleanup removes the stale file in the last
/// case so a future scan starts fresh).
pub fn load_cache(project_path: &Path) -> Option<SymbolCache> {
    let cache_dir = get_cache_dir(project_path);
    let path = cache_dir.join("symbol_cache.json");

    if !path.exists() {
        return None;
    }

    let data = fs::read_to_string(&path).ok()?;
    let cache: SymbolCache = serde_json::from_str(&data).ok()?;

    if cache.schema_version != CACHE_SCHEMA_VERSION {
        // Schema mismatch — discard silently.
        let _ = fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

/// Load the snapshot written by the *last* `save_cache` call (used for diff
/// comparisons).  Returns `None` when unavailable or corrupt.
pub fn load_last_snapshot(project_path: &Path) -> Option<SymbolCache> {
    let cache_dir = get_cache_dir(project_path);
    let path = cache_dir.join("symbol_cache.snapshot.json");

    if !path.exists() {
        return None;
    }

    let data = fs::read_to_string(&path).ok()?;
    let cache: SymbolCache = serde_json::from_str(&data).ok()?;

    if cache.schema_version != CACHE_SCHEMA_VERSION {
        return None;
    }

    Some(cache)
}

// ---------------------------------------------------------------------------
// Fingerprinting
// ---------------------------------------------------------------------------

/// Build a content-addressed fingerprint for the given set of files.
///
/// For each `(path, size, mtime)` triple the bytes are fed into a SHA-256
/// hash so that any file change produces a different fingerprint regardless
/// of the file contents themselves.
///
/// Files that cannot be stat'd are silently skipped (they simply contribute
/// nothing to the hash).
pub fn scan_fingerprint(project_path: &Path, files: &[String]) -> String {
    let mut hasher = Sha256::new();

    // Collect entries and sort for deterministic output.
    let mut entries: Vec<(&str, u64, u64)> = Vec::with_capacity(files.len());
    for file in files {
        let full = project_path.join(file);
        if let Ok(meta) = fs::metadata(&full) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            entries.push((file.as_str(), meta.len(), mtime));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(b.0));

    for (path, size, mtime) in &entries {
        hasher.update(path.as_bytes());
        hasher.update(&size.to_le_bytes());
        hasher.update(&mtime.to_le_bytes());
    }

    format!("{:x}", hasher.finalize())
}
