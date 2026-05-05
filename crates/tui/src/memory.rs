//! 记忆系统 —— 向下兼容层，委托给虫后（queen）。
//!
//! 原来的 `memory.md` 单文件系统保留作为 fallback，
//! 新代码走 `queen` 模块。

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use chrono::Utc;

/// Maximum size of the user memory file.
const MAX_MEMORY_SIZE: usize = 100 * 1024;

/// Read the user memory file at `path`, returning `None` when the file
/// doesn't exist or is empty after trimming.
#[must_use]
pub fn load(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    Some(content)
}

/// Wrap memory content in a `<user_memory>` block.
#[must_use]
pub fn as_system_block(content: &str, source: &Path) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let display = source.display();
    let payload = if content.len() > MAX_MEMORY_SIZE {
        let mut head = content[..MAX_MEMORY_SIZE].to_string();
        head.push_str("\n…(truncated)");
        head
    } else {
        trimmed.to_string()
    };

    Some(format!(
        "<user_memory source=\"{display}\">\n{payload}\n</user_memory>"
    ))
}

/// Compose the `<user_memory>` block for the system prompt.
#[must_use]
pub fn compose_block(enabled: bool, path: &Path) -> Option<String> {
    if !enabled {
        return None;
    }
    let content = load(path)?;
    as_system_block(&content, path)
}

/// Append entry to the memory file.
pub fn append_entry(path: &Path, entry: &str) -> io::Result<()> {
    let trimmed = entry.trim_start_matches('#').trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "memory entry is empty after stripping `#` prefix",
        ));
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let timestamp = Utc::now().format("%Y-%m-%d %H:%M UTC");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "- ({timestamp}) {trimmed}")?;
    Ok(())
}

/// 虫后写入快捷入口。
///
/// 通过虫后写入结构化经验，同时保持向下兼容。
#[allow(dead_code)]
pub fn write_queen_experience(
    queen: &mut crate::queen::Queen,
    title: &str,
    context: &str,
    action: &str,
    result: &str,
    tags: Vec<&str>,
    project: &str,
    outcome: crate::queen::Outcome,
    confidence: f64,
) -> io::Result<()> {
    let exp = crate::queen::Experience::new(
        title.to_string(),
        context.to_string(),
        action.to_string(),
        result.to_string(),
        tags.iter().map(|s| s.to_string()).collect(),
        project.to_string(),
        outcome,
        confidence,
    );
    queen.write_experience(exp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_none_for_missing_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("never-existed.md");
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_none_for_whitespace_only_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "   \n   \n").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_content_for_real_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "remember the milk").unwrap();
        assert_eq!(load(&path).as_deref(), Some("remember the milk"));
    }

    #[test]
    fn as_system_block_produces_xml_wrapper() {
        let block = as_system_block("note 1", Path::new("/tmp/m.md")).unwrap();
        assert!(block.contains("<user_memory source=\"/tmp/m.md\">"));
        assert!(block.contains("note 1"));
        assert!(block.ends_with("</user_memory>"));
    }

    #[test]
    fn as_system_block_returns_none_for_empty_content() {
        assert!(as_system_block("   ", Path::new("/tmp/m.md")).is_none());
    }

    #[test]
    fn append_entry_creates_file_and_writes_one_bullet() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        append_entry(&path, "# remember the milk").unwrap();

        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("remember the milk"), "{body}");
        assert!(
            body.starts_with("- ("),
            "should start with bullet + date: {body}"
        );
    }

    #[test]
    fn queen_write_helper_works() {
        let tmp = tempdir().unwrap();
        let mut queen = crate::queen::Queen::init(tmp.path()).unwrap();

        write_queen_experience(
            &mut queen,
            "测试标题",
            "上下文",
            "行动",
            "结果",
            vec!["test"],
            "project-x",
            crate::queen::Outcome::Success,
            0.9,
        )
        .unwrap();

        assert_eq!(queen.experience_count(), 1);
        assert_eq!(queen.experiences[0].project, "project-x");
    }
}
