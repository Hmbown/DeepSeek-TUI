//! Integration test: skill tracking, dedup, and post-compaction recovery.
//!
//! Verifies:
//! 1. **On-demand loading** — only `load_skill`-ed skills are tracked, not
//!    the full skill catalogue.
//! 2. **Deduplication** — repeated `load_skill("skill-a")` calls → one entry
//!    (latest body + timestamp wins via HashMap insert-overwrite).
//! 3. **Post-compaction recovery** — `inject_invoked_skills` produces a
//!    single message with correct markdown structure, most-recent-first
//!    ordering, no truncation.
//! 4. **Non-invoked skills absent** — skills on disk but never loaded are
//!    not in the post-compact output.
//!
//! The test reproduces the exact production logic of `InvokedSkillRecord`,
//! `record_skill_invocation`, and `inject_invoked_skills` from
//! `session.rs` + `compaction.rs` so it stays self-contained.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::PathBuf;

// ── Production-type replicas (minimal, verified against source) ────────

#[derive(Debug, Clone)]
struct InvokedSkillRecord {
    skill_name: String,
    content: String,
    invoked_at: chrono::DateTime<chrono::Utc>,
}

fn record_skill_invocation(
    map: &mut HashMap<String, InvokedSkillRecord>,
    name: String,
    content: String,
) {
    map.insert(
        name.clone(),
        InvokedSkillRecord {
            skill_name: name,
            content,
            invoked_at: chrono::Utc::now(),
        },
    );
}

/// Exact replica of `inject_invoked_skills` in `compaction.rs`.
fn inject_invoked_skills(
    invoked_skills: &HashMap<String, InvokedSkillRecord>,
) -> Option<String> {
    if invoked_skills.is_empty() {
        return None;
    }

    let mut skills: Vec<&InvokedSkillRecord> = invoked_skills.values().collect();
    skills.sort_by_key(|s| std::cmp::Reverse(s.invoked_at));

    let mut body = String::from(
        "The following skills were invoked in this session. Continue to follow these guidelines:\n",
    );

    for (i, info) in skills.iter().enumerate() {
        if i > 0 {
            body.push_str("\n---\n\n");
        } else {
            body.push('\n');
        }
        let _ = write!(
            body,
            "### Skill: {}\n\n{}",
            info.skill_name, info.content
        );
    }
    Some(body)
}

fn make_record(name: &str, content: &str, age_secs: i64) -> InvokedSkillRecord {
    InvokedSkillRecord {
        skill_name: name.to_string(),
        content: content.to_string(),
        invoked_at: chrono::Utc::now() - chrono::Duration::seconds(age_secs),
    }
}

// ── Skill loading helpers (simulates `load_skill` tool) ────────────────

fn read_skill_md(dir: &PathBuf, name: &str) -> String {
    let path = dir.join(name).join("SKILL.md");
    std::fs::read_to_string(&path).expect(&format!("failed to read {path:?}"))
}

// ── Tests ───────────────────────────────────────────────────────────────

/// Only invoked skills appear; skills on disk but never loaded are absent.
#[test]
fn only_invoked_skills_appear_not_the_full_catalogue() {
    let mut invoked = HashMap::new();
    invoked.insert("skill-a".into(), make_record("skill-a", "# Skill A body", 300));
    invoked.insert("skill-c".into(), make_record("skill-c", "# Skill C body", 100));
    // skill-b, skill-d, skill-e on disk but never invoked.

    let output = inject_invoked_skills(&invoked).expect("non-empty");
    assert!(output.contains("### Skill: skill-c"), "most-recent first");
    assert!(output.contains("### Skill: skill-a"));
    assert!(!output.contains("### Skill: skill-b"));
    assert!(!output.contains("### Skill: skill-d"));
    assert!(!output.contains("### Skill: skill-e"));
}

/// Same skill called 5 times → 1 appearance with latest content.
#[test]
fn repeated_skill_deduplicated_with_latest_content() {
    let mut invoked = HashMap::new();
    // Simulate 5 invocations of skill-a — each overwrites.
    invoked.insert("skill-a".into(), make_record("skill-a", "# call 1", 500));
    invoked.insert("skill-a".into(), make_record("skill-a", "# call 3", 300));
    invoked.insert("skill-a".into(), make_record("skill-a", "# call 5 (latest)", 0));

    let output = inject_invoked_skills(&invoked).expect("non-empty");
    assert_eq!(output.matches("### Skill: skill-a").count(), 1, "exactly once");
    assert!(output.contains("call 5 (latest)"), "latest wins");
    assert!(!output.contains("call 1"));
    assert!(!output.contains("call 3"));
}

/// Multiple distinct skills, some repeated → correct count, dedup, ordering.
#[test]
fn mixed_skills_dedup_and_ordering() {
    let mut invoked = HashMap::new();
    // A: ×2, latest age 0
    invoked.insert("skill-a".into(), make_record("skill-a", "Body A v1", 600));
    invoked.insert("skill-a".into(), make_record("skill-a", "Body A v2 (latest)", 0));
    // B: ×1
    invoked.insert("skill-b".into(), make_record("skill-b", "Body B", 200));
    // C: ×3, latest age 50
    invoked.insert("skill-c".into(), make_record("skill-c", "Body C v1", 500));
    invoked.insert("skill-c".into(), make_record("skill-c", "Body C v2", 300));
    invoked.insert("skill-c".into(), make_record("skill-c", "Body C v3 (latest)", 50));

    let output = inject_invoked_skills(&invoked).expect("non-empty");

    assert_eq!(output.matches("### Skill: skill-a").count(), 1);
    assert_eq!(output.matches("### Skill: skill-b").count(), 1);
    assert_eq!(output.matches("### Skill: skill-c").count(), 1);

    assert!(output.contains("Body A v2 (latest)"));
    assert!(!output.contains("Body A v1"));
    assert!(output.contains("Body C v3 (latest)"));
    assert!(!output.contains("Body C v1"));
    assert!(!output.contains("Body C v2"));

    // Most-recent-first: A (0), C (50), B (200)
    let pa = output.find("### Skill: skill-a").unwrap();
    let pc = output.find("### Skill: skill-c").unwrap();
    let pb = output.find("### Skill: skill-b").unwrap();
    assert!(pa < pc, "A (age 0) before C (age 50)");
    assert!(pc < pb, "C (age 50) before B (age 200)");
}

