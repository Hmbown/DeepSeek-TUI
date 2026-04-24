# Configuration

DeepSeek TUI reads configuration from a TOML file plus environment variables.

## Where It Looks

Default config path:

- `~/.deepseek/config.toml`

Overrides:

- CLI: `deepseek --config /path/to/config.toml`
- Env: `DEEPSEEK_CONFIG_PATH=/path/to/config.toml`

If both are set, `--config` wins. Environment variable overrides are applied after the file is loaded.

The `deepseek` facade and `deepseek-tui` binary share the same config file for
DeepSeek auth and model defaults. `deepseek login --api-key ...` writes the
root `api_key` field that `deepseek-tui` reads directly, and `deepseek --model
deepseek-v4-flash` is forwarded to the TUI as `DEEPSEEK_MODEL`.

To bootstrap MCP and skills directories at their resolved paths, run `deepseek-tui setup`.
To only scaffold MCP, run `deepseek-tui mcp init`.

Note: setup, doctor, mcp, features, sessions, resume/fork, exec, review, and eval
are subcommands of the `deepseek-tui` binary. The `deepseek` dispatcher exposes a
distinct set of commands (`auth`, `config`, `model`, `thread`, `sandbox`,
`app-server`, `mcp-server`, `completion`) and forwards plain prompts to
`deepseek-tui`.

## Profiles

You can define multiple profiles in the same file:

```toml
api_key = "PERSONAL_KEY"
default_text_model = "deepseek-v4-pro"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.deepseek.com"
```

Select a profile with:

- CLI: `deepseek --profile work`
- Env: `DEEPSEEK_PROFILE=work`

If a profile is selected but missing, DeepSeek TUI exits with an error listing available profiles.

## Environment Variables

These override config values:

- `DEEPSEEK_API_KEY`
- `DEEPSEEK_BASE_URL`
- `DEEPSEEK_MODEL` or `DEEPSEEK_DEFAULT_TEXT_MODEL`
- `DEEPSEEK_SKILLS_DIR`
- `DEEPSEEK_MCP_CONFIG`
- `DEEPSEEK_NOTES_PATH`
- `DEEPSEEK_MEMORY_PATH`
- `DEEPSEEK_ALLOW_SHELL` (`1`/`true` enables)
- `DEEPSEEK_APPROVAL_POLICY` (`on-request|untrusted|never`)
- `DEEPSEEK_SANDBOX_MODE` (`read-only|workspace-write|danger-full-access|external-sandbox`)
- `DEEPSEEK_MANAGED_CONFIG_PATH`
- `DEEPSEEK_REQUIREMENTS_PATH`
- `DEEPSEEK_MAX_SUBAGENTS` (clamped to `1..=20`)
- `DEEPSEEK_TASKS_DIR` (runtime task queue/artifact storage, default `~/.deepseek/tasks`)
- `DEEPSEEK_ALLOW_INSECURE_HTTP` (`1`/`true` allows non-local `http://` base URLs; default is reject)
- `DEEPSEEK_CAPACITY_ENABLED`
- `DEEPSEEK_CAPACITY_LOW_RISK_MAX`
- `DEEPSEEK_CAPACITY_MEDIUM_RISK_MAX`
- `DEEPSEEK_CAPACITY_SEVERE_MIN_SLACK`
- `DEEPSEEK_CAPACITY_SEVERE_VIOLATION_RATIO`
- `DEEPSEEK_CAPACITY_REFRESH_COOLDOWN_TURNS`
- `DEEPSEEK_CAPACITY_REPLAN_COOLDOWN_TURNS`
- `DEEPSEEK_CAPACITY_MAX_REPLAY_PER_TURN`
- `DEEPSEEK_CAPACITY_MIN_TURNS_BEFORE_GUARDRAIL`
- `DEEPSEEK_CAPACITY_PROFILE_WINDOW`
- `DEEPSEEK_CAPACITY_PRIOR_CHAT`
- `DEEPSEEK_CAPACITY_PRIOR_REASONER`
- `DEEPSEEK_CAPACITY_PRIOR_FALLBACK`

## Settings File (Persistent UI Preferences)

DeepSeek TUI also stores user preferences in:

- `~/.config/deepseek/settings.toml`

Notable settings include `auto_compact` (default `true`), which automatically summarizes
earlier turns once the conversation grows large. You can inspect or update these from the
TUI with `/settings` and `/config` (interactive editor).

Common settings keys:

- `theme` (default, dark, light, whale)
- `auto_compact` (on/off)
- `show_thinking` (on/off)
- `show_tool_details` (on/off)
- `default_mode` (agent, plan, yolo; legacy `normal` is accepted and normalized to `agent`)
- `max_history` (number of input history entries)
- `default_model` (model name override)

Only `agent`, `plan`, and `yolo` are visible modes in the UI. For compatibility,
older settings files with `default_mode = "normal"` still load as `agent`, and
the hidden `/normal` slash command switches to `Agent`.

Readability semantics:

- Selection uses a unified style across transcript, composer menus, and modals.
- Footer hints use a dedicated semantic role (`FOOTER_HINT`) so hint text stays readable across themes.

### Command Migration Notes

If you are upgrading from older releases:

- Old: `/deepseek`
  New: `/links` (aliases: `/dashboard`, `/api`)
