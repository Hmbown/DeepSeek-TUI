// RepoMapEngine -- codebase analysis pipeline.
//
// Orchestrates file discovery, tree-sitter parsing, dependency-graph
// construction, PageRank ranking, and query delegation to GraphAnalyzer.
// Supports incremental scanning via on-disk cache and session-level
// in-memory cache for repeated `get_or_scan` calls.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ignore::WalkBuilder;

use crate::parser::TreeSitterAdapter;
use crate::ranking::GraphAnalyzer;
use crate::resolver::ImportResolver;
use crate::types::*;

// ---------------------------------------------------------------------------
// Edge weights
// ---------------------------------------------------------------------------

const IMPORT_WEIGHT: f64 = 0.35;
const CALL_WEIGHT: f64 = 0.50;

// ---------------------------------------------------------------------------
// Session-level scan cache
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct CachedScan {
    graph: RepoGraph,
    pagerank: HashMap<String, f64>,
}

/// Session-level cache keyed by canonical project root.
/// Avoids re-scanning the same project multiple times in one session.
static SCAN_CACHE: std::sync::LazyLock<Mutex<HashMap<PathBuf, CachedScan>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Supporting file patterns
// ---------------------------------------------------------------------------

const SUPPORTING_FILE_NAMES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "README.md",
    "CONTRIBUTING.md",
    "CHANGELOG.md",
    "LICENSE",
    "LICENSE.txt",
    "LICENSE.md",
    "Makefile",
    "Makefile.common",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "package.json",
    "package-lock.json",
    "tsconfig.json",
    "tsconfig.build.json",
    "jsconfig.json",
    ".eslintrc",
    ".eslintrc.json",
    ".eslintrc.js",
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.js",
    ".stylelintrc",
    ".stylelintrc.json",
    "babel.config.js",
    "babel.config.json",
    "webpack.config.js",
    "vite.config.js",
    "vite.config.ts",
    "next.config.js",
    "next.config.ts",
    "Cargo.toml",
    "Cargo.lock",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "setup.cfg",
    "go.mod",
    "go.sum",
    "Gemfile",
    "Gemfile.lock",
    "Podfile",
    "Podfile.lock",
    ".gitignore",
    ".dockerignore",
    ".env.example",
    ".env.sample",
    "docker-compose.override.yml",
    ".editorconfig",
    ".nvmrc",
    ".node-version",
    ".python-version",
    ".ruby-version",
    "rust-toolchain.toml",
    "rust-toolchain",
    "clippy.toml",
    ".clang-format",
    ".clang-tidy",
];

// ---------------------------------------------------------------------------
// RepoMapEngine
// ---------------------------------------------------------------------------

/// Top-level analysis engine that owns the dependency graph, scan state,
/// and all query capabilities via delegation to `GraphAnalyzer`.
pub struct RepoMapEngine {
    /// Root directory of the project being analysed.
    project_root: PathBuf,
    /// Tree-sitter adapter for parsing source files.
    ts: TreeSitterAdapter,
    /// The dependency graph built during the last scan.
    graph: RepoGraph,
    /// In-memory modification-time cache keyed by relative file path.
    mtime_cache: HashMap<String, u64>,
    /// Tracks file paths that were selected during the current scan.
    scan_state: Vec<String>,
    /// Maximum file size (in bytes) for source files.
    max_file_bytes: u64,
    /// Accumulated scan statistics from the latest scan.
    scan_stats: ScanStats,
    /// Import resolver (path-alias aware).
    resolver: ImportResolver,
    /// Graph analysis engine (PageRank + queries).
    analyzer: GraphAnalyzer,
    /// Session-persistent incremental cache (loaded from disk).
    inc_cache: Option<IncrementalCache>,
}

