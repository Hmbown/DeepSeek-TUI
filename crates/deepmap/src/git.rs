// Git history helpers for DeepMap.
// Provides blame lookups, recently-changed files, and co-change analysis.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Get git blame info for a symbol in a file at a given line.
pub fn blame_symbol(project_root: &Path, file: &str, line: usize) -> Option<BlameInfo> {
    let output = Command::new("git")
        .args([
            "blame",
            "-L",
            &format!("{},{}", line, line),
            "--porcelain",
            file,
        ])
        .current_dir(project_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_blame_porcelain(&stdout)
}

/// Blame information for a line.
#[derive(Debug, Clone)]
pub struct BlameInfo {
    pub commit_hash: String,
    pub author: String,
    pub author_time: String,
    pub summary: String,
}

fn parse_blame_porcelain(output: &str) -> Option<BlameInfo> {
    let mut commit_hash = String::new();
    let mut author = String::new();
    let mut author_time = String::new();
    let mut summary = String::new();

    for line in output.lines() {
        if commit_hash.is_empty() {
            if let Some(hash) = line.split_whitespace().next() {
                commit_hash = hash.to_string();
            }
        }
        if line.starts_with("author ") {
            author = line.strip_prefix("author ").unwrap_or("").to_string();
        }
        if line.starts_with("author-time ") {
            author_time = line.strip_prefix("author-time ").unwrap_or("").to_string();
        }
        if line.starts_with("summary ") {
            summary = line.strip_prefix("summary ").unwrap_or("").to_string();
        }
    }

    if commit_hash.is_empty() {
        return None;
    }

    Some(BlameInfo {
        commit_hash,
        author,
        author_time,
        summary,
    })
}

/// Get files changed in the last N days.
pub fn recently_changed_files(project_root: &Path, days: u32) -> Vec<String> {
    let since = format!("{} days ago", days);
    let output = match Command::new("git")
        .args([
            "diff",
            "--name-only",
            &format!("HEAD@{{{}}}", since),
            "HEAD",
        ])
        .current_dir(project_root)
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        // Fall back to log-based approach.
        return recent_files_via_log(project_root, days);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn recent_files_via_log(project_root: &Path, days: u32) -> Vec<String> {
    let since = format!("{} days ago", days);
    let output = match Command::new("git")
        .args([
            "log",
            "--name-only",
            "--pretty=format:",
            &format!("--since={}", since),
        ])
        .current_dir(project_root)
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    files.sort();
    files.dedup();
    files
}

/// Compute co-change scores: which files tend to change together.
pub fn co_change_scores(
    project_root: &Path,
    limit: usize,
) -> HashMap<String, Vec<(String, usize)>> {
    let output = match Command::new("git")
        .args([
            "log",
            "--name-only",
            "--pretty=format:",
            &format!("-n {}", limit),
        ])
        .current_dir(project_root)
        .output()
    {
        Ok(o) => o,
        Err(_) => return HashMap::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut commit_files: Vec<Vec<String>> = Vec::new();
    let mut current_commit: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            if !current_commit.is_empty() {
                commit_files.push(std::mem::take(&mut current_commit));
            }
        } else {
            current_commit.push(trimmed);
        }
    }
    if !current_commit.is_empty() {
        commit_files.push(current_commit);
    }

    // Count co-occurrences.
    let mut pair_counts: HashMap<(String, String), usize> = HashMap::new();
    for commit in &commit_files {
        for i in 0..commit.len() {
            for j in (i + 1)..commit.len() {
                let pair = if commit[i] < commit[j] {
                    (commit[i].clone(), commit[j].clone())
                } else {
                    (commit[j].clone(), commit[i].clone())
                };
                *pair_counts.entry(pair).or_insert(0) += 1;
            }
        }
    }

    // Build per-file co-change map.
    let mut co_change: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    for ((a, b), count) in pair_counts {
        co_change
            .entry(a.clone())
            .or_default()
            .push((b.clone(), count));
        co_change.entry(b).or_default().push((a, count));
    }

    // Sort by count descending.
    for v in co_change.values_mut() {
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.truncate(20);
    }

    co_change
}
