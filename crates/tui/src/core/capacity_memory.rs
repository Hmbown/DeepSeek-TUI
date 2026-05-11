//! Persistent memory snapshots for capacity controller interventions.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Canonical compact state persisted by interventions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CanonicalState {
    pub goal: String,
    pub constraints: Vec<String>,
    pub confirmed_facts: Vec<String>,
    pub open_loops: Vec<String>,
    pub pending_actions: Vec<String>,
    pub critical_refs: Vec<String>,
}

/// Replay verification metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayInfo {
    pub tool_id: String,
    pub tool_name: String,
    pub pass: bool,
    pub diff_summary: String,
}

/// JSONL record written for each intervention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityMemoryRecord {
    pub id: String,
    pub ts: String,
    pub turn_index: u64,
    pub action_trigger: String,
    pub h_hat: f64,
    pub c_hat: f64,
    pub slack: f64,
    pub risk_band: String,
    pub canonical_state: CanonicalState,
    pub source_message_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay_info: Option<ReplayInfo>,
    /// Workspace path for cross-session isolation. Empty string = global/legacy.
    #[serde(default)]
    pub workspace: String,
}

fn capacity_memory_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = std::env::var("DEEPSEEK_CAPACITY_MEMORY_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(shellexpand::tilde(trimmed).as_ref())];
        }
    }

    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".deepseek").join("memory"));
    }

    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".deepseek")
        .join("memory");
    dirs.push(cwd);

    dirs.dedup();
    dirs
}

pub fn append_capacity_record(session_id: &str, record: &CapacityMemoryRecord) -> Result<PathBuf> {
    let candidates = candidate_session_memory_paths(session_id);
    append_capacity_record_to_candidates(&candidates, record)
}

