# MCP Integration: context-mode + claude-mem + superpowers

**Date:** 2026-05-06
**Status:** draft
**Scope:** Enable DeepSeek TUI to connect to existing MCP servers (claude-mem, context-mode, superpowers) via the built-in MCP client.

## Problem

DeepSeek TUI is a Claude Code replacement that talks to DeepSeek models via a full-featured terminal UI. It already has a complete MCP client implementation (`crates/tui/src/mcp.rs`, ~2100 lines) but it only reads MCP server config from a workspace-level file (`.deepseek/mcp.json`). The user has significant context built up in claude-mem (cross-session memory) and context-mode (context savings), and wants those tools available inside DeepSeek TUI sessions — not just Claude Code sessions.

The MCP servers already exist as standalone stdio binaries. The MCP client already exists and is fully spec-compliant. The gap is purely configuration and wiring.

## Architecture

### Current state

```
┌─────────────────────────────────┐
│ DeepSeek TUI                    │
│  ┌───────────────────────────┐  │
│  │ Engine                    │  │
│  │  ┌─────────────────────┐  │  │
│  │  │ McpPool             │  │  │   reads mcp.json from
│  │  │  McpConnection ──────────►   workspace/.deepseek/mcp.json
│  │  │  StdioTransport     │  │  │
│  │  │  SseTransport       │  │  │
│  │  └─────────────────────┘  │  │
│  │  ┌─────────────────────┐  │  │
│  │  │ ToolRegistry        │  │  │
│  │  │  McpToolAdapter ────┼──┼──► wraps MCP tools as native tools
│  │  └─────────────────────┘  │  │
│  └───────────────────────────┘  │
└─────────────────────────────────┘
```

The MCP client already supports:
- `initialize` → `notifications/initialized` handshake
- `tools/list` with `inputSchema` per spec
- `tools/call` with `{ content: [...], isError }` per spec
- `resources/list`, `resources/read`
- `prompts/list`, `prompts/get`
- `StdioTransport`: spawns child process, newline-delimited JSON-RPC on stdin/stdout
- `SseTransport`: HTTP POST + SSE stream for server-to-client messages
- Per-server timeouts, network policy gating, graceful shutdown

### Target state

```
┌──────────────────────────────────────┐
│ ~/.deepseek/mcp.json (global)        │  NEW — fallback/merge with workspace
│                                        │
│ "mcpServers": {                       │
│   "claude-mem": {                     │
│     "command": "bun",                 │
│     "args": ["/path/to/mcp-server"]   │
│   },                                  │
│   "context-mode": {                   │
│     "command": "node",                │
│     "args": ["/path/to/start.mjs"],   │
│     "env": {                          │
│       "CONTEXT_MODE_PROJECT_DIR":     │
│         "${WORKSPACE}"                │  dynamic per workspace
│     }                                 │
│   }                                   │
│ }                                     │
└──────────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────┐
│ DeepSeek TUI                    │
│  ┌───────────────────────────┐  │
│  │ Engine                    │  │
│  │  McpPool                  │  │   reads BOTH:
│  │    ~/.deepseek/mcp.json ──┼──►  1. ~/.deepseek/mcp.json (global)
│  │    .deepseek/mcp.json ────┼──►  2. workspace/.deepseek/mcp.json
│  │                           │  │     workspace overrides global
│  │  ┌─────────────────────┐  │  │
│  │  │ StdioTransport      │  │  │
│  │  │  child: node/bun ────────► node /path/to/context-mode/start.mjs
│  │  │  stdin/stdout pipes │  │  │   bun /path/to/claude-mem/mcp-server.cjs
│  │  └─────────────────────┘  │  │
│  └───────────────────────────┘  │
└─────────────────────────────────┘
```

## Changes

### 1. Global MCP config support

**File:** `crates/tui/src/mcp.rs` (McpPool)

Add a second config load path. `McpPool::from_config_path` currently takes a single path. Add a new constructor that merges global + workspace configs:

```rust
impl McpPool {
    /// Merge global config (if present) with workspace config.
    /// Workspace entries override global entries with the same server name.
    pub fn from_workspace_config(workspace_config_path: &Path) -> Result<Self> {
        let global_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".deepseek")
            .join("mcp.json");
        let mut merged = if global_path.exists() {
            McpConfig::from_path(&global_path).unwrap_or_default()
        } else {
            McpConfig::default()
        };
        if workspace_config_path.exists() {
            let workspace_cfg = McpConfig::from_path(workspace_config_path)?;
            merged.servers.extend(workspace_cfg.servers);
            // workspace overrides
            for (name, server) in workspace_cfg.servers {
                merged.servers.insert(name, server);
            }
        }
        Ok(Self::new(merged))
    }
}
```

Call site change in `crates/tui/src/core/engine.rs`:

```rust
// Before:
let mut pool = McpPool::from_config_path(&self.session.mcp_config_path)?;

// After:
let mut pool = McpPool::from_workspace_config(&self.session.mcp_config_path)?;
```

**Rationale:** Users want memory/context tools in every project. Workspace configs can add project-specific MCP servers (e.g., a database MCP server for a specific repo). Global config ensures claude-mem and context-mode are always available.

### 2. Version-independent wrapper script

Plugin cache paths include version numbers that change on upgrade (e.g., `claude-mem/12.6.5/` → `claude-mem/12.7.0/`). Rather than updating `mcp.json` on every upgrade, ship a small shell script that resolves the latest version:

