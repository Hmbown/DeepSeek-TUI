// AI-friendly Markdown report rendering.
// Produces overviews, call chains, file details, query reports, and impact analysis.

use std::collections::HashMap;

use crate::engine::RepoMapEngine;
use crate::topic;

/// Render a full project map overview (max_chars limit).
pub fn render_overview_report(engine: &RepoMapEngine, max_chars: usize) -> String {
    let mut lines = Vec::new();

    lines.push("# Project Map".to_string());
    lines.push(String::new());

    // Scan statistics.
    lines.push("## Scan Statistics".to_string());
    for line in engine.scan_summary_lines() {
        lines.push(line);
    }
    lines.push(String::new());

    // Entry points.
    let entries = engine.entry_points();
    if !entries.is_empty() {
        lines.push("## Entry Points".to_string());
        for e in &entries {
            lines.push(format!("- `{}`", e));
        }
        lines.push(String::new());
    }

    // Reading order.
    let reading = engine.suggested_reading_order(8);
    if !reading.is_empty() {
        lines.push("## Recommended Reading Order".to_string());
        for (i, entry) in reading.iter().enumerate() {
            let file = entry.get("file").map(|s| s.as_str()).unwrap_or("");
            let score = entry.get("score").map(|s| s.as_str()).unwrap_or("");
            lines.push(format!("{}. `{}` (score: {})", i + 1, file, score));
        }
        lines.push(String::new());
    }

    // Module summary.
    let modules = engine.module_summary(6);
    if !modules.is_empty() {
        lines.push("## Module Summary".to_string());
        for m in &modules {
            let name = m.get("module").map(|s| s.as_str()).unwrap_or("");
            let count = m.get("symbols").map(|s| s.as_str()).unwrap_or("");
            let pr = m.get("total_pagerank").map(|s| s.as_str()).unwrap_or("");
            lines.push(format!(
                "- `{}/` — {} symbols, PageRank: {}",
                name, count, pr
            ));
        }
        lines.push(String::new());
    }

    // Hotspots.
    let hot = engine.hotspots(10);
    if !hot.is_empty() {
        lines.push("## Hot Spots".to_string());
        for h in &hot {
            let file = h.get("file").map(|s| s.as_str()).unwrap_or("");
            let count = h.get("symbols").map(|s| s.as_str()).unwrap_or("");
            let pr = h.get("avg_pagerank").map(|s| s.as_str()).unwrap_or("");
            lines.push(format!("- `{}` — {} symbols, avg PR: {}", file, count, pr));
        }
        lines.push(String::new());
    }

    // Key symbols.
    let syms = engine.summary_symbols(6, 4);
    if !syms.is_empty() {
        lines.push("## Key Symbols".to_string());
        let mut current_file = String::new();
        for s in &syms {
            let file = s.get("file").map(|s| s.as_str()).unwrap_or("");
            if file != current_file {
                current_file = file.to_string();
                lines.push(format!("### `{}`", current_file));
            }
            let name = s.get("name").map(|s| s.as_str()).unwrap_or("");
            let kind = s.get("kind").map(|s| s.as_str()).unwrap_or("");
            let line = s.get("line").map(|s| s.as_str()).unwrap_or("");
            let pr = s.get("pagerank").map(|s| s.as_str()).unwrap_or("");
            let sig = s.get("signature").map(|s| s.as_str()).unwrap_or("");
            let sig_str = if sig.is_empty() {
                String::new()
            } else {
                format!(" `{}`", sig)
            };
            lines.push(format!(
                "- `{}` ({}:{}, PR:{}){}",
                name, kind, line, pr, sig_str
            ));
        }
        lines.push(String::new());
    }

    let result = lines.join("\n");
    truncate(&result, max_chars)
}

