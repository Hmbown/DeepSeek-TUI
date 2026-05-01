// Graph ranking and analysis: PageRank, call chains, hotspots, entry points.

use std::collections::{HashMap, VecDeque};

use crate::types::*;

/// Low-signal symbol kinds (markup, selectors, etc.) — excluded from ranking.
const LOW_SIGNAL_KINDS: &[&str] = &[
    "element",
    "selector",
    "class_selector",
    "id_selector",
    "json_key",
];

/// Boilerplate names to exclude.
const BOILERPLATE_NAMES: &[&str] = &["__init__"];

pub struct GraphAnalyzer {
    /// Symbol ID → PageRank score.
    pagerank: HashMap<String, f64>,
}

impl GraphAnalyzer {
    pub fn new() -> Self {
        Self {
            pagerank: HashMap::new(),
        }
    }

    /// Load graph data into the analyzer (transfer ownership of edges).
    pub fn load_graph(&mut self, _graph: &RepoGraph) {
        // The graph is already stored in the engine. The analyzer
        // only needs the PageRank algorithm, which operates on
        // outgoing/incoming edge maps directly.
        self.pagerank.clear();
    }

    pub fn pagerank_scores(&self) -> &HashMap<String, f64> {
        &self.pagerank
    }

    // ── PageRank ──

    /// Standard power-iteration PageRank with convergence detection.
    pub fn calculate_pagerank(
        &mut self,
        graph: &RepoGraph,
        damping: f64,
        max_iter: usize,
        tol: f64,
    ) {
        let sym_ids: Vec<&String> = graph.symbols.keys().collect();
        let n = sym_ids.len();
        if n == 0 {
            return;
        }

        let mut pr: HashMap<String, f64> = sym_ids
            .iter()
            .map(|id| ((*id).clone(), 1.0 / n as f64))
            .collect();

        // Compute outgoing weight sums.
        let out_w: HashMap<String, f64> = sym_ids
            .iter()
            .map(|id| {
                let sum = graph
                    .outgoing
                    .get(*id)
                    .map(|edges| edges.iter().map(|e| e.weight).sum())
                    .unwrap_or(0.0);
                ((*id).clone(), sum)
            })
            .collect();

        let active_srcs: std::collections::HashSet<&String> = out_w
            .iter()
            .filter(|(_, w)| **w > 0.0)
            .map(|(id, _)| id)
            .collect();

        // Build reverse incoming map: target → [(source, weight)]
        let mut inc: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for (src, edges) in &graph.outgoing {
            if active_srcs.contains(src) {
                for e in edges {
                    inc.entry(e.target.clone())
                        .or_default()
                        .push((src.clone(), e.weight));
                }
            }
        }

        let base = (1.0 - damping) / n as f64;

        for _ in 0..max_iter {
            let mut new_pr: HashMap<String, f64> = HashMap::new();
            for id in &sym_ids {
                let score = base
                    + inc.get(*id).map_or(0.0, |sources| {
                        sources
                            .iter()
                            .map(|(src, w)| {
                                damping * pr.get(src).unwrap_or(&0.0) * w
                                    / out_w.get(src).unwrap_or(&1.0)
                            })
                            .sum()
                    });
                new_pr.insert((*id).clone(), score);
            }

            let total: f64 = new_pr.values().sum();
            if total > 0.0 {
                for v in new_pr.values_mut() {
                    *v /= total;
                }
            }

            let delta = sym_ids
                .iter()
                .map(|id| (new_pr.get(*id).unwrap_or(&0.0) - pr.get(*id).unwrap_or(&0.0)).abs())
                .fold(0.0f64, f64::max);

            pr = new_pr;
            if delta < tol {
                break;
            }
        }

        self.pagerank = pr;
    }

    // ── Symbol query ──

