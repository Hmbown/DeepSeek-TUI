#![allow(dead_code)]
//! Web Content Pipeline — deep crawling, content filtering, and smart chunking.
//!
//! Inspired by Crawl4AI's web content extraction pipeline:
//! - **Deep Crawl**: BFS/DFS traversal of linked pages with depth/page limits
//! - **Content Filter**: BM25-style relevance filtering to remove noise
//! - **Smart Chunking**: Split large responses into coherent, processable segments
//!
//! # Architecture
//!
//! ```text
//! URL → Deep Crawl → Filter → Chunk → LLM-ready output
//!  │        │          │        │
//!  │        └─ BFS/DFS └─ BM25  └─ Token/sentence aware
//!  └─ Checkpoint/resume
//! ```

use std::collections::{HashMap, HashSet, VecDeque};
use serde::{Deserialize, Serialize};

// ── Deep Crawl ──────────────────────────────────────────────────────────────

/// Deep crawling strategy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrawlStrategy {
    /// Breadth-first: explore all links at current depth before going deeper.
    BFS,
    /// Depth-first: follow one path deeply before backtracking.
    DFS,
    /// Best-first: prioritize pages with highest relevance scores.
    BestFirst,
}

impl CrawlStrategy {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BFS => "bfs",
            Self::DFS => "dfs",
            Self::BestFirst => "best_first",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "bfs" | "breadth" | "breadth_first" => Some(Self::BFS),
            "dfs" | "depth" | "depth_first" => Some(Self::DFS),
            "best_first" | "best" | "priority" => Some(Self::BestFirst),
            _ => None,
        }
    }
}

/// Configuration for a deep crawl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepCrawlConfig {
    /// Starting URL.
    pub start_url: String,
    /// Crawl strategy.
    pub strategy: CrawlStrategy,
    /// Maximum crawl depth (0 = start page only).
    pub max_depth: u32,
    /// Maximum total pages to crawl.
    pub max_pages: u32,
    /// Optional URL pattern filter (e.g., "*/blog/*").
    pub url_pattern: Option<String>,
    /// Only crawl URLs on the same domain.
    pub same_domain_only: bool,
}

impl Default for DeepCrawlConfig {
    fn default() -> Self {
        Self {
            start_url: String::new(),
            strategy: CrawlStrategy::BFS,
            max_depth: 3,
            max_pages: 10,
            url_pattern: None,
            same_domain_only: true,
        }
    }
}

/// State of a deep crawl — can be checkpointed and resumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepCrawlState {
    pub config: DeepCrawlConfig,
    /// URLs already crawled.
    pub crawled: HashSet<String>,
    /// URLs discovered but not yet crawled.
    pub queue: VecDeque<(String, u32, f64)>, // (url, depth, score)
    /// Pages crawled so far.
    pub pages_crawled: u32,
    /// Collected crawl results.
    pub results: Vec<CrawlPageResult>,
    /// Current status.
    pub status: CrawlStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrawlStatus {
    Idle,
    Running,
    Paused,
    Completed,
    Cancelled,
}

/// Result from crawling a single page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlPageResult {
    pub url: String,
    pub depth: u32,
    pub title: Option<String>,
    pub content_length: usize,
    pub filtered_length: usize,
    pub links_found: Vec<String>,
    pub score: f64,
}

impl DeepCrawlState {
    /// Start a new deep crawl.
    #[must_use]
    pub fn new(config: DeepCrawlConfig) -> Self {
        let mut queue = VecDeque::new();
        queue.push_back((config.start_url.clone(), 0, 0.0));
        Self {
            config,
            crawled: HashSet::new(),
            queue,
            pages_crawled: 0,
            results: Vec::new(),
            status: CrawlStatus::Idle,
        }
    }

    /// Resume from a saved state.
    #[must_use]
    pub fn resume(mut self) -> Self {
        self.status = CrawlStatus::Running;
        self
    }

    /// Get the next URL to crawl.
    #[must_use]
    pub fn next_url(&mut self) -> Option<(String, u32)> {
        if self.pages_crawled >= self.config.max_pages {
            self.status = CrawlStatus::Completed;
            return None;
        }

        match self.config.strategy {
            CrawlStrategy::BFS => self.queue.pop_front(),
            CrawlStrategy::DFS => self.queue.pop_back(),
            CrawlStrategy::BestFirst => {
                // Find highest-scored URL
                let mut best_idx: Option<usize> = None;
                let mut best_score = f64::NEG_INFINITY;
                for (i, (_, _, score)) in self.queue.iter().enumerate() {
                    if *score > best_score {
                        best_score = *score;
                        best_idx = Some(i);
                    }
                }
                best_idx.map(|i| self.queue.remove(i).unwrap())
            }
        }
        .map(|(url, depth, _score)| (url, depth))
    }

