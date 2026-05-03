# v0.8.6 Takeover Prompt — Fresh DeepSeek V4 Session

You are taking over the v0.8.6 sprint for `github.com/Hmbown/DeepSeek-TUI`.
A previous DeepSeek session kept getting interrupted because the parent session
grew too large during long-running work. The user has now pruned local saved
sessions, but that is only temporary relief. Your job is to stabilize the branch
and fix the product so long-running agent work survives by default.

## Prime Directive

Do not run this as one long sequential parent session.

The parent session is the coordinator. Use `agent_spawn` for tool-carrying work,
use `rlm` for batch classification/synthesis over long issue lists or docs, and
keep the parent transcript small. If you find yourself reading files one by one
for the same topic, stop and delegate.

## Immediate Emergency

Start with #402:

- `#402 P0: make long-running sessions survivable by default (Codex-style compaction + bounded transcript state)`

This is now the top priority because it caused the interrupted handoff loop.
The issue body names the exact gap versus `/Volumes/VIXinSSD/codex-main`:

- DeepSeek TUI keeps unbounded `api_messages` and visible `history`.
- `auto_compact = false` and the capacity controller is off by default.
- saved sessions serialize full `messages: Vec<Message>` snapshots.
- the important mocked engine tests for compaction/subagents/parallel execution
  are still ignored because the engine takes a concrete `DeepSeekClient`.
- Codex has runtime pre/mid-turn compaction, replacement history, persisted
  compacted rollout items, and sanitized/last-N subagent fork behavior.

Do not treat this as docs or prompt tuning. Implement runtime guardrails.

## Current Branch State To Verify

Branch should be `feat/v0.8.6`. The prior interrupted session had dirty work.
Verify before trusting any claim:

1. `git status --short --branch`
2. `cargo check --workspace --all-targets --locked`
3. `cargo test --workspace --all-features --locked` if check passes
4. read `AGENTS.md`, `V086_BRIEF.md`, `docs/ARCHITECTURE.md`, and issue #402

Known partial work from the interrupted session:

- Goal mode command dispatch (`/goal`) — inspect `crates/tui/src/commands/goal.rs`
- File tree pane — inspect `crates/tui/src/tui/file_tree.rs`
- user-defined command plumbing — inspect `crates/tui/src/commands/user_commands.rs`
- localization/sidebar/rendering changes across `crates/tui/src/*`

Do not overwrite unrelated dirty files. Work with the existing changes.

## Updated v0.8.6 Issue Set

The original brief said 23 issues, but the live v0.8.6 label now includes more.
Refresh live state with:

```bash
gh issue list --label v0.8.6 --state open --limit 100 --json number,title,body,labels
```

New or especially relevant additions:

- `#402` P0 long-running session survivability: runtime compaction, bounded transcript/session persistence.
- `#401` prune overly defensive assertions: remove brittle prompt-substring/snapshot-style tests.
- `#400` chat/sidebar text bleed-through: timestamp fragments persist across cells when scrolling.
- `#399` lag/freeze audit: sync git on UI thread, unbounded history Vec, file-tree blocking walk.
- `#398` codex-mcp parity: agent-style MCP server tool plus `deepseek mcp add/list/get/remove`.

Existing high-priority v0.8.6 issues still include:

- `#397` Goal mode
- `#396` per-turn cache hit chip
- `#395` cycle-boundary visualization
- `#394` file-tree pane
- `#393` share session URL
- `#392` `/model auto`
- `#391` user-defined slash commands
- `#390` profile hot-switch
- `#389` inline LSP diagnostics
- `#388` crash-recovery prompt
- `#387` self-update
- `#386` `/init`
- `#385` `/diff`
- `#384` `/undo`
- `#383` `/edit`
- `#382` collapse Steer/Queue/Immediate
- `#380` inline diff highlighting
- `#379` smart clipboard
- `#378` docs polish
- `#377` shrink App state
- `#376` native-copy escape
- `#375` right-click context menu
- `#374` clickable file:line
- `#373` Tasks panel ignores shell jobs

## First-Hour Execution Plan

Do this as a fanout, not a serial survey.

1. Parent: create a checklist with lanes below, then run one batched read/status
   turn: `git status`, `gh issue list --label v0.8.6`, focused `rg` for
   compaction/session/history/capacity, and the initial cargo check.

