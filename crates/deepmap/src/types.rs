// Core data types for DeepMap: symbols, edges, graph structure, and scan statistics.

use std::collections::HashMap;

/// File extension to language mapping.
pub fn ext_to_lang(ext: &str) -> Option<&'static str> {
    Some(match ext {
        ".py" | ".pyi" => "python",
        ".js" | ".jsx" | ".mjs" | ".cjs" => "javascript",
        ".ts" | ".tsx" | ".mts" | ".cts" => "typescript",
        ".go" => "go",
        ".rs" => "rust",
        ".java" => "java",
        ".kt" | ".kts" => "kotlin",
        ".swift" => "swift",
        ".cpp" | ".cc" | ".cxx" | ".hpp" | ".h" => "cpp",
        ".cs" => "csharp",
        ".php" => "php",
        ".rb" => "ruby",
        ".html" | ".htm" => "html",
        ".css" => "css",
        ".json" => "json",
        _ => return None,
    })
}

/// Directory names to skip during file traversal.
pub const SKIP_DIR_NAMES: &[&str] = &[
    ".cache", ".git", ".hg", ".idea", ".mypy_cache", ".next", ".nox",
    ".nuxt", ".parcel-cache", ".pnpm-store", ".pytest_cache", ".ruff_cache",
    ".svelte-kit", ".tox", ".turbo", ".venv", ".vscode", ".yarn",
    "__pypackages__", "__pycache__", "build", "coverage", "dist", "env",
    "ENV", "node_modules", "site-packages", "target", "venv",
    "monaco-editor", "monaco", "vendor", "third_party", "third-party",
    "libs", "external",
];

/// File names to skip.
pub const SKIP_FILE_NAMES: &[&str] = &[
    "package-lock.json", "npm-shrinkwrap.json", "bun.lock", "bun.lockb",
    "yarn.lock", "pnpm-lock.yaml", "Cargo.lock",
];

/// Default maximum file size in bytes (512 KB).
pub const DEFAULT_MAX_FILE_BYTES: u64 = 512 * 1024;

/// A code symbol (function, class, interface, etc.).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub col: usize,
    pub visibility: String,
    pub docstring: String,
    pub signature: String,
    pub pagerank: f64,
}

impl Symbol {
    pub fn new(
        id: String, name: String, kind: String, file: String,
        line: usize, end_line: usize, col: usize,
        visibility: String, docstring: String, signature: String,
    ) -> Self {
        Self { id, name, kind, file, line, end_line, col, visibility, docstring, signature, pagerank: 0.0 }
    }
}

/// A directed edge between two symbols in the dependency graph.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub weight: f64,
    pub kind: String,
}

/// JS/TS import binding.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JsImportBinding {
    pub local_name: String,
    pub imported_name: String,
    pub module: String,
    pub line: usize,
    pub kind: String,
}

/// JS/TS export binding.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JsExportBinding {
    pub exported_name: String,
    pub source_name: Option<String>,
    pub module: Option<String>,
    pub line: usize,
    pub kind: String,
}

/// Path alias rule (e.g., tsconfig paths).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathAliasRule {
    pub alias_pattern: String,
    pub target_patterns: Vec<String>,
}

/// Project import configuration (tsconfig/jsconfig).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectImportConfig {
    pub config_path: Option<String>,
    pub config_dir: Option<String>,
    pub base_url: Option<String>,
    pub alias_rules: Vec<PathAliasRule>,
}

/// Scan statistics.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ScanStats {
    pub listed_source_files: usize,
    pub selected_source_files: usize,
    pub processed_files: usize,
    pub filtered_path_files: usize,
    pub filtered_large_files: usize,
    pub truncated_files: usize,
    pub failed_files: Vec<String>,
    pub scan_duration_ms: u64,
    pub timeout_triggered: bool,
}

/// The repository dependency graph.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RepoGraph {
    pub symbols: HashMap<String, Symbol>,
    pub outgoing: HashMap<String, Vec<Edge>>,
    pub incoming: HashMap<String, Vec<Edge>>,
    pub file_symbols: HashMap<String, Vec<String>>,
    pub file_imports: HashMap<String, Vec<String>>,
    pub file_calls: HashMap<String, Vec<(String, usize, String)>>,
    pub file_import_bindings: HashMap<String, Vec<JsImportBinding>>,
    pub file_exports: HashMap<String, Vec<JsExportBinding>>,
}

/// File cache entry for incremental scan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileCacheEntry {
    pub mtime: u64,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<String>,
    pub calls: Vec<(String, usize, String)>,
}

/// Incremental scan baseline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IncrementalCache {
    pub git_head: String,
    pub files: HashMap<String, FileCacheEntry>,
    pub import_configs: Vec<ProjectImportConfig>,
}