impl RepoMapEngine {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new engine for the given project root.
    ///
    /// Reads `DEEPMAP_MAX_FILE_BYTES` from the environment if set;
    /// otherwise falls back to `DEFAULT_MAX_FILE_BYTES`.
    pub fn new(project_root: &Path) -> Self {
        let max_file_bytes = std::env::var("DEEPMAP_MAX_FILE_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_FILE_BYTES);

        let root = project_root.to_path_buf();
        let resolver = ImportResolver::new(&root, &[]);
        let inc_cache = None;

        Self {
            project_root: root,
            ts: TreeSitterAdapter::new(),
            graph: RepoGraph::default(),
            mtime_cache: HashMap::new(),
            scan_state: Vec::new(),
            max_file_bytes,
            scan_stats: ScanStats::default(),
            resolver,
            analyzer: GraphAnalyzer::new(),
            inc_cache,
        }
    }

    /// Return a cached engine for `project_root` if one exists, otherwise
    /// scan and cache the result.
    pub fn get_or_scan(
        project_root: &Path,
        max_files: usize,
        max_scan_secs: u64,
    ) -> Self {
        let canonical = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());

        // Check the session-level cache first.
        {
            let cache = SCAN_CACHE.lock().expect("SCAN_CACHE poisoned");
            if let Some(cached) = cache.get(&canonical) {
                let mut engine = Self::new(project_root);
                engine.graph = cached.graph.clone();
                engine.analyzer.pagerank_scores();
                // Restore PageRank into the analyzer.
                for (id, pr) in &cached.pagerank {
                    if let Some(sym) = engine.graph.symbols.get_mut(id) {
                        sym.pagerank = *pr;
                    }
                }
                // Note: we don't restore analyzer.pagerank directly because
                // it's recalculated below. Instead, set it from cache.
                engine.analyzer = {
                    let mut a = GraphAnalyzer::new();
                    let mut pr_map = HashMap::new();
                    for (id, _) in &engine.graph.symbols {
                        pr_map.insert(
                            id.clone(),
                            engine.graph.symbols[id].pagerank,
                        );
                    }
                    // Re-run a quick PageRank to populate the analyzer.
                    // We still want the analyzer to have the right scores.
                    // This is idempotent.
                    a.calculate_pagerank(&engine.graph, 0.85, 50, 1e-6);
                    a
                };
                engine.scan_stats.processed_files =
                    engine.graph.symbols.len();
                return engine;
            }
        }

        // Not cached -- perform a full scan.
        let mut engine = Self::new(project_root);
        engine.scan(max_files, max_scan_secs);

        // Cache the result.
        {
            let mut cache = SCAN_CACHE.lock().expect("SCAN_CACHE poisoned");
            cache.insert(
                canonical,
                CachedScan {
                    graph: engine.graph.clone(),
                    pagerank: engine.analyzer.pagerank_scores().clone(),
                },
            );
        }

        engine
    }

    // -----------------------------------------------------------------------
    // Scan pipeline
    // -----------------------------------------------------------------------

