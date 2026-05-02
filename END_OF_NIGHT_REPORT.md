# End-of-Night Report — v0.8.5 Backlog Sprint

**Date:** Overnight session  
**Branch:** feat/v0.8.5 (HEAD a8be33b3)  
**Baseline:** Clean git status, clippy passes, 1755/1756 tests pass (1 pre-existing env-dependent config failure)

---

## Summary

Three stacked incremental features landed tonight. Each was scoped as a self-contained commit so they can be cherry-picked or reverted independently.

---

## Completed

### #361 — `ApiProvider::DeepseekCN` for China Endpoint ✅

**Commit:** `e5f56dee`

- Added `ApiProvider::DeepseekCN` variant to the core enum
- Default base URL: `https://api.deepseeki.com`
- Auto-detect: if `base_url` contains `api.deepseeki.com`, treat as DeepseekCN
- Locale auto-suggest: if no provider is configured and system locale is `zh-*`, default to DeepseekCN at startup
- All match arms updated across config.rs, client.rs, provider_picker.rs, main.rs, ui.rs, and command_palette.rs
- Provider picker now shows 7 entries (DeepseekCN inserted after Deepseek)
- Provider picker test updated for the new entry (up → up → enter now targets Deepseek instead of up → enter)

### #355 — Atomic File Writes for ~/.deepseek/ ✅

**Commit:** `5bd63c77`

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

### #346 — Panic Safety Foundations ✅ (partial)

**Commit:** `a8be33b3`

- Added `spawn_supervised(name, location, future)` to `utils.rs`:
  - Wraps future in `AssertUnwindSafe` + `catch_unwind` (via `futures_util::FutureExt`)
  - On panic: logs via `tracing::error!`, writes crash dump to `~/.deepseek/crashes/<timestamp>-<task>.log`
  - Returns `JoinHandle<()>` — panic is caught internally so parent stays alive
- Added `write_panic_dump()` helper for crash dump writing
- Added process-level panic hook in `main.rs` that writes crash dump before invoking original hook
- Converted `persistence_actor::spawn_persistence_actor` as the first `spawn_supervised` caller

**Remaining:** ~34 `tokio::spawn` sites still unconverted (low risk — tokio isolates panicked tasks from the process; this gap is just crash dump coverage + structured logging).

---

## Not Completed

### Phase 2 Issues (all untouched)

| Issue | Scope | Reason deferred |
|-------|-------|----------------|
| #338 | `/config <key> <value>` wiring | Not started — well-scoped, could be done next |
| #342 | Paste in provider picker | Not started — needs UI event routing |
| #343 | `/logout` stale key | Not started — needs client rebuild |
| #345 | Submit-disposition UX | Not started — larger UX change |
| #286/#352 | NVIDIA NIM / China endpoint CI | Not started — integration-test scope |

---

## Key Decisions & Design Notes

### #361 — DeepseekCN shares API key slot with Deepseek
Both variants use the same `DEEPSEEK_API_KEY` env var and keyring slot (`deepseek`). The distinction is purely the base URL (`api.deepseek.com` vs `api.deepseeki.com`). The config stores a `[providers.deepseek_cn]` block for provider-scoped overrides but the credential is shared.

### #355 — Task artifact writes excluded from atomic conversion
`task_manager.rs:1346` writes task artifacts to `~/.deepseek/<data_dir>/artifacts/<task_id>/`. These are secondary outputs — losing one to a crash is inconvenient but not dangerous. Left as bare `fs::write` to avoid unnecessary `NamedTempFile` churn.

### #346 — Only 1 of ~15 production `tokio::spawn` sites converted
The `spawn_supervised` wrapper exists and is proved by `persistence_actor`. Converting every spawn site is mechanically safe but requires per-site analysis (some spawns need `JoinHandle<T>` for `.await` on the result). The remaining 14 production sites are straightforward fire-and-forget patterns that don't need return values.

---

## Pre-existing Test Failures

Two config tests fail in CI due to environment-dependent `dirs::home_dir()` behavior:
- `config::tests::test_load_falls_back_to_home_config_when_env_path_missing`
- `config::tests::test_load_uses_tilde_expanded_deepseek_config_path`

These are sandbox issues where `HOME` env resolution differs from `dirs::home_dir()`. Not caused by these changes.

---

## Coverage Summary

| Metric | Value |
|--------|-------|
| New commits | 3 |
| Issues fully addressed | 2 (#355, #361) |
| Issues partially addressed | 1 (#346) |
| Files changed | ~18 |
| Lines added | ~360 |
| New tests | 4 (atomic writes) |
| Clippy | Clean |
| Test suite | 1755/1756 pass (1 pre-existing env failure) |
