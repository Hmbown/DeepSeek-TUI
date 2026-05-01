// RepoMap engine — coordinates scanning, graph building, PageRank, and queries.
//
// Three-phase scan: list files → parse & extract symbols → build edges → PageRank.
// Supports incremental rescan with mtime-based caching.
// Includes a session-level scan cache so repeated tool calls reuse the same scan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use crate::parser::TreeSitterAdapter;
use crate::ranking::GraphAnalyzer;
use crate::resolver::ImportResolver;
use crate::types::*;

use ignore::WalkBuilder;

/// Edge weight constants (matching Python version).
const IMPORT_WEIGHT: f64 = 0.35;
const CALL_WEIGHT: f64 = 0.50;

/// Session-level scan cache: (workspace path) → cached scan data.
/// Reuses scan results across multiple tool calls within the same session.
static SCAN_CACHE: std::sync::LazyLock<Mutex<HashMap<PathBuf, CachedScan>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cached result of a full scan — lightweight, cloneable, no tree-sitter state.
#[derive(Clone)]
struct CachedScan {
    graph: RepoGraph,
    pagerank: HashMap<String, f64>,
}

/// The main repository map engine.
pub struct RepoMapEngine {
    pub project_root: PathBuf,
    pub ts: TreeSitterAdapter,
    pub graph: RepoGraph,
    /// File path → last mtime (for incremental rescan).
    mtime_cache: HashMap<String, u64>,
    pub scan_state: String,
    pub max_file_bytes: u64,
    pub scan_stats: ScanStats,
    resolver: Option<ImportResolver>,
    analyzer: Option<GraphAnalyzer>,
}

impl RepoMapEngine {
    pub fn new(project_root: &Path) -> Self {
        let max_file_bytes = std::env::var("DEEPMAP_MAX_FILE_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_FILE_BYTES);

        Self {
            project_root: project_root.to_path_buf(),
            ts: TreeSitterAdapter::new(),
            graph: RepoGraph::default(),
            mtime_cache: HashMap::new(),
            scan_state: "idle".into(),
            max_file_bytes,
            scan_stats: ScanStats::default(),
            resolver: None,
            analyzer: None,
        }
    }

    /// Get or create a scanned engine for the given project root.
    /// Uses the session cache to avoid re-scanning on repeated calls.
    pub fn get_or_scan(project_root: &Path, max_files: usize, max_scan_time_secs: f64) -> Self {
        let canonical = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());

        // Try cache first.
        if let Some(cached) = SCAN_CACHE
            .lock()
            .ok()
            .and_then(|cache| cache.get(&canonical).cloned())
        {
            let mut engine = Self::new(&canonical);
            engine.graph = cached.graph;
            engine.scan_state = "scanned".into();
            engine.analyzer = Some(GraphAnalyzer::new());
            if let Some(ref analyzer) = engine.analyzer {
                // Re-load pagerank scores from cache.
                for (id, score) in &cached.pagerank {
                    if let Some(sym) = engine.graph.symbols.get_mut(id) {
                        sym.pagerank = *score;
                    }
                }
            }
            engine.scan_stats.processed_files = engine.graph.file_symbols.len();
            log::info!("DeepMap: using cached scan for {}", canonical.display());
            return engine;
        }

        // Do a fresh scan.
        log::info!("DeepMap: scanning {} ...", canonical.display());
        let mut engine = Self::new(&canonical);
        engine.scan(max_files, max_scan_time_secs);

        // Store in cache.
        if engine.is_scanned() {
            if let Some(ref analyzer) = engine.analyzer {
                let pagerank = analyzer.pagerank_scores().clone();
                if let Ok(mut cache) = SCAN_CACHE.lock() {
                    cache.insert(
                        canonical,
                        CachedScan {
                            graph: engine.graph.clone(),
                            pagerank,
                        },
                    );
                }
            }
        }

        engine
    }

    pub fn is_scanned(&self) -> bool {
        self.scan_state == "scanned"
    }

    // ── Main scan flow ──

