# v0.8.6 Backlog — Work Brief for an AI Agent

This is a structured brief for another AI (Claude Opus, DeepSeek V4, or similar) to 
understand the full v0.8.6 scope and begin implementation. The repo is 
`github.com/Hmbown/DeepSeek-TUI` — Rust workspace, TUI coding agent for DeepSeek V4.

**Branch**: create `feat/v0.8.6` from `main` (current HEAD at v0.8.5 tag).  
**All 23 issues are tagged `v0.8.6`** and live in the repo's GitHub Issues.  
**Zero open issues outside this list** — the board is clean.

## Project Context

DeepSeek TUI is a terminal-native coding agent. Key architectural points:
- **Dispatcher binary** (`deepseek`) delegates to the TUI binary (`deepseek-tui`) 
- **Crate map**: `crates/tui` is the main crate; `crates/cli` handles CLI entry; 
  `crates/config`, `crates/core`, `crates/tools` etc. are sub-crates
- **Engine pattern**: `core/engine.rs` runs the agent loop, processes tool calls
- **TUI**: ratatui-based, alt-screen, composer at bottom, sidebar at right
- **Config**: `~/.deepseek/config.toml`, profiles, providers, settings
- **Key files to read first**: `docs/ARCHITECTURE.md`, `crates/tui/src/main.rs`, 
  `crates/tui/src/tui/app.rs`, `crates/tui/src/core/engine.rs`

Read `AGENTS.md` and `CLAUDE.md` in the repo root for build/test commands.

---

## v0.8.6 Issues — Grouped by Theme

### Group A: UX Polish — Transcript & Clipboard (5 issues)

| # | Title | TL;DR |
|---|-------|-------|
| 380 | Inline diff highlighting | Color +/- in apply_patch/edit_file results |
| 379 | Smart clipboard Ctrl+Y | Copy focused cell to system clipboard |
| 375 | Right-click context menu | Per-cell menu: Copy, Open in editor, Re-run, Hide |
| 374 | Clickable file:line | OSC-8 hyperlinks on path:line in tool output |
| 376 | Native-copy escape | Hold Shift to bypass alt-screen for terminal selection |

### Group B: Workspace UX — Navigation & Visibility (4 issues)

| # | Title | TL;DR |
|---|-------|-------|
| 394 | File-tree pane | Ctrl+E toggles left-side workspace navigator |
| 395 | Cycle-boundary visualization | Inline dividers between coherence cycles |
| 396 | Per-turn cache hit chip | Footer shows cache hit % after each turn |
| 388 | Crash-recovery prompt | On restart, offer to restore interrupted turn |

### Group C: Session & History (3 issues)

| # | Title | TL;DR |
|---|-------|-------|
| 383 | /edit — revise and resubmit | Pull last message into composer, re-run turn |
| 384 | /undo — revert last patch | Surgical undo of apply_patch/edit_file/write_file |
| 385 | /diff — session changes | Show git diff since session start |

### Group D: Tools & Intelligence (4 issues)

| # | Title | TL;DR |
|---|-------|-------|
| 389 | Inline LSP diagnostics | Show rust-analyzer errors after each patch |
| 386 | /init — bootstrap AGENTS.md | Auto-detect project type, write starter AGENTS.md |
| 391 | User-defined slash commands | ~/.deepseek/commands/<name>.md templates |
| 392 | /model auto | Heuristic Pro-vs-Flash routing per turn |

### Group E: Infrastructure & Sharing (4 issues)

| # | Title | TL;DR |
|---|-------|-------|
| 390 | /profile — hot-switch config | Switch config profiles in-session without restart |
| 393 | /share — session URL | Export session as static HTML, upload to gist/S3 |
| 387 | In-app self-update | deepseek update fetches + replaces binary |
| 397 | Goal mode | Stated objective, token budget, self-verification tools |

### Group F: Quality & Fixes (3 issues)

| # | Title | TL;DR |
|---|-------|-------|
| 382 | Collapse Steer/Queue/Immediate | One mental model — everything queues, Ctrl+Enter steers |
| 373 | Sidebar Tasks panel ignores shell jobs | Wire shell jobs into Tasks panel |
| 377 | Shrink App state | Group ~200 fields into typed sub-states |
| 378 | Docs: tighten README + ARCHITECTURE | External-reader polish pass |

---

## Suggested Implementation Order

### Wave 1: Foundation (start here)
1. **#377 (refactor App state)** — do this FIRST. Group fields into sub-state structs 
   before adding more fields. Every subsequent feature touches App.
2. **#382 (collapse Steer/Queue)** — UX clarity fix, low implementation risk.
3. **#373 (Tasks panel shell jobs)** — bugfix, low risk.

### Wave 2: Transcript UX
4. **#380 (inline diff highlighting)** — parser pass on tool output, visible value.
5. **#374 (clickable file:line)** — OSC-8 hyperlinks, high discoverability.
6. **#379 (smart clipboard Ctrl+Y)** — small feature, big ergonomic win.
7. **#375 (right-click context menu)** — depends on mouse event plumbing.
8. **#376 (native-copy escape)** — terminal selection fix.

### Wave 3: Session tools
9. **#383 (/edit)** — requires engine truncation path.
10. **#384 (/undo)** — depends on snapshot infra.
11. **#385 (/diff)** — uses snapshot repo, depends on #380 for rendering.
12. **#388 (crash-recovery prompt)** — uses existing checkpoint infra.

### Wave 4: Intelligence
13. **#386 (/init)** — project detection + AGENTS.md generation.
14. **#389 (LSP diagnostics)** — polls existing LSP client, low-maintenance.
15. **#391 (user-defined commands)** — skills loader reuse.
16. **#392 (/model auto)** — heuristic router, DeepSeek-specific.

### Wave 5: Visibility & sharing
17. **#394 (file-tree pane)** — workspace navigator.
18. **#395 (cycle-boundary visualization)** — coherence cycle dividers.
19. **#396 (cache hit chip)** — footer chip, simple addition.
20. **#393 (/share)** — HTML export, gist/S3 backend.
21. **#387 (self-update)** — binary fetch + verify + replace.

### Wave 6: Docs & goal mode
22. **#378 (docs polish)** — README + ARCHITECTURE refresh.
23. **#397 (Goal mode)** — largest feature, last (benefits from all previous work).

---

## Working Patterns

- **PR-per-issue** (or small clusters). Each merged PR closes one issue.
- **Decomposition first**: read the issue body, identify the files that need to change,
  create a `checklist_write`, then implement.
- **Test gates**: `cargo test --workspace --all-features` must pass before each PR.
- **Lint gates**: `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- **Format**: `cargo fmt --all` before commit.
- **GitHub**: push to `feat/v0.8.6`, create PRs to `main`. Use `gh` CLI.
- **No open issues** except the v0.8.6 list — if new issues emerge, create them but don't block.

## Key Resources

- Repo: `https://github.com/Hmbown/DeepSeek-TUI`
- Architecture: `docs/ARCHITECTURE.md`
- Config reference: `docs/CONFIGURATION.md`
- CLI: `gh issue list --label v0.8.6 --json number,title,body` for full issue text
