# MCP (External Tool Servers)

DeepSeek CLI can load additional tools via MCP (Model Context Protocol). MCP servers are local processes that the CLI starts and communicates with over stdio.

## Config File Location

Default path:

- `~/.deepseek/mcp.json`

Overrides:

- Config: `mcp_config_path = "/path/to/mcp.json"`
- Env: `DEEPSEEK_MCP_CONFIG=/path/to/mcp.json`

After editing the file, restart the TUI.

## Tool Naming

Discovered MCP tools are exposed to the model as:

- `mcp_<server>_<tool>`

Example: a server named `git` with a tool named `status` becomes `mcp_git_status`.

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

## Safety Caveat (Important)

MCP tools currently execute without TUI approval prompts. Only configure MCP servers you trust, and treat MCP server configuration as equivalent to running code on your machine.

## Troubleshooting

- Run `deepseek doctor` to confirm whether the default `~/.deepseek/mcp.json` exists.
- If you override `mcp_config_path` / `DEEPSEEK_MCP_CONFIG`, note that `deepseek doctor` still checks `~/.deepseek/mcp.json`.
- If tools don’t appear, verify the server command works from your shell and that the server supports MCP `tools/list`.

