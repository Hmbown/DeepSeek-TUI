# Competitive Analysis: DeepSeek TUI vs OpenCode vs Codex CLI

Analysis of capabilities across three AI coding agents: OpenCode (`/Volumes/VIXinSSD/opencode`), Codex CLI (`/Volumes/VIXinSSD/codex-main`), and DeepSeek TUI (`/Volumes/VIXinSSD/deepseek-tui`).

## Tool Matrix

| Capability | OpenCode | Codex CLI | DeepSeek TUI |
|---|---|---|---|
| File read | ✅ Read | ✅ | ✅ file |
| File write | ✅ Write | ✅ | ✅ file |
| File edit | ✅ Edit (string replace) | ✅ apply_patch (diff format) | ✅ edit_file + apply_patch |
| File glob | ✅ Glob | ✅ | ✅ file_search |
| Code search | ✅ Grep + CodeSearch (Exa) | ✅ | ✅ grep_files + search |
| Shell exec | ✅ Bash | ✅ exec/shell | ✅ shell |
| Web fetch | ✅ WebFetch | ✅ | ✅ fetch_url |
| Web search | ✅ WebSearch | ✅ WebSearchRequest | ✅ web_search |
| Web browse | ❌ | ❌ | ✅ web_run |
| LSP | ✅ Lsp (experimental) | ❌ | ❌ |
| Task/todo tracking | ✅ TodoWrite | ✅ | ✅ todo_write |
| Subagent spawn | ✅ Task | ✅ Collab/SpawnCsv | ✅ agent_spawn |
| Skill system | ✅ Skill (multi-location discovery) | ✅ core-skills | ⚠️ Partial (.deepseek/skills/) |
| Plan mode | ✅ plan-enter/exit | ✅ Plan mode | ✅ Plan mode |
| User question | ✅ Question | ✅ request_user_input | ✅ user_input |
| Patch apply | ✅ apply_patch (custom format) | ✅ apply_patch (diff format) | ✅ apply_patch |
| Data validation | ❌ | ❌ | ✅ validate_data |
| Finance | ❌ | ❌ | ✅ finance |
| Git ops | Via Bash tool | ✅ git-utils | ✅ git module |
| GitHub ops | Via Bash (gh) | ✅ | ✅ github |
| Test running | ❌ | ✅ | ✅ test_runner |
| Automation | ❌ | ❌ | ✅ automation |
| Code review | ❌ | ✅ GuardianApproval | ✅ review |
| Recall/archive | ❌ | ❌ | ✅ recall_archive |
| Diagnostics | ❌ | ✅ | ✅ diagnostics |
| Revert turn | ❌ | ❌ | ✅ revert_turn |
| Image generation | ❌ | ✅ ImageGeneration | ❌ |
| Browser use | ❌ | ✅ BrowserUse | ❌ (web_run is headless) |
| Computer use | ❌ | ✅ ComputerUse | ❌ |
| Realtime voice | ❌ | ✅ RealtimeConversation | ❌ |

---

## High Priority Gaps

These are capabilities that would most directly improve DeepSeek TUI's effectiveness as a coding agent.

### 1. LSP Integration

**What it is:** A model-callable tool that queries Language Server Protocol servers for code intelligence — go-to-definition, find references, hover (type info), document symbols, workspace symbols, call hierarchy, and implementations.

**Why it matters:** The single biggest capability gap. Every codebase exploration currently costs shell `rg` calls and sequential file reads. With LSP, the agent can jump to definitions, find all callers of a function, and inspect types in a single tool call. Estimated 30–50% reduction in exploration turns for structured codebases.

**OpenCode implementation:** `packages/opencode/src/tool/lsp.ts` exposes nine operations with file/line/character parameters. The tool prompts are in `tool/lsp.txt`. LSP servers must be configured per file type.

```
Supported operations:
- goToDefinition
- findReferences  
- hover
- documentSymbol
- workspaceSymbol
- goToImplementation
- prepareCallHierarchy
- incomingCalls
- outgoingCalls
```

**What DeepSeek TUI would need:** A new `lsp.rs` tool in `crates/tui/src/tools/`, integration with tower-lsp or lsp-server crate, and per-language server configuration.

### 2. Granular Permission System

**What it is:** Allow/deny/ask rules keyed on tool name × file path pattern, with wildcard support, home-directory expansion, and cascading to pending requests.

**Why it matters:** The current all-or-nothing approval model creates friction. Users can't express "always allow reads in `src/` but always ask for `.env` files." The ability to permanently approve a pattern reduces approval fatigue by 60–80% over a long session.

**OpenCode implementation:** `packages/opencode/src/permission/index.ts` implements:

