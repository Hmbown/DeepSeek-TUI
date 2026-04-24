# Modes and Approvals

DeepSeek TUI has two related concepts:

- **TUI mode**: what kind of visible interaction you’re in (Plan/Agent/YOLO).
- **Approval mode**: how aggressively the UI asks before executing tools.

## TUI Modes

Press `Tab` to cycle through the visible modes: **Plan → Agent → YOLO → Plan**.
Press `Shift+Tab` to cycle in reverse.

- **Plan**: design-first prompting. Read-only investigation tools stay available, but shell and patch execution stay off.
- **Agent**: multi-step tool use. Approvals for shell and paid tools (file writes are allowed without a prompt).
- **YOLO**: enables shell + trust mode and auto-approves all tools. Use only in trusted repos.

## Compatibility Notes

- `/normal` is a hidden compatibility alias that switches to `Agent`.
- Older settings files with `default_mode = "normal"` still load as `agent`; saving rewrites the normalized value.

## Escape Key Behavior

`Esc` is a cancel stack, not a mode switch.

- Close slash menus or transient UI first.
- Cancel the active request if a turn is running.
- Discard a queued draft if the composer is empty.
- Clear the current input if text is present.
- Otherwise it is a no-op.

## Approval Mode

You can override approval behavior at runtime:

```text
/config
# edit the approval_mode row to: suggest | auto | never
```

Legacy note: `/set approval_mode ...` was retired in favor of `/config`.

- `suggest` (default): uses the per-mode rules above.
- `auto`: auto-approves all tools (similar to YOLO approval behavior, but without forcing YOLO mode).
- `never`: blocks any tool that isn’t considered safe/read-only.

## Small-Screen Status Behavior

When terminal height is constrained, the status area compacts first so header/chat/composer/footer remain visible:

- Loading and queued status rows are budgeted by available height.
- Queued previews collapse to compact summaries when full previews do not fit.
- `/queue` workflows remain available; compact status only affects rendering density.

## Workspace Boundary and Trust Mode

By default, file tools are restricted to the `--workspace` directory. Enable trust mode to allow file access outside the workspace:

```text
/trust
```

YOLO mode enables trust mode automatically.

## MCP Behavior

MCP tools are exposed as `mcp_<server>_<tool>` and use the same approval flow as built-in tools. Read-only MCP helpers may auto-run in suggestive approval modes; MCP tools with possible side effects require approval.

See `MCP.md`.

## Related CLI Flags

Run `deepseek --help` for the canonical list. Common flags:

- `-p, --prompt <TEXT>`: one-shot prompt mode (prints and exits)
- `--model <MODEL>`: when using the `deepseek` facade, forward a DeepSeek model override to the TUI
- `--workspace <DIR>`: workspace root for file tools
- `--yolo`: start in YOLO mode
- `-r, --resume <ID|PREFIX|latest>`: resume a saved session
- `-c, --continue`: resume the most recent session
- `--max-subagents <N>`: clamp to `1..=20`
- `--no-alt-screen`: run inline without the alternate screen buffer
- `--mouse-capture` / `--no-mouse-capture`: opt in or out of internal mouse scrolling/selection. Mouse capture is enabled by default when the alternate screen is active; use `--no-mouse-capture` when you need terminal-native drag selection.
- `--profile <NAME>`: select config profile
- `--config <PATH>`: config file path
- `-v, --verbose`: verbose logging
