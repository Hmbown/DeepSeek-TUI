// PageRank-based graph analysis and query engine for DeepMap.
//
// Provides: symbol ranking, call-chain traversal, entry-point detection,
// hotspot identification, module summarization, and reading-order
// suggestions -- all driven by the dependency graph built during scanning.

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use crate::types::*;

// ---------------------------------------------------------------------------
// Heuristic filters
// ---------------------------------------------------------------------------

/// Symbol kinds that carry little semantic weight and should be excluded from
/// symbol-level summaries.
const LOW_SIGNAL_KINDS: &[&str] = &[
    "element",
    "selector",
    "class_selector",
    "id_selector",
    "json_key",
];

/// Symbol names that are considered boilerplate.
const BOILERPLATE_NAMES: &[&str] = &["__init__"];

/// Safety limits for call-chain traversal.
const MAX_QUEUE_SIZE: usize = 10_000;
const MAX_RESULTS: usize = 1_000;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a symbol kind to a signal weight (lower means noisier).
fn _signal_weight(kind: &str) -> f64 {
    match kind {
        "element" => 0.3,
        "selector" => 0.2,
        "class_selector" | "id_selector" => 0.2,
        "json_key" => 0.1,
        _ => 1.0,
    }
}

/// Determine whether a file path looks like an application entry point.
fn _is_entry_point(file: &str) -> bool {
    let path = Path::new(file);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let entry_stems: &[&str] = &[
        "main", "app", "index", "server", "run", "setup", "cli", "__main__",
    ];
    if entry_stems.contains(&stem) {
        return true;
    }

    // */lib.rs
    if path.file_name().and_then(|s| s.to_str()) == Some("lib.rs") {
        return true;
    }

    // */src/main.{tsx,jsx,js,ts,py,...}
    if stem == "main" {
        if let Some(parent) = path.parent() {
            if parent.ends_with("src") {
                return true;
            }
        }
    }

    false
}

