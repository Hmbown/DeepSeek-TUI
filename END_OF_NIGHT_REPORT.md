# End-of-Night Report — v0.8.5 Backlog Sprint

**Date:** Overnight session  
**Branch:** feat/v0.8.5 (HEAD a8be33b3)  
**Baseline:** Clean git status, clippy passes, 1755/1756 tests pass (1 pre-existing env-dependent config failure)

---

## Completed

### #355 — Atomic File Writes for ~/.deepseek/ ✅

**Commits:** 5bd63c77

- Added `write_atomic(path, contents)` helper in `utils.rs` using `NamedTempFile` + `fsync` + `persist` (atomic rename)
- Added `open_append(path)` and `flush_and_sync(writer)` for append-only logs
- Converted all non-append write sites:
  - `session_manager.rs`: `save_session`, `save_checkpoint`, `save_offline_queue_state`
  - `workspace_trust.rs`: `write_trust_file_at`
  - `task_manager.rs`: `write_json_atomic` → delegates to `write_atomic`
  - `runtime_threads.rs`: `write_json_atomic` → delegates to `write_atomic`, `append_event` now calls `sync_all`
  - `mcp.rs`: `save_config`, `init_config`, `save_legacy`
  - `audit.rs`: buffered append with `flush_and_sync` after each event
  - `main.rs`: `save_mcp_config` → `write_atomic`
- Added 4 unit tests covering writing, replacing, temp-file cleanup, and append
- **Tests pass.** All verification gates pass.

### #346 — Panic Safety Foundations ✅ (partial)

**Commits:** a8be33b3

- Added `spawn_supervised(name, location, future)` to `utils.rs`:
  - Wraps future in `AssertUnwindSafe` + `catch_unwind` (via `futures_util::FutureExt`)
  - On panic: logs via `tracing::error!`, writes crash dump to `~/.deepseek/crashes/<timestamp>-<task>.log`
  - Returns `JoinHandle<()>` — panic is caught internally so parent stays alive
- Added `write_panic_dump()` helper for crash dump writing
- Added process-level panic hook in `main.rs` that writes crash dump before invoking original hook
- Converted `persistence_actor::spawn_persistence_actor` as the first `spawn_supervised` caller

**Remaining:** ~34 `tokio::spawn` sites still unconverted. These are safe to do in a focused follow-up PR — tokio already isolates spawned tasks from the process, so the gap is just crash dump coverage and structured logging. Existing `catch_unwind` guards on `runtime_threads.rs:1242/1462` and `mcp.rs:332` remain in place.

---

## Not Started

### Phase 1c: #350 — Schema Migration Up-Path
- Per-record migration framework for sessions/threads/tasks
- Backup-before-migrate pattern
- At least one no-op `migrate_v(N)_to_v(N+1)` stub per type

### Phase 2a: #338 — /config <key> <value> silently ignored
### Phase 2b: #342 — /provider API-key paste leaks into composer
### Phase 2c: #343 — /logout stale key fix
### Phase 2d: #345 — submit-disposition UX
### Phase 2e: #286/#352 — NVIDIA NIM / China endpoint CI

---

## Owner Questions for Morning Review

1. **#346 scope:** The issue asks to convert all 36 `tokio::spawn` sites in one PR. That's ~15 production sites plus ~21 test sites. Do you want test-site conversions too, or only production? The existing `catch_unwind` guards (`runtime_threads.rs:1242,1462`, `mcp.rs:332`) — should they also be consolidated into `spawn_supervised`, or is the current pattern fine?

2. **#350 priority:** Schema migration is the last Phase 1 blocker before Phase 2 bugs. If you want these bugs fixed first, I can swap the order. The schema migration is low risk but needs careful design review.

3. **The pre-existing config test failure** (`config::tests::test_load_falls_back_to_home_config_when_env_path_missing` and `test_load_uses_tilde_expanded_deepseek_config_path`) appears to be a sandbox/environment issue where `dirs::home_dir()` returns `None`. Not caused by these changes.

---

## Coverage Summary

| Metric | Value |
|--------|-------|
| Commits this session | 2 |
| Files changed | 10 |
| Lines added | ~255 |
| Tests added | 4 |
| CI-likely passing | Yes (1 pre-existing env failure) |
| Clippy | Clean |
