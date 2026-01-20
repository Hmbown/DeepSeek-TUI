# Modes and Approvals

DeepSeek CLI has two related concepts:

- **TUI mode**: what kind of interaction you’re in (Normal/Plan/Agent/YOLO/RLM).
- **Approval mode**: how aggressively the UI asks before executing tools.

## TUI Modes

Press `Tab` to cycle: **Normal → Plan → Agent → YOLO → RLM → Normal**.

- **Normal**: chat-first. Approvals for file writes, shell, and paid tools.
- **Plan**: design-first prompting. Approvals match Normal.
- **Agent**: multi-step tool use. Approvals for shell and paid tools (file writes are allowed without a prompt).
- **YOLO**: enables shell + trust mode and auto-approves all tools. Use only in trusted repos.
- **RLM**: externalized context store + REPL helpers. Tools are auto-approved (best for large files and long-context work).

## Approval Mode

You can override approval behavior at runtime:

```text
/set approval_mode suggest
/set approval_mode auto
/set approval_mode never
```

- `suggest` (default): uses the per-mode rules above.
- `auto`: auto-approves all tools (similar to YOLO/RLM approval behavior, but without forcing YOLO mode).
- `never`: blocks any tool that isn’t considered safe/read-only.

## Workspace Boundary and Trust Mode

By default, file tools are restricted to the `--workspace` directory. Enable trust mode to allow file access outside the workspace:

```text
/trust
```

YOLO mode enables trust mode automatically.

## MCP Caveat (Important)

MCP tools are exposed as `mcp_<server>_<tool>` and currently execute without TUI approval prompts. Only configure MCP servers you trust.

See `MCP.md`.

## Related CLI Flags

Run `deepseek --help` for the canonical list. Common flags:

- `-p, --prompt <TEXT>`: one-shot prompt mode (prints and exits)
- `--workspace <DIR>`: workspace root for file tools
- `--yolo`: start in YOLO mode
- `-r, --resume <ID|PREFIX|latest>`: resume a saved session
- `-c, --continue`: resume the most recent session
- `--max-subagents <N>`: clamp to `1..=5`
- `--profile <NAME>`: select config profile
- `--config <PATH>`: config file path
- `-v, --verbose`: verbose logging