pub fn append_capacity_record_to_path(path: &Path, record: &CapacityMemoryRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create memory directory {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open memory log {}", path.display()))?;
    let line =
        serde_json::to_string(record).context("Failed to serialize capacity memory record")?;
    writeln!(file, "{line}")
        .with_context(|| format!("Failed to write memory record {}", path.display()))?;
    Ok(())
}

pub fn load_last_k_capacity_records(
    session_id: &str,
    k: usize,
) -> Result<Vec<CapacityMemoryRecord>> {
    let candidates = candidate_session_memory_paths(session_id);
    load_last_k_capacity_records_from_candidates(&candidates, k)
}

pub(super) fn load_last_k_capacity_records_from_path(
    path: &Path,
    k: usize,
) -> Result<Vec<CapacityMemoryRecord>> {
    if k == 0 || !path.exists() {
        return Ok(Vec::new());
    }

    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("Failed to open memory log {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for line in reader.lines() {
        let line = line.with_context(|| format!("Failed reading {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<CapacityMemoryRecord>(&line) {
            records.push(record);
        }
    }

    if records.len() > k {
        Ok(records.split_off(records.len() - k))
    } else {
        Ok(records)
    }
}

fn candidate_session_memory_paths(session_id: &str) -> Vec<PathBuf> {
    capacity_memory_dirs()
        .into_iter()
        .map(|dir| dir.join(format!("{session_id}.jsonl")))
        .collect()
}

fn append_capacity_record_to_candidates(
    paths: &[PathBuf],
    record: &CapacityMemoryRecord,
) -> Result<PathBuf> {
    let mut last_err: Option<anyhow::Error> = None;
    for path in paths {
        match append_capacity_record_to_path(path, record) {
            Ok(()) => return Ok(path.clone()),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("No capacity memory path candidates available")))
}

fn load_last_k_capacity_records_from_candidates(
    paths: &[PathBuf],
    k: usize,
) -> Result<Vec<CapacityMemoryRecord>> {
    if k == 0 {
        return Ok(Vec::new());
    }

    let mut newest: Option<(SystemTime, Vec<CapacityMemoryRecord>)> = None;
    let mut last_err: Option<anyhow::Error> = None;

    for path in paths {
        if !path.exists() {
            continue;
        }

        match load_last_k_capacity_records_from_path(path, k) {
            Ok(records) => {
                if records.is_empty() {
                    continue;
                }
                let modified = fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                let should_replace = newest
                    .as_ref()
                    .map(|(current, _)| modified >= *current)
                    .unwrap_or(true);
                if should_replace {
                    newest = Some((modified, records));
                }
            }
            Err(err) => last_err = Some(err),
        }
    }

    if let Some((_, records)) = newest {
        return Ok(records);
    }
    if let Some(err) = last_err {
        return Err(err);
    }
    Ok(Vec::new())
}

#[must_use]
pub fn new_record_id() -> String {
    format!("cap_{}", &uuid::Uuid::new_v4().to_string()[..8])
}

#[must_use]
pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

/// Scan all memory directories for the newest record across ALL sessions.
/// Returns (session_id, record) if any records exist.
///
/// If `workspace` is non-empty, only records whose `workspace` field matches
/// (or are empty/legacy records with no workspace set) are considered.
pub fn find_latest_cross_session(workspace: &str) -> Option<(String, CapacityMemoryRecord)> {
    let dirs = capacity_memory_dirs();
    let mut newest: Option<(SystemTime, String, CapacityMemoryRecord)> = None;

    for dir in &dirs {
        if !dir.exists() {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(session_id) = path.file_stem().and_then(|s| s.to_str()).map(String::from)
            else {
                continue;
            };
            let Ok(records) = load_last_k_capacity_records_from_path(&path, 1) else {
                continue;
            };
            let Some(record) = records.into_iter().last() else {
                continue;
            };
            // Workspace isolation: skip records whose workspace is set but
            // doesn't match the current workspace. Empty/legacy records
            // (workspace = "") are always included for backward compatibility.
            if !workspace.is_empty()
                && !record.workspace.is_empty()
                && record.workspace != workspace
            {
                continue;
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if newest
                .as_ref()
                .map(|(m, _, _)| modified >= *m)
                .unwrap_or(true)
            {
                newest = Some((modified, session_id, record));
            }
        }
    }

    newest.map(|(_, sid, rec)| (sid, rec))
}

/// Result of a migration pass over `memory/*.jsonl` data.
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    pub files_scanned: usize,
    pub records_migrated: usize,
    pub records_skipped_empty: usize,
    pub records_skipped_has_workspace: usize,
    pub files_backed_up: usize,
    pub errors: Vec<String>,
}

/// Migrate legacy `memory/*.jsonl` records by backfilling the `workspace`
/// field from the corresponding session JSON. Records that already have a
/// non-empty `workspace` are left untouched. Records with empty
/// `canonical_state` (all fields default) are skipped.
///
/// For each `.jsonl` file that contains at least one migrated record, the
/// original file is backed up to `<filename>.pre-migration.bak` before
/// the updated records are written.
pub fn migrate_legacy_memory_to_workspace(sessions_dir: &Path) -> MigrationReport {
    let mut report = MigrationReport::default();
    let dirs = capacity_memory_dirs();

    let marker = dirs
        .first()
        .map(|d| d.join(".migration-v1-complete"))
        .unwrap_or_else(|| PathBuf::from(".migration-v1-complete"));
    if marker.exists() {
        return report;
    }

    for dir in &dirs {
        if !dir.exists() {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            report.files_scanned += 1;

            let Some(session_id) = path.file_stem().and_then(|s| s.to_str()).map(String::from)
            else {
                continue;
            };

            let Ok(records) = load_all_records_from_path(&path) else {
                report
                    .errors
                    .push(format!("Failed to read {}", path.display()));
                continue;
            };

            let workspace = resolve_session_workspace(sessions_dir, &session_id);
            let mut any_migrated = false;
            let mut updated = Vec::with_capacity(records.len());

            for mut record in records {
                if !record.workspace.is_empty() {
                    report.records_skipped_has_workspace += 1;
                    updated.push(record);
                    continue;
                }
                if is_empty_canonical_state(&record.canonical_state) {
                    report.records_skipped_empty += 1;
                    updated.push(record);
                    continue;
                }

                if let Some(ref ws) = workspace {
                    record.workspace = ws.clone();
                    any_migrated = true;
                    report.records_migrated += 1;
                } else {
                    report.records_skipped_empty += 1;
                }
                updated.push(record);
            }

            if any_migrated {
                let bak_path = path.with_extension("jsonl.pre-migration.bak");
                if let Err(e) = fs::rename(&path, &bak_path)
                    && let Err(copy_err) = fs::copy(&path, &bak_path)
                {
                    report.errors.push(format!(
                        "Failed to back up {}: rename={} copy={}",
                        path.display(),
                        e,
                        copy_err
                    ));
                    continue;
                }
                report.files_backed_up += 1;

                if let Err(e) = rewrite_records(&path, &updated) {
                    report
                        .errors
                        .push(format!("Failed to rewrite {}: {e}", path.display()));
                }
            }
        }
    }

    if report.errors.is_empty() {
        if let Some(parent) = marker.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&marker, format!("v1 migrated at {}\n", now_rfc3339()));
    }

    report
}

fn load_all_records_from_path(path: &Path) -> Result<Vec<CapacityMemoryRecord>> {
    let file = OpenOptions::new().read(true).open(path)?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<CapacityMemoryRecord>(&line) {
            records.push(record);
        }
    }
    Ok(records)
}

