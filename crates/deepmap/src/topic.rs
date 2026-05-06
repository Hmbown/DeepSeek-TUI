// Topic scoring and fuzzy search helpers for DeepMap.
//
// Provides identifier tokenisation, file-role classification, noise filtering,
// IDF-weighted topic scoring, fuzzy symbol suggestions, and test-file discovery.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use crate::types::RepoGraph;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A test file matched to a target source file, with a confidence score and
/// explanation of why they are related.
#[derive(Debug, Clone)]
pub struct TestMatch {
    pub test_file: String,
    pub target_file: String,
    pub confidence: f64,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Identifier tokenisation
// ---------------------------------------------------------------------------

/// Split a camelCase / PascalCase / snake_case / kebab-case identifier into
/// lower-cased tokens.
///
/// Examples: `getUserByID` → `["get", "user", "by", "id"]`; `XMLParser` → `["xml", "parser"]`

pub fn split_identifier(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();

    // 1. Split on non-alphanumeric separators.
    for segment in name.split(|c: char| !c.is_alphanumeric()) {
        if segment.is_empty() {
            continue;
        }

        // 2. Split the alphanumeric segment on CamelCase boundaries.
        let mut cur = String::new();
        let chars: Vec<char> = segment.chars().collect();

        for i in 0..chars.len() {
            let c = chars[i];

            if cur.is_empty() {
                cur.push(c);
                continue;
            }

            if c.is_uppercase() {
                let prev = chars[i - 1];
                // Start a new word when:
                //   - previous char is lowercase          -> "camelCase"
                //   - previous is uppercase AND next exists and is lowercase -> "XMLParser"
                let split = prev.is_lowercase()
                    || (i + 1 < chars.len() && chars[i + 1].is_lowercase());

                if split {
                    tokens.push(cur.to_lowercase());
                    cur.clear();
                }
            }

            cur.push(c);
        }

        if !cur.is_empty() {
            tokens.push(cur.to_lowercase());
        }
    }

    tokens
}

// ---------------------------------------------------------------------------
// File role classification
// ---------------------------------------------------------------------------

/// Return a human-readable role label for the file path.
///
/// One of: `"test"`, `"config"`, `"frontend-ui"`, `"frontend-state"`,
/// `"backend"`, or `"other"`.
pub fn classify_file_role(path: &str) -> &str {
    let lower = path.to_lowercase();

    if is_test_like_file(path) {
        return "test";
    }

    if lower.contains("config")
        || lower.contains("setting")
        || lower.ends_with(".env")
    {
        return "config";
    }

    if lower.contains("/ui/")
        || lower.contains("/components/")
        || lower.contains("/views/")
        || lower.contains("/pages/")
        || lower.ends_with(".html")
        || lower.ends_with(".css")
    {
        return "frontend-ui";
    }

    if lower.contains("/store/")
        || lower.contains("/state/")
        || lower.contains("/redux/")
        || lower.contains("/context/")
    {
        return "frontend-state";
    }

    if lower.contains("/api/")
        || lower.contains("/routes/")
        || lower.contains("/controllers/")
        || lower.contains("/services/")
        || lower.contains("/models/")
        || lower.contains("/middleware/")
        || lower.contains("/handlers/")
    {
        return "backend";
    }

    // Default by extension.
    if lower.ends_with(".rs")
        || lower.ends_with(".go")
        || lower.ends_with(".py")
        || lower.ends_with(".java")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
    {
        return "backend";
    }

    "other"
}

// ---------------------------------------------------------------------------
// Noise / test detection
// ---------------------------------------------------------------------------

/// Returns `true` for files that are unlikely to contain meaningful business
/// logic and should be de-prioritised in topic search results.
pub fn is_noise_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower.contains("__pycache__")
        || lower.contains("node_modules")
        || lower.contains(".min.")
        || lower.ends_with(".lock")
        || lower.ends_with(".json")
        || lower.ends_with(".html")
        || lower.ends_with(".css")
    {
        return true;
    }
    // Already handled by test weight, but also mark pure test dirs.
    lower.contains("/test/") || lower.contains("/tests/") || lower.contains("__test__")
}

