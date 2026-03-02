# Parity CI and release checks

This repository now includes parity-oriented CI checks under `.github/workflows/parity.yml`.

## Workflow coverage

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --locked`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --all-features --locked`
- TUI snapshot parity test:
  - `cargo test -p deepseek-tui-core --test snapshot --locked`
- protocol parity smoke test:
  - `cargo test -p deepseek-protocol --test parity_protocol --locked`
- state persistence parity smoke test:
  - `cargo test -p deepseek-state --test parity_state --locked`
- lockfile drift guard:
  - `git diff --exit-code -- Cargo.lock`

The tag-based release workflow now runs the same parity preflight before building artifacts.

## Expected contributor flow

1. Update workspace crates (`core`, `app-server`, `protocol`, `state`, `tools`, `mcp`, `execpolicy`, `hooks`, `tui`, `cli`).
2. Keep protocol and persistence tests green for parity-sensitive changes.
3. Ensure thread/tool/mcp event contracts remain backward-compatible across app-server endpoints.

## Release readiness checklist

- CLI and app-server binaries compile from workspace members.
- Session persistence schema changes include migration-safe SQL updates.
- Protocol changes include test updates in `crates/protocol/tests`.
- New tool lifecycle behavior includes tests in `crates/tools/tests`.
- TUI reducer changes include deterministic snapshot updates in `crates/tui/tests`.
- Release artifacts include `deepseek` (CLI) and `deepseek-tui` (TUI) binaries for all platforms.
