# v0.8.8 Coordinator Prompt

Feed this entire document as a single message to a coordinating agent in **YOLO mode** (`deepseek --mode yolo` or `/mode yolo`). The coordinator will decompose the v0.8.8 milestone into independent workstreams, spawn sub-agents into git worktrees, monitor progress, and integrate results.

## Your role: v0.8.8 release coordinator

You are a coordinating agent responsible for delivering the v0.8.8 milestone. You must NOT attempt to implement issues yourself. Your job is to:

1. Read the issue tracker to understand every issue.
2. Create a high-level plan using `update_plan`.
3. Create git worktrees for each independent workstream.
4. Spawn sub-agents into those worktrees (one per workstream, or further subdivided for large streams).
5. Monitor sub-agent progress via `agent_list` / `agent_result`.
6. Integrate completed worktrees by merging them back into the main branch in dependency order.
7. Run verification gates (`cargo check`, `cargo clippy`, `cargo test`) after each integration.

## Scope: the v0.8.8 milestone

All issues with the `v0.8.8` label on GitHub, plus the 23 v0.8.6 carry-forward issues listed in #482. Use `gh issue list --label v0.8.8 --state open --limit 100` to enumerate them. For the carry-forward list, read issue #482 to get the full set of issue numbers.

Read **every issue body** before spawning work. You need to understand cross-cutting concerns (which issues touch the same files, which depend on others).

## Workstream decomposition

Group the issues into these workstreams. The exact issue-to-stream mapping is for you to determine after reading every issue, but this is the expected shape:

### Stream A: TUI bugfixes (12 issues, no dependencies)
- Bugs: #488 (Option+Backspace), #487 (pending queue leak), #449 (statusline theme), #444 (bounded terminal probes), #443 (keyboard reporting reset), #421 (subagent process leaks), #420 (MCP server shutdown), #419 (heredoc parsing), #417 (dangerous config keys), #416 (git -C auto-approve), #409 (agent_spawn rendering), #403 (todo tool JSON dumps)
- Size: Small, can be a single sub-agent.
- Target worktree: `worktrees/tui-bugfixes`

### Stream B: OPENCODE shared infrastructure (35+ issues)
- Permissions: #410–#418, #426
- LSP diagnostics: #427, #428, #389 (carry-forward)
- Hooks: #455, #456, #460
- Config/profile: #436, #437, #454, #390 (carry-forward)
- Skills: #431, #432, #433, #434
- Subagent infrastructure: #404, #405, #425, #426
- Worktree manager: #452
- Compaction: #429, #406
- Other: #430 (webfetch), #439 (toasts), #440 (prompt stash), #441 (frecency), #442 (Kitty keyboard), #445 (plan-mode suggestions), #446 (plan-exit handshake), #447 (multi-day duration), #448 (turn duration), #450 (screen-reader), #451 (pr command)
- This stream is large enough that you should spawn **multiple sub-agents** within it, subdivided by subsystem (permissions, LSP/hooks, config/skills, agent-infra, misc). Each sub-agent gets its own sub-worktree under `worktrees/opencode-*`.
- Size: Large. Subdivide aggressively.