    /// Three-phase scan: list files → extract symbols → build edges → PageRank.
    pub fn scan(&mut self, max_files: usize, max_scan_time_secs: f64) {
        let start = Instant::now();
        self.scan_state = "invalid".to_string();

        if self.ts.parsers.is_empty() {
            // At minimum, we should have at least one parser loaded.
            // If none loaded, scan will be empty — warn and continue.
        }

        self.graph = RepoGraph::default();
        self.mtime_cache.clear();
        self.scan_stats = ScanStats::default();

        let files = self.list_files(max_files);
        log::info!("Found {} source files", files.len());

        for f in &files {
            // Timeout guard.
            if start.elapsed().as_secs_f64() > max_scan_time_secs {
                self.scan_stats.timeout_triggered = true;
                log::warn!(
                    "Scan timeout triggered: exceeded {}s limit",
                    max_scan_time_secs
                );
                break;
            }

            if let Err(e) = self.process_file(f) {
                if self.scan_stats.failed_files.len() < 5 {
                    self.scan_stats.failed_files.push(format!("{}: {}", f, e));
                }
            }
        }

        self.build_edges();
        self.analyzer = Some(GraphAnalyzer::new());
        self.calculate_pagerank();
        self.scan_state = "scanned".to_string();

        self.scan_stats.scan_duration_ms = start.elapsed().as_millis() as u64;

        let sym_count = self.graph.symbols.len();
        let edge_count: usize = self.graph.outgoing.values().map(|v| v.len()).sum();
        log::info!(
            "Scan complete — {} symbols, {} edges, {}ms",
            sym_count,
            edge_count,
            self.scan_stats.scan_duration_ms
        );
    }

    // ── File listing ──

    fn list_files(&mut self, max_files: usize) -> Vec<String> {
        let valid_exts: Vec<&str> = vec![
            ".py", ".pyi", ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".go",
            ".rs", ".html", ".htm", ".css", ".json",
        ];
        let valid_set: std::collections::HashSet<&str> = valid_exts.into_iter().collect();

        let mut candidates = Vec::new();
        let walker = WalkBuilder::new(&self.project_root)
            .hidden(false)
            .git_ignore(true)
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().map_or(false, |t| t.is_file()) {
                continue;
            }
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let dot_ext = format!(".{}", ext);
            if !valid_set.contains(dot_ext.as_str()) {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(&self.project_root) {
                candidates.push(rel.to_string_lossy().to_string());
            }
        }

        self.scan_stats.listed_source_files = candidates.len();

        // Filter out paths to skip.
        let filtered: Vec<String> = candidates
            .into_iter()
            .filter(|f| !self.should_skip_path(f))
            .collect();

        self.scan_stats.selected_source_files = filtered.len().min(max_files);

        if filtered.len() > max_files {
            self.scan_stats.truncated_files = filtered.len() - max_files;
        }
        filtered.into_iter().take(max_files).collect()
    }

