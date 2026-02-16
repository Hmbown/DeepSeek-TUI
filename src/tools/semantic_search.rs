//! Semantic search tool with hybrid lexical + semantic ranking.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ignore::WalkBuilder;
use serde::Serialize;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_u64, required_str,
};

const DEFAULT_TOP_K: usize = 8;
const MAX_TOP_K: usize = 64;
const DEFAULT_CHUNK_SIZE: usize = 800;
const MIN_CHUNK_SIZE: usize = 200;
const MAX_CHUNK_SIZE: usize = 4_000;
const DEFAULT_MAX_FILES: usize = 300;
const MAX_MAX_FILES: usize = 2_000;
const MAX_FILE_BYTES: u64 = 1_500_000;
const MAX_CANDIDATE_CHUNKS: usize = 4_000;
const EXCERPT_CHARS: usize = 280;
const EMBED_DIM: usize = 256;
const DISABLE_SEMANTIC_ENV: &str = "DEEPSEEK_DISABLE_SEMANTIC_SEARCH";

#[derive(Debug, Clone, Serialize)]
struct SemanticSearchHit {
    path: String,
    line: usize,
    excerpt: String,
    score: f64,
    lexical_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
struct SemanticSearchResponse {
    query: String,
    results: Vec<SemanticSearchHit>,
    scanned_files: usize,
    candidate_chunks: usize,
    fallback_used: bool,
    backend: String,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct ChunkCandidate {
    path: PathBuf,
    line: usize,
    text: String,
    lexical_score: f64,
    semantic_score: f64,
    score: f64,
}

/// Tool for hybrid lexical + semantic code search.
pub struct SemanticSearchTool;

#[async_trait]
impl ToolSpec for SemanticSearchTool {
    fn name(&self) -> &'static str {
        "semantic_search"
    }

    fn description(&self) -> &'static str {
        "Search code/doc chunks using hybrid lexical and semantic ranking."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural-language query to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Optional base path (relative to workspace)"
                },
                "paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional list of base paths (relative to workspace)"
                },
                "top_k": {
                    "type": "integer",
                    "description": "Maximum number of ranked results (default: 8)"
                },
                "chunk_size": {
                    "type": "integer",
                    "description": "Approximate characters per chunk (default: 800)"
                },
                "max_files": {
                    "type": "integer",
                    "description": "Maximum files to scan (default: 300)"
                },
                "include_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files/directories (default: false)"
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?.trim();
        if query.is_empty() {
            return Err(ToolError::invalid_input("query cannot be empty"));
        }

        let top_k = usize::try_from(optional_u64(&input, "top_k", DEFAULT_TOP_K as u64))
            .unwrap_or(DEFAULT_TOP_K)
            .clamp(1, MAX_TOP_K);
        let chunk_size = usize::try_from(optional_u64(
            &input,
            "chunk_size",
            DEFAULT_CHUNK_SIZE as u64,
        ))
        .unwrap_or(DEFAULT_CHUNK_SIZE)
        .clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
        let max_files = usize::try_from(optional_u64(&input, "max_files", DEFAULT_MAX_FILES as u64))
            .unwrap_or(DEFAULT_MAX_FILES)
            .clamp(1, MAX_MAX_FILES);
        let include_hidden = optional_bool(&input, "include_hidden", false);

        let roots = resolve_roots(context, &input)?;
        let files = collect_files(&roots, include_hidden, max_files)?;

        let query_tokens = tokenize(query);
        let query_phrase = normalize_text(query);

        let mut candidates = Vec::new();
        let mut scanned_files = 0usize;

        for file in files {
            if candidates.len() >= MAX_CANDIDATE_CHUNKS {
                break;
            }
            let Ok(metadata) = fs::metadata(&file) else {
                continue;
            };
            if metadata.len() > MAX_FILE_BYTES {
                continue;
            }

            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if !looks_like_text(&content) {
                continue;
            }
            scanned_files += 1;

            let chunks = split_into_chunks(&content, chunk_size);
            if chunks.is_empty() {
                continue;
            }

            let mut best_for_file: Option<ChunkCandidate> = None;
            for (line, chunk_text) in chunks {
                if candidates.len() >= MAX_CANDIDATE_CHUNKS {
                    break;
                }
                let lexical = lexical_score(&chunk_text, &query_tokens, &query_phrase);
                let candidate = ChunkCandidate {
                    path: file.clone(),
                    line,
                    text: chunk_text,
                    lexical_score: lexical,
                    semantic_score: 0.0,
                    score: lexical,
                };

                if lexical > 0.0 {
                    candidates.push(candidate);
                } else if best_for_file
                    .as_ref()
                    .is_none_or(|best| candidate.text.len() > best.text.len())
                {
                    best_for_file = Some(candidate);
                }
            }

            if candidates.iter().all(|c| c.path != file)
                && let Some(fallback_chunk) = best_for_file
            {
                candidates.push(fallback_chunk);
            }
        }

        let mut fallback_used = false;
        let semantic_enabled = semantic_backend_enabled();
        let query_embedding = if semantic_enabled {
            embed(query)
        } else {
            None
        };

        if query_embedding.is_none() {
            fallback_used = true;
        }

        if let Some(query_vec) = query_embedding {
            for candidate in &mut candidates {
                let semantic = embed(&candidate.text)
                    .map(|chunk_vec| cosine_similarity(&query_vec, &chunk_vec))
                    .unwrap_or(0.0);
                candidate.semantic_score = semantic;
                candidate.score = (candidate.lexical_score * 0.35) + (semantic * 0.65);
            }
        } else {
            for candidate in &mut candidates {
                candidate.semantic_score = 0.0;
                candidate.score = candidate.lexical_score;
            }
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
        });

        let truncated = candidates.len() > top_k;
        let hits: Vec<SemanticSearchHit> = candidates
            .into_iter()
            .take(top_k)
            .map(|item| SemanticSearchHit {
                path: item
                    .path
                    .strip_prefix(&context.workspace)
                    .unwrap_or(&item.path)
                    .to_string_lossy()
                    .to_string(),
                line: item.line,
                excerpt: compact_excerpt(&item.text, EXCERPT_CHARS),
                score: round_score(item.score),
                lexical_score: round_score(item.lexical_score),
                semantic_score: if fallback_used {
                    None
                } else {
                    Some(round_score(item.semantic_score))
                },
            })
            .collect();

        let response = SemanticSearchResponse {
            query: query.to_string(),
            scanned_files,
            candidate_chunks: hits.len(),
            results: hits,
            fallback_used,
            backend: if fallback_used {
                "lexical-only".to_string()
            } else {
                "local-hash-embed".to_string()
            },
            truncated,
        };

        ToolResult::json(&response).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

