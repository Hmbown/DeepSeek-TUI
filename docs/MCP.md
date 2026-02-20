# MCP (External Tool Servers)

Wagmii CLI can load additional tools via MCP (Model Context Protocol). MCP servers are local processes that the CLI starts and communicates with over stdio.

Server mode note:
- `wagmii serve --mcp` runs the MCP stdio server.
- `wagmii serve --http` runs the runtime HTTP/SSE API (separate mode).

## Bootstrap MCP Config

Create a starter MCP config at your resolved MCP path:

```bash
wagmii mcp init
```

`wagmii setup --mcp` performs the same MCP bootstrap alongside skills setup.

Common management commands:

```bash
wagmii mcp list
wagmii mcp tools [server]
wagmii mcp add <name> --command "<cmd>" --arg "<arg>"
wagmii mcp add <name> --url "http://localhost:3000/mcp"
wagmii mcp enable <name>
wagmii mcp disable <name>
wagmii mcp remove <name>
wagmii mcp validate
```

## Config File Location

Default path:

- `~/.wagmii/mcp.json`

Overrides:

- Config: `mcp_config_path = "/path/to/mcp.json"`
- Env: `WAGMII_MCP_CONFIG=/path/to/mcp.json`

`wagmii mcp init` (and `wagmii setup --mcp`) writes to this resolved path.

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

- Run `wagmii doctor` to confirm the MCP config path it resolved and whether it exists.
- If the MCP config is missing, run `wagmii mcp init --force` to regenerate it.
- If tools don’t appear, verify the server command works from your shell and that the server supports MCP `tools/list`.
