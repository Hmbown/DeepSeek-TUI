# MCP (External Tool Servers)

DeepSeek TUI can load additional tools via MCP (Model Context Protocol). MCP servers are local processes that the TUI starts and communicates with over stdio.

Browsing note:
- `web.run` is the canonical built-in browsing tool.
- `web_search` remains available as a compatibility alias for older prompts and integrations.

Server mode note:
- `deepseek-tui serve --mcp` runs the MCP stdio server.
- `deepseek-tui serve --http` runs the runtime HTTP/SSE API (separate mode).
- The `deepseek` dispatcher exposes `deepseek mcp-server` as an equivalent stdio
  entrypoint used by the split CLI.

## Bootstrap MCP Config

Create a starter MCP config at your resolved MCP path:

```bash
deepseek-tui mcp init
```

`deepseek-tui setup --mcp` performs the same MCP bootstrap alongside skills setup.

Common management commands:

```bash
deepseek-tui mcp list
deepseek-tui mcp tools [server]
deepseek-tui mcp add <name> --command "<cmd>" --arg "<arg>"
deepseek-tui mcp add <name> --url "http://localhost:3000/mcp"
deepseek-tui mcp enable <name>
deepseek-tui mcp disable <name>
deepseek-tui mcp remove <name>
deepseek-tui mcp validate
```

## Config File Location

Default path:

- `~/.deepseek/mcp.json`

Overrides:

- Config: `mcp_config_path = "/path/to/mcp.json"`
- Env: `DEEPSEEK_MCP_CONFIG=/path/to/mcp.json`

`deepseek-tui mcp init` (and `deepseek-tui setup --mcp`) writes to this resolved path.

After editing the file, restart the TUI.

## Tool Naming

Discovered MCP tools are exposed to the model as:

- `mcp_<server>_<tool>`

Example: a server named `git` with a tool named `status` becomes `mcp_git_status`.

## Resource and Prompt Helpers

The CLI also exposes helper tools when MCP is enabled:

- `list_mcp_resources` (optional `server` filter)
- `list_mcp_resource_templates` (optional `server` filter)
- `mcp_read_resource` / `read_mcp_resource` (aliases)
- `mcp_get_prompt`

## Minimal Example

```json
{
  "timeouts": {
    "connect_timeout": 10,
    "execute_timeout": 60,
    "read_timeout": 120
  },
  "servers": {
    "example": {
      "command": "node",
      "args": ["./path/to/your-mcp-server.js"],
      "env": {},
      "disabled": false
    }
  }
}
```

You can also use `mcpServers` instead of `servers` for compatibility with other clients.

## Running DeepSeek as an MCP Server

You can register your local DeepSeek binary as an MCP server so other DeepSeek sessions (or any MCP client) can call its tools.

### Quick Setup

```bash
deepseek-tui mcp add-self
```

This resolves the current binary path, generates a config entry that runs `deepseek-tui serve --mcp`, and writes it to your MCP config file. The default server name is `deepseek`.

Options:

- `--name <NAME>` — custom server name (default: `deepseek`)
- `--workspace <PATH>` — workspace directory for the server

### Manual Config

Equivalent manual entry in `~/.deepseek/mcp.json`:

```json
{
  "servers": {
    "deepseek": {
      "command": "/path/to/deepseek",
      "args": ["serve", "--mcp"],
      "env": {}
    }
  }
}
```

The `deepseek-tui` binary supports `serve --mcp` directly. The `deepseek`
dispatcher offers the equivalent `deepseek mcp-server` stdio entrypoint. Use
whichever is on your `PATH` (run `which deepseek` or `which deepseek-tui` to
find the full path). The `mcp add-self` command automatically resolves the
correct binary.

### Prerequisites

- The binary referenced in `command` must exist and be executable.
- The MCP server runs as a child process via stdio — no network ports required.
- Each MCP client session spawns its own server process.

### Tool Naming

Tools from a self-hosted DeepSeek server follow the standard naming convention:

- `mcp_deepseek_<tool>` (if the server is named `deepseek`)

For example, the `shell` tool becomes `mcp_deepseek_shell`.

### MCP Server vs HTTP/SSE API

| | `deepseek-tui serve --mcp` | `deepseek-tui serve --http` |
|---|---|---|
| **Protocol** | MCP stdio | HTTP/SSE JSON-RPC |
| **Use case** | Tool server for MCP clients | Runtime API for apps |
| **Config** | `~/.deepseek/mcp.json` entry | Direct URL connection |
| **Lifecycle** | Spawned per client session | Long-running daemon |

Use `mcp add-self` when you want DeepSeek tools available to other MCP clients. Use `serve --http` when building applications that consume the API directly.

### Verification

After adding, test the connection:

```bash
deepseek-tui mcp validate
deepseek-tui mcp tools deepseek
```

## Server Fields

Per-server settings:

- `command` (string, required)
- `args` (array of strings, optional)
- `env` (object, optional)
- `connect_timeout`, `execute_timeout`, `read_timeout` (seconds, optional)
- `disabled` (bool, optional)
- `enabled` (bool, optional, default `true`)
- `required` (bool, optional): startup/connect validation fails if this server cannot initialize.
- `enabled_tools` (array, optional): allowlist of tool names for this server.
- `disabled_tools` (array, optional): denylist applied after `enabled_tools`.

## Safety Notes

MCP tools now flow through the same tool-approval framework as built-in tools. Read-only MCP helpers (resource/prompt listing and reads) can run without prompts in suggestive approval modes, while side-effectful MCP tools require approval.

You should still only configure MCP servers you trust, and treat MCP server configuration as equivalent to running code on your machine.

## Troubleshooting

- Run `deepseek-tui doctor` to confirm the MCP config path it resolved and whether it exists.
- If the MCP config is missing, run `deepseek-tui mcp init --force` to regenerate it.
- If tools don’t appear, verify the server command works from your shell and that the server supports MCP `tools/list`.