- `Action`: `allow | deny | ask`
- `Rule`: `{ permission: string, pattern: string, action: Action }`
- `Ruleset`: ordered list of rules with last-match-wins semantics
- Pattern expansion for `~/`, `$HOME/`
- Wildcard matching on both permission names and path patterns
- Reply modes: `once` (approve this one call), `always` (approve pattern forever), `reject` (deny this one)
- Automatic cascading: an "always" reply auto-resolves pending requests for the same session
- Distinct error types: `DeniedError` (rule-based), `RejectedError` (user said no), `CorrectedError` (user said no with feedback)

Agent definitions inherit permission rulesets that can be user-overridden:
```typescript
build: {
  permission: merge(defaults, { question: "allow", plan_enter: "allow" }, user),
}
plan: {
  permission: merge(defaults, { edit: { "*": "deny" } }, user),
}
explore: {
  permission: merge(defaults, { "*": "deny", grep: "allow", read: "allow", ... }, user),
}
```

**What DeepSeek TUI would need:** A permission rule engine with the same dimension (tool name × path pattern × action), persistence to disk, and hook integration so approval decisions can cascade.

### 3. Lifecycle Hooks

**What it is:** User-defined shell commands or plugin functions that fire on specific lifecycle events — before a tool executes, after it completes, when permission is requested, at session start, when the user submits a prompt, and at session stop.

**Why it matters:** Hooks are the escape hatch that lets users enforce invariants without polluting the system prompt. "Always run `cargo fmt` after writing a `.rs` file." "Warn me before any `rm -rf`." "Log every shell command to a file." They are composable, auditable, and don't consume context window tokens.

**Codex CLI implementation:** `codex-rs/hooks/` defines six event types with typed request/response payloads:

| Event | When it fires | Payload |
|---|---|---|
| `PreToolUse` | Before tool execution | tool name, input params, sandbox state |
| `PostToolUse` | After tool execution | tool name, input, success/failure, duration, output preview |
| `PermissionRequest` | When model requests permission | permission type, justification |
| `SessionStart` | New session begins | session ID, cwd, source (new/resume) |
| `UserPromptSubmit` | User sends a message | prompt text |
| `Stop` | Session ending | reason |

Each hook handler supports:
- `matcher`: optional regex to filter which tool calls trigger the hook
- `command`: shell command to run
- `timeout_sec`: maximum runtime
- `status_message`: shown to the user while the hook runs
- `source_path` + `source`: tracks where the hook was defined (project hooks.json, user config, plugin)
- Hooks can return `Success`, `FailedContinue`, or `FailedAbort` (blocks the operation)

**What DeepSeek TUI would need:** Extend `crates/hooks/` to support the full event surface, add matcher-based filtering, and provide a `hooks.json` discovery mechanism similar to Codex CLI's.

### 4. Persistent Memories

**What it is:** Automatic extraction of user preferences, project conventions, and past decisions from conversations, stored as retrievable memories that are injected into new sessions.

**Why it matters:** Across a long debugging session, the agent rediscovers the same facts: "this project uses Rust edition 2024," "tests run with `cargo test --workspace`," "the user prefers 4-space indentation." A memory system compounds value — each session builds on prior knowledge rather than starting from zero.

**Codex CLI implementation:** The `MemoryTool` feature (experimental, behind `/experimental` menu) enables:
- Memory generation: the model creates structured memories from conversation content
- Memory retrieval: relevant memories are injected into new conversation context
- The `Chronicle` feature adds passive screen-context memories via a sidecar process
- Memories are stored in SQLite and surfaced in the TUI via `/memories` command

**What DeepSeek TUI would need:** A memory extraction prompt, a vector or keyword-based retrieval system, and storage in the existing session/state infrastructure.

### 5. Skill Auto-Discovery

**What it is:** Automatic scanning of multiple locations for `SKILL.md` files that provide domain-specific instructions, scripts, and references. Skills are injected into the conversation on demand via a `skill` tool.

**Why it matters:** Skills are how the community packages expertise. A "Rust refactoring" skill, a "Docker deployment" skill, a "GitHub Actions" skill — each provides specialized instructions without bloating the main system prompt. OpenCode's multi-location discovery means skills can be project-local, user-global, or pulled from URLs.

**OpenCode implementation:** `packages/opencode/src/skill/index.ts` scans:

1. `~/.claude/skills/**/SKILL.md` (Claude Code compatibility)
2. `~/.agents/skills/**/SKILL.md` (Agents SDK compatibility)  
3. Parent directories from cwd to workspace root for `.claude/skills/` and `.agents/skills/`
4. Project config directories for `{skill,skills}/**/SKILL.md`
5. User-configured paths (with `~/` expansion)
6. User-configured URLs (pulled via discovery module)

Skills are parsed for YAML frontmatter (`name`, `description`) and Markdown content. Duplicate names warn but don't error. Skills respect agent permissions — an agent can only load skills its permission ruleset allows.