fn resolve_roots(context: &ToolContext, input: &Value) -> Result<Vec<PathBuf>, ToolError> {
    let mut roots = Vec::new();

    if let Some(path) = input.get("path").and_then(|v| v.as_str())
        && !path.trim().is_empty()
    {
        roots.push(context.resolve_path(path)?);
    }

    if let Some(paths) = input.get("paths").and_then(|v| v.as_array()) {
        for value in paths {
            if let Some(path) = value.as_str() {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    roots.push(context.resolve_path(trimmed)?);
                }
            }
        }
    }

    if roots.is_empty() {
        roots.push(context.workspace.clone());
    }

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        if seen.insert(root.clone()) {
            deduped.push(root);
        }
    }
    Ok(deduped)
}

fn collect_files(
    roots: &[PathBuf],
    include_hidden: bool,
    max_files: usize,
) -> Result<Vec<PathBuf>, ToolError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for root in roots {
        if out.len() >= max_files {
            break;
        }

        if root.is_file() {
            if seen.insert(root.clone()) {
                out.push(root.clone());
            }
            continue;
        }

        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(!include_hidden)
            .follow_links(true)
            .require_git(false);

        for entry in builder.build() {
            if out.len() >= max_files {
                break;
            }
            let entry = entry
                .map_err(|e| ToolError::execution_failed(format!("File walk failed: {e}")))?;
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path().to_path_buf();
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }

    Ok(out)
}

fn split_into_chunks(content: &str, target_chars: usize) -> Vec<(usize, String)> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut start_line = 1usize;
    let mut current_line = 1usize;

    for line in content.lines() {
        let next_len = current.len() + line.len() + 1;
        if !current.is_empty() && next_len > target_chars {
            chunks.push((start_line, current.trim().to_string()));
            current.clear();
            start_line = current_line;
        }

        current.push_str(line);
        current.push('\n');
        current_line += 1;
    }

    if !current.trim().is_empty() {
        chunks.push((start_line, current.trim().to_string()));
    }

    chunks
}

