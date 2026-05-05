//! TF-IDF 语义索引 —— 纯 Rust 实现，零外部依赖。
//!
//! # 分词策略
//!
//! - **中文**：字符 bigram（相邻字符对），如"修复bug" → ["修复", "复bu", "bug"]
//! - **英文**：小写 + 非字母切分，如"Fix buffer overflow" → ["fix", "buffer", "overflow"]
//!
//! # 索引结构
//!
//! 稀疏矩阵：`term → {doc_id → weight}`，持久化为 JSON。
//! 千条经验约 2-5MB。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::Experience;

/// TF-IDF 语义索引。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SemanticIndex {
    /// 文档内容缓存（doc_id → content）。
    pub documents: HashMap<String, String>,
    /// 倒排索引（term → {doc_id → weight}）。
    pub term_docs: HashMap<String, HashMap<String, f64>>,
    /// 文档范数缓存。
    #[serde(default)]
    doc_norms: HashMap<String, f64>,
    /// IDF 缓存。
    #[serde(default)]
    idf_cache: HashMap<String, f64>,
}

impl SemanticIndex {
    /// 从经验列表构建索引。
    #[must_use]
    pub fn build(experiences: &[Experience]) -> Self {
        let mut index = Self::default();
        for exp in experiences {
            index.add_document(exp);
        }
        index
    }

    /// 添加一个文档到索引。
    pub fn add_document(&mut self, exp: &Experience) {
        let doc_id = exp.id.clone();
        let text = exp.search_text();
        self.documents.insert(doc_id.clone(), text.clone());

        let tokens = tokenize(&text);
        let total_tokens = tokens.len() as f64;

        // 计算 TF
        let mut tf_map: HashMap<String, f64> = HashMap::new();
        for token in &tokens {
            *tf_map.entry(token.clone()).or_insert(0.0) += 1.0;
        }

        // 归一化 TF
        let mut norm_sum = 0.0;
        let mut term_weights: HashMap<String, f64> = HashMap::new();
        for (token, count) in &tf_map {
            let tf = count / total_tokens;
            let weight = tf; // 暂存，IDF 在搜索时乘入
            term_weights.insert(token.clone(), weight);
            norm_sum += weight * weight;
        }

        let norm = norm_sum.sqrt();
        self.doc_norms.insert(doc_id.clone(), norm);

        // 更新倒排索引
        for (token, weight) in term_weights {
            self.term_docs
                .entry(token)
                .or_default()
                .insert(doc_id.clone(), weight);
        }

        // 清除 IDF 缓存以便重新计算
        self.idf_cache.clear();
    }

    /// 计算 IDF。
    fn idf(&mut self, term: &str) -> f64 {
        if let Some(cached) = self.idf_cache.get(term) {
            return *cached;
        }
        let total_docs = self.documents.len() as f64;
        let docs_with_term = self
            .term_docs
            .get(term)
            .map_or(0, |map| map.len()) as f64;
        let idf = if docs_with_term == 0.0 {
            0.0
        } else {
            (total_docs / docs_with_term).ln() + 1.0
        };
        self.idf_cache.insert(term.to_string(), idf);
        idf
    }