    /// 4-phase scan: list files, process files (parse + extract), build edges,
    /// calculate PageRank.
    pub fn scan(&mut self, max_files: usize, max_scan_secs: u64) {
        let start = Instant::now();
        let timeout = if max_scan_secs > 0 {
            Some(Duration::from_secs(max_scan_secs))
        } else {
            None
        };

        // Reset scan state.
        self.scan_stats = ScanStats::default();
        self.scan_state.clear();

        // ---- Phase 1: List files -----------------------------------------
        let all_files = self._list_files();
        self.scan_stats.listed_source_files = all_files.len();

        let file_limit = if max_files > 0 && max_files < all_files.len() {
            max_files
        } else {
            all_files.len()
        };
        self.scan_stats.selected_source_files = file_limit;

        // ---- Phase 2: Process files --------------------------------------
        // Attempt incremental cache restore.
        self.inc_cache = self.load_incremental_cache();
        let changed_files = self
            .inc_cache
            .as_ref()
            .map(|_| self._git_changed_files())
            .unwrap_or_default();

        // Clear the graph for a fresh rebuild.
        self.graph = RepoGraph::default();
        self.mtime_cache.clear();

        for file_path in all_files.iter().take(file_limit) {
            // Check timeout.
            if let Some(t) = timeout {
                if start.elapsed() > t {
                    self.scan_stats.timeout_triggered = true;
                    break;
                }
            }

            let rel_path = file_path
                .strip_prefix(&self.project_root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .to_string();

            // Incremental: restore unchanged files from cache.
            // Clone the cache entry first to avoid simultaneous immutable
            // and mutable borrows of `self`.
            let cached_entry = self.inc_cache.as_ref().and_then(|cache| {
                if !changed_files.contains(&rel_path) {
                    cache.files.get(&rel_path).cloned()
                } else {
                    None
                }
            });
            let restored = if let Some(entry) = cached_entry {
                self._restore_from_cache(&rel_path, &entry);
                self.scan_stats.processed_files += 1;
                true
            } else {
                false
            };

            if restored {
                continue;
            }

            // Determine language from file extension.
            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let ext_with_dot = format!(".{}", ext);
            let lang = match ext_to_lang(&ext_with_dot) {
                Some(l) => l,
                None => continue, // Shouldn't happen since _list_files already filters.
            };

            match self._process_file(file_path, &rel_path, lang) {
                Ok(()) => {
                    self.scan_stats.processed_files += 1;
                    self.scan_state.push(rel_path);
                }
                Err(e) => {
                    self.scan_stats.failed_files.push(rel_path);
                    log::warn!("Failed to process file: {}", e);
                }
            }
        }

        // Clean up mtime_cache: remove entries for files not in this scan.
        self.mtime_cache
            .retain(|k, _| self.scan_state.contains(k) || self.graph.file_symbols.contains_key(k));

        // ---- Phase 3: Build edges ----------------------------------------
        self._build_edges();

        // ---- Phase 4: PageRank -------------------------------------------
        self._calculate_pagerank(0.85, 50, 1e-6);

        // Wrap up stats.
        self.scan_stats.scan_duration_ms = start.elapsed().as_millis() as u64;
    }

    // -----------------------------------------------------------------------
    // Private scan helpers
    // -----------------------------------------------------------------------

    /// Walk the project directory and collect source files that match
    /// a known language extension, respecting skip lists and file-size limits.
    fn _list_files(&mut self) -> Vec<PathBuf> {
        let walker = WalkBuilder::new(&self.project_root)
            .git_ignore(true)
            .hidden(false)
            .max_depth(None)
            .build();

        let mut files: Vec<PathBuf> = Vec::new();

        for result in walker {
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Check file extension.
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let ext_with_dot = format!(".{}", ext);
            if ext_to_lang(&ext_with_dot).is_none() {
                continue;
            }

            // Skip user-defined directory names.
            if path
                .components()
                .any(|c| {
                    if let std::path::Component::Normal(name) = c {
                        SKIP_DIR_NAMES.contains(&name.to_str().unwrap_or(""))
                    } else {
                        false
                    }
                })
            {
                self.scan_stats.filtered_path_files += 1;
                continue;
            }

            // Skip user-defined file names.
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if SKIP_FILE_NAMES.contains(&file_name) {
                    self.scan_stats.filtered_path_files += 1;
                    continue;
                }
            }

            // Check file size.
            match path.metadata() {
                Ok(meta) if meta.len() <= self.max_file_bytes => {}
                Ok(_) => {
                    self.scan_stats.filtered_large_files += 1;
                    continue;
                }
                Err(_) => continue,
            }

            files.push(path.to_path_buf());
        }

        files
    }

    /// Parse a single source file, extract symbols / imports / calls /
    /// bindings / exports, and populate the graph.
    fn _process_file(
        &mut self,
        path: &Path,
        rel_path: &str,
        lang: &str,
    ) -> Result<(), String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Read error ({}): {}", rel_path, e))?;

        self.ts
            .parse(path, &content, lang)
            .map_err(|e| format!("Parse error ({}): {}", rel_path, e))?;

        // -- Symbols --------------------------------------------------------
        let symbols = self.ts.symbols();
        for sym in &symbols {
            self.graph.symbols.insert(sym.id.clone(), sym.clone());
            self.graph
                .file_symbols
                .entry(rel_path.to_string())
                .or_default()
                .push(sym.id.clone());
        }

