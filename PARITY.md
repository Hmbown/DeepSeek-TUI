# Parity Spec v2: Codex Harness (2026-02-03)

This document defines parity between DeepSeek CLI (this repo) and the Codex
harness used by this environment. It is intentionally concrete and testable.

## Scope

Parity is evaluated across:

- Tool surface (capabilities and availability)
- Behavioral protocol (when and how tools are used, reporting rules)
- UX/workflow (approvals, prompts, and interaction flows)

## Non-goals

- OAuth or vendor-specific auth flows
- Model quality or response style beyond defined behavioral rules
- Exact tool names when equivalent capabilities exist

## Baseline: Codex Harness Capabilities

The Codex harness baseline (as of 2026-02-03) includes:

- File ops: read/write/edit/patch
- Shell execution with streaming and optional PTY input
- Web browsing via `web.run` (search/open/click/find/screenshot)
- Structured data tools: weather, finance, sports, time, calculator
- Image search via `image_query`
- Multi-tool parallel execution wrapper
- User-input prompts (multiple-choice + free-form)
- MCP resource listing/reading and prompt retrieval
- Sub-agent control (spawn, send_input, wait, close)
- Planning tool (`update_plan`)

## Tool Surface Parity Matrix

| Capability | Codex Harness | DeepSeek CLI (current) | Status | Notes |
| --- | --- | --- | --- | --- |
| File ops | read/write/edit/list | read_file/write_file/edit_file/list_dir | Parity | - |
| Patch apply | apply_patch | apply_patch | Parity | - |
| Code search | rg via shell | grep_files, file_search, exec_shell | Parity | - |
| Shell exec | exec_command + write_stdin | exec_shell | Parity | PTY + stdin streaming via exec_shell_wait/exec_shell_interact |
| Web search/browse | web.run (search/open/click/find/screenshot) | web.run + web_search | Partial | web.run implemented; citations via prompts only (no word-limit enforcement) |
| Image search | image_query | missing | Missing | - |
| Structured data | weather/finance/sports/time/calculator | weather/finance/sports/time/calculator | Partial | Uses public data sources; coverage may vary by league/market |
| Multi-tool parallel | multi_tool_use.parallel | multi_tool_use.parallel | Partial | Read-only tools only; no MCP tools |
| User input tool | request_user_input | request_user_input | Parity | - |
| MCP resources | list/read resources + get prompt | list_mcp_resources, list_mcp_resource_templates, mcp_read_resource, mcp_get_prompt | Parity | - |
| Sub-agents | spawn/send_input/wait/close | agent_spawn/send_input/wait/agent_cancel/agent_list/agent_swarm | Partial | send_input/wait added; close maps to agent_cancel |
| Planning tool | update_plan | update_plan | Parity | - |

## Behavioral Protocol Parity

Codex harness requires these behaviors to be enforced by prompts or code:

- Instruction hierarchy and scope compliance (AGENTS.md, user constraints)
- Use web tools for time-sensitive or uncertain facts, with citations
- Dedicated tools for weather/finance/sports/time when asked
- Citation format and placement rules, including quote limits
- Use plan tool for multi-step tasks and update after steps
- Report validation commands and outcomes for code changes
- Avoid destructive git commands unless explicitly requested

These rules are parity-critical even when tool surface is similar.

Citation format (current): `[cite:ref_id]` using the `ref_id` returned by `web.run`.

## UX/Workflow Parity Targets

- Approval gating for file writes and shell execution
- Trust/workspace boundary controls
- Tool-call progress and results surfaced in the UI
- User input prompt UI (for request_user_input)
- Clear, reproducible reporting with clickable file references

## Gap Backlog (Prioritized)

1. Add image_query tool (image search parity)
2. Enforce web.run citation placement/quote limits in prompts or tooling
3. Expand structured data coverage for edge leagues/markets
4. Allow multi_tool_use.parallel to include MCP tools (where safe)

## Parity Gates (Acceptance)

Hard gates:

- Tool surface gaps 1-4 closed
- No destructive git commands on eval tasks
- Validation commands executed and reported

Soft gates:

- Parity score >= 0.8 across the matrix
- UX parity items covered in at least 2 eval tasks each
