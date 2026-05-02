# v0.8.6 Takeover Prompt — Fresh V4 Session

You are taking over the v0.8.6 sprint for `github.com/Hmbown/DeepSeek-TUI`. 
A previous session attempted this work but ground to a halt from context bloat — 
too many sequential turns without compaction or sub-agent delegation.

## Your mandate: finish fast, stay responsive

**Rule 1: Delegate aggressively.** Every independent sub-task gets a sub-agent.
Read-only investigation? Sub-agent. Single-file edit? Sub-agent. Test run? Sub-agent.
You are the coordinator — spawn, integrate, verify. Do not do the work yourself unless
it's a one-turn read.

**Rule 2: Batch everything.** Independent tool calls go in the same turn.
Reading 3 files, searching 2 patterns, checking git status all at once.
Never fire one tool and wait when you could fire 3.

**Rule 3: Compact early.** Suggest `/compact` at 60% context, not 80%.
A compacted session that stays fast is better than an uncompacted session that dies.

**Rule 4: Max 3 sequential turns per topic before spawning.** If you're on turn 4
of reading files one at a time for the same feature, stop. Spawn sub-agents for the
remaining reads. Synthesize. Move on.

## What we know so far

### Done (in the dead session — verify each claim before trusting it)
The previous session completed or partially completed:
- Goal mode command dispatch (`/goal`) — check `crates/tui/src/commands/goal.rs`
- File tree pane — check `crates/tui/src/tui/file_tree.rs` 
- Some localization additions for new commands
- Some sidebar rendering for goal panel

**Verify before claiming**: read the actual files, run `cargo check`, confirm
what's real vs what the dead session only planned.

### The full scope
Read `V086_BRIEF.md` — it has all 23 issues grouped by theme with suggested order.

### Priority order for takeover
1. **Run `cargo check` and `cargo test` immediately** — assess what compiles, what's broken
2. **Fix any compilation errors** from the dead session's partial work
3. **Then start on remaining issues** in the wave order from V086_BRIEF.md

## Project context
- Rust workspace, `cargo build` / `cargo test --workspace --all-features`
- Main crate: `crates/tui` (TUI app), `crates/cli` (dispatcher)
- Read `AGENTS.md` and `docs/ARCHITECTURE.md` for internals
- All 23 issues tagged `v0.8.6` in GitHub Issues (`gh issue list --label v0.8.6`)
- PR target: `main` branch, create PRs from `feat/v0.8.6`

## Session survival checklist
After every 3 turns, check:
- [ ] Context under 60%? If not, `/compact`
- [ ] Any sub-agents still running? Check `agent_list`
- [ ] Any PRs ready to create? Push and create PR
- [ ] `cargo check` still passes? Run it now

Start by running `cargo check` and `cargo test --workspace --all-features` to
assess the current state, then read `V086_BRIEF.md` and report what's real vs
what the dead session only planned.