/// Returns `true` when the path looks like it belongs to a test file.
pub fn is_test_like_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("test")
        || lower.contains("__test__")
        || lower.contains("/spec.")
        || lower.contains("_test.")
        || lower.contains("spec/")
}

// ---------------------------------------------------------------------------
// IDF-style keyword weighting
// ---------------------------------------------------------------------------

/// Compute IDF-like weights for each keyword.
///
/// Keywords that appear in many files get a lower weight; rare keywords get
/// a higher weight.  The formula is a smoothed version of IDF:
///
///   weight = log10(N / (n + 1)) + 1.0
///
/// where N = total candidate files, n = number of files containing the keyword
/// (either in path or in a symbol name within that file).
pub fn compute_keyword_weights(
    keywords: &[String],
    candidate_files: &[String],
    graph: &RepoGraph,
) -> HashMap<String, f64> {
    let n = candidate_files.len().max(1) as f64;
    let mut weights = HashMap::new();

    for kw in keywords {
        let kw_lower = kw.to_lowercase();
        let mut count = 0usize;

        for file in candidate_files {
            // Check file path.
            if file.to_lowercase().contains(&kw_lower) {
                count += 1;
                continue;
            }
            // Check symbols inside the file.
            if let Some(sym_ids) = graph.file_symbols.get(file) {
                let matched = sym_ids.iter().any(|sid| {
                    graph
                        .symbols
                        .get(sid)
                        .map(|s| s.name.to_lowercase().contains(&kw_lower))
                        .unwrap_or(false)
                });
                if matched {
                    count += 1;
                }
            }
        }

        let weight = (n / (count as f64 + 1.0)).log10().max(0.0) + 1.0;
        weights.insert(kw.clone(), weight);
    }

    weights
}

// ---------------------------------------------------------------------------
// Topic score
// ---------------------------------------------------------------------------

