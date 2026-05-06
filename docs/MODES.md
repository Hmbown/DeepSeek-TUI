# Modes and Approvals

DeepSeek TUI has two related concepts:

- **TUI mode**: what kind of visible interaction you're in (Plan/Agent/YOLO).
- **Approval mode**: how aggressively the UI asks before executing tools.

## TUI Modes

**Tab** completes composer menus, queues drafts as follow-ups while a turn runs,
or cycles through modes when the composer is idle: **Plan → Agent → YOLO → Plan**.
**Shift+Tab** cycles reasoning effort: off → high → max.

- **Plan**: design-first prompting. Investigation tools remain available; shell and patch execution are disabled. Use this to think through problems and create plans for review.
- **Agent**: multi-step tool use with approval gates. File writes are auto-approved; shell and API tools require confirmation.
- **YOLO**: auto-approves all tools and enables trust mode. Use only in trusted repositories.

All modes provide access to the `rlm` tool, a Python REPL where `llm_query_batched` runs 1–16 parallel child calls on `deepseek-v4-flash`. The model uses this for decomposable work.

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

- `suggest` (default): follows the per-mode rules above.
- `auto`: auto-approves all tools without entering YOLO mode.
- `never`: blocks any tool that is not read-only or safe.

## Small-Screen Status Behavior

When terminal height is constrained, the status area compacts first so header/chat/composer/footer remain visible:

- Loading and queued status rows are budgeted by available height.
- Queued previews collapse to compact summaries when full previews do not fit.
- `/queue` workflows remain available; compact status only affects rendering density.

## Workspace Boundary and Trust Mode

By default, file tools can only access the `--workspace` directory. Enable trust mode to allow access outside the workspace:

```text
/trust
```

YOLO mode enables trust mode automatically.

## MCP Behavior

MCP tools are exposed as `mcp_<server>_<tool>` and follow the same approval flow as built-in tools. Read-only MCP helpers may auto-run in suggest mode; tools with side effects require approval.

See `MCP.md`.

## Related CLI Flags

Run `deepseek --help` for the canonical list. Common flags:

- `-p, --prompt <TEXT>`: one-shot prompt mode (prints and exits)
- `--model <MODEL>`: when using the `deepseek` facade, forward a DeepSeek model override to the TUI
- `--workspace <DIR>`: workspace root for file tools
- `--yolo`: start in YOLO mode
- `-r, --resume <ID|PREFIX|latest>`: resume a saved session
- `-c, --continue`: resume the most recent session in this workspace
- `--max-subagents <N>`: clamp to `1..=20`
- `--no-alt-screen`: run inline without the alternate screen buffer
- `--mouse-capture` / `--no-mouse-capture`: opt in or out of internal mouse scrolling, transcript selection, and right-click context actions. Mouse capture is enabled by default on non-Windows terminals so drag selection copies only user/assistant transcript text; hold Shift while dragging or use `--no-mouse-capture` for raw terminal selection. On Windows it defaults off to avoid CMD/terminal mouse escape sequences being inserted into the prompt; use `--mouse-capture` to opt in.
- `--profile <NAME>`: select config profile
- `--config <PATH>`: config file path
- `-v, --verbose`: verbose logging