- Old: `/set model deepseek-reasoner`
  New: `/config` and edit the `model` row to `deepseek-v4-pro` or `deepseek-v4-flash`
- Old: visible `Normal` mode or `default_mode = "normal"`
  New: use `Agent` / `default_mode = "agent"`; legacy `normal` still maps to `agent`
- Old: discover `/set` in slash UX/help
  New: use `/config` for editing and `/settings` for read-only inspection

## Key Reference

### Core keys (used by the TUI/engine)

- `api_key` (string, required): must be non-empty (or set `DEEPSEEK_API_KEY`).
- `base_url` (string, optional): defaults to `https://api.deepseek.com` for DeepSeek's OpenAI-compatible Chat Completions API. `https://api.deepseek.com/v1` is also accepted for SDK compatibility; use `https://api.deepseek.com/beta` only for DeepSeek beta features such as strict tool mode, chat prefix completion, and FIM completion.
- `default_text_model` (string, optional): defaults to `deepseek-v4-pro`. Current public DeepSeek IDs are `deepseek-v4-pro` and `deepseek-v4-flash`, both with 1M context windows and thinking mode enabled by default. Legacy `deepseek-chat` and `deepseek-reasoner` remain compatibility aliases for `deepseek-v4-flash`. Use `/models` or `deepseek models` to discover live IDs from your configured endpoint. `DEEPSEEK_MODEL` overrides this for a single process.
- `reasoning_effort` (string, optional): `off`, `low`, `medium`, `high`, or `max`; defaults to the configured UI tier. `off` sends `thinking = {"type": "disabled"}`. `low` and `medium` are normalized to `high`; `max` sends `reasoning_effort = "max"`.
- `allow_shell` (bool, optional): defaults to `true` (sandboxed).
- `approval_policy` (string, optional): `on-request`, `untrusted`, or `never`. Runtime `approval_mode` editing in `/config` also accepts `on-request` and `untrusted` aliases.
- `sandbox_mode` (string, optional): `read-only`, `workspace-write`, `danger-full-access`, `external-sandbox`.
- `managed_config_path` (string, optional): managed config file loaded after user/env config.
- `requirements_path` (string, optional): requirements file used to enforce allowed approval/sandbox values.
- `max_subagents` (int, optional): defaults to `5` and is clamped to `1..=20`.
- `skills_dir` (string, optional): defaults to `~/.deepseek/skills` (each skill is a directory containing `SKILL.md`). Workspace-local `.agents/skills` or `./skills` are preferred when present.
- `mcp_config_path` (string, optional): defaults to `~/.deepseek/mcp.json`.
- `notes_path` (string, optional): defaults to `~/.deepseek/notes.txt` and is used by the `note` tool.
- `memory_path` (string, optional): defaults to `~/.deepseek/memory.md`.
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
  - `[capacity].deepseek_v3_2_chat_prior` (float, default `3.9`)
  - `[capacity].deepseek_v3_2_reasoner_prior` (float, default `4.1`)
  - `[capacity].fallback_default_prior` (float, default `3.8`)
- `tui.alternate_screen` (string, optional): `auto`, `always`, or `never`. `auto` disables the alternate screen in Zellij; `--no-alt-screen` forces inline mode. Set `never` or run with `--no-alt-screen` when you want real terminal scrollback.
- `tui.mouse_capture` (bool, optional, default `true` when the alternate screen is active): enable internal mouse scrolling/transcript selection. Set this to `false` or run with `--no-mouse-capture` for terminal-native drag selection and highlight-to-copy.
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
web_search = true # enables canonical web.run plus the compatibility web_search alias
apply_patch = true
mcp = true
exec_policy = true
```

You can also override features for a single run:

- `deepseek-tui --enable web_search`
- `deepseek-tui --disable subagents`

Use `deepseek-tui features list` to inspect known flags and their effective state.

## Local Media Attachments

`Ctrl+V` can attach an image from the clipboard, and `/attach <path>` can attach a local image or video file path to the next message. DeepSeek's public Chat Completions API currently accepts text message content, so attachments are sent as explicit local path references instead of native image/video payloads.

## Managed Configuration and Requirements

DeepSeek TUI supports a policy layering model:

1. user config + profile + env overrides
2. managed config (if present)
3. requirements validation (if present)

By default on Unix:
- managed config: `/etc/deepseek/managed_config.toml`
- requirements: `/etc/deepseek/requirements.toml`

Requirements file shape:

```toml
allowed_approval_policies = ["on-request", "untrusted", "never"]
allowed_sandbox_modes = ["read-only", "workspace-write"]
```

If configured values violate requirements, startup fails with a descriptive error.

See `docs/capacity_controller.md` for formulas, intervention behavior, and telemetry.

## Notes On `deepseek-tui doctor`

`deepseek-tui doctor` follows the same config resolution rules as the rest of the
TUI. That means `--config` / `DEEPSEEK_CONFIG_PATH` are respected, and MCP/skills
checks use the resolved `mcp_config_path` / `skills_dir` (including env overrides).

To bootstrap missing MCP/skills paths, run `deepseek-tui setup --all`. You can
also run `deepseek-tui setup --skills --local` to create a workspace-local
`./skills` dir.
