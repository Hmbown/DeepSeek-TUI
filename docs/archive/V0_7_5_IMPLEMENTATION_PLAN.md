# v0.7.5 Implementation Plan

Scope: background shell job UX, in-TUI MCP management/discovery, and V4
context/cache policy. Do not include provider expansion or Whalescale
rename/migration work in this release lane.

## Context/cache decision

Default path:

- Keep the transcript append-only and preserve the stable prefix for DeepSeek V4 cache reuse.
- Disable replacement-style `auto_compact` by default.
- Keep replacement compaction manual or late: if a user enables `auto_compact`, V4 compacts only near the 80% model-window guard (`800000` tokens for 1M-context models), not at reasoning-effort soft caps.
- Keep the Flash seam manager (`[context].enabled`) opt-in until issue #200 has repeatable cache-hit/miss evidence.
- Keep the capacity controller disabled by default. Treat it as telemetry or an experimental guardrail unless `capacity.enabled = true` is set.
- Use emergency overflow recovery only when the request would otherwise exceed the model input budget.

Rationale: V4's 1M-token window and prefix-cache economics make early
replacement compaction suspect. The first shippable slice should prevent old
128K-era heuristics from rewriting context before there is evidence that the
rewrite is cheaper and more reliable than preserving a hot prefix.

## Shippable slices

### Slice 1: Context policy and docs

- Change default `auto_compact` to off.
- Keep V4 replacement-compaction thresholds late and independent of reasoning effort.
- Make `[context].enabled` default to false.
- Make `docs/CONFIGURATION.md`, `docs/capacity_controller.md`, and `config.example.toml` match code defaults.
- Add focused tests for defaults and V4 threshold behavior.

### Slice 2: Background shell job center (#195)

- Add a job-center view fed by `ShellManager::list()`.
- Show command, cwd, linked task id when available, status, elapsed time, exit code, and latest output.
- Add controls to inspect full output, poll latest output, send stdin for PTY/stdin-capable jobs, kill a background job, and attach completed output as task evidence.
- Mark restart-stale jobs explicitly rather than presenting them as live.
- Add lifecycle tests for start, poll, cancel, complete, stale/restart, plus TUI snapshots for running and completed job details.

### Slice 3: MCP manager (#196)

- Add `/mcp` or a command-palette action that opens an MCP manager view.
- Show resolved config path, server enabled/disabled state, transport, command/url, timeout settings, startup errors, and discovered tool/resource/prompt counts.
- Wire `mcp_config_path` into the interactive config surface.
- Support init, add stdio server, add HTTP/SSE server, enable, disable, remove, validate, reconnect, and inspect tools/resources/prompts.
- Preserve both `servers` and `mcpServers` config shapes.

### Slice 4: MCP discoverability (#197)

- Add an MCP command-palette section backed by the same discovery state as the manager.
- Group tools/resources/prompts by server.
- Show disabled/failed servers without blocking palette rendering.
- Keep model-visible names consistent with `mcp_<server>_<tool>`.

## Stop rules

- Do not close #159 or #162 unless a verified PR actually resolves them.
- Do not add provider expansion.
- Do not rename or migrate anything to Whalescale.
- Do not broaden the TUI into a large redesign; each slice should remain independently testable and shippable.
