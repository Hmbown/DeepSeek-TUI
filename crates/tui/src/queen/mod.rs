//! 虫后（Chóng Hòu）—— 单兵记忆系统。
//!
//! 每个 DeepSeek TUI 实例是一个"虫后"，独立作战，记忆本地存储。
//! 同一设备上的多个实例共享 `~/.deepseek/queen/` 目录，
//! 通过文件系统自然同步，无需握手协议。
//!
//! # 设计原则
//!
//! - **零配置**：安装即用，默认开启
//! - **结构化**：每个经验是一个 JSON 文件，UUID 命名
//! - **语义检索**：TF-IDF + 余弦相似度，纯 Rust 实现
//! - **自动总结**：任务完成后自动提取关键经验
//! - **多实例同步**：共享目录，文件级自然同步

pub mod index;
pub mod search;
pub mod summary;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 虫后目录名。
pub const QUEEN_DIR_NAME: &str = "queen";

/// 经验子目录名。
pub const EXPERIENCES_DIR: &str = "experiences";

/// 索引子目录名。
pub const INDEX_DIR: &str = "index";

/// 统计文件名。
pub const STATS_FILE: &str = "stats.json";

/// 结果的分类。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Partial,
    Failure,
}

impl Outcome {
}

/// 虫后经验 —— 一次任务解决的结构化记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    /// UUID 标识。
    pub id: String,
    /// 简短标题。
    pub title: String,
    /// 场景/问题描述。
    pub context: String,
    /// 采取的行动。
    pub action: String,
    /// 结果。
    pub result: String,
    /// 标签。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 所属项目（从 workspace 自动获取）。
    #[serde(default)]
    pub project: String,
    /// 结果分类。
    pub outcome: Outcome,
    /// 置信度 0.0 ~ 1.0。
    pub confidence: f64,
    /// 复用次数。
    #[serde(default)]
    pub reuse_count: u64,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 最后使用时间。
    pub last_used_at: DateTime<Utc>,
}

impl Experience {
    /// 创建新经验。
    #[must_use]
    pub fn new(
        title: String,
        context: String,
        action: String,
        result: String,
        tags: Vec<String>,
        project: String,
        outcome: Outcome,
        confidence: f64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            context,
            action,
            result,
            tags,
            project,
            outcome,
            confidence,
            reuse_count: 0,
            created_at: now,
            last_used_at: now,
        }
    }

    /// 搜索用的文本内容（拼接多个字段）。
    #[must_use]
    pub fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.title,
            self.context,
            self.action,
            self.result,
            self.tags.join(" ")
        )
    }
}

/// 虫后状态统计。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueenStats {
    /// 总经验数。
    pub total_experiences: u64,
    /// 提炼技能数。
    pub total_skills: u64,
    /// 总检索次数。
    pub total_queries: u64,
    /// 最后更新时间。
    pub last_updated: Option<String>,
}

/// 虫后实例。
#[derive(Debug)]
pub struct Queen {
    /// 虫后根目录。
    pub dir: PathBuf,
    /// 语义索引。
    pub semantic_index: index::SemanticIndex,
    /// 所有经验的缓存（id → Experience）。
    pub experiences: Vec<Experience>,
    /// 状态统计。
    pub stats: QueenStats,
}

impl Queen {
    /// 初始化虫后，创建目录并加载已有经验。
    ///
    /// # Errors
    ///
    /// 目录创建失败时返回错误。
    pub fn init(base_dir: &Path) -> io::Result<Self> {
        let dir = base_dir.join(QUEEN_DIR_NAME);
        Self::ensure_dirs(&dir)?;

        let stats = Self::load_stats(&dir);
        let experiences = Self::load_all_experiences(&dir);
        let semantic_index = index::SemanticIndex::build(&experiences);

        Ok(Self {
            dir,
            semantic_index,
            experiences,
            stats,
        })
    }