    /// Record a crawl result and enqueue newly discovered links.
    pub fn record_result(&mut self, url: String, depth: u32, result: CrawlPageResult) {
        self.crawled.insert(url);
        self.pages_crawled += 1;

        // Enqueue new links for the next depth level
        if depth < self.config.max_depth && self.pages_crawled < self.config.max_pages {
            for link in &result.links_found {
                if self.crawled.contains(link) {
                    continue;
                }
                if let Some(ref pattern) = self.config.url_pattern
                    && !url_matches_pattern(link, pattern)
                {
                    continue;
                }
                if self.config.same_domain_only
                    && let Some(start_domain) = extract_domain(&self.config.start_url)
                    && let Some(link_domain) = extract_domain(link)
                    && link_domain != start_domain
                {
                    continue;
                }
                // Already queued?
                if self.queue.iter().any(|(u, _, _)| u == link) {
                    continue;
                }
                self.queue.push_back((link.clone(), depth + 1, result.score));
            }
        }

        self.results.push(result.clone());

        if self.queue.is_empty() || self.pages_crawled >= self.config.max_pages {
            self.status = CrawlStatus::Completed;
        }
    }
}

fn url_matches_pattern(url: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_matches('*');
    url.contains(pattern)
}

fn extract_domain(url: &str) -> Option<String> {
    let url = url.trim_start_matches("https://").trim_start_matches("http://");
    url.split('/').next().map(|d| d.split(':').next().unwrap_or(d).to_lowercase())
}

// ── Content Filter ──────────────────────────────────────────────────────────

/// BM25-inspired content relevance filter.
///
/// Scores content chunks against a query or topic using term frequency
/// and inverse document frequency-like weighting.
#[derive(Debug, Clone)]
pub struct ContentFilter {
    /// Terms to boost (positive weight).
    boost_terms: Vec<String>,
    /// Terms to penalize (negative weight).
    penalty_terms: Vec<String>,
    /// Minimum score threshold for keeping content.
    threshold: f64,
    /// Minimum word count for a chunk to be considered.
    min_word_count: usize,
}

impl Default for ContentFilter {
    fn default() -> Self {
        Self {
            boost_terms: Vec::new(),
            penalty_terms: vec![
                "cookie".into(), "privacy".into(), "advertisement".into(),
                "subscribe".into(), "newsletter".into(), "popup".into(),
                "click here".into(), "sign up".into(), "sponsored".into(),
            ],
            threshold: 0.0,
            min_word_count: 5,
        }
    }
}

impl ContentFilter {
    /// Create a filter focused on a specific query.
    #[must_use]
    pub fn for_query(query: &str) -> Self {
        let boost_terms: Vec<String> = query
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| w.to_lowercase())
            .collect();
        Self {
            boost_terms,
            ..Default::default()
        }
    }

    /// Score a text block against the filter. Higher is more relevant.
    #[must_use]
    pub fn score(&self, text: &str) -> f64 {
        let lower = text.to_lowercase();
        let word_count = lower.split_whitespace().count();
        if word_count < self.min_word_count {
            return -1.0;
        }

        let mut score = 1.0; // baseline

        // Boost terms
        for term in &self.boost_terms {
            let count = lower.matches(term.as_str()).count();
            if count > 0 {
                score += (count as f64) * 0.5;
            }
        }

        // Penalty terms
        for term in &self.penalty_terms {
            if lower.contains(term.as_str()) {
                score -= 0.3;
            }
        }

        // Content richness signals
        // Has headings?
        if lower.contains("# ") || text.contains("## ") {
            score += 0.2;
        }
        // Has code blocks?
        if lower.contains("```") {
            score += 0.3;
        }
        // Has tables?
        if lower.contains("| --") || lower.contains("|---|---") {
            score += 0.2;
        }
        // Lots of links → likely navigation, penalize
        let link_count = lower.matches("http").count();
        if link_count > 10 {
            score -= (link_count as f64 - 10.0) * 0.1;
        }

        score.max(0.0)
    }

    /// Filter a text block: returns Some(text) if it passes the threshold.
    #[must_use]
    pub fn filter(&self, text: &str) -> Option<String> {
        let s = self.score(text);
        if s >= self.threshold {
            Some(text.to_string())
        } else {
            None
        }
    }

    /// Filter a collection of text blocks, returning only those above threshold.
    #[must_use]
    pub fn filter_many(&self, texts: &[String]) -> Vec<(String, f64)> {
        let mut results: Vec<(String, f64)> = texts
            .iter()
            .map(|t| (t.clone(), self.score(t)))
            .filter(|(_, s)| *s >= self.threshold)
            .collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }
}

