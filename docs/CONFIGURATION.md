# Configuration

Wagmii CLI reads configuration from a TOML file plus environment variables.

## Where It Looks

Default config path:

- `~/.wagmii/config.toml`

Overrides:

- CLI: `wagmii --config /path/to/config.toml`
- Env: `WAGMII_CONFIG_PATH=/path/to/config.toml`

If both are set, `--config` wins. Environment variable overrides are applied after the file is loaded.

To bootstrap MCP and skills directories at their resolved paths, run `wagmii setup`.
To only scaffold MCP, run `wagmii mcp init`.

## Profiles

You can define multiple profiles in the same file:

```toml
api_key = "PERSONAL_KEY"
default_text_model = "wagmii-reasoner"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.wagmii.com"
```

Select a profile with:

- CLI: `wagmii --profile work`
- Env: `WAGMII_PROFILE=work`

If a profile is selected but missing, Wagmii CLI exits with an error listing available profiles.

## Environment Variables

These override config values:

- `WAGMII_API_KEY`
- `WAGMII_BASE_URL`
- `WAGMII_SKILLS_DIR`
- `WAGMII_MCP_CONFIG`
- `WAGMII_NOTES_PATH`
- `WAGMII_MEMORY_PATH`
- `WAGMII_ALLOW_SHELL` (`1`/`true` enables)
- `WAGMII_APPROVAL_POLICY` (`on-request|untrusted|never`)
- `WAGMII_SANDBOX_MODE` (`read-only|workspace-write|danger-full-access|external-sandbox`)
- `WAGMII_MANAGED_CONFIG_PATH`
- `WAGMII_REQUIREMENTS_PATH`
- `WAGMII_MAX_SUBAGENTS` (clamped to `1..=20`)
- `WAGMII_TASKS_DIR` (runtime task queue/artifact storage, default `~/.wagmii/tasks`)
- `WAGMII_ALLOW_INSECURE_HTTP` (`1`/`true` allows non-local `http://` base URLs; default is reject)
- `WAGMII_CAPACITY_ENABLED`
- `WAGMII_CAPACITY_LOW_RISK_MAX`
- `WAGMII_CAPACITY_MEDIUM_RISK_MAX`
- `WAGMII_CAPACITY_SEVERE_MIN_SLACK`
- `WAGMII_CAPACITY_SEVERE_VIOLATION_RATIO`
- `WAGMII_CAPACITY_REFRESH_COOLDOWN_TURNS`
- `WAGMII_CAPACITY_REPLAN_COOLDOWN_TURNS`
- `WAGMII_CAPACITY_MAX_REPLAY_PER_TURN`
- `WAGMII_CAPACITY_MIN_TURNS_BEFORE_GUARDRAIL`
- `WAGMII_CAPACITY_PROFILE_WINDOW`
- `WAGMII_CAPACITY_PRIOR_CHAT`
- `WAGMII_CAPACITY_PRIOR_REASONER`
- `WAGMII_CAPACITY_PRIOR_FALLBACK`

## Settings File (Persistent UI Preferences)

Wagmii CLI also stores user preferences in:

- `~/.config/wagmii/settings.toml`

Notable settings include `auto_compact` (default `true`), which automatically summarizes
earlier turns once the conversation grows large. You can inspect or update these from the
TUI with `/settings` and `/config` (interactive editor).

Common settings keys:

- `theme` (default, dark, light, whale)
- `auto_compact` (on/off)
- `show_thinking` (on/off)
- `show_tool_details` (on/off)
- `default_mode` (normal, agent, plan, yolo)
- `max_history` (number of input history entries)
- `default_model` (model name override)

Readability semantics:

- Selection uses a unified style across transcript, composer menus, and modals.
- Footer hints use a dedicated semantic role (`FOOTER_HINT`) so hint text stays readable across themes.

### Command Migration Notes

If you are upgrading from older releases:

- Old: `/wagmii`
  New: `/links` (aliases: `/dashboard`, `/api`)
- Old: `/set model wagmii-reasoner`
  New: `/config` and edit the `model` row to `wagmii-reasoner`
- Old: discover `/set` in slash UX/help
  New: use `/config` for editing and `/settings` for read-only inspection

## Key Reference

### Core keys (used by the TUI/engine)