    pub fn query_symbol<'a>(&self, name: &str, graph: &'a RepoGraph) -> Vec<&'a Symbol> {
        let lower = name.to_lowercase();
        let mut matches: Vec<&Symbol> = graph
            .symbols
            .values()
            .filter(|s| s.name.to_lowercase().contains(&lower))
            .filter(|s| !LOW_SIGNAL_KINDS.contains(&s.kind.as_str()))
            .collect();

        matches.sort_by(|a, b| {
            self.pagerank
                .get(&b.id)
                .unwrap_or(&0.0)
                .partial_cmp(self.pagerank.get(&a.id).unwrap_or(&0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches
    }

    // ── Call chain ──

    pub fn call_chain(
        &self,
        symbol_id: &str,
        direction: &str,
        max_depth: usize,
        graph: &RepoGraph,
    ) -> HashMap<String, Vec<Symbol>> {
        let mut result = HashMap::new();

        if direction == "callers" || direction == "both" {
            result.insert(
                "callers".into(),
                self.trace_chain(symbol_id, "incoming", max_depth, graph),
            );
        }
        if direction == "callees" || direction == "both" {
            result.insert(
                "callees".into(),
                self.trace_chain(symbol_id, "outgoing", max_depth, graph),
            );
        }
        result
    }

    fn trace_chain(
        &self,
        start_id: &str,
        edge_dir: &str,
        max_depth: usize,
        graph: &RepoGraph,
    ) -> Vec<Symbol> {
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut result = Vec::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        queue.push_back((start_id.to_string(), 0));
        visited.insert(start_id.to_string());

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let neighbors = if edge_dir == "incoming" {
                graph.incoming.get(&current_id)
            } else {
                graph.outgoing.get(&current_id)
            };

            if let Some(edges) = neighbors {
                for edge in edges {
                    let neighbor_id = if edge_dir == "incoming" {
                        &edge.source
                    } else {
                        &edge.target
                    };
                    if visited.insert(neighbor_id.clone()) {
                        if let Some(sym) = graph.symbols.get(neighbor_id) {
                            result.push(sym.clone());
                        }
                        queue.push_back((neighbor_id.clone(), depth + 1));
                    }
                }
            }
        }

        result.sort_by(|a, b| {
            self.pagerank
                .get(&b.id)
                .unwrap_or(&0.0)
                .partial_cmp(self.pagerank.get(&a.id).unwrap_or(&0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result
    }

    // ── Hotspots ──

    /// Identify high-density files (most symbols, highest PageRank).
    pub fn hotspots(&self, limit: usize, graph: &RepoGraph) -> Vec<HashMap<String, String>> {
        let mut file_scores: HashMap<&str, (usize, f64)> = HashMap::new();

        for (file, sym_ids) in &graph.file_symbols {
            let count = sym_ids.len();
            let avg_pr: f64 = if count > 0 {
                sym_ids
                    .iter()
                    .map(|id| self.pagerank.get(id).unwrap_or(&0.0))
                    .sum::<f64>()
                    / count as f64
            } else {
                0.0
            };
            file_scores.insert(file.as_str(), (count, avg_pr));
        }

        let mut entries: Vec<(&str, (usize, f64))> = file_scores.into_iter().collect();
        entries.sort_by(|a, b| {
            b.1.0.cmp(&a.1.0).then(
                b.1.1
                    .partial_cmp(&a.1.1)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });

        entries
            .into_iter()
            .take(limit)
            .map(|(file, (count, avg_pr))| {
                let mut m = HashMap::new();
                m.insert("file".into(), file.to_string());
                m.insert("symbols".into(), count.to_string());
                m.insert("avg_pagerank".into(), format!("{:.6}", avg_pr));
                m
            })
            .collect()
    }

    // ── Entry points ──

    /// Identify entry-point files (main, app, index, etc.).
    pub fn entry_points(&self, graph: &RepoGraph) -> Vec<String> {
        let entry_names = [
            "main", "app", "index", "server", "run", "setup", "cli", "__main__",
        ];

        let mut entries: Vec<(&str, usize)> = graph
            .file_symbols
            .keys()
            .filter_map(|file| {
                let stem = std::path::Path::new(file)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if entry_names.contains(&stem) {
                    let sym_count = graph.file_symbols.get(file).map(|v| v.len()).unwrap_or(0);
                    Some((file.as_str(), sym_count))
                } else {
                    None
                }
            })
            .collect();

        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries.into_iter().map(|(f, _)| f.to_string()).collect()
    }

    // ── File analysis ──

    /// Compute complexity and connectivity metrics per file.
    pub fn file_analysis(&self, graph: &RepoGraph) -> HashMap<String, HashMap<String, f64>> {
        let mut analysis = HashMap::new();

        for (file, sym_ids) in &graph.file_symbols {
            let mut metrics = HashMap::new();
            metrics.insert("symbol_count".into(), sym_ids.len() as f64);

            let out_edges: usize = sym_ids
                .iter()
                .map(|id| graph.outgoing.get(id).map(|e| e.len()).unwrap_or(0))
                .sum();
            let in_edges: usize = sym_ids
                .iter()
                .map(|id| graph.incoming.get(id).map(|e| e.len()).unwrap_or(0))
                .sum();

            metrics.insert("outgoing_edges".into(), out_edges as f64);
            metrics.insert("incoming_edges".into(), in_edges as f64);

            let avg_pr: f64 = if sym_ids.is_empty() {
                0.0
            } else {
                sym_ids
                    .iter()
                    .map(|id| self.pagerank.get(id).unwrap_or(&0.0))
                    .sum::<f64>()
                    / sym_ids.len() as f64
            };
            metrics.insert("avg_pagerank".into(), avg_pr);

            analysis.insert(file.clone(), metrics);
        }
        analysis
    }

    // ── Module summary ──

    /// Group symbols by top-level directory/module.
    pub fn module_summary(&self, limit: usize, graph: &RepoGraph) -> Vec<HashMap<String, String>> {
        let mut modules: HashMap<String, (usize, f64)> = HashMap::new();

        for (file, sym_ids) in &graph.file_symbols {
            let module = std::path::Path::new(file)
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .unwrap_or_else(|| ".".into());

            let entry = modules.entry(module).or_insert((0, 0.0));
            entry.0 += sym_ids.len();
            let sum_pr: f64 = sym_ids
                .iter()
                .map(|id| self.pagerank.get(id).unwrap_or(&0.0))
                .sum();
            entry.1 += sum_pr;
        }

        let mut entries: Vec<(String, (usize, f64))> = modules.into_iter().collect();
        entries.sort_by(|a, b| b.1.0.cmp(&a.1.0));

        entries
            .into_iter()
            .take(limit)
            .map(|(name, (count, total_pr))| {
                let mut m = HashMap::new();
                m.insert("module".into(), name);
                m.insert("symbols".into(), count.to_string());
                m.insert("total_pagerank".into(), format!("{:.6}", total_pr));
                m
            })
            .collect()
    }

    // ── Suggested reading order ──

    /// Generate recommended reading order for AI consumption.
    pub fn suggested_reading_order(
        &self,
        limit: usize,
        graph: &RepoGraph,
    ) -> Vec<HashMap<String, String>> {
        let entry_names = ["main", "app", "index", "server", "run", "setup", "cli"];

        let mut file_scores: Vec<(&str, f64)> = graph
            .file_symbols
            .keys()
            .filter(|file| {
                let stem = std::path::Path::new(file)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                // Exclude test/noise files.
                !file.contains("test")
                    && !file.contains("__pycache__")
                    && !LOW_SIGNAL_KINDS.contains(&stem)
            })
            .map(|file| {
                let sym_count = graph.file_symbols.get(file).map(|v| v.len()).unwrap_or(0) as f64;
                let avg_pr: f64 = graph
                    .file_symbols
                    .get(file)
                    .map(|ids| {
                        if ids.is_empty() {
                            0.0
                        } else {
                            ids.iter()
                                .map(|id| self.pagerank.get(id).unwrap_or(&0.0))
                                .sum::<f64>()
                                / ids.len() as f64
                        }
                    })
                    .unwrap_or(0.0);
                // Entry points get a boost.
                let entry_boost = if entry_names.contains(
                    &std::path::Path::new(file)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(""),
                ) {
                    2.0
                } else {
                    1.0
                };
                let score = avg_pr * sym_count.ln().max(1.0) * entry_boost;
                (file.as_str(), score)
            })
            .collect();

        file_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        file_scores
            .into_iter()
            .take(limit)
            .map(|(file, score)| {
                let mut m = HashMap::new();
                m.insert("file".into(), file.to_string());
                m.insert("score".into(), format!("{:.4}", score));
                m
            })
            .collect()
    }

    // ── Summary symbols ──

    /// Return key implementation symbols suitable for overview display.
    pub fn summary_symbols(
        &self,
        limit_files: usize,
        per_file: usize,
        graph: &RepoGraph,
    ) -> Vec<HashMap<String, String>> {
        let reading_order = self.suggested_reading_order(limit_files, graph);

        let mut result = Vec::new();
        for entry in &reading_order {
            let file = entry.get("file").map(|s| s.as_str()).unwrap_or("");
            if let Some(sym_ids) = graph.file_symbols.get(file) {
                let mut syms: Vec<&Symbol> = sym_ids
                    .iter()
                    .filter_map(|id| graph.symbols.get(id))
                    .filter(|s| !LOW_SIGNAL_KINDS.contains(&s.kind.as_str()))
                    .filter(|s| !BOILERPLATE_NAMES.contains(&s.name.as_str()))
                    .collect();

                syms.sort_by(|a, b| {
                    self.pagerank
                        .get(&b.id)
                        .unwrap_or(&0.0)
                        .partial_cmp(self.pagerank.get(&a.id).unwrap_or(&0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                for sym in syms.iter().take(per_file) {
                    let mut m = HashMap::new();
                    m.insert("file".into(), file.to_string());
                    m.insert("name".into(), sym.name.clone());
                    m.insert("kind".into(), sym.kind.clone());
                    m.insert("line".into(), sym.line.to_string());
                    m.insert(
                        "pagerank".into(),
                        format!("{:.6}", self.pagerank.get(&sym.id).unwrap_or(&0.0)),
                    );
                    if !sym.signature.is_empty() {
                        m.insert("signature".into(), sym.signature.clone());
                    }
                    result.push(m);
                }
            }
        }
        result
    }
}