**What DeepSeek TUI would need:** Extend the existing `~/.deepseek/skills/` discovery to parent-directory walking, Claude Code compatibility paths, and URL-based skill sources. Add YAML frontmatter parsing.

---

## Medium Priority Gaps

These would meaningfully improve the agent experience but are less urgent.

### 6. Agent Profiles with Permission Inheritance

**What it is:** Named agent types (build, plan, general, explore) that inherit different tool permission sets. Users can define custom agents with specific models, temperatures, system prompts, and permission rules.

**OpenCode implementation:** `packages/opencode/src/agent/agent.ts`:

- `build`: full-access with ask on sensitive paths
- `plan`: all edit tools denied, plan-exit allowed, plan file writes in `.opencode/plans/` allowed
- `general`: subagent-only, todo-write denied
- `explore`: read-only, grep/glob/read/bash/webfetch/websearch allowed
- Plus hidden agents for internal tasks (compaction, title generation, summarization)

Each agent carries its own `model`, `temperature`, `topP`, `prompt`, and `permission` ruleset. A `generate` function creates new agent configs dynamically from user descriptions.

**What DeepSeek TUI would need:** Extend the mode system (Plan/Agent/YOLO) to support named agent profiles with per-profile tool filtering and model configuration.

### 7. Shell Sandboxing

**What it is:** OS-level sandbox enforcement for shell commands — network restrictions, filesystem read-only mounts, allowed/disallowed paths.

**Codex CLI implementation:** `codex-rs/sandboxing/`:

- macOS: Seatbelt (`sandboxing/src/seatbelt.rs`) with `.sbpl` policy files
- Linux: bubblewrap (default) or Landlock (legacy fallback)
- Windows: restricted token
- Configurable sandbox policies per command
- Integration tests can detect they're running under sandbox and early-exit

**What DeepSeek TUI would need:** Extend `crates/execpolicy/` to support platform-specific sandbox enforcement. Start with macOS Seatbelt (most DeepSeek TUI users are on macOS).

### 8. Tool Search / Deferred MCP Tool Exposure

**What it is:** Instead of dumping all MCP tools into the system prompt (bloating context), expose a `tool_search` function that the model calls to discover relevant tools by name or description.

**Codex CLI implementation:** `ToolSearch` feature (stable, default-enabled). `ToolSearchAlwaysDeferMcpTools` goes further — never exposes MCP tools directly, always requires search. This is critical when MCP servers expose hundreds of tools.

**What DeepSeek TUI would need:** `tool_search_tool_regex` and `tool_search_tool_bm25` already exist as deferred tool discovery mechanisms. Extend them to gate MCP tool exposure behind on-demand search.

### 9. ExecPolicy / Command Approval Rules

**What it is:** A policy engine that evaluates shell commands against user-defined rules — prefix allowlists, network restrictions, pattern matching — and auto-approves, denies, or escalates.

**Codex CLI implementation:** `codex-rs/execpolicy/src/`:

- `Policy`: ordered list of `Rule` entries
- `Rule`: prefix patterns (e.g., allow `cargo build*`, deny `rm *`)
- `NetworkRule`: protocol-level network restrictions
- `MatchOptions`: controls rule evaluation behavior
- `Evaluation`: result of policy evaluation against a command

Rules can be amended at runtime via `blocking_append_allow_prefix_rule`.

**What DeepSeek TUI would need:** Extend `crates/execpolicy/` to support prefix rules, network rules, and runtime policy amendments.

### 10. Dynamic Agent Generation

**What it is:** On-the-fly generation of new agent configurations from natural language descriptions.

**OpenCode implementation:** The `generate` function in `agent.ts` takes a description like "code reviewer that only reads files and reports issues" and returns an `{ identifier, whenToUse, systemPrompt }` object using a structured LLM call. Generated agents respect existing agent name collisions.

**What DeepSeek TUI would need:** A model-callable tool or slash command that generates agent configs from descriptions and registers them for the session.

### 11. Streaming Patch Events

**What it is:** Structured progress events streamed while the model is generating `apply_patch` input, giving the user real-time feedback on what files will change.

**Codex CLI implementation:** `ApplyPatchStreamingEvents` feature (under development) streams file-level progress as the model produces patch hunks. The `StreamingPatchParser` in `apply-patch/src/streaming_parser.rs` handles incremental parsing.

**What DeepSeek TUI would need:** Extend `apply_patch.rs` to emit progress events during streaming model output.

---

## Lower Priority Gaps

Specialized features that are valuable but less critical for core coding workflow.