/// Score a single file against a natural-language query using the IDF-weighted
/// topic model.
///
/// Components:
///   - path_score (30 %): fraction of query tokens appearing anywhere in path
///   - name_score (25 %): fraction appearing in the file stem
///   - symbol_hits (15 %): fraction of the file's symbols whose name contains
///     at least one query token
///
/// Modifiers:
///   - test_weight (0.55): test-like files are penalised
///   - noise_penalty (0.05): noise files are almost completely suppressed
///
/// The final score is clamped to `[0.0, 100.0]`.
pub fn topic_score(
    query: &str,
    file_path: &str,
    graph: &RepoGraph,
    keyword_weights: &HashMap<String, f64>,
) -> f64 {
    let tokens = split_identifier(query);
    if tokens.is_empty() {
        return 0.0;
    }
    let n_tokens = tokens.len() as f64;

    let file_lower = file_path.to_lowercase();
    let file_stem = Path::new(file_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // --- path_score (30 %) ---
    let mut path_weighted_matches = 0.0_f64;
    let mut path_total_weight = 0.0_f64;
    for token in &tokens {
        let w = keyword_weights.get(token).copied().unwrap_or(1.0);
        path_total_weight += w;
        if file_lower.contains(token) {
            path_weighted_matches += w;
        }
    }
    let path_score = if path_total_weight > 0.0 {
        (path_weighted_matches / path_total_weight) * 100.0
    } else {
        0.0
    };

    // --- name_score (25 %) ---
    let name_matches = tokens
        .iter()
        .filter(|t| file_stem.contains(*t))
        .count() as f64;
    let name_score = (name_matches / n_tokens) * 100.0;

    // --- symbol_hits (15 %) ---
    let (matched_syms, total_syms) =
        if let Some(sym_ids) = graph.file_symbols.get(file_path) {
            let total = sym_ids.len();
            let matched = sym_ids
                .iter()
 .filter(|sid| {
        graph.symbols.get(*sid).map_or(false, |sym| {
            let sym_tokens = split_identifier(&sym.name);
            tokens.iter().any(|qt| sym_tokens.iter().any(|st| st == qt))
        })
    })
    .count();
            (matched, total)
        } else {
            (0, 0)
        };
    let symbol_score = if total_syms > 0 {
        (matched_syms as f64 / total_syms as f64) * 100.0
    } else {
        0.0
    };

    // -- Combine --
    let mut raw = path_score * 0.30 + name_score * 0.25 + symbol_score * 0.15;

    if is_test_like_file(file_path) {
        raw *= 0.55;
    }
    if is_noise_file(file_path) {
        raw *= 0.05;
    }

    raw.clamp(0.0, 100.0)
}

// ---------------------------------------------------------------------------
// Fuzzy symbol suggestions (edit distance <= 3)
// ---------------------------------------------------------------------------

/// Return up to `max_results` symbol names whose Levenshtein distance from
/// `query` is at most 3.
///
/// The results are sorted by edit distance (closest first).  Duplicate names
/// are returned only once.
pub fn fuzzy_symbol_suggest(
    query: &str,
    graph: &RepoGraph,
    max_results: usize,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut candidates: Vec<(String, usize)> = Vec::new();

    for sym in graph.symbols.values() {
        if !seen.insert(&sym.name) {
            continue;
        }
        let dist = levenshtein(query, &sym.name);
        if dist <= 3 {
            candidates.push((sym.name.clone(), dist));
        }
    }

    candidates.sort_by_key(|(_, d)| *d);
    candidates.truncate(max_results);
    candidates.into_iter().map(|(n, _)| n).collect()
}

/// Classic iterative Levenshtein distance (two-row optimisation).
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (curr[j - 1] + 1)
                .min(prev[j] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

// ---------------------------------------------------------------------------
// Related test discovery
// ---------------------------------------------------------------------------

/// Find test files related to one or more target source files.
///
/// Three strategies are tried (results are deduplicated):
///
/// 1. **Name match** — a test file whose stem contains the target's stem
///    (e.g. `login_test.ts` → `login.ts`).  Confidence: 0.85.
///
/// 2. **Directory proximity** — a test file located in the same directory or
///    a sibling `tests/` directory.  Confidence: 0.60.
///
/// 3. **Import match** — a test file that imports from the target file
///    (checked via `graph.file_imports`).  Confidence: 0.75.
pub fn find_related_tests(
    targets: &[String],
    graph: &RepoGraph,
    _project_root: &Path,
) -> Vec<TestMatch> {
    // Collect all test-like files from the graph.
    let test_files: Vec<&String> = graph
        .file_symbols
        .keys()
        .filter(|f| is_test_like_file(f))
        .collect();

    if test_files.is_empty() || targets.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<TestMatch> = Vec::new();

    for target in targets {
        let target_stem = Path::new(target)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        // -- Strategy 1: name match --
        for tf in &test_files {
            let tf_stem = Path::new(tf)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            if tf_stem.contains(&target_stem) || target_stem.contains(&tf_stem) {
                results.push(TestMatch {
                    test_file: (*tf).clone(),
                    target_file: target.clone(),
                    confidence: 0.85,
                    reason: format!("name match: `{}` / `{}`", tf_stem, target_stem),
                });
            }
        }

        // -- Strategy 2: directory proximity --
        let target_dir = Path::new(target).parent();
        for tf in &test_files {
            let tf_path = Path::new(tf);
            let proximate = target_dir.map_or(false, |td| {
                tf_path.parent().map_or(false, |tp| {
                    tp == td
                        || tp.to_string_lossy().contains("tests")
                        || td.to_string_lossy().contains("tests")
                })
            });

            if proximate
                && !results.iter().any(|r: &TestMatch| {
                    r.test_file == **tf && r.target_file == *target
                })
            {
                results.push(TestMatch {
                    test_file: (*tf).clone(),
                    target_file: target.clone(),
                    confidence: 0.60,
                    reason: format!("directory proximity: `{}`", tf),
                });
            }
        }

        // -- Strategy 3: import match --
        for tf in &test_files {
            let imports_from_target = graph.file_imports.get(*tf).map_or(false, |imports| {
                imports
                    .iter()
                    .any(|imp| target.contains(imp.trim_start_matches("./")))
            });

            if imports_from_target
                && !results.iter().any(|r: &TestMatch| {
                    r.test_file == **tf && r.target_file == *target
                })
            {
                results.push(TestMatch {
                    test_file: (*tf).clone(),
                    target_file: target.clone(),
                    confidence: 0.75,
                    reason: format!("import match: `{}` imports from target", tf),
                });
            }
        }
    }

    // Sort by confidence descending.
    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}
