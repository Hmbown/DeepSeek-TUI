// Markdown report renderer for DeepMap.
//
// Produces AI-friendly reports: overview, call-chain, file detail, topic
// search, impact analysis, and diff-risk assessment.

use std::collections::{HashMap, HashSet};
use crate::engine::RepoMapEngine;
use crate::topic;
use crate::types::Symbol;

// ---------------------------------------------------------------------------
// Truncation helper
// ---------------------------------------------------------------------------

/// Truncate `text` to at most `max_chars` characters (UTF-8 aware).
///
/// When truncation occurs a `... [truncated]` note is appended.  If
/// `max_chars` is very small (< 10) the function returns text unchanged.
fn truncate(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars || max_chars < 10 {
        return text.to_string();
    }

    const NOTE: &str = "\n\n... [truncated]";
    let cutoff = max_chars.saturating_sub(16);
    let mut result: String = text.chars().take(cutoff).collect();
    result.push_str(NOTE);
    result
}

// ---------------------------------------------------------------------------
// Risk assessment
// ---------------------------------------------------------------------------

/// Classify a set of changed files and return a `(level, reasons)` pair.
///
/// Scoring rules:
/// - path contains `auth` / `login` / `token` / `password`       +3 each
/// - path contains `db` / `sql` / `migrat` / `database`          +3 each
/// - path contains `config` / `setting` / `env`                   +2 each
/// - file extension is `.rs` or `.go`                             +1 each
///
/// Thresholds: HIGH >= 10, MEDIUM >= 5, LOW < 5.
pub fn assess_risk(files: &[String]) -> (String, Vec<String>) {
    let auth_kws: &[&str] = &["auth", "login", "token", "password", "credential", "oauth"];
    let db_kws: &[&str] = &["db", "sql", "migrat", "database", "query", "schema"];
    let cfg_kws: &[&str] = &["config", "setting", "env"];

    let mut score: i64 = 0;
    let mut reasons: Vec<String> = Vec::new();

    for file in files {
        let lower = file.to_lowercase();

        let auth_hit = auth_kws.iter().any(|kw| lower.contains(kw));
        if auth_hit {
            score += 3;
            reasons.push(format!("auth-related: `{}`", file));
        }

        let db_hit = db_kws.iter().any(|kw| lower.contains(kw));
        if db_hit {
            score += 3;
            reasons.push(format!("db-related: `{}`", file));
        }

        let cfg_hit = cfg_kws.iter().any(|kw| lower.contains(kw));
        if cfg_hit {
            score += 2;
            reasons.push(format!("config-related: `{}`", file));
        }

        if file.ends_with(".rs") || file.ends_with(".go") {
            score += 1;
            reasons.push(format!("type-safe language file: `{}`", file));
        }
    }

    let level = if score >= 10 {
        "HIGH"
    } else if score >= 5 {
        "MEDIUM"
    } else {
        "LOW"
    };

    (level.to_string(), reasons)
}

// ---------------------------------------------------------------------------
// 1. Overview report
// ---------------------------------------------------------------------------