/// Check whether a file path looks like test / mock / spec noise.
fn _is_test_or_noise_file(file: &str) -> bool {
    let path = Path::new(file);

    // Check path components for test-related directories.
    for comp in path.components() {
        if let std::path::Component::Normal(name) = comp {
            let s = name.to_str().unwrap_or("");
            if s == "test"
                || s == "tests"
                || s == "__tests__"
                || s == "__test__"
                || s == "mock"
                || s == "mocks"
                || s == "__mocks__"
                || s == "spec"
                || s == "fixtures"
            {
                return true;
            }
        }
    }

    // Check file name for test indicators.
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if file_name.starts_with("test_")
        || file_name.ends_with("_test.rs")
        || file_name.ends_with("_test.py")
        || file_name.ends_with("_spec.rb")
        || file_name.contains(".spec.")
        || file_name.contains(".test.")
        || file_name.contains("_mock")
        || file_name.contains("_fixture")
        || file_name == "__init__.py"
    {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// GraphAnalyzer
// ---------------------------------------------------------------------------

/// Performs PageRank-based analysis of a `RepoGraph` and exposes query
/// methods that return ranked symbols and file-level summaries.
pub struct GraphAnalyzer {
    pagerank: HashMap<String, f64>,
}

impl GraphAnalyzer {
    /// Create a new analyzer with an empty PageRank map.
    pub fn new() -> Self {
        Self {
            pagerank: HashMap::new(),
        }
    }

    /// Clear any previously computed PageRank scores.
    pub fn load_graph(&mut self) {
        self.pagerank.clear();
    }

    /// Reference to the current PageRank scores (symbol id -> score).
    pub fn pagerank_scores(&self) -> &HashMap<String, f64> {
        &self.pagerank
    }

    // -----------------------------------------------------------------------
    // PageRank computation
    // -----------------------------------------------------------------------

    /// Standard power-iteration PageRank with sink handling and convergence
    /// detection. Uses `&RepoGraph` -- does **not** clone the graph.
    pub fn calculate_pagerank(
        &mut self,
        graph: &RepoGraph,
        damping: f64,
        max_iter: usize,
        tol: f64,
    ) {
        let n = graph.symbols.len();
        if n == 0 {
            self.pagerank.clear();
            return;
        }

        let d = damping.clamp(0.0, 1.0);
        let initial = 1.0 / n as f64;

        let mut pr: HashMap<String, f64> = HashMap::with_capacity(n);
        for id in graph.symbols.keys() {
            pr.insert(id.clone(), initial);
        }

        for _iter in 0..max_iter {
            // Sink PR: sum of PageRank for nodes with no outgoing edges.
            let mut sink_pr = 0.0;
            for (id, rank) in &pr {
                let out_degree = graph
                    .outgoing
                    .get(id)
                    .map_or(0, |edges| edges.len());
                if out_degree == 0 {
                    sink_pr += rank;
                }
            }

            let mut new_pr: HashMap<String, f64> = HashMap::with_capacity(n);
            let mut max_diff = 0.0;

            for (id, _sym) in &graph.symbols {
                // Contribution from nodes that point to this symbol.
                let mut incoming_sum = 0.0;
                if let Some(edges) = graph.incoming.get(id) {
                    for edge in edges {
                        let pr_q = pr.get(&edge.source).copied().unwrap_or(0.0);
                        let out_weight_sum: f64 = graph
                            .outgoing
                            .get(&edge.source)
                            .map_or(0.0, |es| es.iter().map(|e| e.weight).sum());
                        if out_weight_sum > 0.0 {
                            incoming_sum += pr_q * edge.weight / out_weight_sum;
                        }
                    }
                }

                let rank = (1.0 - d) / n as f64
                    + d * (incoming_sum + sink_pr / n as f64);
                new_pr.insert(id.clone(), rank);

                let diff =
                    (rank - pr.get(id).copied().unwrap_or(0.0)).abs();
                if diff > max_diff {
                    max_diff = diff;
                }
            }

            pr = new_pr;

            if max_diff < tol {
                break;
            }
        }

        self.pagerank = pr;
    }

    // -----------------------------------------------------------------------
    // Query methods
    // -----------------------------------------------------------------------

    /// Case-insensitive symbol lookup. Filters out low-signal kinds and sorts
    /// results by PageRank descending.
    pub fn query_symbol<'a>(
        &self,
        name: &str,
        graph: &'a RepoGraph,
    ) -> Vec<&'a Symbol> {
        let lower_query = name.to_lowercase();
        let mut results: Vec<&Symbol> = graph
            .symbols
            .values()
            .filter(|sym| {
                sym.name.to_lowercase().contains(&lower_query)
                    && !LOW_SIGNAL_KINDS.contains(&sym.kind.as_str())
            })
            .collect();

        results.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results
    }

    /// BFS-based call-chain traversal.
    ///
    /// `direction` is one of "callers", "callees", or "both".
    /// Returns a map with keys "callers" and/or "callees", each containing
    /// a list of symbols sorted by PageRank descending.
    ///
    /// Safety limits: queue capped at MAX_QUEUE_SIZE;
    /// total returned symbols capped at MAX_RESULTS.
    pub fn call_chain(
        &self,
        symbol_id: &str,
        direction: &str,
        max_depth: usize,
        graph: &RepoGraph,
    ) -> HashMap<String, Vec<Symbol>> {
        if max_depth == 0 || !graph.symbols.contains_key(symbol_id) {
            return HashMap::new();
        }

        let mut callers: Vec<Symbol> = Vec::new();
        let mut callees: Vec<Symbol> = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut total_found: usize = 0;

        visited.insert(symbol_id.to_string());
        queue.push_back((symbol_id.to_string(), 0));

        let mut halt = false;

        while let Some((current, depth)) = queue.pop_front() {
            if halt || depth >= max_depth {
                continue;
            }

            match direction {
                "callers" => {
                    if let Some(edges) = graph.incoming.get(&current) {
                        for edge in edges {
                            if edge.kind != "call" {
                                continue;
                            }
                            if visited.insert(edge.source.clone())
                                && total_found < MAX_RESULTS
                            {
                                if let Some(sym) = graph.symbols.get(&edge.source)
                                {
                                    callers.push(sym.clone());
                                    total_found += 1;
                                    if queue.len() < MAX_QUEUE_SIZE {
                                        queue.push_back((
                                            edge.source.clone(),
                                            depth + 1,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                "callees" => {
                    if let Some(edges) = graph.outgoing.get(&current) {
                        for edge in edges {
                            if edge.kind != "call" {
                                continue;
                            }
                            if visited.insert(edge.target.clone())
                                && total_found < MAX_RESULTS
                            {
                                if let Some(sym) = graph.symbols.get(&edge.target)
                                {
                                    callees.push(sym.clone());
                                    total_found += 1;
                                    if queue.len() < MAX_QUEUE_SIZE {
                                        queue.push_back((
                                            edge.target.clone(),
                                            depth + 1,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                "both" => {
                    // Incoming = callers
                    if let Some(edges) = graph.incoming.get(&current) {
                        for edge in edges {
                            if edge.kind != "call" {
                                continue;
                            }
                            if visited.insert(edge.source.clone())
                                && total_found < MAX_RESULTS
                            {
                                if let Some(sym) = graph.symbols.get(&edge.source)
                                {
                                    callers.push(sym.clone());
                                    total_found += 1;
                                    if queue.len() < MAX_QUEUE_SIZE {
                                        queue.push_back((
                                            edge.source.clone(),
                                            depth + 1,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    // Outgoing = callees
                    if let Some(edges) = graph.outgoing.get(&current) {
                        for edge in edges {
                            if edge.kind != "call" {
                                continue;
                            }
                            if visited.insert(edge.target.clone())
                                && total_found < MAX_RESULTS
                            {
                                if let Some(sym) = graph.symbols.get(&edge.target)
                                {
                                    callees.push(sym.clone());
                                    total_found += 1;
                                    if queue.len() < MAX_QUEUE_SIZE {
                                        queue.push_back((
                                            edge.target.clone(),
                                            depth + 1,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            halt = total_found >= MAX_RESULTS;
        }

        // Sort results by PageRank desc.
        callers.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        callees.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut result = HashMap::new();
        if !callers.is_empty() {
            result.insert("callers".to_string(), callers);
        }
        if !callees.is_empty() {
            result.insert("callees".to_string(), callees);
        }
        result
    }

    /// Identify hotspot files: score = symbol_count * avg_pagerank.
    /// Returns the top `limit` entries; 0 means no limit.
    pub fn hotspots(
        &self,
        limit: usize,
        graph: &RepoGraph,
    ) -> Vec<HashMap<String, String>> {
        let mut scored: Vec<(String, f64, usize)> = Vec::new();

        for (file, sym_ids) in &graph.file_symbols {
            if sym_ids.is_empty() {
                continue;
            }
            let mut pr_sum = 0.0;
            for sid in sym_ids {
                if let Some(pr) = self.pagerank.get(sid) {
                    pr_sum += pr;
                }
            }
            let avg_pr = pr_sum / sym_ids.len() as f64;
            let score = sym_ids.len() as f64 * avg_pr;
            scored.push((file.clone(), score, sym_ids.len()));
        }

        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if limit > 0 && scored.len() > limit {
            scored.truncate(limit);
        }

        scored
            .into_iter()
            .map(|(file, score, count)| {
                let mut m = HashMap::new();
                m.insert("file".to_string(), file);
                m.insert(
                    "avg_pagerank".to_string(),
                    format!("{:.6}", score / count as f64),
                );
                m.insert("symbols".to_string(), count.to_string());
                m
            })
            .collect()
    }

    /// Detect application entry points.
    pub fn entry_points(&self, graph: &RepoGraph) -> Vec<String> {
        let mut eps: Vec<String> = graph
            .file_symbols
            .keys()
            .filter(|f| _is_entry_point(f))
            .cloned()
            .collect();
        eps.sort();
        eps
    }

    /// Per-file analysis: symbol_count, outgoing_edges, incoming_edges,
    /// avg_pagerank.
    pub fn file_analysis(
        &self,
        graph: &RepoGraph,
    ) -> HashMap<String, HashMap<String, f64>> {
        let mut analysis: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for (file, sym_ids) in &graph.file_symbols {
            let sym_count = sym_ids.len();
            let mut out_count: usize = 0;
            let mut in_count: usize = 0;
            let mut pr_sum: f64 = 0.0;

            for sid in sym_ids {
                if let Some(edges) = graph.outgoing.get(sid) {
                    out_count += edges.len();
                }
                if let Some(edges) = graph.incoming.get(sid) {
                    in_count += edges.len();
                }
                if let Some(pr) = self.pagerank.get(sid) {
                    pr_sum += pr;
                }
            }

            let avg_pr = if sym_count > 0 {
                pr_sum / sym_count as f64
            } else {
                0.0
            };

            let mut m = HashMap::new();
            m.insert("symbol_count".to_string(), sym_count as f64);
            m.insert("outgoing_edges".to_string(), out_count as f64);
            m.insert("incoming_edges".to_string(), in_count as f64);
            m.insert("avg_pagerank".to_string(), avg_pr);
            analysis.insert(file.clone(), m);
        }

        analysis
    }

    /// Group files by top-level directory, rank by sum of PageRank
    /// (semantic weight), not raw symbol count.
    pub fn module_summary(
        &self,
        limit: usize,
        graph: &RepoGraph,
    ) -> Vec<HashMap<String, String>> {
        // module -> (total_pr, symbol_names)
        let mut modules: HashMap<String, (f64, Vec<String>)> = HashMap::new();

        for (file, sym_ids) in &graph.file_symbols {
            let module = Path::new(file)
                .components()
                .next()
                .and_then(|c| {
                    if let std::path::Component::Normal(name) = c {
                        name.to_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "root".to_string());

            let entry = modules.entry(module).or_default();

            for sid in sym_ids {
                let pr = self.pagerank.get(sid).copied().unwrap_or(0.0);
                entry.0 += pr;
                entry.1.push(sid.clone());
            }
        }

        let mut sorted: Vec<(String, f64, Vec<String>)> = modules
            .into_iter()
            .map(|(k, (pr, syms))| (k, pr, syms))
            .collect();
        sorted.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if limit > 0 && sorted.len() > limit {
            sorted.truncate(limit);
        }

        sorted
            .into_iter()
            .map(|(module, total_pr, syms)| {
                let mut m = HashMap::new();
                m.insert("module".to_string(), module);
                m.insert("symbols".to_string(), syms.len().to_string());
                m.insert(
                    "total_pagerank".to_string(),
                    format!("{:.6}", total_pr),
                );
                m
            })
            .collect()
    }

    /// Suggest a reading order for files.
    ///
    /// Score = avg_pagerank * ln(symbol_count + 1) * entry_boost.
    /// Test / noise files are filtered out.
    pub fn suggested_reading_order(
        &self,
        limit: usize,
        graph: &RepoGraph,
    ) -> Vec<HashMap<String, String>> {
        let mut scored: Vec<(String, f64)> = Vec::new();

        for (file, sym_ids) in &graph.file_symbols {
            if sym_ids.is_empty() || _is_test_or_noise_file(file) {
                continue;
            }

            let mut pr_sum = 0.0;
            for sid in sym_ids {
                if let Some(pr) = self.pagerank.get(sid) {
                    pr_sum += pr;
                }
            }
            let avg_pr = pr_sum / sym_ids.len() as f64;
            let entry_boost = if _is_entry_point(file) { 2.0 } else { 1.0 };
            let score =
                avg_pr * (sym_ids.len() as f64 + 1.0).ln() * entry_boost;

            scored.push((file.clone(), score));
        }

        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if limit > 0 && scored.len() > limit {
            scored.truncate(limit);
        }

        scored
            .into_iter()
            .map(|(file, score)| {
                let mut m = HashMap::new();
                m.insert("file".to_string(), file);
                m.insert("score".to_string(), format!("{:.6}", score));
                m
            })
            .collect()
    }

    /// Return the top symbols from the top files.
    ///
    /// For each file in `suggested_reading_order(limit_files)`, return the
    /// top `per_file` symbols sorted by composite score. Excludes
    /// `LOW_SIGNAL_KINDS` and `BOILERPLATE_NAMES`.
    pub fn summary_symbols(
        &self,
        limit_files: usize,
        per_file: usize,
        graph: &RepoGraph,
    ) -> Vec<HashMap<String, String>> {
        let top_files = self.suggested_reading_order(limit_files, graph);

        let mut result: Vec<HashMap<String, String>> = Vec::new();

        for entry in &top_files {
            let file = match entry.get("file") {
                Some(f) => f.as_str(),
                None => continue,
            };
            if file.is_empty() {
                continue;
            }

            let sym_ids = match graph.file_symbols.get(file) {
                Some(ids) => ids,
                None => continue,
            };

            // Score each symbol in this file.
            let mut scored: Vec<(String, f64)> = Vec::new();
            for sid in sym_ids {
                let sym = match graph.symbols.get(sid) {
                    Some(s) => s,
                    None => continue,
                };

                // Exclude low-signal kinds and boilerplate names.
                if LOW_SIGNAL_KINDS.contains(&sym.kind.as_str()) {
                    continue;
                }
                if BOILERPLATE_NAMES.contains(&sym.name.as_str()) {
                    continue;
                }

                let pr = self.pagerank.get(sid).copied().unwrap_or(0.0);
                let score = self._summary_symbol_score(sym, graph, pr);
                scored.push((sid.clone(), score));
            }

            scored.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if per_file > 0 && scored.len() > per_file {
                scored.truncate(per_file);
            }

            for (sid, _score) in &scored {
                if let Some(sym) = graph.symbols.get(sid) {
                    let mut m = HashMap::new();
                    m.insert("file".to_string(), file.to_string());
                    m.insert("symbol".to_string(), sym.name.clone());
                    m.insert("kind".to_string(), sym.kind.clone());
                    m.insert(
                        "pagerank".to_string(),
                        format!("{:.6}", sym.pagerank),
                    );
                    result.push(m);
                }
            }
        }

        result
    }

    // -----------------------------------------------------------------------
    // Internal scoring
    // -----------------------------------------------------------------------

    /// Composite symbol score for summary ranking.
    ///
    /// Formula: incoming_calls * 3 + outgoing_calls * 2 + imports * 1
    ///          + visibility_bonus + kind_signal_weight + pagerank
    fn _summary_symbol_score(
        &self,
        sym: &Symbol,
        graph: &RepoGraph,
        pagerank: f64,
    ) -> f64 {
        let incoming_calls = graph
            .incoming
            .get(&sym.id)
            .map_or(0, |edges| {
                edges.iter().filter(|e| e.kind == "call").count()
            }) as f64;

        let outgoing_calls = graph
            .outgoing
            .get(&sym.id)
            .map_or(0, |edges| {
                edges.iter().filter(|e| e.kind == "call").count()
            }) as f64;

        let imports = graph
            .outgoing
            .get(&sym.id)
            .map_or(0, |edges| {
                edges.iter().filter(|e| e.kind == "import").count()
            }) as f64;

        let visibility_bonus = match sym.visibility.as_str() {
            "export" | "pub" | "public" | "extern" => 2.0,
            _ => 0.0,
        };

        let signal_weight = _signal_weight(&sym.kind);

        incoming_calls * 3.0
            + outgoing_calls * 2.0
            + imports * 1.0
            + visibility_bonus
            + signal_weight
            + pagerank
    }
}

impl Default for GraphAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