2. Spawn sub-agent A: #402 runtime/session survivability.
   Ownership: `crates/tui/src/core/engine.rs`, `crates/tui/src/compaction.rs`,
   `crates/tui/src/session_manager.rs`, `crates/tui/src/tui/app.rs`,
   `crates/tui/tests/integration_mock_llm.rs`, and relevant config docs.
   Task: design and implement the smallest runtime guardrail slice that bounds
   parent model history/session persistence and unblocks real integration tests.

3. Spawn sub-agent B: current dirty-tree compile repair.
   Ownership: partial v0.8.6 files from the interrupted session:
   `commands/goal.rs`, `commands/user_commands.rs`, `tui/file_tree.rs`,
   `commands/mod.rs`, `localization.rs`, `tui/sidebar.rs`, `tui/ui.rs`.
   Task: make the branch compile without widening scope.

4. Spawn sub-agent C: UI performance/bleed-through lane (#399/#400/#394).
   Ownership: transcript rendering/cache, sidebar rendering, file-tree traversal.
   Task: fix the regression and identify any blocking synchronous UI work.

5. Spawn sub-agent D: issue/test hygiene lane (#401 plus ignored mock tests).
   Ownership: brittle tests, prompt snapshot tests, and ignored integration tests.
   Task: remove brittle assertions where appropriate and convert #402 acceptance
   criteria into real tests.

6. Spawn sub-agent E only if needed: MCP parity (#398) or command surface
   follow-through (#391/#397). Keep it separate from #402 so the P0 fix is not
   tangled with feature work.

## RLM Usage

Use `rlm` when the input is large enough that pasting/reading it in the parent
would bloat the session. Good RLM tasks here:

- classify all live `v0.8.6` issue bodies into independent implementation lanes;
- compare #402 against Codex files by giving RLM extracted snippets from both
  repos and asking for a bounded acceptance checklist;
- batch-review a long test list for brittle assertions related to #401;
- summarize long cargo/clippy output into file-owned fix clusters.

Inside RLM, use `llm_query_batched()` for independent classifications and
`rlm_query()` only for recursive critique/decomposition. The parent should get
the final synthesis, not every intermediate chunk.

## Session Survival Rules

- Keep at most 5 sub-agents running.
- After spawning agents, keep doing non-overlapping local coordination work.
- Use `agent_wait` only when blocked on results.
- Use `agent_result` for completed agents and summarize results into the parent.
- Suggest `/compact` at 60% context, but do not rely on that as the product fix.
- If the parent reaches 3 sequential turns on the same topic, spawn or RLM it.
- Do not paste full logs into the parent. Store logs as artifacts or ask RLM to
  summarize them.

## PR Workflow

Use GitHub PRs as an extra review surface. Do not let a giant local branch pile
up without outside checks.

- Prefer small PRs by issue or tightly related lane: #402 can be its own PR,
  compile-repair can be its own PR, UI performance/regression fixes can be their
  own PR, and command-surface features can be separate.
- Push work branches and open PRs early once each slice compiles and has focused
  tests. Include `Closes #...` only when the PR actually satisfies the issue.
- Let CI and any GitHub AI/code-review agents inspect the code. Treat review
  comments as real work: address them with follow-up commits rather than
  hand-waving them away.
- When a PR comes back clean, merge it into the target branch and continue from
  the updated branch. When it comes back with requested fixes, make the fixes,
  rerun the relevant gates, and wait for the updated checks before merging.
- Keep the parent session tracking PR state with `gh pr view`, `gh pr checks`,
  and `gh issue view`; do not manually close issues unless acceptance is
  verified and the merge did not close them automatically.

## Verification Gates

Before claiming anything is done:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

For #402 specifically, also add or enable focused tests proving:

- compaction/cycle guardrail runs before dangerous context growth;
- live `api_messages` or equivalent model history is bounded after compaction;
- visible transcript/session persistence is bounded or virtualized;
- sub-agent result ingestion into the parent is summarized/bounded;
- child fork history can use sanitized last-N behavior;
- session save/checkpoint does not rewrite arbitrary huge full transcripts.

## Final Report Format

Use these headings:

- Implemented
- Verified
- Issues safe to close
- Issues still open and why
- Commands run
- Residual risks

Be explicit about what is local-only, what is committed, what is pushed, and what
is merely planned. Do not close issues unless acceptance criteria are verified.
