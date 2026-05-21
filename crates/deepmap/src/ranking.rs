//! PageRank-powered graph analysis for the codebase dependency graph.
//!
//! Provides symbol ranking, call-chain traversal, hotspot detection,
//! entry-point discovery, file-level metrics, module summaries,
//! reading-order suggestions, and compact symbol summaries.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::types::{RepoGraph, Symbol};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Symbol kinds that carry too little signal for query results.
pub const LOW_SIGNAL_KINDS: &[&str] = &["element", "selector", "class_selector", "id_selector", "json_key"];

/// Names that are almost always boilerplate.
pub const BOILERPLATE_NAMES: &[&str] = &["__init__", "__main__"];

// ---------------------------------------------------------------------------
// PageRank analyser
// ---------------------------------------------------------------------------

/// Drives PageRank computation over a symbol-level dependency graph and
/// provides query and analysis helpers that consume the ranked scores.
pub struct GraphAnalyzer {
    /// Per-symbol PageRank score (symbol_id -> score).
    pub pagerank: HashMap<String, f64>,
}

impl GraphAnalyzer {
    pub fn new() -> Self {
        Self {
            pagerank: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // PageRank computation
    // -----------------------------------------------------------------------

    /// Run standard power-iteration PageRank.
    ///
    /// `graph` is the full symbol dependency graph.  Every symbol becomes a
    /// node.  Outgoing edge weights are summed per node and used to
    /// distribute probability mass to neighbours.  Dangling nodes (no
    /// outgoing edges) contribute to a uniform teleport term.
    ///
    /// Scores are normalised so that the sum of all scores equals 1.0.
    pub fn calculate_pagerank(&mut self, graph: &RepoGraph, damping: f64, max_iter: usize, tol: f64) {
        self.pagerank.clear();

        let n = graph.symbols.len();
        if n == 0 {
            return;
        }

        // Collect node IDs as &str keys that borrow from `graph`.
        let node_ids: Vec<&str> = graph.symbols.keys().map(|s| s.as_str()).collect();

        // --- outgoing weight sums ---
        let mut out_w: HashMap<&str, f64> = HashMap::with_capacity(n);
        for (node, edges) in &graph.outgoing {
            let sum: f64 = edges.iter().map(|e| e.weight).sum();
            out_w.insert(node.as_str(), sum);
        }
        // Nodes without outgoing edges get weight 0.0.
        for node in &node_ids {
            out_w.entry(node).or_insert(0.0);
        }

        // --- reverse incoming index (only edges from nodes with out_w > 0) ---
        let mut inc: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();
        for (_, edges) in &graph.outgoing {
            for edge in edges {
                let src_out = *out_w.get(edge.source.as_str()).unwrap_or(&0.0);
                if src_out > 0.0 {
                    inc.entry(edge.target.as_str())
                        .or_default()
                        .push((edge.source.as_str(), edge.weight));
                }
            }
        }

        // --- initialise PageRank ---
        let init = 1.0 / n as f64;
        let base = (1.0 - damping) / n as f64;
        let mut pr: HashMap<&str, f64> = HashMap::with_capacity(n);
        for node in &node_ids {
            pr.insert(node, init);
        }

        // --- power iteration ---
        for _iter in 0..max_iter {
            let dangling_sum: f64 = node_ids
                .iter()
                .filter(|node| *out_w.get(*node).unwrap_or(&0.0) == 0.0)
                .map(|node| pr.get(node).copied().unwrap_or(0.0))
                .sum();
            let dangling_contrib = dangling_sum / n as f64;

            let mut new_pr: HashMap<&str, f64> = HashMap::with_capacity(n);
            let mut max_delta = 0.0_f64;

            for node in &node_ids {
                let incoming_sum: f64 = match inc.get(node) {
                    Some(edges) => edges
                        .iter()
                        .map(|&(src, w)| {
                            let src_pr = pr.get(src).copied().unwrap_or(0.0);
                            let src_out = *out_w.get(src).unwrap_or(&1.0);
                            if src_out > 0.0 {
                                w * src_pr / src_out
                            } else {
                                0.0
                            }
                        })
                        .sum(),
                    None => 0.0,
                };

                let score = base + damping * (incoming_sum + dangling_contrib);
                new_pr.insert(node, score);

                let old = pr.get(node).copied().unwrap_or(0.0);
                max_delta = max_delta.max((score - old).abs());
            }

            pr = new_pr;
            if max_delta < tol {
                break;
            }
        }

        // --- normalise so total sum = 1.0 ---
        let total: f64 = pr.values().sum();
        if total > 0.0 {
            for score in pr.values_mut() {
                *score /= total;
            }
        }

        // Convert &str keys back to owned Strings.
        self.pagerank = pr.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    }

    /// Read-only access to the computed PageRank scores.
    pub fn pagerank_scores(&self) -> &HashMap<String, f64> {
        &self.pagerank
    }

    // -----------------------------------------------------------------------
    // Query methods
    // -----------------------------------------------------------------------

    /// Case-insensitive substring symbol search, filtered by signal, sorted
    /// by PageRank descending.
    pub fn query_symbol<'a>(&self, name: &str, graph: &'a RepoGraph) -> Vec<&'a Symbol> {
        let lower = name.to_lowercase();
        let mut results: Vec<&'a Symbol> = graph
            .symbols
            .values()
            .filter(|s| {
                !LOW_SIGNAL_KINDS.contains(&s.kind.as_str())
                    && s.name.to_lowercase().contains(&lower)
            })
            .collect();
        results.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// BFS traversal of the call graph.
    ///
    /// Returns a map of `depth_N` -> `Vec<Symbol>` sorted by PageRank.
    ///
    /// `direction` controls which edges are followed:
    /// - `"callers"`  -- walk incoming edges (who calls this symbol)
    /// - `"callees"`  -- walk outgoing edges (whom this symbol calls)
    /// - `"both"`     -- walk both directions
    pub fn call_chain(
        &self,
        symbol_id: &str,
        direction: &str,
        max_depth: usize,
        graph: &RepoGraph,
    ) -> HashMap<String, Vec<Symbol>> {
        const MAX_QUEUE: usize = 10_000;
        const MAX_RESULTS: usize = 1_000;

        let mut result: HashMap<String, Vec<Symbol>> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut total: usize = 0;

        visited.insert(symbol_id.to_string());
        queue.push_back((symbol_id.to_string(), 0));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth || total >= MAX_RESULTS {
                continue;
            }
            if queue.len() > MAX_QUEUE {
                break;
            }

            let neighbors: Vec<(String, f64)> = match direction {
                "callers" => graph
                    .incoming
                    .get(&current)
                    .map(|edges| edges.iter().map(|e| (e.source.clone(), e.weight)).collect())
                    .unwrap_or_default(),
                "callees" => graph
                    .outgoing
                    .get(&current)
                    .map(|edges| edges.iter().map(|e| (e.target.clone(), e.weight)).collect())
                    .unwrap_or_default(),
                "both" => {
                    let mut all = Vec::new();
                    if let Some(inc) = graph.incoming.get(&current) {
                        all.extend(inc.iter().map(|e| (e.source.clone(), e.weight)));
                    }
                    if let Some(out) = graph.outgoing.get(&current) {
                        all.extend(out.iter().map(|e| (e.target.clone(), e.weight)));
                    }
                    all
                }
                _ => Vec::new(),
            };

            for (neighbor, _weight) in &neighbors {
                if !visited.contains(neighbor) && total < MAX_RESULTS {
                    visited.insert(neighbor.clone());
                    if queue.len() < MAX_QUEUE {
                        queue.push_back((neighbor.clone(), depth + 1));
                    }
                    total += 1;

                    let level = format!("depth_{}", depth + 1);
                    if let Some(sym) = graph.symbols.get(neighbor) {
                        result.entry(level).or_default().push(sym.clone());
                    }
                }
            }
        }

        // Sort each depth level by PageRank descending.
        for symbols in result.values_mut() {
            symbols.sort_by(|a, b| {
                b.pagerank
                    .partial_cmp(&a.pagerank)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        result
    }

    /// Files with the highest `symbol_count * avg_pagerank`.
    pub fn hotspots(&self, limit: usize, graph: &RepoGraph) -> Vec<HotspotInfo> {
        let mut file_scores: HashMap<String, Vec<f64>> = HashMap::new();
        let mut file_counts: HashMap<String, usize> = HashMap::new();

        for sym in graph.symbols.values() {
            file_scores.entry(sym.file.clone()).or_default().push(sym.pagerank);
            *file_counts.entry(sym.file.clone()).or_insert(0) += 1;
        }

        let mut results: Vec<HotspotInfo> = file_scores
            .into_iter()
            .map(|(file, scores)| {
                let count = file_counts.remove(&file).unwrap_or(0);
                let avg = if scores.is_empty() {
                    0.0
                } else {
                    scores.iter().sum::<f64>() / scores.len() as f64
                };
                HotspotInfo {
                    file,
                    symbol_count: count,
                    avg_pagerank: avg,
                }
            })
            .collect();

        results.sort_by(|a, b| {
            let sa = a.symbol_count as f64 * a.avg_pagerank;
            let sb = b.symbol_count as f64 * b.avg_pagerank;
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    /// Identify well-known entry-point files by stem or path pattern.
    pub fn entry_points(&self, graph: &RepoGraph) -> Vec<String> {
        let entry_stems: HashSet<&str> =
            ["main", "app", "index", "server", "run", "setup", "cli", "__main__"]
                .iter()
                .copied()
                .collect();

        let mut entries: Vec<String> = Vec::new();

        for file in graph.file_symbols.keys() {
            let path = Path::new(file);
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if entry_stems.contains(stem) {
                    entries.push(file.clone());
                    continue;
                }
            }
            // Also match specific common paths.
            let normalised = file.replace('\\', "/");
            if normalised.ends_with("/src/main.tsx") || normalised.ends_with("/lib.rs") {
                if !entries.contains(file) {
                    entries.push(file.clone());
                }
            }
        }

        entries.sort();
        entries
    }

    /// Per-file symbol count, edge counts, and average PageRank.
    pub fn file_analysis(&self, graph: &RepoGraph) -> HashMap<String, FileMetrics> {
        let mut raw: HashMap<String, (usize, usize, usize, f64)> = HashMap::new();

        for (sym_id, sym) in &graph.symbols {
            let entry = raw.entry(sym.file.clone()).or_default();
            entry.0 += 1; // symbol count
            entry.1 += graph.outgoing.get(sym_id).map(|v| v.len()).unwrap_or(0);
            entry.2 += graph.incoming.get(sym_id).map(|v| v.len()).unwrap_or(0);
            entry.3 += sym.pagerank;
        }

        raw.into_iter()
            .map(|(file, (cnt, out, inc, pr_sum))| {
                let avg = if cnt > 0 { pr_sum / cnt as f64 } else { 0.0 };
                (
                    file,
                    FileMetrics {
                        symbol_count: cnt,
                        outgoing_edges: out,
                        incoming_edges: inc,
                        avg_pagerank: avg,
                    },
                )
            })
            .collect()
    }

    /// Group symbols by top-level directory, sort by total PageRank descending.
    pub fn module_summary(&self, limit: usize, graph: &RepoGraph) -> Vec<ModuleInfo> {
        let mut modules: HashMap<String, (usize, f64)> = HashMap::new();

        for sym in graph.symbols.values() {
            let module = Path::new(&sym.file)
                .components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
                .unwrap_or("root")
                .to_string();
            let entry = modules.entry(module).or_default();
            entry.0 += 1;
            entry.1 += sym.pagerank;
        }

        let mut result: Vec<ModuleInfo> = modules
            .into_iter()
            .map(|(module, (cnt, pr))| ModuleInfo {
                module,
                symbol_count: cnt,
                total_pagerank: pr,
            })
            .collect();

        result.sort_by(|a, b| {
            b.total_pagerank
                .partial_cmp(&a.total_pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result.truncate(limit);
        result
    }

    /// Suggest files to read first, scored by `avg_pr * ln(sym_count) * entry_boost`.
    ///
    /// Test and noise files are excluded.
    pub fn suggested_reading_order(&self, limit: usize, graph: &RepoGraph) -> Vec<ReadingOrderEntry> {
        let entry_stems: HashSet<&str> =
            ["main", "app", "index", "server", "run", "setup", "cli", "__main__"]
                .iter()
                .copied()
                .collect();
        let noise_patterns: &[&str] = &["test", "spec", ".test.", ".spec.", "__test__", "node_modules"];

        // Collect per-file stats and entry-point flag.
        let mut file_counts: HashMap<&str, usize> = HashMap::new();
        let mut file_pr_sums: HashMap<&str, f64> = HashMap::new();
        let mut file_is_entry: HashMap<&str, bool> = HashMap::new();

        for sym in graph.symbols.values() {
            *file_counts.entry(sym.file.as_str()).or_insert(0) += 1;
            *file_pr_sums.entry(sym.file.as_str()).or_insert(0.0) += sym.pagerank;

            file_is_entry.entry(sym.file.as_str()).or_insert_with(|| {
                let path = Path::new(&sym.file);
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                entry_stems.contains(stem)
                    || sym.file.ends_with("/src/main.tsx")
                    || sym.file.ends_with("/lib.rs")
            });
        }

        let mut entries: Vec<ReadingOrderEntry> = Vec::new();

        for (file_str, count) in &file_counts {
            let file: &str = file_str;

            // Skip noise files.
            if noise_patterns.iter().any(|p| file.contains(p)) {
                continue;
            }

            let pr_sum = file_pr_sums.get(file).copied().unwrap_or(0.0);
            let avg_pr = if *count > 0 { pr_sum / *count as f64 } else { 0.0 };
            let boost = if *file_is_entry.get(file).unwrap_or(&false) {
                2.0
            } else {
                1.0
            };
            let ln_count = if *count > 1 {
                (*count as f64).ln()
            } else {
                0.0
            };
            let score = avg_pr * ln_count * boost;

            entries.push(ReadingOrderEntry {
                file: file.to_string(),
                score,
            });
        }

        entries.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries.truncate(limit);
        entries
    }

    /// For the top `limit_files` files (by reading order), return up to
    /// `per_file` symbols sorted by a composite importance score.
    ///
    /// Low-signal kinds are excluded.
    pub fn summary_symbols(
        &self,
        limit_files: usize,
        per_file: usize,
        graph: &RepoGraph,
    ) -> Vec<SymbolSummary> {
        let reading_order = self.suggested_reading_order(limit_files, graph);

        let mut summaries = Vec::new();

        for entry in &reading_order {
            let sym_ids = match graph.file_symbols.get(&entry.file) {
                Some(ids) => ids,
                None => continue,
            };

            let mut file_syms: Vec<&Symbol> = sym_ids
                .iter()
                .filter_map(|id| graph.symbols.get(id))
                .filter(|s| !LOW_SIGNAL_KINDS.contains(&s.kind.as_str()))
                .collect();

            // Sort by composite importance: incoming_calls*3 + outgoing_calls*2 + kind_weight.
            file_syms.sort_by(|a, b| {
                let ia = incoming_edge_count(a, graph) as f64 * 3.0
                    + outgoing_edge_count(a, graph) as f64 * 2.0
                    + sym_kind_weight(&a.kind);
                let ib = incoming_edge_count(b, graph) as f64 * 3.0
                    + outgoing_edge_count(b, graph) as f64 * 2.0
                    + sym_kind_weight(&b.kind);
                ib.partial_cmp(&ia)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            for sym in file_syms.iter().take(per_file) {
                summaries.push(SymbolSummary {
                    file: sym.file.clone(),
                    name: sym.name.clone(),
                    kind: sym.kind.clone(),
                    line: sym.line,
                    pagerank: sym.pagerank,
                    signature: sym.signature.clone(),
                });
            }
        }

        summaries
    }
}

// ===========================================================================
// Helper functions
// ===========================================================================

fn incoming_edge_count(sym: &Symbol, graph: &RepoGraph) -> usize {
    graph.incoming.get(&sym.id).map(|v| v.len()).unwrap_or(0)
}

fn outgoing_edge_count(sym: &Symbol, graph: &RepoGraph) -> usize {
    graph.outgoing.get(&sym.id).map(|v| v.len()).unwrap_or(0)
}

fn sym_kind_weight(kind: &str) -> f64 {
    match kind {
        "function" | "method" | "constructor" => 5.0,
        "class" | "struct" | "interface" | "trait" | "impl" | "enum" => 4.0,
        "module" | "namespace" => 3.0,
        "variable" | "constant" => 2.0,
        _ => 1.0,
    }
}

// ===========================================================================
// Public result types
// ===========================================================================

/// Per-file hotspot info for the `hotspots()` query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HotspotInfo {
    pub file: String,
    pub symbol_count: usize,
    pub avg_pagerank: f64,
}

/// Per-file metrics for the `file_analysis()` query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileMetrics {
    pub symbol_count: usize,
    pub outgoing_edges: usize,
    pub incoming_edges: usize,
    pub avg_pagerank: f64,
}

/// Module-level summary for the `module_summary()` query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModuleInfo {
    pub module: String,
    pub symbol_count: usize,
    pub total_pagerank: f64,
}

/// Reading-order entry for the `suggested_reading_order()` query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReadingOrderEntry {
    pub file: String,
    pub score: f64,
}

/// Compact symbol info for the `summary_symbols()` query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SymbolSummary {
    pub file: String,
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub pagerank: f64,
    pub signature: String,
}