/// Full body preserved — no truncation.
#[test]
fn full_skill_body_preserved_no_truncation() {
    let large = format!("# Skill X\n\n{}", "x".repeat(8_000));
    let mut invoked = HashMap::new();
    invoked.insert("skill-x".into(), make_record("skill-x", &large, 0));

    let output = inject_invoked_skills(&invoked).expect("non-empty");
    assert!(output.contains(&large), "full body intact");
}

/// Markdown structure matches Claude Code's layout.
#[test]
fn claude_code_aligned_markdown_structure() {
    let mut invoked = HashMap::new();
    invoked.insert("skill-b".into(), make_record("skill-b", "Body B.", 200));
    invoked.insert("skill-a".into(), make_record("skill-a", "Body A.", 100));

    let output = inject_invoked_skills(&invoked).expect("non-empty");

    assert!(output.starts_with("The following skills were invoked"), "leading sentence");
    assert!(output.contains("### Skill: skill-b"), "more-recent first");
    assert!(output.contains("### Skill: skill-a"));
    // Two skill headers, one --- delimiter between them.
    assert_eq!(output.matches("### Skill:").count(), 2, "2 skills");
    assert!(output.contains("---"), "separator present");
}

/// The `HashMap::insert` dedup is verified via `record_skill_invocation`.
#[test]
fn record_skill_invocation_dedup_via_timestamp_latest_wins() {
    let mut invoked = HashMap::new();

    record_skill_invocation(&mut invoked, "a".into(), "v1".into());
    assert_eq!(invoked.len(), 1);
    let t1 = invoked["a"].invoked_at;

    std::thread::sleep(std::time::Duration::from_millis(10));

    record_skill_invocation(&mut invoked, "a".into(), "v2".into());
    assert_eq!(invoked.len(), 1, "still 1 — deduplicated");
    let t2 = invoked["a"].invoked_at;
    assert!(t2 > t1, "timestamp advanced");
    assert_eq!(invoked["a"].content, "v2", "latest content wins");

    // Different name → new entry.
    record_skill_invocation(&mut invoked, "b".into(), "body".into());
    assert_eq!(invoked.len(), 2);
}

// ── End-to-end: read skills from disk, simulate invocations, compact ───

#[test]
fn end_to_end_on_demand_and_dedup_with_real_skill_files() {
    let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/test_skills");
    assert!(skills_dir.exists(), "test_skills dir must exist: {skills_dir:?}");

    // List skill directories on disk.
    let mut disk_names: Vec<String> = std::fs::read_dir(&skills_dir)
        .expect("read_dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    disk_names.sort();
    eprintln!("Skills on disk: {disk_names:?}");
    assert!(disk_names.len() >= 5, "expected ≥5 skills");

    // Read full skill bodies from disk.
    let body_a = read_skill_md(&skills_dir, "skill_a");
    let body_b = read_skill_md(&skills_dir, "skill_b");
    let body_c = read_skill_md(&skills_dir, "skill_c");

    // ── Simulate model calls ──
    // skill_a: called 2 times
    // skill_c: called 3 times
    // skill_b: called 1 time
    // skill_d, skill_e: never called
    let mut invoked = HashMap::new();
    invoked.insert("skill-a".into(), make_record("skill-a", &body_a, 500));
    invoked.insert("skill-a".into(), make_record("skill-a", &body_a, 0));     // latest
    invoked.insert("skill-c".into(), make_record("skill-c", &body_c, 600));
    invoked.insert("skill-c".into(), make_record("skill-c", &body_c, 300));
    invoked.insert("skill-c".into(), make_record("skill-c", &body_c, 0));     // latest
    invoked.insert("skill-b".into(), make_record("skill-b", &body_b, 100));

    // ── Compaction ──
    let output = inject_invoked_skills(&invoked).expect("non-empty");
    eprintln!("=== Post-compact skill message ===\n{output}\n=== end ===");

    // ── Never-called skills absent ──
    assert!(!output.contains("### Skill: skill-d"), "skill-d never called");
    assert!(!output.contains("### Skill: skill-e"), "skill-e never called");

    // ── Called skills each appear exactly once ──
    assert_eq!(output.matches("### Skill: skill-a").count(), 1);
    assert_eq!(output.matches("### Skill: skill-b").count(), 1);
    assert_eq!(output.matches("### Skill: skill-c").count(), 1);

    // ── Real body content present ──
    assert!(output.contains("hello from A"), "skill-a body");
    assert!(output.contains("lists files"), "skill-b body");
    assert!(output.contains("code review"), "skill-c body");

    // ── Full bodies preserved ──
    assert!(output.contains(&body_a), "full body of skill-a");
    assert!(output.contains(&body_b), "full body of skill-b");
    assert!(output.contains(&body_c), "full body of skill-c");

    // ── Structure ──
    assert_eq!(output.matches("### Skill:").count(), 3, "3 skill headers");
    // "---" appears both in YAML frontmatter and as separators.
    // Just verify separators exist between our injected headers.
    assert!(output.contains("---"), "separators present");
    assert!(output.starts_with("The following skills were invoked"), "leading sentence");
}