    /// 确保目录结构存在。
    fn ensure_dirs(dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir.join(EXPERIENCES_DIR))?;
        fs::create_dir_all(dir.join(INDEX_DIR))?;
        Ok(())
    }

    /// 加载所有经验文件。
    fn load_all_experiences(dir: &Path) -> Vec<Experience> {
        let exp_dir = dir.join(EXPERIENCES_DIR);
        let Ok(entries) = fs::read_dir(&exp_dir) else {
            return Vec::new();
        };

        let mut experiences = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(exp) = serde_json::from_str::<Experience>(&content) {
                        experiences.push(exp);
                    }
                }
            }
        }
        experiences
    }

    /// 加载统计文件。
    fn load_stats(dir: &Path) -> QueenStats {
        let path = dir.join(STATS_FILE);
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 保存统计文件。
    fn save_stats(&self) -> io::Result<()> {
        let path = self.dir.join(STATS_FILE);
        let json = serde_json::to_string_pretty(&self.stats)?;
        atomic_write(&path, &json)
    }

    /// 写入一条新经验。
    ///
    /// # Errors
    ///
    /// 文件写入失败时返回错误。
    pub fn write_experience(&mut self, exp: Experience) -> io::Result<()> {
        let path = self.dir.join(EXPERIENCES_DIR).join(format!("{}.json", exp.id));
        let json = serde_json::to_string_pretty(&exp)?;
        atomic_write(&path, &json)?;

        // 更新内存缓存
        self.experiences.push(exp.clone());
        self.semantic_index.add_document(&exp);

        // 更新统计
        self.stats.total_experiences = self.experiences.len() as u64;
        self.stats.last_updated = Some(Utc::now().to_rfc3339());
        self.save_stats().ok();

        Ok(())
    }

    /// 搜索相关经验，返回按分数降序排列的结果。
    #[must_use]
    #[allow(dead_code)]
    pub fn search(&mut self, query: &str, top_k: usize) -> Vec<(f64, &Experience)> {
        let results = self.semantic_index.search(query, top_k);
        results
            .into_iter()
            .filter_map(|(score, id)| {
                self.experiences
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| (score, e))
            })
            .collect()
    }

    /// 虫后目录是否存在且有经验。
    #[must_use]
    #[allow(dead_code)]
    pub fn has_experiences(&self) -> bool {
        !self.experiences.is_empty()
    }

    /// 经验数量。
    #[must_use]
    #[allow(dead_code)]
    pub fn experience_count(&self) -> usize {
        self.experiences.len()
    }
}