fn rewrite_records(path: &Path, records: &[CapacityMemoryRecord]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    for record in records {
        let line = serde_json::to_string(record)?;
        writeln!(file, "{line}")?;
    }
    Ok(())
}

fn resolve_session_workspace(sessions_dir: &Path, session_id: &str) -> Option<String> {
    let session_path = sessions_dir.join(format!("{session_id}.json"));
    if !session_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&session_path).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&content).ok()?;
    meta.get("metadata")
        .and_then(|m| m.get("workspace"))
        .and_then(|w| w.as_str())
        .map(|s| s.to_string())
}

fn is_empty_canonical_state(state: &CanonicalState) -> bool {
    state.goal.is_empty()
        && state.constraints.is_empty()
        && state.confirmed_facts.is_empty()
        && state.open_loops.is_empty()
        && state.pending_actions.is_empty()
        && state.critical_refs.is_empty()
}

/// Restore a `.jsonl` file from its `.pre-migration.bak` backup.
/// Returns `true` if a backup was found and restored.
#[allow(dead_code)]
pub fn rollback_migration_for_file(jsonl_path: &Path) -> bool {
    let bak_path = PathBuf::from(jsonl_path.as_os_str()).with_extension("jsonl.pre-migration.bak");
    if !bak_path.exists() {
        return false;
    }
    if fs::rename(&bak_path, jsonl_path).is_ok()
        || (fs::copy(&bak_path, jsonl_path).is_ok() && fs::remove_file(&bak_path).is_ok())
    {
        return true;
    }
    false
}