/// Render a call chain report for a given symbol name.
pub fn render_call_chain_report(
    engine: &RepoMapEngine,
    symbol_name: &str,
    max_depth: usize,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("# Call Chain: `{}`", symbol_name));
    lines.push(String::new());

    // Find the best matching symbol.
    let matches = engine.query_symbol(symbol_name);
    if matches.is_empty() {
        lines.push("_No symbols found matching this name._".to_string());
        return lines.join("\n");
    }

    let best = &matches[0];
    lines.push(format!(
        "Best match: `{}` ({}:{}, PR:{:.6})",
        best.name, best.kind, best.line, best.pagerank
    ));
    lines.push(String::new());

    let chains = engine.call_chain(&best.id, "both", max_depth);

    if let Some(callers) = chains.get("callers") {
        lines.push("## Callers".to_string());
        if callers.is_empty() {
            lines.push("_None found._".to_string());
        } else {
            for sym in callers {
                lines.push(format!(
                    "- `{}` ({}:{}, PR:{:.6})",
                    sym.name, sym.kind, sym.line, sym.pagerank
                ));
            }
        }
        lines.push(String::new());
    }

    if let Some(callees) = chains.get("callees") {
        lines.push("## Callees".to_string());
        if callees.is_empty() {
            lines.push("_None found._".to_string());
        } else {
            for sym in callees {
                lines.push(format!(
                    "- `{}` ({}:{}, PR:{:.6})",
                    sym.name, sym.kind, sym.line, sym.pagerank
                ));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Render a detailed file view with symbols.
pub fn render_file_detail_report(
    engine: &RepoMapEngine,
    file_path: &str,
    max_symbols: usize,
    max_chars: usize,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("# File Detail: `{}`", file_path));
    lines.push(String::new());

    // Find symbols in this file.
    let all_syms = engine.query_symbol("");
    let file_syms: Vec<_> = all_syms
        .iter()
        .filter(|s| s.file == file_path)
        .take(max_symbols)
        .collect();

    if file_syms.is_empty() {
        lines.push("_No symbols found in this file._".to_string());
    } else {
        lines.push("## Symbols".to_string());
        for sym in &file_syms {
            let sig_str = if sym.signature.is_empty() {
                String::new()
            } else {
                format!(" `{}`", sym.signature)
            };
            lines.push(format!(
                "- `{}` [{}:{}] (PR:{:.6}){}",
                sym.name, sym.kind, sym.line, sym.pagerank, sig_str
            ));
            if !sym.docstring.is_empty() {
                lines.push(format!("  > {}", sym.docstring));
            }
        }
    }

    let result = lines.join("\n");
    truncate(&result, max_chars)
}

/// Render a topic-based query report.
pub fn render_query_report(
    engine: &RepoMapEngine,
    query: &str,
    max_files: usize,
    max_chars: usize,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("# Query: `{}`", query));
    lines.push(String::new());

    let keywords: Vec<String> = topic::split_identifier(query);
    if keywords.is_empty() {
        lines.push("_Empty query._".to_string());
        return lines.join("\n");
    }

    // Score all files by topic relevance.
    let candidate_files: Vec<String> = engine.graph.file_symbols.keys().cloned().collect();
    let keyword_weights =
        topic::compute_keyword_weights(&keywords, &candidate_files, &engine.graph);

    let mut scored: Vec<(String, f64, &str)> = candidate_files
        .iter()
        .map(|f| {
            let score = topic::topic_score(query, f, &engine.graph, &keyword_weights);
            let role = topic::classify_file_role(f);
            (f.clone(), score, role)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Separate into primary files and tests.
    let primary: Vec<_> = scored
        .iter()
        .filter(|(_, _, role)| *role != "test")
        .take(max_files)
        .collect();

    let tests: Vec<_> = scored
        .iter()
        .filter(|(_, _, role)| *role == "test")
        .take(max_files / 2)
        .collect();

    // Top files.
    lines.push("## Top Files".to_string());
    if primary.is_empty() {
        lines.push("_No matching files found._".to_string());
    } else {
        for (file, score, role) in &primary {
            let sym_count = engine
                .graph
                .file_symbols
                .get(file.as_str())
                .map(|v| v.len())
                .unwrap_or(0);
            lines.push(format!(
                "- `{}` [{}] — {} symbols, score: {:.2}",
                file, role, sym_count, score
            ));
        }
    }
    lines.push(String::new());

    // Related tests.
    if !tests.is_empty() {
        lines.push("## Related Tests".to_string());
        for (file, score, _) in &tests {
            lines.push(format!("- `{}` (score: {:.2})", file, score));
        }
        lines.push(String::new());
    }

    // Key symbols in top files.
    lines.push("## Key Symbols".to_string());
    let mut found = false;
    for (file, _, _) in primary.iter().take(6) {
        if let Some(sym_ids) = engine.graph.file_symbols.get(file.as_str()) {
            let mut syms: Vec<_> = sym_ids
                .iter()
                .filter_map(|id| engine.graph.symbols.get(id))
                .collect();
            syms.sort_by(|a, b| {
                b.pagerank
                    .partial_cmp(&a.pagerank)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for s in syms.iter().take(4) {
                let match_indicator = if keywords
                    .iter()
                    .any(|kw| s.name.to_lowercase().contains(kw.as_str()))
                {
                    " ★"
                } else {
                    ""
                };
                lines.push(format!(
                    "- `{}::{}` ({}:{}, PR:{:.4}){}",
                    file, s.name, s.kind, s.line, s.pagerank, match_indicator
                ));
                found = true;
            }
        }
    }
    if !found {
        lines.push("_No symbols found._".to_string());
    }

    let result = lines.join("\n");
    truncate(&result, max_chars)
}

/// Render an impact analysis report for changed files.
pub fn render_impact_report(
    engine: &RepoMapEngine,
    target_files: &[String],
    max_chars: usize,
) -> String {
    let mut lines = Vec::new();
    lines.push("# Impact Analysis".to_string());
    lines.push(String::new());

    lines.push("## Changed Files".to_string());
    for f in target_files {
        let role = topic::classify_file_role(f);
        lines.push(format!("- `{}` [{}]", f, role));
    }
    lines.push(String::new());

    // Find symbols in the changed files.
    let mut changed_symbols: Vec<&crate::types::Symbol> = Vec::new();
    for f in target_files {
        if let Some(sym_ids) = engine.graph.file_symbols.get(f.as_str()) {
            for sid in sym_ids {
                if let Some(sym) = engine.graph.symbols.get(sid) {
                    changed_symbols.push(sym);
                }
            }
        }
    }

    // Find files that depend on the changed symbols (via incoming edges).
    let mut affected_files: HashMap<&str, usize> = HashMap::new();
    for sym in &changed_symbols {
        if let Some(incoming) = engine.graph.incoming.get(&sym.id) {
            for edge in incoming {
                if !target_files.contains(&edge.source) {
                    if let Some(src_sym) = engine.graph.symbols.get(&edge.source) {
                        *affected_files.entry(&src_sym.file).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut affected: Vec<(&&str, &usize)> = affected_files.iter().collect();
    affected.sort_by(|a, b| b.1.cmp(a.1));

    lines.push("## Affected Files (dependents)".to_string());
    if affected.is_empty() {
        lines.push("_No dependent files found._".to_string());
    } else {
        for (file, count) in affected.iter().take(20) {
            let role = topic::classify_file_role(file);
            lines.push(format!("- `{}` [{}] — {} dependencies", file, role, count));
        }
    }
    lines.push(String::new());

    // Risk assessment.
    let risk = assess_risk(target_files);
    lines.push("## Risk Assessment".to_string());
    lines.push(format!("- **Level**: {}", risk.level));
    if !risk.reasons.is_empty() {
        for r in &risk.reasons {
            lines.push(format!("- {}", r));
        }
    }

    let result = lines.join("\n");
    truncate(&result, max_chars)
}

/// Render a diff-risk report for pending changes.
pub fn render_diff_risk_report(
    engine: &RepoMapEngine,
    changed_files: &[String],
    max_chars: usize,
) -> String {
    let mut lines = Vec::new();
    lines.push("# Diff Risk Assessment".to_string());
    lines.push(String::new());

    let impact = render_impact_report(engine, changed_files, max_chars / 2);
    lines.push(impact);
    lines.push(String::new());

    // Manual verification suggestions.
    lines.push("## Manual Verification".to_string());
    let mut suggestions = Vec::new();
    for f in changed_files {
        let lower = f.to_lowercase();
        if lower.contains("auth") || lower.contains("login") || lower.contains("session") {
            suggestions.push(format!("- Verify authentication flow: `{}`", f));
        }
        if lower.contains("db") || lower.contains("sql") || lower.contains("migration") {
            suggestions.push(format!("- Check database migration impact: `{}`", f));
        }
        if lower.contains("config") || lower.contains(".toml") || lower.contains(".json") {
            suggestions.push(format!("- Confirm configuration changes: `{}`", f));
        }
    }
    if suggestions.is_empty() {
        suggestions.push("- No specific verification items flagged.".to_string());
    }
    for s in &suggestions {
        lines.push(s.clone());
    }

    // Test commands.
    lines.push(String::new());
    lines.push("## Suggested Test Commands".to_string());
    let has_rs = changed_files.iter().any(|f| f.ends_with(".rs"));
    let has_py = changed_files.iter().any(|f| f.ends_with(".py"));
    let has_js = changed_files
        .iter()
        .any(|f| f.ends_with(".js") || f.ends_with(".ts"));

    if has_rs {
        lines.push("- `cargo test`".to_string());
    }
    if has_py {
        lines.push("- `pytest`".to_string());
    }
    if has_js {
        lines.push("- `npm test`  or  `vitest`".to_string());
    }
    if !has_rs && !has_py && !has_js {
        lines.push("- _No test framework detected._".to_string());
    }

    let result = lines.join("\n");
    truncate(&result, max_chars)
}

/// Risk level and reasons for a set of changed files.
struct RiskAssessment {
    level: String,
    reasons: Vec<String>,
}

fn assess_risk(files: &[String]) -> RiskAssessment {
    let mut reasons = Vec::new();
    let mut score = 0u32;

    for f in files {
        let lower = f.to_lowercase();
        if lower.contains("auth") || lower.contains("login") || lower.contains("token") {
            score += 3;
            reasons.push(format!("Security-sensitive file: `{}`", f));
        }
        if lower.contains("db") || lower.contains("sql") || lower.contains("migrat") {
            score += 3;
            reasons.push(format!("Database-related file: `{}`", f));
        }
        if lower.contains("config") || lower.contains("setting") {
            score += 2;
            reasons.push(format!("Configuration file: `{}`", f));
        }
        if lower.ends_with(".rs") || lower.ends_with(".go") {
            score += 1;
        }
    }

    // Scale by number of files.
    score = score.saturating_add(files.len() as u32);

    let level = if score >= 10 {
        "🔴 HIGH".to_string()
    } else if score >= 5 {
        "🟡 MEDIUM".to_string()
    } else {
        "🟢 LOW".to_string()
    };

    RiskAssessment { level, reasons }
}

/// Truncate output to max_chars, adding a note if truncated.
fn truncate(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars.saturating_sub(50)).collect();
    format!(
        "{}\n\n_... (truncated at {} chars, total {})_",
        truncated,
        max_chars,
        text.len()
    )
}