/// 原子写入：写入临时文件 → fsync → rename。
///
/// # Errors
///
/// 文件操作失败时返回错误。
pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp_path)?;
        use std::io::Write;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_experience(title: &str, context: &str, project: &str) -> Experience {
        Experience::new(
            title.to_string(),
            context.to_string(),
            "执行了修复操作".to_string(),
            "修复成功，测试通过".to_string(),
            vec!["bugfix".to_string(), "rust".to_string()],
            project.to_string(),
            Outcome::Success,
            0.95,
        )
    }

    #[test]
    fn queen_init_creates_directories() {
        let tmp = tempdir().unwrap();
        let queen = Queen::init(tmp.path()).unwrap();
        assert!(queen.dir.join(EXPERIENCES_DIR).exists());
        assert!(queen.dir.join(INDEX_DIR).exists());
        assert!(queen.experiences.is_empty());
    }

    #[test]
    fn queen_write_and_load_experience() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        let exp = create_test_experience("测试经验", "测试上下文", "test-project");
        let exp_id = exp.id.clone();
        queen.write_experience(exp).unwrap();

        // 重新初始化，检查持久化
        let queen2 = Queen::init(tmp.path()).unwrap();
        assert_eq!(queen2.experiences.len(), 1);
        assert_eq!(queen2.experiences[0].id, exp_id);
        assert_eq!(queen2.experiences[0].project, "test-project");
    }

    #[test]
    fn queen_search_finds_relevant() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        queen.write_experience(create_test_experience(
            "修复 Buffer 溢出",
            "ChatWidget 渲染时 tool 卡片内容溢出到侧边栏",
            "deepseek-tui",
        ))
        .unwrap();

        queen.write_experience(create_test_experience(
            "优化 Python 异步性能",
            "asyncio.gather 并行处理多个协程",
            "data-pipeline",
        ))
        .unwrap();

        let results = queen.search("buffer 溢出 渲染", 5);
        assert!(!results.is_empty());
        assert!(results[0].1.title.contains("Buffer"));
    }

    #[test]
    fn atomic_write_is_atomic() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("test.json");

        atomic_write(&path, "{\"key\": \"value\"}").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "{\"key\": \"value\"}");
        // tmp 文件应该被清理
        assert!(!path.with_extension("tmp").exists());
    }

    // ── Outcome 序列化 ────────────────────────────────────

    #[test]
    fn outcome_serialization_roundtrip() {
        let cases = [
            (Outcome::Success, "\"success\""),
            (Outcome::Partial, "\"partial\""),
            (Outcome::Failure, "\"failure\""),
        ];
        for (outcome, expected_json) in &cases {
            let json = serde_json::to_string(outcome).unwrap();
            assert_eq!(json, *expected_json);
            let deserialized: Outcome = serde_json::from_str(&json).unwrap();
            assert_eq!(*outcome, deserialized);
        }
    }

    #[test]
    fn outcome_invalid_variant_returns_error() {
        let result: Result<Outcome, _> = serde_json::from_str("\"unknown\"");
        assert!(result.is_err());
    }

    // ── Experience ─────────────────────────────────────────

    #[test]
    fn experience_new_creates_unique_ids() {
        let exp1 = create_test_experience("任务1", "上下文1", "proj1");
        let exp2 = create_test_experience("任务2", "上下文2", "proj2");
        assert_ne!(exp1.id, exp2.id, "each experience must have a unique UUID");
    }

    #[test]
    fn experience_new_sets_reuse_count_to_zero() {
        let exp = create_test_experience("标题", "上下文", "proj");
        assert_eq!(exp.reuse_count, 0);
    }

    #[test]
    fn experience_new_created_at_is_recent() {
        let before = chrono::Utc::now();
        let exp = create_test_experience("标题", "上下文", "proj");
        let after = chrono::Utc::now();
        assert!(exp.created_at >= before && exp.created_at <= after);
    }

    #[test]
    fn experience_new_last_used_equals_created() {
        let exp = create_test_experience("标题", "上下文", "proj");
        assert_eq!(exp.created_at, exp.last_used_at);
    }

    #[test]
    fn experience_confidence_defaults_to_provided_value() {
        let exp = Experience::new(
            "标题".to_string(),
            "上下文".to_string(),
            "行动".to_string(),
            "结果".to_string(),
            vec![],
            "proj".to_string(),
            Outcome::Partial,
            0.42,
        );
        assert_eq!(exp.confidence, 0.42);
        assert_eq!(exp.outcome, Outcome::Partial);
        assert!(exp.tags.is_empty());
    }

    #[test]
    fn search_text_includes_all_fields() {
        let exp = Experience::new(
            "修复Bug".to_string(),
            "Widget 渲染出错".to_string(),
            "添加边界检查".to_string(),
            "问题解决".to_string(),
            vec!["bugfix".to_string(), "rendering".to_string()],
            "deepseek-tui".to_string(),
            Outcome::Success,
            0.9,
        );
        let text = exp.search_text();
        assert!(text.contains("修复Bug"));
        assert!(text.contains("Widget 渲染出错"));
        assert!(text.contains("添加边界检查"));
        assert!(text.contains("问题解决"));
        assert!(text.contains("bugfix"));
        assert!(text.contains("rendering"));
    }

    // ── Queen ──────────────────────────────────────────────

    #[test]
    fn queen_empty_search_returns_empty() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();
        let results = queen.search("anything", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn queen_has_experiences_false_when_empty() {
        let tmp = tempdir().unwrap();
        let queen = Queen::init(tmp.path()).unwrap();
        assert!(!queen.has_experiences());
    }

    #[test]
    fn queen_has_experiences_true_after_write() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();
        queen.write_experience(create_test_experience("标题", "上下文", "proj")).unwrap();
        assert!(queen.has_experiences());
    }

    #[test]
    fn queen_experience_count_matches_writes() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();
        assert_eq!(queen.experience_count(), 0);

        queen.write_experience(create_test_experience("A", "ctx", "p")).unwrap();
        assert_eq!(queen.experience_count(), 1);

        queen.write_experience(create_test_experience("B", "ctx", "p")).unwrap();
        assert_eq!(queen.experience_count(), 2);
    }

    #[test]
    fn queen_load_stats_defaults_when_file_missing() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(QUEEN_DIR_NAME);
        fs::create_dir_all(dir.join(EXPERIENCES_DIR)).unwrap();
        fs::create_dir_all(dir.join(INDEX_DIR)).unwrap();

        let stats = Queen::load_stats(&dir);
        assert_eq!(stats.total_experiences, 0);
        assert_eq!(stats.total_skills, 0);
        assert_eq!(stats.total_queries, 0);
        assert!(stats.last_updated.is_none());
    }

    #[test]
    fn queen_skips_corrupt_json_files() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();

        // Write a valid experience
        queen.write_experience(create_test_experience("有效经验", "上下文", "proj")).unwrap();
        assert_eq!(queen.experience_count(), 1);

        // Write a corrupt JSON file directly
        let corrupt_path = queen.dir.join(EXPERIENCES_DIR).join("corrupt.json");
        fs::write(&corrupt_path, "not valid json at all").unwrap();

        // Write a non-JSON file
        let txt_path = queen.dir.join(EXPERIENCES_DIR).join("readme.txt");
        fs::write(&txt_path, "this is a text file").unwrap();

        // Re-init — should skip corrupt files and still load valid ones
        let queen2 = Queen::init(tmp.path()).unwrap();
        assert_eq!(queen2.experience_count(), 1);
        assert_eq!(queen2.experiences[0].title, "有效经验");
    }

    // ── Stats ──────────────────────────────────────────────

    #[test]
    fn queen_stats_increment_on_write() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();
        assert_eq!(queen.stats.total_experiences, 0);

        queen.write_experience(create_test_experience("A", "ctx", "p")).unwrap();
        assert_eq!(queen.stats.total_experiences, 1);

        queen.write_experience(create_test_experience("B", "ctx", "p")).unwrap();
        assert_eq!(queen.stats.total_experiences, 2);
    }

    #[test]
    fn queen_stats_last_updated_is_set_after_write() {
        let tmp = tempdir().unwrap();
        let mut queen = Queen::init(tmp.path()).unwrap();
        assert!(queen.stats.last_updated.is_none());

        queen.write_experience(create_test_experience("A", "ctx", "p")).unwrap();
        assert!(queen.stats.last_updated.is_some());
    }

    #[test]
    fn queen_stats_persist_across_reinit() {
        let tmp = tempdir().unwrap();
        {
            let mut queen = Queen::init(tmp.path()).unwrap();
            queen.write_experience(create_test_experience("A", "ctx", "p")).unwrap();
            queen.write_experience(create_test_experience("B", "ctx", "p")).unwrap();
        }
        // Re-init — stats should be loaded from disk
        let queen2 = Queen::init(tmp.path()).unwrap();
        assert_eq!(queen2.stats.total_experiences, 2);
        assert!(queen2.stats.last_updated.is_some());
    }
}