fn looks_like_text(content: &str) -> bool {
    !content.contains('\u{0}')
}

fn normalize_text(text: &str) -> String {
    text.to_ascii_lowercase()
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut counts: HashMap<String, usize> = HashMap::new();
    for token in tokens {
        *counts.entry(token).or_insert(0) += 1;
    }

    counts
        .into_iter()
        .filter(|(token, _)| token.len() > 1)
        .map(|(token, _)| token)
        .collect()
}

fn lexical_score(chunk: &str, query_tokens: &[String], query_phrase: &str) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }

    let chunk_norm = normalize_text(chunk);
    let mut hits = 0usize;
    for token in query_tokens {
        if chunk_norm.contains(token) {
            hits += 1;
        }
    }

    let token_score = hits as f64 / query_tokens.len() as f64;
    let phrase_bonus = if !query_phrase.is_empty() && chunk_norm.contains(query_phrase) {
        0.2
    } else {
        0.0
    };

    (token_score + phrase_bonus).min(1.0)
}

fn semantic_backend_enabled() -> bool {
    !std::env::var(DISABLE_SEMANTIC_ENV)
        .ok()
        .as_deref()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn embed(text: &str) -> Option<Vec<f64>> {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return None;
    }

    let mut vec = vec![0.0_f64; EMBED_DIM];
    for token in tokens {
        let idx = (stable_hash(&token) as usize) % EMBED_DIM;
        vec[idx] += 1.0;
    }

    let norm = vec.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm <= f64::EPSILON {
        return None;
    }

    for value in &mut vec {
        *value /= norm;
    }

    Some(vec)
}

fn stable_hash(text: &str) -> u64 {
    // FNV-1a 64-bit
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut sum = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        sum += x * y;
    }
    sum.clamp(0.0, 1.0)
}

fn compact_excerpt(text: &str, max_chars: usize) -> String {
    let normalized = text.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let head: String = normalized.chars().take(max_chars).collect();
    format!("{}...", head.trim_end())
}

fn round_score(score: f64) -> f64 {
    (score * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use crate::tools::spec::{ToolContext, ToolSpec};

    use super::SemanticSearchTool;

    #[tokio::test]
    async fn semantic_search_finds_relevant_chunk() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();

        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn analyze() {\n  // ownership and lifetimes are central\n}\n",
        )
        .expect("write");
        std::fs::write(root.join("README.md"), "terminal color theme settings\n").expect("write");

        let ctx = ToolContext::new(root.to_path_buf());
        let tool = SemanticSearchTool;
        let result = tool
            .execute(json!({"query": "ownership lifetimes", "top_k": 3}), &ctx)
            .await
            .expect("execute");

        assert!(result.success);
        assert!(result.content.contains("src/lib.rs"));
    }

    #[tokio::test]
    async fn semantic_search_respects_path_filter() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();

        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::create_dir_all(root.join("docs")).expect("mkdir");
        std::fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").expect("write");
        std::fs::write(root.join("docs/guide.md"), "alpha beta gamma\n").expect("write");

        let ctx = ToolContext::new(root.to_path_buf());
        let tool = SemanticSearchTool;
        let result = tool
            .execute(
                json!({"query": "alpha", "paths": ["docs"], "top_k": 3}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.success);
        assert!(result.content.contains("docs/guide.md"));
        assert!(!result.content.contains("src/lib.rs"));
    }

    #[tokio::test]
    async fn semantic_search_reports_fallback_when_disabled() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(root.join("notes.txt"), "offline semantic fallback\n").expect("write");

        // Safety: test-only environment mutation scoped to this test.
        unsafe {
            std::env::set_var("DEEPSEEK_DISABLE_SEMANTIC_SEARCH", "1");
        }

        let ctx = ToolContext::new(root.to_path_buf());
        let tool = SemanticSearchTool;
        let result = tool
            .execute(json!({"query": "offline fallback"}), &ctx)
            .await
            .expect("execute");

        // Safety: test-only environment cleanup scoped to this test.
        unsafe {
            std::env::remove_var("DEEPSEEK_DISABLE_SEMANTIC_SEARCH");
        }

        assert!(result.success);
        assert!(result.content.contains("\"fallback_used\": true"));
    }
}