/// Full project overview.
///
/// Sections: Scan Statistics, Entry Points, Recommended Reading Order, Module
/// Summary, Hot Spots, Key Symbols.
///
/// When the symbol count exceeds 5 000 fewer items are shown per section
/// (compact mode) so the report stays readable.
pub fn render_overview_report(engine: &RepoMapEngine, max_chars: usize) -> String {
    let graph = engine.graph();
    let compact = graph.symbols.len() > 5_000;
    let mut sections: Vec<String> = Vec::new();

    // ---- Scan Statistics ----
    {
        let mut sec = "## Scan Statistics\n\n".to_string();
        for line in engine.scan_summary_lines() {
            sec.push_str(&format!("- {}\n", line));
        }
        sections.push(sec);
    }

    // ---- Entry Points ----
    {
        let mut sec = "## Entry Points\n\n".to_string();
        let mut candidates: Vec<&Symbol> = graph
            .symbols
            .values()
            .filter(|s| {
                let in_deg = graph
                    .incoming
                    .get(&s.id)
                    .map(|e| e.len())
                    .unwrap_or(0);
                in_deg <= 1 && s.pagerank > 0.3
            })
            .collect();
        candidates.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let limit = if compact { 5 } else { 10 };
        candidates.truncate(limit);

        if candidates.is_empty() {
            sec.push_str("None identified.\n");
        } else {
            for sym in &candidates {
                sec.push_str(&format!(
                    "- `{}` ({}) in `{}` \u{2014} PR: {:.2}\n",
                    sym.name, sym.kind, sym.file, sym.pagerank
                ));
            }
        }
        sections.push(sec);
    }

    // ---- Recommended Reading Order ----
    {
        let mut sec = "## Recommended Reading Order\n\n".to_string();
        let mut sorted: Vec<&Symbol> = graph.symbols.values().collect();
        sorted.sort_by(|a, b| {
            b.pagerank
                .partial_cmp(&a.pagerank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let limit = if compact { 8 } else { 15 };
        for (i, sym) in sorted.iter().enumerate().take(limit) {
            sec.push_str(&format!(
                "{}. `{}` (PR: {:.2}) \u{2014} {} in `{}`\n",
                i + 1,
                sym.name,
                sym.pagerank,
                sym.kind,
                sym.file
            ));
        }
        sections.push(sec);
    }

    // ---- Module Summary ----
    {
        let mut sec = "## Module Summary\n\n".to_string();
        let mut dirs: HashMap<&str, (usize, f64)> = HashMap::new();
        for sym in graph.symbols.values() {
            let dir = sym.file.split('/').next().unwrap_or("root");
            let entry = dirs.entry(dir).or_default();
            entry.0 += 1;
            entry.1 += sym.pagerank;
        }

        let mut sorted: Vec<(&&str, &(usize, f64))> = dirs.iter().collect();
        sorted.sort_by(|a, b| {
            b.1 .1
                .partial_cmp(&a.1 .1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let limit = if compact { 5 } else { 10 };
        for (dir, (count, score)) in sorted.iter().take(limit) {
            sec.push_str(&format!(
                "- `{}` \u{2014} {} files, PageRank: {:.2}\n",
                dir, count, score
            ));
        }
        sections.push(sec);
    }

    // ---- Hot Spots ----
    {
        let mut sec = "## Hot Spots\n\n".to_string();
        let mut spots: Vec<(&String, &Vec<String>)> = graph.file_symbols.iter().collect();
        spots.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let limit = if compact { 4 } else { 8 };
        for (file, syms) in spots.iter().take(limit) {
            sec.push_str(&format!(
                "- `{}` \u{2014} {} symbols\n",
                file,
                syms.len()
            ));
        }
        sections.push(sec);
    }

    // ---- Key Symbols ----
    {
        let mut sec = "## Key Symbols\n\n".to_string();
        let mut spots: Vec<(&String, &Vec<String>)> = graph.file_symbols.iter().collect();
        spots.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let file_limit = if compact { 5 } else { 10 };
        let sym_limit = if compact { 3 } else { 5 };

        for (file, syms) in spots.iter().take(file_limit) {
            sec.push_str(&format!("### `{}`\n", file));
            for sym_id in syms.iter().take(sym_limit) {
                if let Some(sym) = graph.symbols.get(sym_id) {
                    sec.push_str(&format!(
                        "- `{}` ({}) \u{2014} PR: {:.2}, `{}`\n",
                        sym.name,
                        sym.kind,
                        sym.pagerank,
                        truncate(&sym.signature, 80)
                    ));
                }
            }
            sec.push('\n');
        }
        sections.push(sec);
    }

    // Assemble.
    let mut report = String::from("# Project Map\n\n");
    for sec in &sections {
        report.push_str(sec);
        report.push('\n');
    }

    truncate(&report, max_chars)
}

// ---------------------------------------------------------------------------
// 2. Call-chain report
// ---------------------------------------------------------------------------

/// Show callers and callees for a given symbol, with PageRank scores.
///
/// The chain is displayed as a tree up to `max_depth` levels deep (both
/// incoming and outgoing directions).
pub fn render_call_chain_report(
    engine: &RepoMapEngine,
    symbol_name: &str,
    max_depth: usize,
) -> String {
    let graph = engine.graph();

    // `query_symbol` returns Vec — take the best (first) match.
    let symbol = match engine.query_symbol(symbol_name).first() {
        Some(s) => *s,
        None => {
            return format!(
                "# Call Chain\n\nSymbol `{}` not found.\n",
                symbol_name
            )
        }
    };

    let mut report = format!("# Call Chain: `{}`\n\n", symbol.name);
    report.push_str(&format!("- Kind: {}\n", symbol.kind));
    report.push_str(&format!("- File: `{}`:{}\n", symbol.file, symbol.line));
    report.push_str(&format!("- PageRank: {:.4}\n\n", symbol.pagerank));

    // Callers (incoming edges).
    report.push_str("## Callers\n\n");
    let start_len = report.len();
    let mut visited = HashSet::new();
    visited.insert(symbol.id.clone());
    format_call_tree(
        &symbol.id,
        graph,
        "callers",
        max_depth,
        0,
        &mut visited,
        &mut report,
    );
    if report.len() == start_len {
        report.push_str("None found.\n");
    }

    // Callees (outgoing edges).
    report.push_str("\n## Callees\n\n");
    let start2_len = report.len();
    let mut visited = HashSet::new();
    visited.insert(symbol.id.clone());
    format_call_tree(
        &symbol.id,
        graph,
        "callees",
        max_depth,
        0,
        &mut visited,
        &mut report,
    );
    if report.len() == start2_len {
        report.push_str("None found.\n");
    }

    report
}

/// Recursive tree formatter for callers / callees.
fn format_call_tree(
    sym_id: &str,
    graph: &crate::types::RepoGraph,
    direction: &str,
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<String>,
    out: &mut String,
) {
    if current_depth >= max_depth || current_depth > 8 {
        return;
    }

    let edges = match direction {
        "callers" => graph.incoming.get(sym_id),
        "callees" => graph.outgoing.get(sym_id),
        _ => None,
    };

    let Some(edge_list) = edges else { return };

    for edge in edge_list {
        let target_id = match direction {
            "callers" => &edge.source,
            "callees" => &edge.target,
            _ => continue,
        };

        // Avoid cycles.
        if !visited.insert(target_id.clone()) {
            continue;
        }

        if let Some(sym) = graph.symbols.get(target_id) {
            let indent = "  ".repeat(current_depth);
            out.push_str(&format!(
                "{}| `{}` (PR: {:.4}) in `{}`:{}\n",
                indent, sym.name, sym.pagerank, sym.file, sym.line,
            ));

            format_call_tree(
                target_id,
                graph,
                direction,
                max_depth,
                current_depth + 1,
                visited,
                out,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3. File detail report
// ---------------------------------------------------------------------------

/// List all symbols in a file with kind, signature, PageRank, and visibility.
pub fn render_file_detail_report(
    engine: &RepoMapEngine,
    file_path: &str,
    max_symbols: usize,
    max_chars: usize,
) -> String {
    let graph = engine.graph();

    let sym_ids = match graph.file_symbols.get(file_path) {
        Some(ids) => ids,
        None => {
            return format!(
                "# File Detail\n\nFile `{}` not found in the graph.\n",
                file_path
            )
        }
    };

    let mut report = format!("# File: `{}`\n\n", file_path);

    // File-level stats.
    let sym_count = sym_ids.len();
    report.push_str(&format!("- Symbols: {}\n", sym_count));

    // Risk assessment for this single file.
    let (level, _reasons) = assess_risk(&[file_path.to_string()]);
    report.push_str(&format!("- Risk class: {}\n\n", level));

    report.push_str("## Symbols\n\n");

    let mut symbols: Vec<&Symbol> = sym_ids
        .iter()
        .filter_map(|id| graph.symbols.get(id))
        .collect();
    symbols.sort_by(|a, b| {
        b.pagerank
            .partial_cmp(&a.pagerank)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for sym in symbols.iter().take(max_symbols) {
        report.push_str(&format!("### `{}` ({})\n", sym.name, sym.kind));
        report.push_str(&format!("- Line: {}-{}\n", sym.line, sym.end_line));
        report.push_str(&format!("- Visibility: {}\n", sym.visibility));
        report.push_str(&format!("- PageRank: {:.4}\n", sym.pagerank));
        if !sym.signature.is_empty() {
            report.push_str(&format!("- Signature: `{}`\n", sym.signature));
        }
        if !sym.docstring.is_empty() {
            report.push_str(&format!(
                "- Doc: {}\n",
                truncate(&sym.docstring, 200)
            ));
        }
        report.push('\n');
    }

    if symbols.len() > max_symbols {
        report.push_str(&format!(
            "\n... and {} more symbols (showing top {}).\n",
            symbols.len() - max_symbols,
            max_symbols
        ));
    }

    truncate(&report, max_chars)
}

// ---------------------------------------------------------------------------
// 4. Topic-search report
// ---------------------------------------------------------------------------

/// Topic-based file search using `topic::topic_score`.
///
/// Sections: Top Files (with role), Related Tests, Key Symbols (★ for
/// query-matching symbols).
pub fn render_query_report(
    engine: &RepoMapEngine,
    query: &str,
    max_files: usize,
    max_chars: usize,
) -> String {
    let graph = engine.graph();
    let mut report = format!("# Topic Report: \"{}\"\n\n", query);

    // Collect candidate files from the graph.
    let candidate_files: Vec<String> = graph.file_symbols.keys().cloned().collect();

    // Pre-compute IDF keyword weights.
    let query_tokens = topic::split_identifier(query);
    let keyword_weights =
        topic::compute_keyword_weights(&query_tokens, &candidate_files, graph);

    // Score every file.
    let mut scored: Vec<(f64, &String)> = candidate_files
        .iter()
        .map(|f| {
            let score = topic::topic_score(query, f, graph, &keyword_weights);
            (score, f)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
    });

    let top_n = max_files.min(scored.len());
    if top_n == 0 {
        report.push_str("No matching files found.\n");
        return report;
    }

    // ---- Top Files ----
    report.push_str("## Top Files\n\n");
    for (i, (score, file)) in scored.iter().enumerate().take(top_n) {
        let role = topic::classify_file_role(file);
        report.push_str(&format!(
            "{}. `{}` (role: {}, score: {:.1})\n",
            i + 1, file, role, score
        ));
    }
    report.push('\n');

    // ---- Related Tests ----
    let top_targets: Vec<String> =
        scored.iter().take(top_n).map(|(_, f)| (*f).clone()).collect();
    let test_matches = topic::find_related_tests(
        &top_targets,
        graph,
        engine.project_root(),
    );

    report.push_str("## Related Tests\n\n");
    if test_matches.is_empty() {
        report.push_str("No related tests found.\n\n");
    } else {
        report.push_str("| Test File | Target File | Confidence | Reason |\n");
        report.push_str("|---|---|---|---|\n");
        for tm in test_matches.iter().take(10) {
            report.push_str(&format!(
                "| `{}` | `{}` | {:.0}% | {} |\n",
                tm.test_file, tm.target_file, tm.confidence * 100.0, tm.reason
            ));
        }
        report.push('\n');
    }

    // ---- Key Symbols ----
    report.push_str("## Key Symbols\n\n");
    for (_score, file) in scored.iter().take(top_n) {
        if let Some(sym_ids) = graph.file_symbols.get(file.as_str()) {
            for sym_id in sym_ids.iter().take(5) {
                if let Some(sym) = graph.symbols.get(sym_id) {
                    let matched = query_tokens.iter().any(|tok| {
                        topic::split_identifier(&sym.name).contains(tok)
                    });
                    let star = if matched { " \u{2605}" } else { "" };
                    report.push_str(&format!(
                        "- `{}` ({}) in `{}`{} \u{2014} PR: {:.2}\n",
                        sym.name,
                        sym.kind,
                        file,
                        star,
                        sym.pagerank
                    ));
                }
            }
        }
    }

    truncate(&report, max_chars)
}

// ---------------------------------------------------------------------------
// 5. Impact report
// ---------------------------------------------------------------------------

/// Analyse the impact of changes to the given files.
///
/// Lists changed files, affected dependents (via incoming edges), and
/// computes a keyword-based risk assessment.
pub fn render_impact_report(
    engine: &RepoMapEngine,
    target_files: &[String],
    max_chars: usize,
) -> String {
    let graph = engine.graph();
    let mut report = String::from("# Impact Report\n\n");

    // ---- Changed Files ----
    report.push_str("## Changed Files\n\n");
    if target_files.is_empty() {
        report.push_str("No files provided.\n\n");
        return report;
    }
    for f in target_files {
        report.push_str(&format!("- `{}`\n", f));
    }
    report.push('\n');

    // ---- Affected Dependents ----
    report.push_str("## Affected Dependents\n\n");
    let mut dependents: Vec<String> = Vec::new();
    let target_sym_ids: HashSet<&str> = target_files
        .iter()
        .flat_map(|f| graph.file_symbols.get(f.as_str()))
        .flatten()
        .map(|s| s.as_str())
        .collect();

    for sym_id in &target_sym_ids {
        if let Some(edges) = graph.incoming.get(*sym_id) {
            for edge in edges {
                if let Some(caller) = graph.symbols.get(&edge.source) {
                    dependents.push(format!(
                        "`{}` in `{}` depends via `{}`",
                        caller.name, caller.file, sym_id
                    ));
                }
            }
        }
    }

    dependents.sort();
    dependents.dedup();

    if dependents.is_empty() {
        report.push_str("No affected dependents found.\n\n");
    } else {
        for dep in dependents.iter().take(20) {
            report.push_str(&format!("- {}\n", dep));
        }
        if dependents.len() > 20 {
            report.push_str(&format!(
                "\n... and {} more (showing top 20).\n",
                dependents.len() - 20
            ));
        }
        report.push('\n');
    }

    // ---- Risk Assessment ----
    report.push_str("## Risk Assessment\n\n");
    let (level, reasons) = assess_risk(target_files);
    report.push_str(&format!("**{}**\n\n", level));
    for reason in &reasons {
        report.push_str(&format!("- {}\n", reason));
    }
    report.push('\n');

    truncate(&report, max_chars)
}

// ---------------------------------------------------------------------------
// 6. Diff-risk report
// ---------------------------------------------------------------------------

/// Extended impact analysis for diff reviews.
///
/// Same as the impact report plus manual-verification suggestions and
/// auto-detected test commands.
pub fn render_diff_risk_report(
    engine: &RepoMapEngine,
    changed_files: &[String],
    max_chars: usize,
) -> String {
    // Reuse the basic impact report.
    let impact = render_impact_report(engine, changed_files, max_chars);
    let mut report = impact;

    if !report.ends_with('\n') {
        report.push('\n');
    }

    // ---- Manual Verification Suggestions ----
    report.push_str("## Manual Verification Suggestions\n\n");

    let (level, reasons) = assess_risk(changed_files);
    let mut suggestions: Vec<String> = Vec::new();

    if level == "HIGH" || level == "MEDIUM" {
        suggestions.push(
            "Review security-sensitive changes (auth, credentials, permissions)."
                .to_string(),
        );
    }
    if reasons.iter().any(|r| r.starts_with("db-related")) {
        suggestions
            .push("Verify database migration scripts for backward compatibility.".to_string());
        suggestions.push("Check that schema changes are idempotent.".to_string());
    }
    if reasons.iter().any(|r| r.starts_with("config-related")) {
        suggestions.push(
            "Review configuration changes and ensure environment variables are updated."
                .to_string(),
        );
    }
    if reasons.iter().any(|r| r.starts_with("auth-related")) {
        suggestions.push(
            "Check authentication / authorisation flow for regressions.".to_string(),
        );
    }
    if level == "HIGH" {
        suggestions.push("Consider a code review by a second developer.".to_string());
    }

    if suggestions.is_empty() {
        report.push_str("No specific verification suggestions.\n\n");
    } else {
        for s in &suggestions {
            report.push_str(&format!("- {}\n", s));
        }
        report.push('\n');
    }

    // ---- Suggested Test Commands ----
    report.push_str("## Suggested Test Commands\n\n");
    let mut commands: Vec<String> = Vec::new();

    for f in changed_files {
        if f.ends_with(".rs") {
            commands.push("```bash\ncargo test\n```".to_string());
        } else if f.ends_with(".py") {
            commands.push("```bash\npytest\n```".to_string());
        } else if f.ends_with(".js") || f.ends_with(".ts") || f.ends_with(".jsx") || f.ends_with(".tsx")
        {
            commands.push("```bash\nnpm test\n```".to_string());
        }
    }

    // Fallback: use the engine project root for language detection.
    if commands.is_empty() {
        let graph = engine.graph();
        let all_files: HashSet<&str> = graph.file_symbols.keys().map(|s| s.as_str()).collect();

        if all_files.iter().any(|f| f.ends_with(".rs")) {
            commands.push("```bash\ncargo test\n```".to_string());
        }
        if all_files.iter().any(|f| f.ends_with(".py")) {
            commands.push("```bash\npytest\n```".to_string());
        }
        if all_files
            .iter()
            .any(|f| f.ends_with(".js") || f.ends_with(".ts"))
        {
            commands.push("```bash\nnpm test\n```".to_string());
        }
    }

    commands.dedup();

    if commands.is_empty() {
        report.push_str("Unable to detect test framework.\n\n");
    } else {
        for cmd in &commands {
            report.push_str(cmd);
            report.push('\n');
        }
    }

    truncate(&report, max_chars)
}