### Stream C: Agent / UX improvements (7 issues)
- #403 (todo rendering), #404 (subagent categories), #405 (agent archiving), #406 (auto-archive), #407 (Agents workbench), #408 (Plan panel reconciliation), #409 (agent_spawn rendering)
- Note: #403, #406, #409 may overlap with Stream A. Coordinate to avoid conflicts — either put them in one stream or the other, not both.
- Depends on: Stream B for subagent infrastructure (#404, #405, #425, #426).
- Target worktree: `worktrees/agent-ux`

### Stream D: App-server enhancements (8 issues)
- #457 (server attach), #458 (mDNS publishing), #459 (OpenAPI generation), #470 (ACP bridge), #452 (worktree manager — may be shared with Stream B), #475 (auth for --http), plus carry-forward items touching app-server.
- Depends on: Stream B for permission/hook infrastructure.
- Target worktree: `worktrees/app-server`

### Stream E: Web UI (8 issues + umbrella)
- #471 (scaffold), #472 (composer/transcript), #473 (file browser/Monaco), #474 (approval modal), #475 (auth), #476 (mode switcher), #477 (theme), #478 (PWA)
- Umbrella: #481
- Depends on: Stream D (app-server must be stable). The web UI talks to `deepseek serve --http`.
- Target worktree: `worktrees/web-ui`

### Stream F: VS Code extension (9 issues + umbrella)
- #461 (scaffold), #462 (server auto-detect), #463 (chat webview), #464 (editor context), #465 (inline edits), #466 (diagnostics), #467 (commands), #468 (status bar), #469 (marketplace)
- Umbrella: #480
- Depends on: Stream D (app-server), and Stream E if the webview reuses Web UI components.
- Target worktree: `worktrees/vscode`

### Stream G: v0.8.6 carry-forward (23 issues)
- Numbers listed in #482 body. Read #482 to get the full list.
- Sizes and dependencies vary. Read each carry-forward issue and assign it to the appropriate stream (A–F) rather than treating it as a separate workstream. Many will map to Stream B (OPENCODE) or Stream A (bugfixes).

## Git worktree isolation pattern

Each workstream gets its own git worktree to prevent file conflicts, stale checkouts, and merge headaches:

```bash
# Bootstrap (you do this once, before spawning sub-agents):
git worktree add ../worktrees/tui-bugfixes -b feat/v0.8.8-tui-bugfixes
git worktree add ../worktrees/opencode-permissions -b feat/v0.8.8-opencode-permissions
git worktree add ../worktrees/opencode-lsp-hooks -b feat/v0.8.8-opencode-lsp-hooks
git worktree add ../worktrees/opencode-config -b feat/v0.8.8-opencode-config
git worktree add ../worktrees/opencode-agents -b feat/v0.8.8-opencode-agents
git worktree add ../worktrees/opencode-misc -b feat/v0.8.8-opencode-misc
git worktree add ../worktrees/agent-ux -b feat/v0.8.8-agent-ux
git worktree add ../worktrees/app-server -b feat/v0.8.8-app-server
git worktree add ../worktrees/web-ui -b feat/v0.8.8-web-ui
git worktree add ../worktrees/vscode -b feat/v0.8.8-vscode
```

When spawning a sub-agent, set its `cwd` to the corresponding worktree path. The sub-agent works entirely within that worktree and never touches the main checkout or other worktrees.

Once issue #452 (worktree manager) is completed, you may switch to using `/worktree new` instead of manual `git worktree add`. Until then, use the manual approach above.

## Sub-agent spawning strategy

For each workstream, spawn one or more sub-agents:

```
agent_spawn(
  type: "general",
  cwd: "../worktrees/<name>",
  prompt: "<detailed workstream prompt, including specific issue numbers and acceptance criteria>"
)
```

Each sub-agent prompt should include:
- The exact issue numbers assigned to that sub-agent.
- A reminder to read each issue body with `gh issue view <N>` before implementing.
- The acceptance criteria from each issue.
- A mandate to run `cargo check` and `cargo test --workspace --all-features` before reporting completion.
- Instructions to commit with messages like `feat(v0.8.8): <issue title> (fixes #NNN)`.

**For large streams (B, E, F)**, subdivide further: spawn one sub-agent per subsystem, each in its own worktree. Do NOT spawn one agent for all 35 OPENCODE issues — it will thrash.

**Parallelism rule**: spawn independent workstreams concurrently. Example: once worktrees exist, spawn A, B-permissions, B-lsp-hooks, B-config, B-agents, and B-misc all in one turn (up to 5 max in flight). When some complete, spawn the next batch.

## Integration order (critical)

Merge worktrees in this order to minimize conflicts:

1. **Stream A** (TUI bugfixes) — merge first. Quick wins, low conflict risk.
2. **Stream B subsystems** — merge in this order within B:
   a. `opencode-config` (config/profile changes — other things depend on config shape)
   b. `opencode-permissions` (permission model — agents depend on it)
   c. `opencode-lsp-hooks` (LSP and hooks infrastructure)
   d. `opencode-agents` (subagent infrastructure — needed by Stream C)
   e. `opencode-misc` (everything else in Stream B)
3. **Stream C** (Agent/UX) — merge after B-agents.
4. **Stream D** (App-server) — merge after B.
5. **Stream E** (Web UI) — merge after D.
6. **Stream F** (VS Code) — merge after D (and E, if components are shared).

For each merge:
```bash
cd /Volumes/VIXinSSD/deepseek-tui   # main worktree
git merge feat/v0.8.8-<stream> --no-ff -m "merge(v0.8.8): <stream> workstream"
```

Run verification gates after every merge:
```bash
cargo check --workspace --all-features
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features
```

If a merge fails or gates break, do NOT proceed to the next stream. Diagnose, report the conflict, and either fix it yourself or spawn a remediation sub-agent.

## Conflict prevention

These areas have high collision risk across streams:

| Area | Streams touching it | Mitigation |
|---|---|---|
| `crates/tui/src/tools/subagent/` | A (#421, #409), B (#404, #405, #425, #426), C (#407) | Assign subagent tooling changes to B-agents ONLY. Streams A and C depend on B-agents completing first. |
| `crates/tui/src/tui/sidebar.rs` | A (#409), C (#403, #407, #408) | Assign ALL sidebar changes to Stream C. Stream A skips #403, #409. |
| `crates/app-server/` | D (all), E (#471, #472, #475), F (#462, #463) | Stream D completes first. Streams E and F build on D's stable API. |
| `crates/execpolicy/` | B (#410–#418) | All execpolicy changes in B-permissions only. |
| `crates/config/` | B (#436, #437, #454), D (#475) | All config changes in B-config first. Stream D reads the new config shape. |
| `Cargo.toml` / workspace members | E (new `apps/web/` crate), F (new `extensions/vscode/`), B (new crates) | New crate additions handled in their respective streams. Merge order prevents Cargo.toml conflicts — add new members in dependency order. |

The coordinator is responsible for detecting file-level overlaps when reading issue bodies and adjusting assignments so no two workstreams independently modify the same file.

## Verification gates

Every sub-agent must pass these before reporting completion:

```
cargo check --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo fmt --all -- --check
```

After every merge, the coordinator runs the same gates on the main worktree. Use `task_gate_run` to record structured evidence.

For the Web UI stream (E), also verify:
```
cd apps/web && npm run build
```

For the VS Code stream (F), also verify:
```
cd extensions/vscode && npm run compile
```

## Progress tracking

Use `update_plan` with phases matching the integration order:

1. Phase 1: Bootstrap (create worktrees, read all issues) — `in_progress`
2. Phase 2: Stream A (TUI bugfixes) — `pending`
3. Phase 3: Stream B (OPENCODE infra) — `pending`
4. Phase 4: Stream C (Agent/UX) — `pending`
5. Phase 5: Stream D (App-server) — `pending`
6. Phase 6: Stream E (Web UI) — `pending`
7. Phase 7: Stream F (VS Code) — `pending`
8. Phase 8: Integration and final gates — `pending`

Use `checklist_write` under each phase with individual sub-agent tasks. Update status as sub-agents complete.

## Safety rails

- **Never force-push** to the main branch or any shared worktree branch.
- **Never delete worktrees** until the stream is merged and verified.
- **If a sub-agent fails**, read its result with `agent_result`, diagnose, and either respawn with corrected instructions or fix the issue in the worktree directly.
- **If two streams conflict on merge**, create a remediation worktree (`worktrees/merge-fix-<name>`), spawn a sub-agent to resolve the conflict, and retry the merge.
- **Do not close issues** until the merge containing their fix lands on the main branch AND verification gates pass.
- **Commit often** within each worktree. Squash only at the end if desired.

## Deferred / excluded from 0.8.8

These issues are explicitly NOT implemented in 0.8.8:
- #479 (Share-link mode) — design doc only, deferred.

Read each umbrella issue (#480, #481) carefully — they may scope some child issues as "Phase 2" or "deferred." Only implement what's accepted for 0.8.8.

## Start here

1. Run `gh issue list --label v0.8.8 --state open --limit 100 --json number,title,body,labels` and read every body.
2. Run `gh issue view 482` to get the v0.8.6 carry-forward list, then read those issue bodies.
3. Create `update_plan` with the 8 phases above.
4. Create all git worktrees.
5. Begin spawning sub-agents for Phase 2 (Phase 1 is reading issues, which you're doing now).
