// Topic scoring, file role classification, and test matching.
//
// Zero external dependencies — all scoring is heuristic.

use std::collections::HashMap;
use std::path::Path;

use crate::types::RepoGraph;

/// A file match result with score and role.
#[derive(Debug, Clone)]
pub struct FileMatch {
    pub path: String,
    pub role: String,
    pub score: f64,
    pub reasons: Vec<String>,
}

/// A test match with confidence level.
#[derive(Debug, Clone)]
pub struct TestMatch {
    pub test_file: String,
    pub target_file: String,
    pub confidence: String,
    pub reason: String,
}

/// Check if a file path matches known noise patterns.
pub fn is_noise_file(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    lower.contains("test")
        || lower.contains("__pycache__")
        || lower.contains("node_modules")
        || lower.contains(".min.")
        || lower.ends_with(".lock")
        || lower.ends_with(".json")
        || lower.ends_with(".html")
        || lower.ends_with(".css")
}

/// Classify a file by role based on path patterns.
pub fn classify_file_role(file_path: &str) -> &str {
    let lower = file_path.to_lowercase();
    if lower.contains("test") || lower.contains("__test__") || lower.contains("spec.") {
        return "test";
    }
    if lower.contains(".tsx") || lower.contains(".jsx") || lower.contains("component") {
        return "frontend-ui";
    }
    if lower.contains("store") || lower.contains("state") || lower.contains("reducer") {
        return "frontend-state";
    }
    if lower.contains("config") || lower.contains(".toml") || lower.contains(".json") {
        return "config";
    }
    "backend"
}

/// Split an identifier into lowercase tokens (camelCase/PascalCase/snake_case/kebab-case).
pub fn split_identifier(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in name.chars() {
        if ch == '_' || ch == '-' || ch == '.' {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        } else if ch.is_uppercase() && !current.is_empty() {
            tokens.push(current.to_lowercase());
            current.clear();
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens
}

/// Compute relevance score for a file given a query.
/// Weighted scoring: path (30%), file name (25%), symbol hits (15%),
/// noise penalty (×0.05), test down-weight (×0.55).
pub fn topic_score(
    query: &str,
    file_path: &str,
    graph: &RepoGraph,
    _keyword_weights: &HashMap<String, f64>,
) -> f64 {
    let query_lower = query.to_lowercase();
    let query_tokens: Vec<String> = split_identifier(&query_lower);

    // Path score (max 30).
    let path_lower = file_path.to_lowercase();
    let path_tokens = split_identifier(&path_lower);
    let path_hits: usize = query_tokens
        .iter()
        .filter(|t| path_tokens.contains(t))
        .count();
    let path_score = if query_tokens.is_empty() {
        0.0
    } else {
        (path_hits as f64 / query_tokens.len() as f64) * 30.0
    };

    // File name score (max 25).
    let file_stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let stem_tokens = split_identifier(file_stem);
    let name_hits: usize = query_tokens
        .iter()
        .filter(|t| stem_tokens.contains(t))
        .count();
    let name_score = if query_tokens.is_empty() {
        0.0
    } else {
        (name_hits as f64 / query_tokens.len() as f64) * 25.0
    };

    // Symbol hit score (max 15).
    let sym_score = if let Some(sym_ids) = graph.file_symbols.get(file_path) {
        let hits: usize = sym_ids
            .iter()
            .filter_map(|id| graph.symbols.get(id))
            .filter(|s| {
                let name_lower = s.name.to_lowercase();
                query_tokens.iter().any(|t| name_lower.contains(t.as_str()))
            })
            .count();
        if sym_ids.is_empty() {
            0.0
        } else {
            (hits as f64 / sym_ids.len() as f64) * 15.0
        }
    } else {
        0.0
    };

    let base = path_score + name_score + sym_score;

    // Noise penalty.
    let noise_penalty = if is_noise_file(file_path) { 0.05 } else { 1.0 };

    // Test down-weight.
    let test_weight = if classify_file_role(file_path) == "test" {
        0.55
    } else {
        1.0
    };

    base * noise_penalty * test_weight
}

/// Compute IDF-like weights for keywords (frequent words get lower weight).
pub fn compute_keyword_weights(
    keywords: &[String],
    candidate_files: &[String],
    graph: &RepoGraph,
) -> HashMap<String, f64> {
    let mut weights = HashMap::new();
    let n = candidate_files.len().max(1) as f64;

    for kw in keywords {
        let mut doc_count = 0;
        for file in candidate_files {
            let tokens = split_identifier(file);
            if tokens.contains(kw) {
                doc_count += 1;
            } else if let Some(sym_ids) = graph.file_symbols.get(file) {
                let found = sym_ids.iter().any(|id| {
                    graph
                        .symbols
                        .get(id)
                        .map_or(false, |s| s.name.to_lowercase().contains(kw.as_str()))
                });
                if found {
                    doc_count += 1;
                }
            }
        }
        let idf = ((n + 1.0) / (doc_count as f64 + 1.0)).ln() + 1.0;
        weights.insert(kw.clone(), idf);
    }
    weights
}

/// Check if a file looks like a test file.
pub fn is_test_like_file(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    lower.contains("test")
        || lower.contains("__test__")
        || lower.contains("spec.")
        || lower.contains("_test.")
}

/// Find related tests for target files using multiple strategies.
pub fn find_related_tests(
    _target_files: &[String],
    _graph: &RepoGraph,
    _project_root: &Path,
) -> Vec<TestMatch> {
    // TODO: implement 5-strategy test matching
    // 1. Exact file name match (e.g., foo.py → test_foo.py)
    // 2. Directory proximity (e.g., src/foo.py → tests/test_foo.py)
    // 3. Import match (test imports the target)
    // 4. Symbol edge (test calls target symbols)
    // 5. Git co-change history
    Vec::new()
}
