# Trimtab Protocol: deepseek-tui

> Canonical workflow protocol for closed-loop, self-verifying agentic development.
> Claude Code and Codex entrypoints both delegate here.

## Topology

```
           Claude Code (Opus) — Orchestrator
          /           |              \
   Sub-Opus agents  Codex MCP       Direct work
   (UI, research,   (infra, backend, (small edits,
    review, plan)    Rust systems)    conversation)
                         |
                  Fresh Codex context
                  (Closure verifier / Coach)
```

- **Orchestrator / Player:** Claude Code (Opus)
- **Workers:** Claude sub-agents (Agent tool) + Codex MCP sessions
- **Closure Verifier / Coach:** Fresh Codex context (never the same context that wrote the code)

## No-Self-Verdict Rule

The agent that wrote or modified code MUST NOT be the one to declare it passes.
Every batch — including review-only or zero-edit batches — goes to an independent
verifier (fresh Codex context or separate sub-agent) before anyone says PASS.

## Task Surface

This repo uses **Linear** as the canonical issue tracker.

- **Project:** https://linear.app/shannon-labs/project/deepseek-tui-6213bbbeaa26
- **Team:** Shannon Labs (SHA)

Operative task sources (priority order):
1. **Linear issues** — canonical for all tracked work (SHA-2794 through SHA-2803)
2. **DEPENDENCY_GRAPH.md** — local mirror of the task graph with ready queue
3. **AI_HANDOFF.md** — implementation notes and architecture context
4. **todo.md** — high-level goals

When starting a session:
1. List Linear issues for the project (filter by state: not Done/Canceled)
2. Identify the highest-priority unblocked issue
3. Read the issue body directly — it is the task packet
4. Execute using the waterfall rule

The issue body contains: Goal, Files, Pass/Fail Criteria, Boundary, Dependencies.
Do not invent a second prompt when the issue already has the packet.

## Waterfall Rule

After a verified issue closes, the player continues to the next unblocked issue
unless the operator explicitly reprioritizes. Do not stop and ask "what next?"
when the answer is visible in the task graph.

## Work Packet Structure

Each task should be expressed as a packet with:

```
GOAL:       One sentence describing the desired end state
FILES:      List of files expected to change
VERIFY:     Concrete acceptance criteria (tests pass, clippy clean, visual check)
BOUNDARY:   What is out of scope for this packet
```

## Build / Test / Lint Commands

```bash
cargo build                                          # Debug build
cargo build --release                                # Release build
cargo test --workspace --all-features                # Full test suite
cargo fmt --all -- --check                           # Format check
cargo clippy --workspace --all-targets --all-features  # Full lint
cargo doc --workspace --no-deps                      # Build docs
```

CI runs: fmt check, clippy, tests (Ubuntu/macOS/Windows), build.

## Delegation Guidelines

### Use Codex MCP for:
- Rust systems work, infrastructure, CI/CD, backend, shell scripting
- Second opinions on architecture or tradeoffs
- Debugging infra/backend issues
- Config: `approval-policy: "never"`, `sandbox: "workspace-write"`, `cwd: "/Volumes/VIXinSSD/deepseek-tui"`
- For advice-only: `sandbox: "read-only"`

### Use Claude sub-agents (Agent tool) for:
- TUI/UI work, research, codebase exploration
- Code review and quality analysis
- Planning and architecture docs

### Do directly:
- Small file edits, quick answers, reading/searching known files

### Shared workspace warning:
When dispatching into a directory where other agents may be working, include:
"Note: You are in a shared workspace. Other agents may be reading or editing files
concurrently. Focus only on your assigned task."

## Session Protocol

### Session Start
1. Read CLAUDE.md, AI_HANDOFF.md, DEPENDENCY_GRAPH.md
2. Check `git status` and recent commits
3. Identify the current task from the task surface
4. Announce what you're working on

### Session Close
1. `git status` — review all changes
2. `git add` specific files (never `git add -A` blindly)
3. `git commit` with conventional commit message
4. Verify: `cargo test --workspace --all-features`
5. Verify: `cargo clippy --workspace --all-targets --all-features`
6. Update AI_HANDOFF.md if task state changed
7. Push only if operator approves

### Commit Convention
Use conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`

## Crate Architecture Reference

```
crates/
  cli/          deepseek-cli         -> `deepseek` binary
  tui/          deepseek-tui         -> `deepseek-tui` binary
  app-server/   deepseek-app-server  HTTP/SSE + JSON-RPC server
  core/         deepseek-core        Agent loop, session, turns
  protocol/     deepseek-protocol    Request/response types
  config/       deepseek-config      Config loading, profiles
  state/        deepseek-state       SQLite persistence
  tools/        deepseek-tools       Tool registry + specs
  mcp/          deepseek-mcp         MCP server integration
  hooks/        deepseek-hooks       Lifecycle hooks
  execpolicy/   deepseek-execpolicy  Approval policy engine
  agent/        deepseek-agent       Model/provider registry
  tui-core/     deepseek-tui-core    TUI state machine scaffold
```

See DEPENDENCY_GRAPH.md for the full dependency graph.

## Key Architectural Decisions

- Edition 2024, Rust 1.85+
- Workspace version 0.3.30 (all crates share version)
- TUI binary still references monolith source (src/) — migration incremental
- DeepSeek API: Responses API preferred, chat completions fallback
- Sandbox: macOS Seatbelt, Linux Landlock
- Modes: Normal, Plan, Agent, YOLO, RLM, Duo (each gates different tools)
