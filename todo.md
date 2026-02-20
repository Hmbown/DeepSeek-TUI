# Wagmii TUI Overhaul Tasks

## Goal
- [ ] Improve Wagmii CLI UI/UX and make interaction feel less choppy.
- [ ] Replace the broken finance data source and keep behavior stable.
- [ ] Update task/run documentation for the new tooling and workflow.
- [ ] Prepare and cut a new release.

## UI/UX Improvements
- [ ] Audit `src/tui` rendering, input, and event handling for perceived latency.
- [ ] Reduce blocking work on UI paths; keep expensive calls off the main loop.
- [ ] Add clearer loading/failure states for long-running actions.
- [ ] Improve transitions and scrolling/paging smoothness.
- [ ] Verify no regressions in mode switching and overlays.

## Finance Tool Replacement
- [ ] Replace the current Stooq-backed implementation in `src/tools/finance.rs`.
- [ ] Add a more reliable data path (preferred: `yfinance` style source) with solid error handling.
- [ ] Keep crypto behavior compatible and preserve output format where possible.
- [ ] Add fallback behavior when the primary finance endpoint is unavailable.
- [ ] Handle common symbols like `AAPL`, `SPY`, and `BTC` correctly.

## Docs / Workflow
- [ ] Update `AGENTS.md` with current finance source and known issue notes.
- [ ] Update `README.md` tool section and troubleshooting notes to match the new behavior.
- [ ] Include release notes and any migration/compatibility notes.

## Release
- [ ] Bump crate version in `Cargo.toml`.
- [ ] Add/verify changelog note in `CHANGELOG.md` (if needed).
- [ ] Create and push a new git tag for the release.
- [ ] Publish GitHub release notes.
- [ ] Publish to crates.io (if auth and permissions allow).