        // -- Imports --------------------------------------------------------
        let imports = self.ts.imports();
        self.graph
            .file_imports
            .insert(rel_path.to_string(), imports);

        // -- Calls ----------------------------------------------------------
        let calls = self.ts.calls();
        let call_tuples: Vec<(String, usize, String)> = calls
            .iter()
            .map(|(name, line, kind)| {
                (name.clone(), *line, kind.clone())
            })
            .collect();
        self.graph
            .file_calls
            .insert(rel_path.to_string(), call_tuples);

        // -- JS/TS import bindings ------------------------------------------
        let bindings = self.ts.import_bindings();
        self.graph
            .file_import_bindings
            .insert(rel_path.to_string(), bindings);

        // -- JS/TS exports --------------------------------------------------
        let exports = self.ts.exports();
        self.graph
            .file_exports
            .insert(rel_path.to_string(), exports);

        // Mark exported symbols (updates visibility for JS/TS).
        if let Some(export_bindings) = self.graph.file_exports.get(rel_path) {
            for binding in export_bindings {
                let target_name = binding
                    .source_name
                    .as_deref()
                    .unwrap_or(&binding.exported_name);
                if let Some(symbols) = self.graph.file_symbols.get(rel_path) {
                    for sym_id in symbols {
                        if let Some(sym) = self.graph.symbols.get_mut(sym_id) {
                            if sym.name == target_name
                                || sym.name == binding.exported_name
                            {
                                sym.visibility = "export".to_string();
                            }
                        }
                    }
                }
            }
        }

        // -- Mtime cache ----------------------------------------------------
        let mtime = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.mtime_cache.insert(rel_path.to_string(), mtime);