// ── Smart Chunking ──────────────────────────────────────────────────────────

/// Chunk a large text into coherent segments for LLM processing.
///
/// Chunking strategies match Crawl4AI's approach:
/// - **Sentence**: Split at sentence boundaries, merge up to target size
/// - **Paragraph**: Split at paragraph breaks
/// - **Heading**: Split at markdown headings (#, ##, ###)
/// - **Fixed**: Split at fixed character intervals
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChunkStrategy {
    Sentence,
    Paragraph,
    Heading,
    Fixed,
}

/// Configuration for text chunking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkConfig {
    pub strategy: ChunkStrategy,
    /// Target characters per chunk.
    pub target_size: usize,
    /// Overlap between adjacent chunks in characters.
    pub overlap: usize,
    /// Maximum chunks to produce.
    pub max_chunks: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            strategy: ChunkStrategy::Sentence,
            target_size: 4000,
            overlap: 200,
            max_chunks: 20,
        }
    }
}

/// Split text into chunks using the configured strategy.
#[must_use]
pub fn chunk_text(text: &str, config: &ChunkConfig) -> Vec<String> {
    let segments = match config.strategy {
        ChunkStrategy::Sentence => split_sentences(text),
        ChunkStrategy::Paragraph => text.split("\n\n").map(|s| s.to_string()).collect(),
        ChunkStrategy::Heading => split_by_headings(text),
        ChunkStrategy::Fixed => {
            text.chars()
                .collect::<Vec<char>>()
                .chunks(config.target_size)
                .map(|c| c.iter().collect::<String>())
                .collect()
        }
    };

    // Merge small segments up to target size
    let mut chunks = Vec::new();
    let mut current = String::new();
    for seg in segments {
        if current.len() + seg.len() > config.target_size && !current.is_empty() {
            chunks.push(current);
            current = String::new();
            // Add overlap from previous chunk
            if config.overlap > 0 && let Some(last) = chunks.last() {
                let overlap_chars: String = last
                    .chars()
                    .rev()
                    .take(config.overlap)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                current.push_str(&overlap_chars);
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&seg);
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks.truncate(config.max_chunks);
    chunks
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | '\n') && current.len() > 20 {
            sentences.push(current.trim().to_string());
            current = String::new();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

fn split_by_headings(text: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.starts_with('#') && !current.is_empty() {
            sections.push(current.trim().to_string());
            current = String::new();
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        sections.push(current.trim().to_string());
    }
    sections
}

// ── ToolSpec ────────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::json;
use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool for deep crawling, content filtering, and smart chunking.
pub struct WebPipelineTool {
    crawls: std::sync::Arc<tokio::sync::Mutex<HashMap<String, DeepCrawlState>>>,
}

impl WebPipelineTool {
    pub fn new() -> Self {
        Self {
            crawls: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ToolSpec for WebPipelineTool {
    fn name(&self) -> &'static str {
        "web_pipeline"
    }

    fn description(&self) -> &'static str {
        "Deep crawl websites using BFS/DFS/best-first strategies, filter content with BM25-style relevance scoring, and chunk large responses for LLM processing. Actions: crawl_start, crawl_next, crawl_record, crawl_status, filter_content, chunk_text."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["crawl_start", "crawl_next", "crawl_record", "crawl_status", "filter_content", "chunk_text"],
                    "description": "Pipeline action"
                },
                "crawl_id": {"type": "string"},
                "url": {"type": "string", "description": "Starting URL"},
                "strategy": {"type": "string", "enum": ["bfs", "dfs", "best_first"]},
                "max_depth": {"type": "integer", "description": "Max crawl depth (default: 3)"},
                "max_pages": {"type": "integer", "description": "Max pages (default: 10)"},
                "url_pattern": {"type": "string", "description": "URL pattern filter e.g. '*/docs/*'"},
                "same_domain": {"type": "boolean", "description": "Only crawl same domain (default: true)"},
                "query": {"type": "string", "description": "Query for content filtering"},
                "content": {"type": "string", "description": "Content to filter or chunk"},
                "title": {"type": "string", "description": "Page title for crawl_record"},
                "links": {"type": "array", "items": {"type": "string"}, "description": "Discovered links"},
                "score": {"type": "number", "description": "Relevance score"},
                "strategy_chunk": {"type": "string", "enum": ["sentence", "paragraph", "heading", "fixed"], "description": "Chunking strategy"},
                "target_size": {"type": "integer", "description": "Target chunk size in chars (default: 4000)"},
                "overlap": {"type": "integer", "description": "Chunk overlap in chars (default: 200)"},
                "max_chunks": {"type": "integer", "description": "Max chunks (default: 20)"}
            },
            "required": ["action"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'action'"))?;

        match action {
            "crawl_start" => {
                let url = get_str(&input, "url")?;
                let strategy = input
                    .get("strategy")
                    .and_then(|v| v.as_str())
                    .and_then(CrawlStrategy::from_str)
                    .unwrap_or(CrawlStrategy::BFS);
                let max_depth = input.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
                let max_pages = input.get("max_pages").and_then(|v| v.as_u64()).unwrap_or(10) as u32;

                let config = DeepCrawlConfig {
                    start_url: url,
                    strategy,
                    max_depth,
                    max_pages,
                    url_pattern: get_str(&input, "url_pattern").ok(),
                    same_domain_only: input.get("same_domain").and_then(|v| v.as_bool()).unwrap_or(true),
                };

                let state = DeepCrawlState::new(config);
                let id = format!("crawl_{}", &uuid::Uuid::new_v4().to_string()[..8]);
                self.crawls.lock().await.insert(id.clone(), state);

                Ok(ToolResult::success(format!(
                    "Deep crawl started: {id}. Use 'crawl_next' to get next URL, then 'fetch_url' to fetch it, then 'crawl_record' to record results."
                )))
            }

            "crawl_next" => {
                let crawl_id = get_str(&input, "crawl_id")?;
                let mut crawls = self.crawls.lock().await;
                let state = crawls
                    .get_mut(&crawl_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Crawl '{crawl_id}' not found")))?;

                match state.next_url() {
                    Some((url, depth)) => {
                        Ok(ToolResult::success(format!(
                            "Next URL: {url} (depth: {depth}, crawled: {}/{})",
                            state.pages_crawled + 1,
                            state.config.max_pages
                        )))
                    }
                    None => {
                        Ok(ToolResult::success(format!(
                            "Crawl complete: {} pages crawled",
                            state.pages_crawled
                        )))
                    }
                }
            }

            "crawl_record" => {
                let crawl_id = get_str(&input, "crawl_id")?;
                let url = get_str(&input, "url")?;
                let depth = input.get("depth").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let title = get_str(&input, "title").ok();
                let content_len = get_str(&input, "content").map(|c| c.len()).unwrap_or(0);

                let links: Vec<String> = input
                    .get("links")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                let score = input.get("score").and_then(|v| v.as_f64()).unwrap_or(0.5);

                let result = CrawlPageResult {
                    url: url.clone(),
                    depth,
                    title,
                    content_length: content_len,
                    filtered_length: content_len,
                    links_found: links,
                    score,
                };

                let mut crawls = self.crawls.lock().await;
                let state = crawls
                    .get_mut(&crawl_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Crawl '{crawl_id}' not found")))?;

                state.record_result(url, depth, result);

                Ok(ToolResult::success(format!(
                    "Recorded page. {}/{} pages crawled. Queue: {} URLs remaining.",
                    state.pages_crawled,
                    state.config.max_pages,
                    state.queue.len()
                )))
            }

            "crawl_status" => {
                let crawl_id = get_str(&input, "crawl_id")?;
                let crawls = self.crawls.lock().await;
                let state = crawls
                    .get(&crawl_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Crawl '{crawl_id}' not found")))?;

                let summary = json!({
                    "id": crawl_id,
                    "status": format!("{:?}", state.status),
                    "strategy": state.config.strategy.as_str(),
                    "start_url": state.config.start_url,
                    "pages_crawled": state.pages_crawled,
                    "max_pages": state.config.max_pages,
                    "queue_size": state.queue.len(),
                    "results": state.results.iter().map(|r| json!({
                        "url": r.url,
                        "depth": r.depth,
                        "title": r.title,
                        "content_length": r.content_length,
                        "score": r.score
                    })).collect::<Vec<_>>()
                });

                Ok(ToolResult::success(format!(
                    "Crawl status:\n{}",
                    serde_json::to_string_pretty(&summary).unwrap_or_default()
                )))
            }

            "filter_content" => {
                let content = get_str(&input, "content")?;
                let query = get_str(&input, "query").ok();

                let filter = if let Some(q) = query {
                    ContentFilter::for_query(&q)
                } else {
                    ContentFilter::default()
                };

                let score = filter.score(&content);
                let passed = score >= filter.threshold;

                Ok(ToolResult::success(format!(
                    "Content scored {:.2} (threshold: {:.1}) → {}",
                    score,
                    filter.threshold,
                    if passed { "PASS" } else { "FILTERED OUT" }
                )))
            }

            "chunk_text" => {
                let content = get_str(&input, "content")?;
                let strategy = input
                    .get("strategy_chunk")
                    .and_then(|v| v.as_str())
                    .map(|s| match s {
                        "sentence" => ChunkStrategy::Sentence,
                        "paragraph" => ChunkStrategy::Paragraph,
                        "heading" => ChunkStrategy::Heading,
                        "fixed" => ChunkStrategy::Fixed,
                        _ => ChunkStrategy::Sentence,
                    })
                    .unwrap_or(ChunkStrategy::Sentence);

                let config = ChunkConfig {
                    strategy,
                    target_size: input.get("target_size").and_then(|v| v.as_u64()).unwrap_or(4000) as usize,
                    overlap: input.get("overlap").and_then(|v| v.as_u64()).unwrap_or(200) as usize,
                    max_chunks: input.get("max_chunks").and_then(|v| v.as_u64()).unwrap_or(20) as usize,
                };

                let chunks = chunk_text(&content, &config);

                let output: Vec<String> = chunks
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("--- Chunk {} ({} chars) ---\n{}", i + 1, c.len(), c))
                    .collect();

                Ok(ToolResult::success(format!(
                    "Split into {} chunks (strategy: {:?}):\n\n{}",
                    chunks.len(),
                    config.strategy,
                    output.join("\n\n")
                )))
            }

            _ => Err(ToolError::invalid_input(format!(
                "Unknown action '{action}'"
            ))),
        }
    }
}

fn get_str(input: &serde_json::Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| ToolError::invalid_input(format!("Missing '{key}'")))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deep_crawl_bfs() {
        let config = DeepCrawlConfig {
            start_url: "https://example.com".into(),
            strategy: CrawlStrategy::BFS,
            max_depth: 2,
            max_pages: 5,
            url_pattern: None,
            same_domain_only: true,
        };
        let mut state = DeepCrawlState::new(config);

        // Page 0
        let (url, depth) = state.next_url().unwrap();
        assert_eq!(url, "https://example.com");
        assert_eq!(depth, 0);

        state.record_result(
            url.clone(),
            depth,
            CrawlPageResult {
                url: url.clone(),
                depth,
                title: Some("Home".into()),
                content_length: 5000,
                filtered_length: 3000,
                links_found: vec!["https://example.com/about".into(), "https://example.com/blog".into()],
                score: 0.8,
            },
        );

        assert_eq!(state.pages_crawled, 1);
        assert_eq!(state.queue.len(), 2); // about + blog enqueued
    }

    #[test]
    fn test_content_filter() {
        let filter = ContentFilter::for_query("rust programming async");
        let good = "# Rust Async Programming\n\nTokio is an async runtime for Rust...";
        let bad = "Click here to subscribe to our newsletter! Privacy policy cookie consent...";

        assert!(filter.score(good) > filter.score(bad));
        assert!(filter.score(good) > 1.0);
        assert!(filter.score(bad) < 0.5);
    }

    #[test]
    fn test_chunk_text() {
        let text = "First sentence. Second sentence. Third sentence. Fourth. Fifth. Sixth. Seventh. Eighth. Ninth. Tenth.";
        let config = ChunkConfig {
            strategy: ChunkStrategy::Sentence,
            target_size: 50,
            overlap: 0,
            max_chunks: 5,
        };
        let chunks = chunk_text(text, &config);
        assert!(chunks.len() <= 5);
        assert!(chunks.iter().all(|c| c.len() > 0));
    }

    #[test]
    fn test_same_domain_filter() {
        let config = DeepCrawlConfig {
            start_url: "https://docs.example.com/guide".into(),
            strategy: CrawlStrategy::BFS,
            max_depth: 1,
            max_pages: 3,
            url_pattern: None,
            same_domain_only: true,
        };
        let mut state = DeepCrawlState::new(config);

        let (url, depth) = state.next_url().unwrap();
        state.record_result(
            url.clone(),
            depth,
            CrawlPageResult {
                url: url.clone(),
                depth,
                title: None,
                content_length: 100,
                filtered_length: 80,
                links_found: vec![
                    "https://docs.example.com/api".into(),  // same domain ✓
                    "https://other.com/page".into(),        // different domain ✗
                ],
                score: 0.5,
            },
        );

        // Only the same-domain link should be queued
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue[0].0, "https://docs.example.com/api");
    }
}