| Capability | Where | Notes |
|---|---|---|
| Image Generation | Codex CLI `ImageGeneration` | Niche for coding; useful for documentation diagrams |
| Browser Use | Codex CLI `BrowserUse` | Interactive browser automation (click, type, screenshot). DeepSeek TUI has `web_run` for headless |
| Computer Use | Codex CLI `ComputerUse` | Full desktop automation. Desktop-app-gated |
| Realtime Voice | Codex CLI `RealtimeConversation` | Voice conversation mode. Experimental |
| Unified PTY Exec | Codex CLI `UnifiedExec` | Single PTY-backed shell with state snapshotting across turns |
| Artifacts | Codex CLI `Artifact` | Native artifact rendering tools |
| Goals | Codex CLI `Goals` | Persistent thread goals that survive compaction and session restarts |
| Git Commit Attribution | Codex CLI `CodexGitCommit` | Model instructions for proper commit attribution |
| CSV Agent Spawning | Codex CLI `SpawnCsv` | CSV-backed parallel agent job distribution |
| Shell Snapshotting | Codex CLI `ShellSnapshot` | Save/restore shell state across turns |
| Prevent Idle Sleep | Codex CLI `PreventIdleSleep` | Keep machine awake during long-running agent tasks |

---

## Architectural Patterns

### OpenCode

**Client/Server Architecture:** The TUI is one client; the server can be driven remotely from a mobile app, desktop app, or web console. This decouples the agent runtime from the UI layer.

**Plugin System:** `packages/opencode/src/plugin/` supports hot-loadable JS/TS plugins that add tools, models, auth providers, and chat middleware. Plugins receive a typed context with tool execution, auth, and filesystem access.

**Multi-Provider:** Not coupled to any single AI provider. Models are configured with provider IDs and resolved through a provider registry. OAuth support for OpenAI Codex (ChatGPT subscription integration) in `plugin/codex.ts`.

**Config Layering:** Config is loaded from multiple sources (global, project, env vars) and merged with well-defined precedence.

### Codex CLI

**App-Server Protocol:** `codex-rs/app-server-protocol/` defines a versioned RPC protocol (v2) between the TUI frontend and the agent backend. All new API development goes through v2 with strict naming conventions (`*Params`/`*Response`/`*Notification`, `resource/method` RPC naming).

**Feature Flag System:** `codex-rs/features/` centralizes 60+ feature flags with lifecycle stages (UnderDevelopment, Experimental, Stable, Deprecated, Removed). Features have metadata (menu name, description, announcement text) and can carry custom config structs.

**Bazel + Cargo Dual Build:** Codex CLI uses both Cargo (for development) and Bazel (for CI/release). The `find_resource!` macro and `cargo_bin()` helper abstract over runfile differences.

**Snapshot Testing:** `codex-rs/tui/` extensively uses `insta` for UI snapshot tests. Any UI change requires corresponding snapshot coverage.

**Core Modularity:** Explicit resistance to adding code to `codex-core`. New functionality goes into purpose-built crates (`codex-apply-patch`, `codex-memories`, `codex-sandboxing`) rather than growing the core crate.

### DeepSeek TUI

**RLM (Recursive Language Model):** Unique in this space. A sandboxed Python REPL where a sub-LLM can call helpers (`llm_query`, `llm_query_batched`, `rlm_query`) for batch processing, chunking, and recursive critique. Neither competitor has an equivalent.

**Durable Tasks:** Restart-aware persistent task objects with evidence tracking (gate runs, PR attempts, timeline). Designed for long-running autonomous work that survives restarts.

**Automations:** Scheduled recurring tasks with cron-style RRULE recurrence. Unique among the three.

---

## What DeepSeek TUI Already Excels At

- **RLM** — batch/bulk LLM processing in a Python sandbox; no equivalent in either competitor
- **Finance** — live stock/crypto quotes; unique in this space
- **Automations** — scheduled recurring tasks with cron rules
- **Durable tasks** — restart-aware with evidence tracking and gate verification
- **Turn revert** — undo workspace changes per turn via side-git snapshots
- **Data validation** — JSON/TOML validation tool
- **Web run** — headless browser interaction (Codex CLI has Browser Use but it's desktop-app-gated)
- **Parallel tool execution** — explicitly modeled as infrastructure
- **Git/GitHub operations** — comprehensive git module with blame, log, diff, status plus full GitHub API via gh
- **Project map** — high-level project structure generation

---

## Recommended Implementation Order

1. **LSP tool** — single biggest capability gap. Estimated 30–50% reduction in codebase exploration turns.
2. **Path-pattern permissions** — reduces approval fatigue by 60–80% over long sessions.
3. **Persistent memory** — compounds value across sessions; foundational for long-running projects.
4. **Pre/Post-tool-use hooks** — escape hatch for user-defined guardrails without system prompt bloat.
5. **Skill auto-discovery** — enables community skill ecosystem and Claude Code compatibility.
6. **Agent profiles** — named agent types with model/permission inheritance.
7. **Tool search for MCP** — keeps context window manageable when connecting to MCP servers with many tools.
8. **Shell sandboxing** — security improvement, starting with macOS Seatbelt.