    /// 搜索与 query 最相关的文档，返回 `(score, doc_id)` 列表。
    #[must_use]
    pub fn search(&mut self, query: &str, top_k: usize) -> Vec<(f64, String)> {
        let query_tokens = tokenize(query);
        if query_tokens.is_empty() || self.documents.is_empty() {
            return Vec::new();
        }

        // 计算 query 的 TF
        let query_len = query_tokens.len() as f64;
        let mut query_tf: HashMap<String, f64> = HashMap::new();
        for token in &query_tokens {
            *query_tf.entry(token.clone()).or_insert(0.0) += 1.0;
        }

        // 计算每个文档的余弦相似度
        let mut scores: HashMap<String, f64> = HashMap::new();
        let mut query_norm_sum = 0.0;

        for (token, q_count) in &query_tf {
            let q_tf = q_count / query_len;
            let idf = self.idf(token);
            let q_weight = q_tf * idf;
            query_norm_sum += q_weight * q_weight;

            if let Some(doc_weights) = self.term_docs.get(token) {
                for (doc_id, d_weight) in doc_weights {
                    *scores.entry(doc_id.clone()).or_insert(0.0) += q_weight * d_weight;
                }
            }
        }

        let query_norm = query_norm_sum.sqrt();
        if query_norm == 0.0 {
            return Vec::new();
        }

        // 归一化并排序
        let mut results: Vec<(f64, String)> = scores
            .into_iter()
            .filter_map(|(doc_id, dot_product)| {
                let doc_norm = self.doc_norms.get(&doc_id).copied().unwrap_or(1.0);
                let score = dot_product / (query_norm * doc_norm);
                if score > 0.0 {
                    Some((score, doc_id))
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// 从索引中移除文档。
    #[allow(dead_code)]
    pub fn remove_document(&mut self, doc_id: &str) {
        self.documents.remove(doc_id);
        self.doc_norms.remove(doc_id);
        self.term_docs.retain(|_, docs| {
            docs.remove(doc_id);
            !docs.is_empty()
        });
        self.idf_cache.clear();
    }

    /// 索引中文档数量。
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// 索引是否为空。
    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

/// 分词：中文 bigram + 英文 word。
fn tokenize(text: &str) -> Vec<String> {
    let text = text.to_lowercase();
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let len = chars.len();

    while i < len {
        let ch = chars[i];
        if ch.is_ascii_alphanumeric() || ch == '_' {
            // 英文/数字词
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect::<String>());
        } else if ch.is_whitespace() {
            i += 1;
        } else {
            // 中文/其他字符：bigram
            if i + 1 < len && !chars[i + 1].is_ascii_alphanumeric() && !chars[i + 1].is_whitespace()
            {
                let bigram: String = chars[i..=i + 1].iter().collect();
                tokens.push(bigram);
            }
            // 单字也作为 token
            tokens.push(ch.to_string());
            i += 1;
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_exp(id: &str, title: &str, context: &str, action: &str, result: &str, tags: Vec<&str>) -> Experience {
        Experience {
            id: id.to_string(),
            title: title.to_string(),
            context: context.to_string(),
            action: action.to_string(),
            result: result.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            project: String::new(),
            outcome: super::super::Outcome::Success,
            confidence: 1.0,
            reuse_count: 0,
            created_at: chrono::Utc::now(),
            last_used_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn tokenize_chinese_bigram() {
        let tokens = tokenize("修复Buffer溢出");
        // "修复" → bigram "修复"
        // "复B" → bigram (中英混合边界)
        // "Buffer" → 英文词
        // "溢出" → bigram "溢出"
        assert!(tokens.contains(&"修复".to_string()));
        assert!(tokens.contains(&"buffer".to_string()));
        assert!(tokens.contains(&"溢出".to_string()));
    }

    #[test]
    fn tokenize_english() {
        let tokens = tokenize("Fix buffer overflow bug");
        assert!(tokens.contains(&"fix".to_string()));
        assert!(tokens.contains(&"buffer".to_string()));
        assert!(tokens.contains(&"overflow".to_string()));
        assert!(tokens.contains(&"bug".to_string()));
    }

    #[test]
    fn semantic_index_build_and_search() {
        let mut index = SemanticIndex::default();

        index.add_document(&make_exp(
            "1",
            "修复 Buffer 溢出",
            "ChatWidget 渲染溢出",
            "添加边界检查",
            "修复成功",
            vec!["bugfix", "rendering"],
        ));

        index.add_document(&make_exp(
            "2",
            "Python 异步优化",
            "asyncio.gather 并行",
            "重构为 gather",
            "性能提升 3 倍",
            vec!["python", "async"],
        ));

        let results = index.search("buffer 渲染 溢出", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].1, "1");
    }

    #[test]
    fn search_returns_empty_for_unrelated_query() {
        let mut index = SemanticIndex::default();
        index.add_document(&make_exp(
            "1",
            "Python 异步",
            "asyncio 协程",
            "使用 gather",
            "性能提升",
            vec!["python"],
        ));

        let results = index.search("javascript react 前端", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn remove_document_cleanly() {
        let mut index = SemanticIndex::default();
        index.add_document(&make_exp("1", "标题", "上下文", "行动", "结果", vec![]));
        assert_eq!(index.len(), 1);

        index.remove_document("1");
        assert!(index.is_empty());
    }

    // ── tokenize 边界情况 ──────────────────────────────────

    #[test]
    fn tokenize_empty_string() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_whitespace_only() {
        let tokens = tokenize("   \t\n  ");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_numbers() {
        let tokens = tokenize("v1.2.3 修复 issue #1234");
        assert!(tokens.contains(&"v1".to_string()));
        assert!(tokens.contains(&"2".to_string()));
        assert!(tokens.contains(&"3".to_string()));
        assert!(tokens.contains(&"issue".to_string()));
        assert!(tokens.contains(&"1234".to_string()));
    }

    #[test]
    fn tokenize_special_characters() {
        let tokens = tokenize("hello_world foo-bar");
        // underscore is part of the token
        assert!(tokens.contains(&"hello_world".to_string()));
        // hyphen is not ascii alphanumeric, so "foo" and "bar" are separate
        assert!(tokens.contains(&"foo".to_string()));
        assert!(tokens.contains(&"bar".to_string()));
    }

    #[test]
    fn tokenize_lowercases_english() {
        let tokens = tokenize("Fix Buffer Overflow");
        assert!(tokens.contains(&"fix".to_string()));
        assert!(tokens.contains(&"buffer".to_string()));
        assert!(tokens.contains(&"overflow".to_string()));
    }

    // ── SemanticIndex 边界情况 ─────────────────────────────

    #[test]
    fn semantic_index_empty_returns_empty() {
        let mut index = SemanticIndex::default();
        let results = index.search("anything", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn semantic_index_search_with_empty_query() {
        let mut index = SemanticIndex::default();
        index.add_document(&make_exp("1", "测试", "上下文", "行动", "结果", vec![]));

        let results = index.search("", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn semantic_index_top_k_limits_results() {
        let mut index = SemanticIndex::default();
        for i in 0..5 {
            index.add_document(&make_exp(
                &format!("d{i}"),
                "通用主题",
                &format!("上下文 {i}"),
                "行动",
                "结果",
                vec![],
            ));
        }

        // All docs have the same content, so all will match with positive score.
        // top_k=2 should return at most 2 results.
        let results = index.search("通用", 2);
        assert!(results.len() <= 2);
    }

    #[test]
    fn semantic_index_idf_caching_works() {
        let mut index = SemanticIndex::default();
        index.add_document(&make_exp("1", "Rust", "Rust 编程语言", "写代码", "编译通过", vec![]));
        index.add_document(&make_exp("2", "Python", "Python 脚本语言", "写脚本", "运行通过", vec![]));

        // First call computes IDF, second call uses cache
        let results1 = index.search("Rust", 5);
        let results2 = index.search("Rust", 5);
        assert_eq!(results1, results2);
    }

    #[test]
    fn semantic_index_results_ordered_by_score() {
        let mut index = SemanticIndex::default();
        index.add_document(&make_exp("1", "Rust 并发编程", "Rust 的 async/await 并发模型", "使用 tokio", "并发性能好", vec![]));
        index.add_document(&make_exp("2", "Python 基础语法", "Python 的基本语法", "写 hello world", "学会了", vec![]));

        // Searching for Rust-related terms should rank doc 1 higher
        let results = index.search("Rust 并发 tokio", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].1, "1");
    }
}