- `api_key` (string, required): must be non-empty (or set `WAGMII_API_KEY`).
- `base_url` (string, optional): defaults to `https://api.wagmii.com` (OpenAI-compatible Responses API).
- `default_text_model` (string, optional): defaults to `wagmii-reasoner`. Supported IDs are `wagmii-reasoner` and `wagmii-chat`.
- `allow_shell` (bool, optional): defaults to `true` (sandboxed).
- `approval_policy` (string, optional): `on-request`, `untrusted`, or `never`. Runtime `approval_mode` editing in `/config` also accepts `on-request` and `untrusted` aliases.
- `sandbox_mode` (string, optional): `read-only`, `workspace-write`, `danger-full-access`, `external-sandbox`.
- `managed_config_path` (string, optional): managed config file loaded after user/env config.
- `requirements_path` (string, optional): requirements file used to enforce allowed approval/sandbox values.
- `max_subagents` (int, optional): defaults to `5` and is clamped to `1..=20`.
- `skills_dir` (string, optional): defaults to `~/.wagmii/skills` (each skill is a directory containing `SKILL.md`). Workspace-local `.agents/skills` or `./skills` are preferred when present.
- `mcp_config_path` (string, optional): defaults to `~/.wagmii/mcp.json`.
- `notes_path` (string, optional): defaults to `~/.wagmii/notes.txt` and is used by the `note` tool.
- `memory_path` (string, optional): defaults to `~/.wagmii/memory.md`.
- `retry.*` (optional): retry/backoff settings for API requests:
  - `[retry].enabled` (bool, default `true`)
  - `[retry].max_retries` (int, default `3`)
  - `[retry].initial_delay` (float seconds, default `1.0`)
  - `[retry].max_delay` (float seconds, default `60.0`)
  - `[retry].exponential_base` (float, default `2.0`)
- `capacity.*` (optional): runtime context-capacity controller:
  - `[capacity].enabled` (bool, default `true`)
  - `[capacity].low_risk_max` (float, default `0.34`)
  - `[capacity].medium_risk_max` (float, default `0.62`)
  - `[capacity].severe_min_slack` (float, default `-0.25`)
  - `[capacity].severe_violation_ratio` (float, default `0.40`)
  - `[capacity].refresh_cooldown_turns` (int, default `2`)
  - `[capacity].replan_cooldown_turns` (int, default `5`)
  - `[capacity].max_replay_per_turn` (int, default `1`)
  - `[capacity].min_turns_before_guardrail` (int, default `2`)
  - `[capacity].profile_window` (int, default `8`)
  - `[capacity].wagmii_v3_2_chat_prior` (float, default `3.9`)
  - `[capacity].wagmii_v3_2_reasoner_prior` (float, default `4.1`)
  - `[capacity].fallback_default_prior` (float, default `3.8`)
- `tui.alternate_screen` (string, optional): `auto`, `always`, or `never`. `auto` disables the alternate screen in Zellij; `--no-alt-screen` forces inline mode.
- `hooks` (optional): lifecycle hooks configuration (see `config.example.toml`).
- `features.*` (optional): feature flag overrides (see below).

### Parsed but currently unused (reserved for future versions)

These keys are accepted by the config loader but not currently used by the interactive TUI or built-in tools:

- `tools_file`

## Feature Flags

Feature flags live under the `[features]` table and are merged across profiles.
Defaults are enabled for built-in tooling, so you only need to set entries you
want to force on or off.

```toml
[features]
shell_tool = true
subagents = true
web_search = true # enables web.run and web_search
apply_patch = true
mcp = true
exec_policy = true
```

You can also override features for a single run:

- `wagmii --enable web_search`
- `wagmii --disable subagents`

Use `wagmii features list` to inspect known flags and their effective state.

## Managed Configuration and Requirements

Wagmii CLI supports a policy layering model:

1. user config + profile + env overrides
2. managed config (if present)
3. requirements validation (if present)

By default on Unix:
- managed config: `/etc/wagmii/managed_config.toml`
- requirements: `/etc/wagmii/requirements.toml`

Requirements file shape:

```toml
allowed_approval_policies = ["on-request", "untrusted", "never"]
allowed_sandbox_modes = ["read-only", "workspace-write"]
```

If configured values violate requirements, startup fails with a descriptive error.

See `docs/capacity_controller.md` for formulas, intervention behavior, and telemetry.

## Notes On `wagmii doctor`

`wagmii doctor` now follows the same config resolution rules as the rest of the CLI.
That means `--config` / `WAGMII_CONFIG_PATH` are respected, and MCP/skills checks
use the resolved `mcp_config_path` / `skills_dir` (including env overrides).

To bootstrap missing MCP/skills paths, run `wagmii setup --all`. You can also
run `wagmii setup --skills --local` to create a workspace-local `./skills` dir.
