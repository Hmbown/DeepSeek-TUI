//! 虫后查询接口 —— 搜索相关经验并构建注入块。

use super::Queen;

impl Queen {
    /// 为给定 query 搜索相关经验，返回格式化的注入块。
    ///
    /// 返回 `<chonghou_memory>` XML 块，包含分数 > 0.5 的经验。
    /// 最多返回 5 条，按分数降序排列。
    #[must_use]
    pub fn query_for_prompt(&mut self, query: &str, top_k: usize) -> Option<String> {
        let results = self.semantic_index.search(query, top_k);
        let relevant: Vec<(f64, String)> = results
            .into_iter()
            .filter(|(score, _)| *score > 0.5)
            .collect();

        if relevant.is_empty() {
            return None;
        }

        let mut block = String::from("<chonghou_memory>\n");
        for (score, doc_id) in &relevant {
            if let Some(exp) = self.experiences.iter().find(|e| e.id == *doc_id) {
                block.push_str(&format!(
                    "  <experience score=\"{:.2}\">\n\
                     Title: {}\n\
                     Context: {}\n\
                     Action: {}\n\
                     Result: {}\n\
                     Outcome: {}\n\
                     Tags: {}\n\
                     Project: {}\n\
                     </experience>\n",
                    score,
                    exp.title,
                    exp.context,
                    exp.action,
                    exp.result,
                    match exp.outcome {
                        super::Outcome::Success => "success",
                        super::Outcome::Partial => "partial",
                        super::Outcome::Failure => "failure",
                    },
                    exp.tags.join(", "),
                    exp.project,
                ));
            }
        }
        block.push_str("</chonghou_memory>");
        Some(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queen::Experience;
    use crate::queen::Outcome;
    use tempfile::tempdir;

    fn make_exp(title: &str, context: &str, action: &str, result: &str, tags: Vec<&str>, project: &str, outcome: Outcome) -> Experience {
        Experience::new(
            title.to_string(),
            context.to_string(),
            action.to_string(),
            result.to_string(),
            tags.iter().map(|s| s.to_string()).collect(),
            project.to_string(),
            outcome,
            0.9,
        )
    }

    #[test]
    fn query_for_prompt_returns_block_with_relevant_experiences() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        // Write an experience and query with its own title to get a high score
        queen.write_experience(make_exp(
            "Rust异步编程优化",
            "使用tokio运行时的async/await并发模型",
            "将同步代码重构为异步",
            "性能提升3倍以上",
            vec!["rust", "async", "tokio"],
            "backend",
            Outcome::Success,
        )).unwrap();

        // Write another unrelated experience
        queen.write_experience(make_exp(
            "CSS布局修复",
            "Flexbox跨浏览器兼容性问题",
            "添加浏览器前缀和fallback",
            "布局在各浏览器一致",
            vec!["css", "frontend"],
            "web-ui",
            Outcome::Success,
        )).unwrap();

        // Use quee.search() to verify the full flow works (search bypasses score threshold)
        let results = queen.search("Rust异步", 5);
        assert!(!results.is_empty(), "queen.search should find relevant experience");
        assert!(results[0].1.title.contains("Rust异步"));

        // query_for_prompt with exact-match terminology should pass score > 0.5
        let result = queen.query_for_prompt("Rust 异步 tokio 并发", 5);
        assert!(result.is_some(), "query_for_prompt should return block for high-scoring query");
        let block = result.unwrap();
        assert!(block.starts_with("<chonghou_memory>"));
        assert!(block.ends_with("</chonghou_memory>"));
        assert!(block.contains("score="));
        assert!(block.contains("<experience"));
    }

    #[test]
    fn query_for_prompt_returns_none_for_empty_queen() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        let result = queen.query_for_prompt("anything", 5);
        assert!(result.is_none());
    }

    #[test]
    fn query_for_prompt_returns_none_for_unrelated_query() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        queen.write_experience(make_exp(
            "Python 异步优化",
            "asyncio.gather 并行处理协程",
            "重构为 gather + wait",
            "性能提升 3 倍",
            vec!["python", "async"],
            "data-pipeline",
            Outcome::Success,
        )).unwrap();

        // Completely unrelated topic should return None (score ≤ 0.5)
        let result = queen.query_for_prompt("javascript react 前端框架", 5);
        assert!(result.is_none());
    }

    #[test]
    fn query_for_prompt_respects_top_k_limit() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        // Write 3 experiences with overlapping topic
        for i in 0..3 {
            queen.write_experience(make_exp(
                &format!("Bug fix {}", i),
                &format!("Issue #{} description", i),
                &format!("Fix applied for issue {}", i),
                &format!("Issue {} resolved", i),
                vec!["bugfix"],
                "test-project",
                Outcome::Success,
            )).unwrap();
        }

        // Should return at most 2 results
        let result = queen.query_for_prompt("bug fix", 2);
        if let Some(block) = result {
            let count = block.matches("<experience").count();
            assert!(count <= 2, "expected at most 2 experiences, got {count}");
        }
    }

    #[test]
    fn query_for_prompt_includes_all_experience_fields() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        queen.write_experience(make_exp(
            "Redis 连接修复",
            "生产环境 Redis 连接池耗尽",
            "调整 max_connections 和超时参数",
            "连接恢复，延迟降低 50%",
            vec!["redis", "infra", "performance"],
            "backend-service",
            Outcome::Success,
        )).unwrap();

        let result = queen.query_for_prompt("redis 连接 生产", 5);
        assert!(result.is_some());
        let block = result.unwrap();

        // All fields should be present in the formatted block
        assert!(block.contains("Redis 连接修复"));
        assert!(block.contains("生产环境 Redis 连接池耗尽"));
        assert!(block.contains("调整 max_connections 和超时参数"));
        assert!(block.contains("连接恢复，延迟降低 50%"));
        assert!(block.contains("success"));
        assert!(block.contains("backend-service"));
        assert!(block.contains("redis, infra, performance"));
    }

    #[test]
    fn query_for_prompt_formats_outcome_correctly() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        // Test with Failure outcome
        queen.write_experience(make_exp(
            "失败部署",
            "CI 部署到生产环境",
            "执行滚动更新",
            "部署失败，回滚到上一版本",
            vec!["deploy"],
            "backend",
            Outcome::Failure,
        )).unwrap();

        let result = queen.query_for_prompt("部署 失败", 5);
        assert!(result.is_some());
        let block = result.unwrap();
        assert!(block.contains("failure"));
    }
}