/// Roll back all migrations by restoring `.pre-migration.bak` files.
/// Returns the number of files restored.
#[allow(dead_code)]
pub fn rollback_all_migrations() -> usize {
    let dirs = capacity_memory_dirs();
    let mut restored = 0;
    for dir in &dirs {
        if !dir.exists() {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".pre-migration.bak") {
                continue;
            }
            let original =
                path.with_file_name(name.strip_suffix(".pre-migration.bak").unwrap_or(name));
            if fs::rename(&path, &original).is_ok()
                || (fs::copy(&path, &original).is_ok() && fs::remove_file(&path).is_ok())
            {
                restored += 1;
            }
        }
    }
    restored
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use tempfile::tempdir;

    struct ScopedEnv {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl ScopedEnv {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous) = self.previous.take() {
                    std::env::set_var(self.key, previous);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    fn clear_migration_marker() {
        let dirs = capacity_memory_dirs();
        for dir in &dirs {
            let marker = dir.join(".migration-v1-complete");
            let _ = fs::remove_file(marker);
        }
    }

    #[test]
    fn memory_jsonl_round_trip() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("session.jsonl");

        let record = CapacityMemoryRecord {
            id: "cap_1".to_string(),
            ts: now_rfc3339(),
            turn_index: 2,
            action_trigger: "targeted_context_refresh".to_string(),
            h_hat: 1.2,
            c_hat: 3.8,
            slack: 2.6,
            risk_band: "medium".to_string(),
            canonical_state: CanonicalState {
                goal: "Ship feature".to_string(),
                ..CanonicalState::default()
            },
            source_message_ids: vec!["m1".to_string()],
            replay_info: None,
            workspace: String::new(),
        };

        append_capacity_record_to_path(&path, &record).expect("append");
        let records = load_last_k_capacity_records_from_path(&path, 1).expect("load");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].canonical_state.goal, "Ship feature");
    }

    #[test]
    fn append_falls_back_to_next_candidate_path() {
        let tmp = tempdir().expect("tempdir");
        let blocked_root = tmp.path().join("blocked");
        fs::write(&blocked_root, "file").expect("create blocking file");
        let blocked_path = blocked_root.join("session.jsonl");
        let fallback_path = tmp.path().join("fallback").join("session.jsonl");

        let record = CapacityMemoryRecord {
            id: "cap_fallback".to_string(),
            ts: now_rfc3339(),
            turn_index: 1,
            action_trigger: "targeted_context_refresh".to_string(),
            h_hat: 1.0,
            c_hat: 3.8,
            slack: 2.8,
            risk_band: "medium".to_string(),
            canonical_state: CanonicalState::default(),
            source_message_ids: vec!["m1".to_string()],
            replay_info: None,
            workspace: String::new(),
        };

        let chosen = append_capacity_record_to_candidates(
            &[blocked_path.clone(), fallback_path.clone()],
            &record,
        )
        .expect("append with fallback");
        assert_eq!(chosen, fallback_path);
        assert!(chosen.exists());
    }

    #[test]
    fn load_prefers_newest_candidate_records() {
        let tmp = tempdir().expect("tempdir");
        let older = tmp.path().join("older.jsonl");
        let newer = tmp.path().join("newer.jsonl");

        let old_record = CapacityMemoryRecord {
            id: "cap_old".to_string(),
            ts: now_rfc3339(),
            turn_index: 1,
            action_trigger: "targeted_context_refresh".to_string(),
            h_hat: 1.0,
            c_hat: 3.8,
            slack: 2.8,
            risk_band: "medium".to_string(),
            canonical_state: CanonicalState {
                goal: "old".to_string(),
                ..CanonicalState::default()
            },
            source_message_ids: vec!["m1".to_string()],
            replay_info: None,
            workspace: String::new(),
        };
        let new_record = CapacityMemoryRecord {
            id: "cap_new".to_string(),
            ts: now_rfc3339(),
            turn_index: 2,
            action_trigger: "verify_and_replan".to_string(),
            h_hat: 1.4,
            c_hat: 3.8,
            slack: 2.4,
            risk_band: "high".to_string(),
            canonical_state: CanonicalState {
                goal: "new".to_string(),
                ..CanonicalState::default()
            },
            source_message_ids: vec!["m2".to_string()],
            replay_info: None,
            workspace: String::new(),
        };

        append_capacity_record_to_path(&older, &old_record).expect("write older");
        std::thread::sleep(std::time::Duration::from_millis(10));
        append_capacity_record_to_path(&newer, &new_record).expect("write newer");

        let records = load_last_k_capacity_records_from_candidates(&[older, newer], 1)
            .expect("load newest records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].canonical_state.goal, "new");
    }

    #[test]
    fn migration_backfills_workspace_from_session_json() {
        let tmp = tempdir().expect("tempdir");
        let mem_dir = tmp.path().join("memory");
        let sessions_dir = tmp.path().join("sessions");
        fs::create_dir_all(&mem_dir).expect("dirs");
        fs::create_dir_all(&sessions_dir).expect("dirs");

        let session_id = "abc-123";
        let workspace = "/home/user/project";

        let session_json = sessions_dir.join(format!("{session_id}.json"));
        fs::write(
            &session_json,
            format!(r#"{{"metadata":{{"id":"{session_id}","workspace":"{workspace}"}}}}"#),
        )
        .expect("session json");

        let jsonl_path = mem_dir.join(format!("{session_id}.jsonl"));
        let record = CapacityMemoryRecord {
            id: "cap_m1".to_string(),
            ts: now_rfc3339(),
            turn_index: 1,
            action_trigger: "engine_shutdown".to_string(),
            h_hat: 0.0,
            c_hat: 0.0,
            slack: 0.0,
            risk_band: "low".to_string(),
            canonical_state: CanonicalState {
                goal: "Ship feature".to_string(),
                ..CanonicalState::default()
            },
            source_message_ids: vec![],
            replay_info: None,
            workspace: String::new(),
        };
        append_capacity_record_to_path(&jsonl_path, &record).expect("write");

        let _env = ScopedEnv::set("DEEPSEEK_CAPACITY_MEMORY_DIR", mem_dir.to_str().unwrap());
        clear_migration_marker();
        let report = migrate_legacy_memory_to_workspace(&sessions_dir);

        assert_eq!(report.records_migrated, 1);
        assert_eq!(report.files_backed_up, 1);
        assert!(
            jsonl_path
                .with_extension("jsonl.pre-migration.bak")
                .exists()
        );

        let records = load_last_k_capacity_records_from_path(&jsonl_path, 1).expect("load");
        assert_eq!(records[0].workspace, workspace);
    }

    #[test]
    fn migration_skips_empty_canonical_state() {
        let tmp = tempdir().expect("tempdir");
        let mem_dir = tmp.path().join("memory");
        let sessions_dir = tmp.path().join("sessions");
        fs::create_dir_all(&mem_dir).expect("dirs");
        fs::create_dir_all(&sessions_dir).expect("dirs");

        let session_id = "empty-456";
        let session_json = sessions_dir.join(format!("{session_id}.json"));
        fs::write(
            &session_json,
            format!(r#"{{"metadata":{{"id":"{session_id}","workspace":"/ws"}}}}"#),
        )
        .expect("session json");

        let jsonl_path = mem_dir.join(format!("{session_id}.jsonl"));
        let record = CapacityMemoryRecord {
            id: "cap_e1".to_string(),
            ts: now_rfc3339(),
            turn_index: 1,
            action_trigger: "engine_shutdown".to_string(),
            h_hat: 0.0,
            c_hat: 0.0,
            slack: 0.0,
            risk_band: "low".to_string(),
            canonical_state: CanonicalState::default(),
            source_message_ids: vec![],
            replay_info: None,
            workspace: String::new(),
        };
        append_capacity_record_to_path(&jsonl_path, &record).expect("write");

        let _env = ScopedEnv::set("DEEPSEEK_CAPACITY_MEMORY_DIR", mem_dir.to_str().unwrap());
        clear_migration_marker();
        let report = migrate_legacy_memory_to_workspace(&sessions_dir);

        assert_eq!(report.records_migrated, 0);
        assert_eq!(report.records_skipped_empty, 1);
        assert_eq!(report.files_backed_up, 0);
        assert!(
            !jsonl_path
                .with_extension("jsonl.pre-migration.bak")
                .exists()
        );
    }

    #[test]
    fn rollback_restores_from_backup() {
        let tmp = tempdir().expect("tempdir");
        let jsonl_path = tmp.path().join("test.jsonl");
        let bak_path = tmp.path().join("test.jsonl.pre-migration.bak");

        let original_content = "original line 1\noriginal line 2\n";
        fs::write(&bak_path, original_content).expect("bak");
        fs::write(&jsonl_path, "modified content\n").expect("modified");

        assert!(rollback_migration_for_file(&jsonl_path));
        assert!(!bak_path.exists());
        assert_eq!(fs::read_to_string(&jsonl_path).unwrap(), original_content);
    }

    #[test]
    fn rollback_all_restores_all_backups() {
        let tmp = tempdir().expect("tempdir");
        let mem_dir = tmp.path().join("memory");
        fs::create_dir_all(&mem_dir).expect("dir");

        fs::write(mem_dir.join("a.jsonl.pre-migration.bak"), "a-original\n").expect("bak a");
        fs::write(mem_dir.join("a.jsonl"), "a-modified\n").expect("mod a");
        fs::write(mem_dir.join("b.jsonl.pre-migration.bak"), "b-original\n").expect("bak b");
        fs::write(mem_dir.join("b.jsonl"), "b-modified\n").expect("mod b");

        let _env = ScopedEnv::set("DEEPSEEK_CAPACITY_MEMORY_DIR", mem_dir.to_str().unwrap());
        let restored = rollback_all_migrations();

        assert_eq!(restored, 2);
        assert_eq!(
            fs::read_to_string(mem_dir.join("a.jsonl")).unwrap(),
            "a-original\n"
        );
        assert_eq!(
            fs::read_to_string(mem_dir.join("b.jsonl")).unwrap(),
            "b-original\n"
        );
    }

    #[test]
    fn is_empty_canonical_state_detects_default() {
        assert!(is_empty_canonical_state(&CanonicalState::default()));
        assert!(!is_empty_canonical_state(&CanonicalState {
            goal: "something".to_string(),
            ..CanonicalState::default()
        }));
        assert!(!is_empty_canonical_state(&CanonicalState {
            confirmed_facts: vec!["fact".to_string()],
            ..CanonicalState::default()
        }));
    }

    #[test]
    fn tier1_rehydrate_from_current_session_memory() {
        let tmp = tempdir().expect("tempdir");
        let mem_dir = tmp.path().join("memory");
        fs::create_dir_all(&mem_dir).expect("dir");

        let session_id = "tier1-session";
        let jsonl_path = mem_dir.join(format!("{session_id}.jsonl"));
        let record = CapacityMemoryRecord {
            id: "cap_t1".to_string(),
            ts: now_rfc3339(),
            turn_index: 5,
            action_trigger: "engine_shutdown".to_string(),
            h_hat: 0.0,
            c_hat: 0.0,
            slack: 0.0,
            risk_band: "low".to_string(),
            canonical_state: CanonicalState {
                goal: "Tier-1 goal from current session".to_string(),
                confirmed_facts: vec!["fact1".to_string()],
                ..CanonicalState::default()
            },
            source_message_ids: vec![],
            replay_info: None,
            workspace: "/workspace/a".to_string(),
        };
        append_capacity_record_to_path(&jsonl_path, &record).expect("write");

        let _env = ScopedEnv::set("DEEPSEEK_CAPACITY_MEMORY_DIR", mem_dir.to_str().unwrap());
        let records = load_last_k_capacity_records(session_id, 1).expect("load");

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].canonical_state.goal,
            "Tier-1 goal from current session"
        );
    }

    #[test]
    fn tier3_rehydrate_from_cross_session_memory() {
        let tmp = tempdir().expect("tempdir");
        let mem_dir = tmp.path().join("memory");
        fs::create_dir_all(&mem_dir).expect("dir");

        let old_session_id = "old-session-789";
        let old_path = mem_dir.join(format!("{old_session_id}.jsonl"));
        let record = CapacityMemoryRecord {
            id: "cap_t3".to_string(),
            ts: now_rfc3339(),
            turn_index: 10,
            action_trigger: "engine_shutdown".to_string(),
            h_hat: 0.0,
            c_hat: 0.0,
            slack: 0.0,
            risk_band: "low".to_string(),
            canonical_state: CanonicalState {
                goal: "Tier-3 goal from different session".to_string(),
                confirmed_facts: vec!["cross-fact".to_string()],
                ..CanonicalState::default()
            },
            source_message_ids: vec![],
            replay_info: None,
            workspace: "/workspace/x".to_string(),
        };
        append_capacity_record_to_path(&old_path, &record).expect("write");

        let _env = ScopedEnv::set("DEEPSEEK_CAPACITY_MEMORY_DIR", mem_dir.to_str().unwrap());
        let result = find_latest_cross_session("/workspace/x");

        let (sid, rec) = result.expect("should find cross-session record");
        assert_eq!(sid, old_session_id);
        assert_eq!(
            rec.canonical_state.goal,
            "Tier-3 goal from different session"
        );
    }

    #[test]
    fn tier3_cross_session_isolates_workspace() {
        let tmp = tempdir().expect("tempdir");
        let mem_dir = tmp.path().join("memory");
        fs::create_dir_all(&mem_dir).expect("dir");

        let ws_a = mem_dir.join("ws-a.jsonl");
        let record_a = CapacityMemoryRecord {
            id: "cap_a".to_string(),
            ts: now_rfc3339(),
            turn_index: 1,
            action_trigger: "engine_shutdown".to_string(),
            h_hat: 0.0,
            c_hat: 0.0,
            slack: 0.0,
            risk_band: "low".to_string(),
            canonical_state: CanonicalState {
                goal: "Workspace A goal".to_string(),
                ..CanonicalState::default()
            },
            source_message_ids: vec![],
            replay_info: None,
            workspace: "/workspace/a".to_string(),
        };
        append_capacity_record_to_path(&ws_a, &record_a).expect("write a");

        let _env = ScopedEnv::set("DEEPSEEK_CAPACITY_MEMORY_DIR", mem_dir.to_str().unwrap());
        let result = find_latest_cross_session("/workspace/b");

        assert!(
            result.is_none(),
            "should not find records from different workspace"
        );
    }

    #[test]
    fn rehydration_ladder_tier1_takes_priority_over_tier3() {
        let tmp = tempdir().expect("tempdir");
        let mem_dir = tmp.path().join("memory");
        fs::create_dir_all(&mem_dir).expect("dir");

        let current_session = "current-session";
        let current_path = mem_dir.join(format!("{current_session}.jsonl"));
        let current_record = CapacityMemoryRecord {
            id: "cap_current".to_string(),
            ts: now_rfc3339(),
            turn_index: 3,
            action_trigger: "engine_shutdown".to_string(),
            h_hat: 0.0,
            c_hat: 0.0,
            slack: 0.0,
            risk_band: "low".to_string(),
            canonical_state: CanonicalState {
                goal: "Current session goal (tier 1)".to_string(),
                ..CanonicalState::default()
            },
            source_message_ids: vec![],
            replay_info: None,
            workspace: "/workspace/x".to_string(),
        };
        append_capacity_record_to_path(&current_path, &current_record).expect("write current");

        let old_session = "old-session";
        let old_path = mem_dir.join(format!("{old_session}.jsonl"));
        let old_record = CapacityMemoryRecord {
            id: "cap_old".to_string(),
            ts: now_rfc3339(),
            turn_index: 10,
            action_trigger: "engine_shutdown".to_string(),
            h_hat: 0.0,
            c_hat: 0.0,
            slack: 0.0,
            risk_band: "low".to_string(),
            canonical_state: CanonicalState {
                goal: "Old session goal (tier 3)".to_string(),
                ..CanonicalState::default()
            },
            source_message_ids: vec![],
            replay_info: None,
            workspace: "/workspace/x".to_string(),
        };
        append_capacity_record_to_path(&old_path, &old_record).expect("write old");

        let _env = ScopedEnv::set("DEEPSEEK_CAPACITY_MEMORY_DIR", mem_dir.to_str().unwrap());
        let tier1 = load_last_k_capacity_records(current_session, 1).expect("tier1");

        assert_eq!(tier1.len(), 1);
        assert_eq!(
            tier1[0].canonical_state.goal,
            "Current session goal (tier 1)"
        );
    }
}
