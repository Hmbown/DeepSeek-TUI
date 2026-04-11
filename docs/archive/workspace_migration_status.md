# DeepSeek Workspace Migration Status

This document is a historical snapshot of the initial workspace migration implementation for Linear issues `SHA-1554` to `SHA-1568`.

It is not maintained as a live status board. Some items below describe work that was still in progress at the time this patch landed and may no longer reflect the current codebase. For current behavior, use the active docs in `docs/` and the current source tree.

## Implemented in the initial patch

- `SHA-1554`:
  - Root converted to Cargo workspace.
  - New crate boundaries added:
    - `crates/core`
    - `crates/cli`
    - `crates/app-server`
    - `crates/protocol`
    - `crates/config`
    - `crates/agent`
    - `crates/tui`
    - `crates/tui` (TUI binary pointing at monolith source)
  - Stable entry binaries now follow `cli` + `app-server` + `tui` split.

- `SHA-1555`:
  - Added `deepseek-config` crate with `ConfigToml` schema.
  - Added provider-aware env precedence (`DEEPSEEK_API_KEY`, `OPENAI_API_KEY`, provider/base-url/model overrides).
  - Added config read/write/list/set/unset operations.

- `SHA-1556`:
  - Added codex-style command grouping in `deepseek` CLI:
    - `run`
    - `auth`
    - `config`
    - `model`
    - `app-server`
    - `completion`
  - Added global runtime override flags (`provider`, `model`, logging/telemetry/output/sandbox/approval controls).

- `SHA-1557`:
  - Added dual-provider auth model (`deepseek` + `openai`) with clear precedence and CLI management commands.
  - Added `auth status|set|clear` command flow.

- `SHA-1558`:
  - Added `deepseek-protocol` crate with `thread/app/prompt` request-response framing and event frames.
  - Added `deepseek-app-server` with `/thread`, `/app`, `/prompt`, `/healthz`.
  - Added `/tool`, `/jobs`, and `/mcp/startup` transport endpoints for tool/job/MCP parity flows.
  - Added stdio JSON-RPC 2.0 parity framing (`id`/`method`/`params` -> `result`/`error`) for `thread/*`, `app/*`, `prompt/*`, plus `healthz`/capabilities handlers.

- `SHA-1560`:
  - Added `deepseek-agent` model/provider registry with alias resolution and fallback strategy.

- `SHA-1564`:
  - Added `deepseek-tui-core` event-driven state machine scaffold (`UiState::reduce`).
  - Expanded reducer with job/approval states and deterministic snapshot support.

- `SHA-1559`:
  - Added `deepseek-state` crate with persistent thread/session metadata in SQLite.
  - Added thread list/read/archive/unarchive/name persistence operations and session index mirror.

- `SHA-1561`:
  - Added `deepseek-tools` crate with typed tool specs, call lifecycle, mutating gate, timeout handling, and read/write lock parallelism model.

- `SHA-1562`:
  - Added `deepseek-mcp` crate with server lifecycle events, qualified tool naming, filter support, resource listing/reads, and proxy call API.
  - Added MCP stdio JSON-RPC 2.0 server mode parity for `tools/list`, `tools/call`, `resources/list`, `resources/read`, and server lifecycle operations.
  - Added persisted MCP server definition round-trip through existing config APIs so server-mode definitions survive restarts.

- `SHA-1563`:
  - Added `deepseek-execpolicy` crate with approval mode model and policy decision/requirement evaluation.

- `SHA-1565`:
  - Added durable-style `JobManager` abstraction in core for queue/progress/cancel/recovery semantics.

- `SHA-1566`:
  - Added `deepseek-hooks` crate with stdout/jsonl/webhook sinks and standardized lifecycle events.

- `SHA-1567`:
  - Added parity tests for protocol/state/tools and TUI snapshot behavior.

- `SHA-1568`:
  - Added parity CI workflow at `.github/workflows/parity.yml` with workspace fmt/check/clippy/test gates, lockfile drift guard, and explicit snapshot/protocol/state parity tests.
  - Added matching release preflight parity gates in `.github/workflows/release.yml`.
  - Updated release artifact naming to include explicit `deepseek` entrypoint compatibility.

## Open items at the time of the initial patch

- Codex-level protocol field-by-field parity for every `thread/*` operation remains in progress.
- MCP transport now provides stdio JSON-RPC compatibility flows; external subprocess execution remains scaffolded.
- Execution policy supports decision modeling and command gating; full user-interactive approval UX remains in progress.
- Background jobs are persisted conceptually at runtime boundary; cross-process recovery orchestration is still in progress.

## Migration strategy note

`crates/tui` intentionally points at existing `src/main.rs` to preserve current behavior while new workspace crates are phased in. This enables incremental replacement without blocking ongoing feature work.
