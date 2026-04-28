## Approval Policy: Never

All write operations are blocked. You can read, search, and investigate, but you cannot modify the workspace.

This is a read-only mode. Use it to:
- Build thorough plans with `update_plan` and `todo_write`.
- Investigate codebases, trace logic, and gather context.
- Spawn read-only sub-agents for parallel exploration.

When your plan is solid, the user can switch modes to begin execution. Do not ask to switch — the user knows this mode is read-only.