        Ok(())
    }

    /// Restore a file's data from the incremental cache (avoid re-parsing).
    fn _restore_from_cache(
        &mut self,
        file: &str,
        entry: &FileCacheEntry,
    ) {
        self.mtime_cache.insert(file.to_string(), entry.mtime);
        self.scan_state.push(file.to_string());

        for sym in &entry.symbols {
            self.graph.symbols.insert(sym.id.clone(), sym.clone());
            self.graph
                .file_symbols
                .entry(file.to_string())
                .or_default()
                .push(sym.id.clone());
        }

        self.graph
            .file_imports
            .insert(file.to_string(), entry.imports.clone());

        self.graph
            .file_calls
            .insert(file.to_string(), entry.calls.clone());
    }

    /// Build dependency edges from imports and calls.
    ///
    /// Iterates by reference to avoid cloning large maps.
    fn _build_edges(&mut self) {
        let mut new_outgoing: HashMap<String, Vec<Edge>> = HashMap::new();
        let mut new_incoming: HashMap<String, Vec<Edge>> = HashMap::new();

        // Resolve call target name to a symbol id.
        let resolve_symbol = |name: &str, graph: &RepoGraph| -> Option<String> {
            graph
                .symbols
                .values()
                .find(|s| s.name == name)
                .map(|s| s.id.clone())
        };

        // ---- Import edges ------------------------------------------------
        for (file, imports) in &self.graph.file_imports {
            let source_symbols: Vec<String> = self
                .graph
                .file_symbols
                .get(file)
                .cloned()
                .unwrap_or_default();

            for import_path in imports {
                let target_file =
                    match self.resolver.resolve_import(file, import_path) {
                        Some(f) => f,
                        None => continue,
                    };

                let target_symbols: Vec<String> = self
                    .graph
                    .file_symbols
                    .get(&target_file)
                    .cloned()
                    .unwrap_or_default();

                for src_id in &source_symbols {
                    for tgt_id in &target_symbols {
                        let edge = Edge {
                            source: src_id.clone(),
                            target: tgt_id.clone(),
                            weight: IMPORT_WEIGHT,
                            kind: "import".to_string(),
                        };
                        new_outgoing
                            .entry(src_id.clone())
                            .or_default()
                            .push(edge.clone());
                        new_incoming
                            .entry(tgt_id.clone())
                            .or_default()
                            .push(edge);
                    }
                }
            }
        }

        // ---- Call edges --------------------------------------------------
        for (file, calls) in &self.graph.file_calls {
            let calling_symbols: Vec<String> = self
                .graph
                .file_symbols
                .get(file)
                .cloned()
                .unwrap_or_default();

            for (target_name, _line, _kind) in calls {
                let target_id = match resolve_symbol(target_name, &self.graph)
                {
                    Some(id) => id,
                    None => continue,
                };

                for src_id in &calling_symbols {
                    let edge = Edge {
                        source: src_id.clone(),
                        target: target_id.clone(),
                        weight: CALL_WEIGHT,
                        kind: "call".to_string(),
                    };
                    new_outgoing
                        .entry(src_id.clone())
                        .or_default()
                        .push(edge.clone());
                    new_incoming
                        .entry(target_id.clone())
                        .or_default()
                        .push(edge);
                }
            }
        }

        self.graph.outgoing = new_outgoing;
        self.graph.incoming = new_incoming;
    }

    /// Calculate PageRank on the current graph and update all symbol scores.
    fn _calculate_pagerank(
        &mut self,
        damping: f64,
        max_iter: usize,
        tol: f64,
    ) {
        self.analyzer
            .calculate_pagerank(&self.graph, damping, max_iter, tol);

        // Propagate PageRank scores back into each Symbol.
        for (id, pr) in self.analyzer.pagerank_scores() {
            if let Some(sym) = self.graph.symbols.get_mut(id) {
                sym.pagerank = *pr;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Git helpers for incremental scan
    // -----------------------------------------------------------------------

    /// Returns a list of files changed since HEAD.
    fn _git_changed_files(&self) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["diff", "--name-only", "HEAD"])
            .current_dir(&self.project_root)
            .output();

        match output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Returns the current git HEAD revision hash, or None.
    fn _get_git_head(&self) -> Option<String> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.project_root)
            .output()
            .ok()?;

        if output.status.success() {
            Some(
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string(),
            )
        } else {
            None
        }
    }

    // -----------------------------------------------------------------------
    // Supporting files
    // -----------------------------------------------------------------------

    /// Return paths of well-known supporting / config files that exist
    /// in the project root. These files are valuable context but don't
    /// require AST parsing.
    pub fn supporting_files(&self) -> Vec<PathBuf> {
        SUPPORTING_FILE_NAMES
            .iter()
            .map(|name| self.project_root.join(name))
            .filter(|p| p.exists())
            .collect()
    }

    // -----------------------------------------------------------------------
    // Incremental cache persistence
    // -----------------------------------------------------------------------

    /// Persist the current scan state to an incremental cache file
    /// (`.deepmap_cache.json`) in the project root.
    pub fn save_incremental_cache(&self) {
        let cache_path = self.project_root.join(".deepmap_cache.json");

        let git_head = self._get_git_head().unwrap_or_default();

        let mut files: HashMap<String, FileCacheEntry> = HashMap::new();
        for (file, mtime) in &self.mtime_cache {
            let symbols: Vec<Symbol> = self
                .graph
                .file_symbols
                .get(file)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| self.graph.symbols.get(id))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            let imports = self
                .graph
                .file_imports
                .get(file)
                .cloned()
                .unwrap_or_default();

            let calls = self
                .graph
                .file_calls
                .get(file)
                .cloned()
                .unwrap_or_default();

            files.insert(
                file.clone(),
                FileCacheEntry {
                    mtime: *mtime,
                    symbols,
                    imports,
                    calls,
                },
            );
        }

        let cache = IncrementalCache {
            git_head,
            files,
            import_configs: Vec::new(), // Resolver configs kept separately.
        };

        if let Ok(json) = serde_json::to_string(&cache) {
            let _ = std::fs::write(&cache_path, json);
        }
    }

    /// Load an incremental cache from disk if one exists and is valid.
    pub fn load_incremental_cache(&self) -> Option<IncrementalCache> {
        let cache_path = self.project_root.join(".deepmap_cache.json");
        let content = std::fs::read_to_string(cache_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    // -----------------------------------------------------------------------
    // Query interface (delegates to GraphAnalyzer)
    // -----------------------------------------------------------------------

    /// Case-insensitive symbol lookup, sorted by PageRank descending.
    pub fn query_symbol(&self, name: &str) -> Vec<&Symbol> {
        self.analyzer.query_symbol(name, &self.graph)
    }

    /// BFS call-chain traversal.
    pub fn call_chain(
        &self,
        symbol_id: &str,
        direction: &str,
        max_depth: usize,
    ) -> HashMap<String, Vec<Symbol>> {
        self.analyzer
            .call_chain(symbol_id, direction, max_depth, &self.graph)
    }

    /// Hotspot files ranked by `symbol_count * avg_pagerank`.
    pub fn hotspots(&self, limit: usize) -> Vec<HashMap<String, String>> {
        self.analyzer.hotspots(limit, &self.graph)
    }

    /// Application entry-point files.
    pub fn entry_points(&self) -> Vec<String> {
        self.analyzer.entry_points(&self.graph)
    }

    /// Per-file analysis statistics.
    pub fn file_analysis(&self) -> HashMap<String, HashMap<String, f64>> {
        self.analyzer.file_analysis(&self.graph)
    }

    /// Module-level summary (top-level directories) sorted by total PageRank.
    pub fn module_summary(
        &self,
        limit: usize,
    ) -> Vec<HashMap<String, String>> {
        self.analyzer.module_summary(limit, &self.graph)
    }

    /// Suggested reading order for files in the project.
    pub fn suggested_reading_order(
        &self,
        limit: usize,
    ) -> Vec<HashMap<String, String>> {
        self.analyzer.suggested_reading_order(limit, &self.graph)
    }

    /// Top symbols from the top files.
    pub fn summary_symbols(
        &self,
        limit_files: usize,
        per_file: usize,
    ) -> Vec<HashMap<String, String>> {
        self.analyzer
            .summary_symbols(limit_files, per_file, &self.graph)
    }

    /// One-line-per-bullet summary of the last scan, suitable for Markdown.
    pub fn scan_summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let edge_count: usize = self
            .graph
            .outgoing
            .values()
            .map(|v| v.len())
            .sum();

        lines.push(format!(
            "Processed files: {}",
            self.scan_stats.processed_files
        ));
        lines.push(format!("Symbols found: {}", self.graph.symbols.len()));
        lines.push(format!("Dependency edges: {}", edge_count));
        if self.scan_stats.scan_duration_ms > 0 {
            lines.push(format!(
                "Scan duration: {} ms",
                self.scan_stats.scan_duration_ms
            ));
        }
        if self.scan_stats.timeout_triggered {
            lines.push(
                "WARNING: scan timed out \u{2014} results may be incomplete"
                    .to_string(),
            );
        }
        if !self.scan_stats.failed_files.is_empty() {
            lines.push(format!(
                "Failed files: {}",
                self.scan_stats.failed_files.len()
            ));
        }
        if self.scan_stats.filtered_path_files > 0 {
            lines.push(format!(
                "Skipped (path rules): {}",
                self.scan_stats.filtered_path_files
            ));
        }
        if self.scan_stats.filtered_large_files > 0 {
            lines.push(format!(
                "Skipped (too large): {}",
                self.scan_stats.filtered_large_files
            ));
        }
        if self.scan_stats.truncated_files > 0 {
            lines.push(format!(
                "Truncated files: {}",
                self.scan_stats.truncated_files
            ));
        }
        lines
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Reference to the project root path.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Reference to the dependency graph.
    pub fn graph(&self) -> &RepoGraph {
        &self.graph
    }

    /// Reference to the scan statistics.
    pub fn scan_stats(&self) -> &ScanStats {
        &self.scan_stats
    }

    /// Reference to the PageRank scores from the analyzer.
    pub fn pagerank_scores(&self) -> &HashMap<String, f64> {
        self.analyzer.pagerank_scores()
    }
}