    fn should_skip_path(&self, file: &str) -> bool {
        let path = Path::new(file);
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".min.js") {
                return true;
            }
            if SKIP_FILE_NAMES.contains(&name) {
                return true;
            }
        }
        path.components().any(|c| {
            if let std::path::Component::Normal(part) = c {
                SKIP_DIR_NAMES.contains(&part.to_str().unwrap_or(""))
            } else {
                false
            }
        })
    }

    // ── File processing ──

    fn process_file(&mut self, file: &str) -> Result<(), String> {
        let path = self.project_root.join(file);
        if !path.exists() {
            return Ok(());
        }

        // Check file size.
        if let Ok(meta) = path.metadata() {
            let mtime = meta
                .modified()
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
                .unwrap_or(0);

            if mtime > 0 && self.mtime_cache.get(file) == Some(&mtime) {
                return Ok(()); // Unchanged, skip.
            }

            if meta.len() > self.max_file_bytes {
                self.scan_stats.filtered_large_files += 1;
                return Ok(());
            }

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let dot_ext = format!(".{}", ext);
            let lang = match crate::types::ext_to_lang(&dot_ext) {
                Some(l) => l,
                None => return Ok(()),
            };

            if !self.ts.parsers.contains_key(lang) {
                return Ok(());
            }

            let content = std::fs::read(&path).map_err(|e| e.to_string())?;
            let tree = match self.ts.parse(&content, lang) {
                Some(t) => t,
                None => return Ok(()),
            };

            let source = String::from_utf8_lossy(&content);

            // Extract symbols.
            let symbols = self.ts.extract_symbols(&tree, lang, file, &source);
            for sym in &symbols {
                self.graph.symbols.insert(sym.id.clone(), sym.clone());
            }
            self.graph.file_symbols.insert(
                file.to_string(),
                symbols.iter().map(|s| s.id.clone()).collect(),
            );

            // Extract imports.
            let imports = self.ts.extract_imports(&tree, lang, &source);
            let import_modules: Vec<String> = imports.iter().map(|(m, _)| m.clone()).collect();

            // Extract JS/TS import bindings.
            let import_bindings = self.ts.extract_js_ts_import_bindings(&tree, lang, &source);
            let mut all_modules = import_modules.clone();
            for b in &import_bindings {
                if !b.module.is_empty() && !all_modules.contains(&b.module) {
                    all_modules.push(b.module.clone());
                }
            }
            all_modules.sort();
            all_modules.dedup();
            self.graph
                .file_imports
                .insert(file.to_string(), all_modules);
            self.graph
                .file_import_bindings
                .insert(file.to_string(), import_bindings);

            // Extract exports.
            let exports = self.ts.extract_js_ts_export_bindings(&tree, lang, &source);
            self.graph.file_exports.insert(file.to_string(), exports);
            self.mark_exported_symbols(file);

            // Extract calls.
            let calls = self.ts.extract_calls(&tree, lang, &source);
            self.graph.file_calls.insert(file.to_string(), calls);

            // Cache mtime.
            self.mtime_cache.insert(file.to_string(), mtime);
            self.scan_stats.processed_files += 1;

            // Clean stale cache entries.
            let project_root = self.project_root.clone();
            self.mtime_cache
                .retain(|k, _| project_root.join(k).exists());
        }

        Ok(())
    }

    fn mark_exported_symbols(&mut self, file: &str) {
        let exports = match self.graph.file_exports.get(file) {
            Some(e) => e,
            None => return,
        };
        let exported_names: std::collections::HashSet<&str> = exports
            .iter()
            .filter(|b| b.module.is_none() && b.source_name.as_deref() != Some("*"))
            .filter_map(|b| b.source_name.as_deref())
            .collect();

        if exported_names.is_empty() {
            return;
        }

        for sym_id in self
            .graph
            .file_symbols
            .get(file)
            .iter()
            .flat_map(|v| v.iter())
        {
            if let Some(sym) = self.graph.symbols.get_mut(sym_id) {
                if exported_names.contains(sym.name.as_str()) {
                    sym.visibility = "exported".to_string();
                }
            }
        }
    }

    // ── Edge building ──

    fn build_edges(&mut self) {
        let resolver = ImportResolver::new(&self.project_root, &self.graph);
        // Build import edges and call edges.
        let mut edges_to_add: Vec<(String, Edge)> = Vec::new();

        // Collect all file-symbol mappings for quick lookup.
        for (file, calls) in &self.graph.file_calls.clone() {
            for (call_name, call_line, call_kind) in calls {
                if let Some(target_id) = resolver.resolve_call_target(
                    file,
                    call_name,
                    *call_line,
                    call_kind,
                    &HashMap::new(), // TODO: file local_names
                ) {
                    if let Some(containing_sym) =
                        resolver.resolve_calling_symbol_with_graph(file, *call_line, &self.graph)
                    {
                        let edge = Edge {
                            source: containing_sym,
                            target: target_id,
                            weight: CALL_WEIGHT,
                            kind: "call".into(),
                        };
                        edges_to_add.push((edge.source.clone(), edge));
                    }
                }
            }
        }

        // Build import edges.
        for (file, imports) in &self.graph.file_imports.clone() {
            for imp in imports {
                let targets = resolver.resolve_import_targets(file, imp);
                for target_file in &targets {
                    if let Some(target_syms) = self.graph.file_symbols.get(target_file) {
                        for src_sym_id in self
                            .graph
                            .file_symbols
                            .get(file)
                            .iter()
                            .flat_map(|v| v.iter())
                        {
                            for tgt_sym_id in target_syms {
                                let edge = Edge {
                                    source: src_sym_id.clone(),
                                    target: tgt_sym_id.clone(),
                                    weight: IMPORT_WEIGHT,
                                    kind: "import".into(),
                                };
                                edges_to_add.push((edge.source.clone(), edge));
                            }
                        }
                    }
                }
            }
        }

        // Insert edges into graph.
        for (source, edge) in edges_to_add {
            let target_id = edge.target.clone();
            let kind = edge.kind.clone();
            let weight = edge.weight;
            self.graph
                .outgoing
                .entry(source.clone())
                .or_default()
                .push(edge);
            self.graph
                .incoming
                .entry(target_id.clone())
                .or_default()
                .push(Edge {
                    source: target_id,
                    target: source,
                    weight,
                    kind,
                });
        }

        self.resolver = Some(resolver);
    }

    // ── PageRank ──

    fn calculate_pagerank(&mut self) {
        let analyzer = self.analyzer.as_mut().expect("analyzer not initialized");
        // Transfer graph data to the analyzer's internal structures.
        analyzer.load_graph(&self.graph);
        analyzer.calculate_pagerank(&self.graph, 0.85, 50, 1e-6);
        // Write PageRank scores back to symbols.
        let scores = analyzer.pagerank_scores();
        for (sym_id, score) in scores {
            if let Some(sym) = self.graph.symbols.get_mut(sym_id) {
                sym.pagerank = *score;
            }
        }
    }

    // ── Query interface ──

    pub fn query_symbol(&self, name: &str) -> Vec<&crate::types::Symbol> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return Vec::new(),
        };
        analyzer.query_symbol(name, &self.graph)
    }

    pub fn call_chain(
        &self,
        symbol_id: &str,
        direction: &str,
        max_depth: usize,
    ) -> HashMap<String, Vec<crate::types::Symbol>> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return HashMap::new(),
        };
        analyzer.call_chain(symbol_id, direction, max_depth, &self.graph)
    }

    pub fn hotspots(&self, limit: usize) -> Vec<HashMap<String, String>> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return Vec::new(),
        };
        analyzer.hotspots(limit, &self.graph)
    }

    pub fn entry_points(&self) -> Vec<String> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return Vec::new(),
        };
        analyzer.entry_points(&self.graph)
    }

    pub fn file_analysis(&self) -> HashMap<String, HashMap<String, f64>> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return HashMap::new(),
        };
        analyzer.file_analysis(&self.graph)
    }

    pub fn module_summary(&self, limit: usize) -> Vec<HashMap<String, String>> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return Vec::new(),
        };
        analyzer.module_summary(limit, &self.graph)
    }

    pub fn suggested_reading_order(&self, limit: usize) -> Vec<HashMap<String, String>> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return Vec::new(),
        };
        analyzer.suggested_reading_order(limit, &self.graph)
    }

    pub fn summary_symbols(
        &self,
        limit_files: usize,
        per_file: usize,
    ) -> Vec<HashMap<String, String>> {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return Vec::new(),
        };
        analyzer.summary_symbols(limit_files, per_file, &self.graph)
    }

    // ── Scan summary ──

    pub fn scan_summary_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("- Files processed: {}", self.scan_stats.processed_files),
            format!("- Symbols: {}", self.graph.symbols.len()),
            format!(
                "- Edges: {}",
                self.graph.outgoing.values().map(|v| v.len()).sum::<usize>()
            ),
            format!("- Filtered paths: {}", self.scan_stats.filtered_path_files),
            format!(
                "- Filtered large files: {}",
                self.scan_stats.filtered_large_files
            ),
        ];
        if let Some(ref resolver) = self.resolver {
            lines.push(format!(
                "- Import configs: {}",
                resolver.import_configs.len()
            ));
        }
        if self.scan_stats.timeout_triggered {
            lines.push("- ⚠️ Scan timeout triggered: results may be incomplete".to_string());
        }
        if !self.scan_stats.failed_files.is_empty() {
            lines.push(format!(
                "- Failed files: {}",
                self.scan_stats.failed_files.len()
            ));
            for ff in self.scan_stats.failed_files.iter().take(3) {
                lines.push(format!("  - {}", ff));
            }
        }
        lines
    }
}