```bash
#!/bin/bash
# ~/.deepseek/mcp-resolve.sh — resolves latest plugin version
# Usage: mcp-resolve.sh <plugin-name> <script-path>
#   plugin-name: "claude-mem" or "context-mode"
#   script-path: relative path inside the version dir

PLUGIN_CACHE="$HOME/.claude/plugins/cache"

case "$1" in
  claude-mem)
    latest=$(ls -d "$PLUGIN_CACHE/thedotmack/claude-mem/"*/ 2>/dev/null | sort -V | tail -1)
    ;;
  context-mode)
    latest=$(ls -d "$PLUGIN_CACHE/context-mode/context-mode/"*/ 2>/dev/null | sort -V | tail -1)
    ;;
esac

if [ -n "$latest" ]; then
  echo "${latest}$2"
else
  echo "Error: $1 not found" >&2
  exit 1
fi
```

mcp.json entries reference the wrapper:

```json
{
  "mcpServers": {
    "claude-mem": {
      "command": "bun",
      "args": ["$(~/.deepseek/mcp-resolve.sh claude-mem scripts/mcp-server.cjs)"]
    }
  }
}
```

**Alternative considered:** Direct paths with hardcoded versions. Rejected — breaks on every plugin update.

### 3. CONTEXT_MODE_PROJECT_DIR env var

context-mode needs to know which project it operates on (for its FTS5 knowledge base, session DB, etc.). The MCP client already supports per-server env vars in `McpServerConfig`. The config just needs to include it.

For the global config, we need a dynamic value — the current workspace. This requires either:
- The wrapper script to accept a `--project-dir` argument and set the env var internally
- Or the MCP client to inject `CONTEXT_MODE_PROJECT_DIR` automatically for context-mode connections

**Decision:** Pass it as an env var pointing at a wrapper. Simpler, no code changes:

```json
{
  "context-mode": {
    "command": "bash",
    "args": ["-c", "CONTEXT_MODE_PROJECT_DIR=$(pwd) exec node $(~/.deepseek/mcp-resolve.sh context-mode start.mjs)"]
  }
}
```

But this breaks on Windows. **Better approach:** add automatic injection in `StdioTransport` or `McpConnection::connect_with_policy` — if the server name is "context-mode", inject `CONTEXT_MODE_PROJECT_DIR` from the current workspace. This is a single-line change in `mcp.rs` and works cross-platform.

### 4. mcp.json template

Ship `config.example.mcp.json` in the repo root with commented examples:

```json
{
  "timeouts": {
    "connect_timeout": 10,
    "execute_timeout": 60,
    "read_timeout": 120
  },
  "mcpServers": {
    "claude-mem": {
      "command": "bash",
      "args": ["-c", "exec bun $(~/.deepseek/mcp-resolve.sh claude-mem scripts/mcp-server.cjs)"]
    },
    "context-mode": {
      "command": "bash",
      "args": ["-c", "exec node $(~/.deepseek/mcp-resolve.sh context-mode start.mjs)"]
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/dir"]
    }
  }
}
```

### 5. Protocol version bump

The MCP client sends `"protocolVersion": "2024-11-05"`. This should be bumped to `"2025-03-26"` to match the current spec. Both context-mode and claude-mem target 2025-03-26.

**File:** `crates/tui/src/mcp.rs`, line 660

## What does NOT change

- `crates/mcp/` — the legacy stdio MCP server. Unrelated to client-side MCP.
- `McpConnection::initialize()` — handshake is correct per spec.
- `McpToolAdapter` — wrapping MCP tools as native ToolSpec impls is correct.
- `ToolRegistry` registration — MCP tools flow through the existing pipeline.
- Tool dispatch in `turn_loop.rs` — `mcp__server__tool` qualified dispatch is correct.
- `McpPool::call_tool()` — the dispatch path is correct.
- `config.example.toml` — MCP config lives in mcp.json, separate from the main config.

## Files modified

| File | Change | Lines |
|------|--------|-------|
| `crates/tui/src/mcp.rs` | Add `from_workspace_config`, auto-inject `CONTEXT_MODE_PROJECT_DIR` for context-mode, bump protocol version | ~30 |
| `crates/tui/src/core/engine.rs` | Call `from_workspace_config` instead of `from_config_path` | ~3 |
| `config.example.mcp.json` | New — commented MCP server config template | ~30 |
| `scripts/mcp-resolve.sh` | New — version-independent plugin path resolver | ~20 |

## Testing

1. **Unit:** `McpPool::from_workspace_config` merges global + workspace correctly (workspace overrides)
2. **Integration:** Start `deepseek`, verify `mcp__claude_mem__search` and `mcp__context_mode__ctx_search` appear in available tools
3. **End-to-end:** Call `mcp__claude_mem__search` with a query, verify it returns past observation data
4. **Upgrade resilience:** Bump a plugin version, verify `mcp-resolve.sh` resolves the new path
5. **Protocol compat:** Verify handshake succeeds against context-mode and claude-mem MCP servers

## Open questions

1. **superpowers MCP server:** Does superpowers have a standalone stdio MCP server? It may be a collection of skills (markdown files + Skill tool invocations) rather than an MCP server. If not, it may need a thin MCP wrapper or be loaded differently (e.g., as a skill directory).

2. **Windows support:** `mcp-resolve.sh` is bash. On Windows, need a PowerShell equivalent or embed the resolution logic in Rust.

3. **Mixed transport:** context-mode and claude-mem only support stdio. If the user wants to connect to HTTP/SSE MCP servers, those are already supported by the SSE transport — just configure `"url"` instead of `"command"`.
