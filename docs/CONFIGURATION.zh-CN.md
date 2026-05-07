# 配置

DeepSeek TUI 从 TOML 文件和环境变量中读取配置。进程启动时，如果工作区中存在 `.env` 文件，也会自动加载。请使用仓库中的 `.env.example` 作为模板；复制为 `.env`，然后仅编辑你需要的提供商和安全相关配置项。

## 配置查找路径

默认配置文件路径：

- `~/.deepseek/config.toml`

覆盖方式：

- CLI：`deepseek --config /path/to/config.toml`
- 环境变量：`DEEPSEEK_CONFIG_PATH=/path/to/config.toml`

如果两者同时设置，`--config` 优先。环境变量覆盖在文件加载之后应用。

### 项目级覆盖（#485）

当 TUI 在包含 `<workspace>/.deepseek/config.toml` 文件的工作区中启动时，该文件中声明的值将合并到全局配置之上。这使得仓库可以锁定自己的提供商、模型、沙箱策略或审批策略，而无需修改用户的 `~/.deepseek/config.toml`。使用 `--no-project-config` 可以在单次启动中跳过项目级覆盖。

项目覆盖支持的键（仅顶层字段）：

| 键 | 作用 |
|---|---|
| `provider` | 切换后端（例如企业仓库使用 `"nvidia-nim"`） |
| `model` | 覆盖 `default_text_model` |
| `api_key` | 使用仓库专属密钥（通常从 `.env` 读取，**不要提交**） |
| `base_url` | 指向自托管端点 |
| `reasoning_effort` | 对复杂的仓库强制设为 `"high"` / `"max"` |
| `approval_policy` | `"never"` / `"on-request"` / `"untrusted"` |
| `sandbox_mode` | `"read-only"` / `"workspace-write"` / `"danger-full-access"` |
| `mcp_config_path` | 仓库专属 MCP 服务器集 |
| `notes_path` | 将笔记保留在仓库内 |
| `max_subagents` | 限制仓库的并发数（限制在 1..=20） |
| `allow_shell` | 设为 `false` 时关闭 shell 工具访问 |

项目覆盖范围是有意收窄的 —— 它涵盖了仓库维护者最可能希望在贡献者之间统一设置的字段。其他设置（skills_dir、hooks、capacity、retry 等）保持用户全局级别。如果你的仓库需要更多覆盖项，请提交 issue 描述具体的使用场景。

`deepseek` 门面程序和 `deepseek-tui` 二进制共享同一个配置文件，用于 DeepSeek 认证和模型默认值。`deepseek auth set --provider deepseek`（以及旧版别名 `deepseek login --api-key ...`）将密钥保存到 `~/.deepseek/config.toml`，`deepseek --model deepseek-v4-flash` 会以 `DEEPSEEK_MODEL` 的形式转发给 TUI。

对于托管或自托管提供商，设置 `provider = "nvidia-nim"`、`"fireworks"`、`"sglang"`、`"vllm"` 或 `"ollama"`，或使用 `deepseek --provider <name>`。门面程序将提供商凭据保存到共享的用户配置中，并将解析后的密钥、base URL、提供商和模型转发给 TUI 进程。使用 `deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"` 或 `deepseek auth set --provider fireworks --api-key "YOUR_FIREWORKS_API_KEY"` 来通过门面程序保存托管提供商的密钥。SGLang、vLLM 和 Ollama 为自托管模式，默认不需要 API 密钥。Ollama 默认使用 `http://localhost:11434/v1`，直接传递模型标签如 `deepseek-coder:1.3b` 或 `qwen2.5-coder:7b`。

需要额外请求头的第三方 OpenAI 兼容网关，可以在顶层或提供商表（如 `[providers.deepseek]`）下设置 `http_headers = { "X-Model-Provider-Id" = "your-model-provider" }`。配置后，DeepSeek TUI 会在模型 API 请求中发送这些自定义请求头。等效的环境变量覆盖为 `DEEPSEEK_HTTP_HEADERS`，使用逗号分隔的 `name=value` 对，例如 `X-Model-Provider-Id=your-model-provider,X-Gateway-Route=dev`。`Authorization` 和 `Content-Type` 由客户端管理，不受此设置的影响。

要引导 MCP 和技能目录到其解析路径，运行 `deepseek-tui setup`。仅搭建 MCP，运行 `deepseek-tui mcp init`。

注意：setup、doctor、mcp、features、sessions、resume/fork、exec、review 和 eval 是 `deepseek-tui` 二进制的子命令。`deepseek` 调度器暴露了一组不同的命令（`auth`、`config`、`model`、`thread`、`sandbox`、`app-server`、`mcp-server`、`completion`），并将普通提示词转发给 `deepseek-tui`。

## Profiles

你可以在同一个文件中定义多个 profile：

```toml
api_key = "PERSONAL_KEY"
default_text_model = "deepseek-v4-pro"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.deepseek.com"

[profiles.nvidia-nim]
provider = "nvidia-nim"
api_key = "NVIDIA_KEY"
base_url = "https://integrate.api.nvidia.com/v1"
default_text_model = "deepseek-ai/deepseek-v4-pro"

[profiles.fireworks]
provider = "fireworks"
default_text_model = "accounts/fireworks/models/deepseek-v4-pro"

[profiles.sglang]
provider = "sglang"
base_url = "http://localhost:30000/v1"
default_text_model = "deepseek-ai/DeepSeek-V4-Pro"

[profiles.vllm]
provider = "vllm"
base_url = "http://localhost:8000/v1"
default_text_model = "deepseek-ai/DeepSeek-V4-Pro"

[profiles.ollama]
provider = "ollama"
base_url = "http://localhost:11434/v1"
default_text_model = "deepseek-coder:1.3b"
```

选择 profile 的方式：

- CLI：`deepseek --profile work`
- 环境变量：`DEEPSEEK_PROFILE=work`

如果选择的 profile 不存在，DeepSeek TUI 会退出并列出可用的 profile。

## 环境变量

以下环境变量会覆盖配置值：

- `DEEPSEEK_API_KEY`
- `DEEPSEEK_BASE_URL`
- `DEEPSEEK_HTTP_HEADERS`（自定义模型请求头，逗号分隔的 `name=value` 对）
- `DEEPSEEK_PROVIDER`（`deepseek|deepseek-cn|nvidia-nim|openrouter|novita|fireworks|sglang|vllm|ollama`）
- `DEEPSEEK_MODEL`
- `NVIDIA_API_KEY` 或 `NVIDIA_NIM_API_KEY`（提供商为 `nvidia-nim` 时优先使用；回退到 `DEEPSEEK_API_KEY`）
- `NVIDIA_NIM_BASE_URL`、`NIM_BASE_URL` 或 `NVIDIA_BASE_URL`
- `NVIDIA_NIM_MODEL`
- `FIREWORKS_API_KEY`
- `FIREWORKS_BASE_URL`
- `SGLANG_BASE_URL`
- `SGLANG_MODEL`
- `SGLANG_API_KEY`（可选；许多本地 SGLang 服务器不需要认证）
- `VLLM_BASE_URL`
- `VLLM_MODEL`
- `VLLM_API_KEY`（可选；许多本地 vLLM 服务器不需要认证）
- `OLLAMA_BASE_URL`
- `OLLAMA_MODEL`
- `OLLAMA_API_KEY`（可选；许多本地 Ollama 服务器不需要认证）
- `OPENROUTER_API_KEY`
- `OPENROUTER_BASE_URL`
- `NOVITA_API_KEY`
- `NOVITA_BASE_URL`
- `DEEPSEEK_LOG_LEVEL` 或 `RUST_LOG`（`info`/`debug`/`trace` 可启用轻量级详细日志）
- `DEEPSEEK_SKILLS_DIR`
- `DEEPSEEK_MCP_CONFIG`
- `DEEPSEEK_NOTES_PATH`
- `DEEPSEEK_MEMORY`（`1|on|true|yes|y|enabled` 启用用户记忆）
- `DEEPSEEK_MEMORY_PATH`
- `DEEPSEEK_ALLOW_SHELL`（设为 `1`/`true` 启用）
- `DEEPSEEK_APPROVAL_POLICY`（`on-request|untrusted|never`）
- `DEEPSEEK_SANDBOX_MODE`（`read-only|workspace-write|danger-full-access|external-sandbox`）
- `DEEPSEEK_MANAGED_CONFIG_PATH`
- `DEEPSEEK_REQUIREMENTS_PATH`
- `DEEPSEEK_MAX_SUBAGENTS`（限制在 `1..=20`）
- `DEEPSEEK_TASKS_DIR`（运行时任务队列/工件存储，默认 `~/.deepseek/tasks`）
- `DEEPSEEK_ALLOW_INSECURE_HTTP`（设为 `1`/`true` 允许非本地的 `http://` base URL；默认拒绝）
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
- `DEEPSEEK_CAPACITY_PRIOR_V4_PRO`
- `DEEPSEEK_CAPACITY_PRIOR_V4_FLASH`
- `DEEPSEEK_CAPACITY_PRIOR_FALLBACK`
- `NO_ANIMATIONS`（设为 `1|true|yes|on` 在启动时强制 `low_motion = true` 和 `fancy_animations = false`，忽略已保存的设置；参见 [`docs/ACCESSIBILITY.md`](./ACCESSIBILITY.md)）
- `SSL_CERT_FILE` — 企业代理/TLS 中间人检查用户将此指向 PEM 包（或单个 DER 证书），证书会被添加到平台系统信任存储之外。加载失败会记录警告并继续 —— 现有的系统根证书仍然生效。

### 指令来源（`instructions = [...]`，#454）

添加额外的系统提示词来源列表，按照声明顺序与自动加载的 `AGENTS.md` 拼接：

```toml
instructions = [
    "./AGENTS.md",
    "~/.deepseek/global.md",
    "~/team/agents-shared.md",
]
```

规则：

- 路径经过 `expand_path` 处理，因此 `~` 和环境变量均有效。
- 每个文件限制在 100 KiB 以内；超出限制的文件会被截断并标记 `[…elided]`，而不是直接跳过。
- 不存在的文件会被跳过并记录 tracing 警告，过时的条目不会导致启动失败。
- 项目配置（`<workspace>/.deepseek/config.toml`）会**整体替换**用户数组而非合并。如果你想保留两者，请在项目数组中列出 `~/global.md`。在项目中设置 `instructions = []` 可清除该仓库的用户列表。

### `/hooks` 列表

在 TUI 中运行 `/hooks`（或 `/hooks list`）可按事件分组查看所有已配置的生命周期钩子，包括每个钩子的名称、命令预览、超时和条件。`[hooks].enabled` 标志的状态会显示在顶部，以便清楚地看到钩子何时被全局禁用。钩子在 `[[hooks.hooks]]` 条目下配置 —— 完整 schema 请参见现有钩子系统文档。

### 输入暂存（`/stash`，Ctrl+S）

在输入框中按 **Ctrl+S** 将当前草稿暂存到 `~/.deepseek/composer_stash.jsonl`。`/stash list` 列出已暂存的草稿（含单行预览和时间戳）；`/stash pop` 恢复最近暂存的草稿（LIFO）；`/stash clear` 清空文件。上限为 200 条；多行草稿完整保留。

## 设置文件（持久化 UI 偏好）

DeepSeek TUI 还将用户偏好存储在：

- `~/.config/deepseek/settings.toml`

重要设置包括 `auto_compact`（默认 `false`），启用后仅在接近活跃模型限制时才进行替换式摘要。默认的 V4 路径保留稳定的消息前缀以利用缓存；仅在明确需要自动替换压缩时使用手动 `/compact` 或启用 `auto_compact`。你可以在 TUI 中使用 `/settings` 和 `/config`（交互式编辑器）查看或更新这些设置。

常用设置键：

- `theme`（`dark`、`light`、`system`；默认 `dark`）
- `auto_compact`（`true`/`false`，默认 `false`）
- `calm_mode`（`true`/`false`，默认 `false`）：减少状态噪音，更积极地折叠详情
- `low_motion`（`true`/`false`，默认 `false`）：减少动画和重绘抖动
- `fancy_animations`（`true`/`false`，默认 `false`）：启用页脚动画效果
- `bracketed_paste`（`true`/`false`，默认 `true`）：终端括号粘贴模式
- `paste_burst_detection`（`true`/`false`，默认 `true`）：对不支持括号粘贴事件的终端进行快速按键粘贴检测。此选项独立于终端括号粘贴模式
- `show_thinking`（`true`/`false`）
- `show_tool_details`（`true`/`false`）
- `composer_density`（`compact`、`comfortable`、`spacious`；默认 `comfortable`）：输入框布局密度
- `composer_border`（`true`/`false`，默认 `true`）：输入框区域边框
- `composer_vim_mode`（`normal`、`vim`；默认 `normal`）：设置为 `vim` 时输入框以 Normal 模式启动，按 `i`/`a`/`o` 进入 Insert 模式，按 `Esc` 回到 Normal 模式
- `transcript_spacing`（`compact`、`comfortable`、`spacious`；默认 `comfortable`）：对话间距风格
- `sidebar_width_percent`（默认 `28`）：侧边栏宽度百分比
- `sidebar_focus`（`auto`、`plan`、`todos`、`tasks`、`agents`、`context`；默认 `auto`）
- `context_panel`（`true`/`false`，默认 `false`）：启用会话上下文面板（#504），显示工作集、token、费用、MCP/LSP 状态、轮次计数和记忆信息
- `locale`（`auto`、`en`、`ja`、`zh-Hans`、`pt-BR`；默认 `auto`）：UI 界面语言。`auto` 依次检查 `LC_ALL`、`LC_MESSAGES`、`LANG`；不支持或缺失的语言回退到英语。这不会强制模型输出语言
- `cost_currency`（`usd`、`cny`；默认 `usd`）：页脚、上下文面板、`/cost`、`/tokens` 和长轮次通知摘要中使用的货币。别名 `rmb` 和 `yuan` 标准化为 `cny`
- `default_mode`（`agent`、`plan`、`yolo`；旧版 `normal` 被接受并标准化为 `agent`）
- `max_input_history`（默认 `100`）：保存的输入历史条目数（已清除的草稿也会在本地保留，用于输入框历史搜索）
- `default_model`（模型名称覆盖）

UI 中仅 `agent`、`plan` 和 `yolo` 三种模式可见。为兼容性考虑，旧版设置文件中 `default_mode = "normal"` 仍然加载为 `agent`，隐藏的 `/normal` 斜杠命令切换到 `Agent` 模式。

本地化范围记录在 [LOCALIZATION.md](LOCALIZATION.md) 中。v0.7.6 核心包仅覆盖高可见度的 TUI 界面；提供商/工具 schema、个性化提示词和完整文档仍为英文，除非后续明确翻译。

可读性语义：

- 选择操作使用统一的样式，贯穿对话记录、输入框菜单和模态框。
- 页脚提示使用专用的语义角色（`FOOTER_HINT`），确保提示文本在不同主题下可读。
- 页脚包含一个紧凑的 `coherence` 状态指示器，描述当前会话的稳定性和专注程度。可能的状态为 `healthy`、`crowded`、`refreshing`、`verifying` 和 `resetting`；这些状态根据容量和压缩事件推导，不会在常规 UI 中暴露内部公式。

### Token 数量与驱动因素

DeepSeek V4 前缀缓存使 token 标签变得重要。以下数量被区分开来：

| 数量 | 含义 | 允许驱动的操作 |
|---|---|---|
| 活跃请求输入估算 | 下一次请求中活跃系统提示词和对话载荷的保守估算。 | 页眉/页脚上下文百分比、硬循环触发、可选的 Flash 缝触发、紧急溢出预检。 |
| 保留的响应空间 | 请求的 `max_tokens` 预算加上安全余量。v0.7.5 将普通轮次的输出 token 保持为 `262144`，并额外添加 `1024` 个安全 token 用于上下文窗口检查。 | 仅用于硬循环和紧急溢出预算检查。 |
| 累计 API 用量 | 提供商报告的已完成 API 调用中输入加输出 token 的总和；多工具轮次中，相同的稳定前缀可能被重复计入。 | 仅用于会话用量和近似费用遥测。 |
| 提示词缓存命中/未命中 | 最近一次调用（如有）的提供商缓存遥测。 | 仅用于缓存命中显示和费用估算；不用于压缩、缝或循环触发。 |
| 上下文百分比 | 活跃请求输入估算除以模型上下文窗口。 | 仅用于显示；反映上下文保护机制使用的活跃输入基准。 |
| 费用估算 | 基于提供商用量和已配置 DeepSeek 价格的近似消费。 | 仅用于显示。 |

对于默认的 V4 路径，当活跃输入达到配置的循环阈值（`768000`）和模型窗口减去保留响应空间后两者中较小值时触发硬循环。替换式压缩保持可选（默认 `auto_compact = false`），Flash 缝管理器保持可选（默认 `[context].enabled = false`），容量控制器保持禁用，除非已配置。

### 命令迁移说明

如果从旧版本升级：

- 旧：`/deepseek`
  新：`/links`（别名：`/dashboard`、`/api`）
- 旧：`/set model deepseek-reasoner`
  新：`/config`，编辑 `model` 行为 `deepseek-v4-pro` 或 `deepseek-v4-flash`
- 旧：可见的 `Normal` 模式或 `default_mode = "normal"`
  新：使用 `Agent` / `default_mode = "agent"`；旧版 `normal` 仍映射到 `agent`
- 旧：在斜杠命令/帮助中发现 `/set`
  新：使用 `/config` 进行编辑，`/settings` 进行只读查看

## 关键配置参考

### 核心键（TUI/引擎使用）

- `provider`（字符串，可选）：`deepseek`（默认）、`deepseek-cn`、`nvidia-nim`、`openrouter`、`novita`、`fireworks`、`sglang`、`vllm` 或 `ollama`。`deepseek-cn` 使用 DeepSeek 的中国大陆端点（`https://api.deepseeki.com`）；`nvidia-nim` 指向 NVIDIA 的 NIM 托管 DeepSeek 端点（`https://integrate.api.nvidia.com/v1`）；`fireworks` 指向 `https://api.fireworks.ai/inference/v1`；`sglang` 指向自托管 OpenAI 兼容端点，默认 `http://localhost:30000/v1`；`vllm` 指向自托管 vLLM OpenAI 兼容端点，默认 `http://localhost:8000/v1`；`ollama` 指向 Ollama 的 OpenAI 兼容端点，默认 `http://localhost:11434/v1`
- `api_key`（字符串，托管提供商必需）：对于 DeepSeek/托管提供商必须非空（或设置提供商 API 密钥环境变量）。自托管 SGLang、vLLM 和 Ollama 可以省略
- `base_url`（字符串，可选）：默认为 `https://api.deepseek.com`（DeepSeek 的 OpenAI 兼容 Chat Completions API）、`https://api.deepseeki.com`（`provider = "deepseek-cn"`），或托管/自托管提供商的对应端点。`https://api.deepseek.com/v1` 也接受以兼容 SDK；仅在需要 DeepSeek beta 功能（如严格工具模式、chat prefix completion 和 FIM completion）时使用 `https://api.deepseek.com/beta`
- `default_text_model`（字符串，可选）：DeepSeek 默认为 `deepseek-v4-pro`，NVIDIA NIM 为 `deepseek-ai/deepseek-v4-pro`，OpenRouter 为 `deepseek/deepseek-v4-pro`，Novita 为 `deepseek/deepseek-v4-pro`，Fireworks 为 `accounts/fireworks/models/deepseek-v4-pro`，SGLang/vLLM 为 `deepseek-ai/DeepSeek-V4-Pro`，Ollama 为 `deepseek-coder:1.3b`。当前公开的 DeepSeek ID 为 `deepseek-v4-pro` 和 `deepseek-v4-flash`，均支持 1M 上下文窗口且默认启用思考模式。旧版 `deepseek-chat` 和 `deepseek-reasoner` 保持为 `deepseek-v4-flash` 的兼容别名。提供商特定映射将 `deepseek-v4-pro` / `deepseek-v4-flash` 转换为各提供商的模型 ID（如支持）。Ollama 模型标签直接透传。使用 `/models` 或 `deepseek models` 从已配置的端点发现最新的 ID。`DEEPSEEK_MODEL` 可在单次进程中覆盖此设置
- `reasoning_effort`（字符串，可选）：`off`、`low`、`medium`、`high` 或 `max`；默认为已配置的 UI 等级。DeepSeek Platform 接收顶层的 `thinking` / `reasoning_effort` 字段。NVIDIA NIM 通过 `chat_template_kwargs` 接收等效设置
- `allow_shell`（布尔值，可选）：默认为 `true`（沙箱化）
- `approval_policy`（字符串，可选）：`on-request`、`untrusted` 或 `never`。在 `/config` 中编辑运行时 `approval_mode` 时也接受 `on-request` 和 `untrusted` 别名
- `sandbox_mode`（字符串，可选）：`read-only`、`workspace-write`、`danger-full-access`、`external-sandbox`
- `managed_config_path`（字符串，可选）：在用户/环境配置之后加载的托管配置文件
- `requirements_path`（字符串，可选）：用于强制允许审批/沙箱值的要求文件
- `max_subagents`（整数，可选）：默认为 `10`，限制在 `1..=20`
- `subagents.*`（可选）：为 `agent_spawn` 及相关子代理工具设置的按角色/类型模型默认值。优先级：显式工具的 `model` 值 → 角色/类型覆盖 → 父运行时模型。支持的便捷键为 `default_model`、`worker_model`、`explorer_model`、`awaiter_model`、`review_model`、`custom_model` 和 `max_concurrent`。`[subagents] max_concurrent` 值覆盖顶层的 `max_subagents`，同样限制在 `1..=20`。`[subagents.models]` 接受小写角色或类型键，如 `worker`、`explorer`、`general`、`explore`、`plan` 和 `review`。值必须在生成代理之前标准化为受支持的 DeepSeek 模型 ID
- `skills_dir`（字符串，可选）：默认为 `~/.deepseek/skills`（每个技能是一个包含 `SKILL.md` 的目录）。工作区本地 `.agents/skills` 或 `./skills` 优先；运行时也会发现全局 agentskills.io 兼容的 `~/.agents/skills` 和更广泛的 Claude 生态 `~/.claude/skills`
- `mcp_config_path`（字符串，可选）：默认为 `~/.deepseek/mcp.json`。在 `/config` 中可见，可在 TUI 中更改。新路径会立即被 `/mcp` 使用，但重建模型可见的 MCP 工具池需要重启 TUI
- `notes_path`（字符串，可选）：默认为 `~/.deepseek/notes.txt`，由 `note` 工具使用
- `[memory].enabled`（布尔值，可选）：默认为 `false`。设为 `true` 时，TUI 将用户记忆文件加载到 `<user_memory>` 提示块中，在输入框中启用 `# foo` 快速捕获，显示 `/memory` 斜杠命令，并注册 `remember` 工具。同样的开关可通过 `DEEPSEEK_MEMORY=on` 使用
- `memory_path`（字符串，可选）：默认为 `~/.deepseek/memory.md`。用户记忆功能启用时使用 —— 参见 [`MEMORY.md`](MEMORY.md) 了解完整功能面（`# foo` 输入框前缀、`/memory` 斜杠命令、`remember` 工具、可选开关）
- `snapshots.*`（可选）：用于文件回滚的旁路 git 工作区快照：
  - `[snapshots].enabled`（布尔值，默认 `true`）
  - `[snapshots].max_age_days`（整数，默认 `7`）
  - 快照保存在 `~/.deepseek/snapshots/<project_hash>/<worktree_hash>/.git`，不适用工作区自身的 `.git` 目录
- `context.*`（可选）：仅追加 Flash 缝管理器，当前为可选功能。阈值使用活跃请求输入估算，而非终身累计 API 用量：
  - `[context].enabled`（布尔值，默认 `false`）
  - `[context].verbatim_window_turns`（整数，默认 `16`）
  - `[context].l1_threshold`（整数，默认 `192000`）
  - `[context].l2_threshold`（整数，默认 `384000`）
  - `[context].l3_threshold`（整数，默认 `576000`）
  - `[context].cycle_threshold`（整数，默认 `768000`）
  - `[context].seam_model`（字符串，默认 `deepseek-v4-flash`）
- `retry.*`（可选）：API 请求的重试/退避设置：
  - `[retry].enabled`（布尔值，默认 `true`）
  - `[retry].max_retries`（整数，默认 `3`）
  - `[retry].initial_delay`（浮点数，秒，默认 `1.0`）
  - `[retry].max_delay`（浮点数，秒，默认 `60.0`）
  - `[retry].exponential_base`（浮点数，默认 `2.0`）
- `capacity.*`（可选）：运行时上下文容量控制器。默认关闭，因为其主动干预可能会重写活跃对话记录：
  - `[capacity].enabled`（布尔值，默认 `false`）
  - `[capacity].low_risk_max`（浮点数，默认 `0.50`）
  - `[capacity].medium_risk_max`（浮点数，默认 `0.62`）
  - `[capacity].severe_min_slack`（浮点数，默认 `-0.25`）
  - `[capacity].severe_violation_ratio`（浮点数，默认 `0.40`）
  - `[capacity].refresh_cooldown_turns`（整数，默认 `6`）
  - `[capacity].replan_cooldown_turns`（整数，默认 `5`）
  - `[capacity].max_replay_per_turn`（整数，默认 `1`）
  - `[capacity].min_turns_before_guardrail`（整数，默认 `4`）
  - `[capacity].profile_window`（整数，默认 `8`）
  - `[capacity].deepseek_v3_2_chat_prior`（浮点数，默认 `3.9`）
  - `[capacity].deepseek_v3_2_reasoner_prior`（浮点数，默认 `4.1`）
  - `[capacity].deepseek_v4_pro_prior`（浮点数，默认 `3.5`）
  - `[capacity].deepseek_v4_flash_prior`（浮点数，默认 `4.2`）
  - `[capacity].fallback_default_prior`（浮点数，默认 `3.8`）
- `[notifications].method`（字符串，可选）：`auto`、`osc9`、`bel` 或 `off`。默认为 `auto`。TUI 在完成的（成功）轮次耗时达到 `threshold_secs` 时触发通知；失败和取消的轮次静默。`auto` 对 `iTerm.app`、`Ghostty` 和 `WezTerm`（通过 `$TERM_PROGRAM` 检测）解析为 `osc9`。否则在 macOS / Linux 上回退到 `bel`，Windows 上回退到 `off`（Windows 的 BEL 映射为系统错误提示音 —— 详见下文的[通知](#通知)章节，#583）
- `[notifications].threshold_secs`（整数，可选）：默认为 `30`。只有已完成且耗时达到或超过此值的轮次才触发通知
- `[notifications].include_summary`（布尔值，可选）：默认为 `false`。设为 `true` 时，通知正文包含耗时和该轮次的费用（以配置的显示货币计）
- `tui.alternate_screen`（字符串，可选）：`auto`、`always` 或 `never`。`auto` 在 Zellij 中禁用交替屏幕；`--no-alt-screen` 强制内联模式。当你需要真实的终端回滚时，设为 `never` 或使用 `--no-alt-screen`
- `tui.mouse_capture`（布尔值，可选）：非 Windows 终端且交替屏幕活跃时默认 `true`；在 Windows 和 JetBrains JediTerm（PyCharm/IDEA/CLion 等）中默认 `false`（因为在 JediTerm 中鼠标事件转义会作为乱码文本泄漏到输入流，参见 #878 / #898）。启用内部鼠标滚动、对话记录选择和右键上下文操作。TUI 拥有的拖拽选择仅复制用户/助手对话记录文本。设为 `false` 或使用 `--no-mouse-capture` 以使用原始终端选择；设为 `true` 或使用 `--mouse-capture` 在默认关闭的环境中手动启用
- `tui.terminal_probe_timeout_ms`（整数，可选，默认 `500`）：启动时终端模式探测超时（毫秒）。值限制在 `100..=5000`；超时时发出警告并中止启动，而非无限挂起
- `tui.osc8_links`（布尔值，可选，默认 `true`）：在对话记录输出中的 URL 周围发出 OSC 8 转义序列，使支持它们的终端（iTerm2、Terminal.app 13+、Ghostty、Kitty、WezTerm、Alacritty、较新的 gnome-terminal/konsole）将其渲染为 Cmd+click 超链接。不支持 OSC 8 的终端渲染纯 URL 并忽略转义。设为 `false` 以应对错误渲染该序列的终端；选择/剪贴板输出始终会去除这些转义
- `hooks`（可选）：生命周期钩子配置（参见 `config.example.toml`）
- `features.*`（可选）：功能标志覆盖（参见下文）

### 用户记忆

用户记忆分为一个顶层路径设置和一个可选开关表：

```toml
memory_path = "~/.deepseek/memory.md"

[memory]
enabled = true
```

注意事项：

- `memory_path` 与 `notes_path` 和 `skills_dir` 一样保留在顶层；它不属于 `[memory]` 表内
- `DEEPSEEK_MEMORY_PATH` 可从环境变量覆盖文件路径
- `DEEPSEEK_MEMORY=on`（也接受 `1`、`true`、`yes`、`y` 或 `enabled`）可在不编辑 `config.toml` 的情况下启用此功能
- 功能禁用时处于惰性状态：不注入文件，`# foo` 回退为普通消息提交，模型不会看到 `remember` 工具
- 参见 [`MEMORY.md`](MEMORY.md) 了解示例和完整的 `/memory` 命令面

### 通知

当一个轮次**成功完成**且耗时超过阈值时，TUI 可以发出桌面通知（OSC 9 转义或纯 BEL），以便你在长任务运行时可切到其他窗口。失败或取消的轮次有意识地静默 —— 通知是"任务已完成"的提示，而非通用提醒。配置在 `[notifications]` 下：

```toml
[notifications]
method          = "auto"  # auto | osc9 | bel | off
threshold_secs  = 30      # 仅当轮次耗时 >= 此秒数时通知
include_summary = false   # 通知正文包含耗时和费用
```

方法语义：

- `auto`（默认） — 对 `iTerm.app`、`Ghostty` 和 `WezTerm`（通过 `$TERM_PROGRAM` 检测）选择 `osc9`。在 macOS 和 Linux 上回退到 `bel`。**在 Windows 上回退到 `off`** 而非 `bel`，因为 Windows 音频栈将 `\x07` 映射为 `SystemAsterisk` / `MB_OK` 提示音 —— 即应用程序错误弹窗使用的同一种声音，导致成功完成的通知听起来像报错（#583）
- `osc9` — 发出 `\x1b]9;<msg>\x07`。在 tmux 中该序列被包裹在 DCS 透传中，以到达外层终端
- `bel` — 发出单个 `\x07` 字节。仅在 Windows 上主动希望恢复提示音时使用
- `off` — 完全禁用轮次后通知

在 Windows 上使用已知 OSC-9 终端（例如 WezTerm on Windows）的用户仍会收到 OSC-9 通知；`off` 回退仅在未检测到已知的 `TERM_PROGRAM` 时生效。

### 已解析但目前未使用（为未来版本保留）

以下键被配置加载器接受，但当前交互式 TUI 或内置工具尚未使用：

- `tools_file`

## 功能标志

功能标志位于 `[features]` 表下，跨 profile 合并。内置工具默认启用，因此你只需设置想要强制开启或关闭的条目。

```toml
[features]
shell_tool = true
subagents = true
web_search = true # 启用标准 web.run 以及兼容性别名 web_search
apply_patch = true
mcp = true
exec_policy = true
```

也可以为单次运行覆盖功能：

- `deepseek-tui --enable web_search`
- `deepseek-tui --disable subagents`

使用 `deepseek-tui features list` 查看已知标志及其生效状态。

## 本地媒体附件

在输入框中使用 `@path/to/file` 将本地文本文件或目录上下文添加到下一条消息。使用 `/attach <path>` 添加本地图片/视频媒体路径，或按 `Ctrl+V` 从剪贴板附加图片。DeepSeek 的公开 Chat Completions API 目前接受文本消息内容，因此媒体附件以显式的本地路径引用方式发送，而非原生图片/视频载荷。附件行在提交前显示在输入框上方；将光标移至输入框开头，按 `↑` 选择附件行，然后按 `Backspace` 或 `Delete` 移除，无需手动编辑占位符文本。

## 托管配置和要求

DeepSeek TUI 支持策略分层模型：

1. 用户配置 + profile + 环境变量覆盖
2. 托管配置（如果存在）
3. 要求验证（如果存在）

Unix 默认：
- 托管配置：`/etc/deepseek/managed_config.toml`
- 要求文件：`/etc/deepseek/requirements.toml`

要求文件格式：

```toml
allowed_approval_policies = ["on-request", "untrusted", "never"]
allowed_sandbox_modes = ["read-only", "workspace-write"]
```

如果配置值违反要求，启动将失败并显示描述性错误。

参见 `docs/capacity_controller.md` 了解公式、干预行为和遥测。

## 关于 `deepseek-tui doctor`

`deepseek-tui doctor` 遵循与 TUI 其余部分相同的配置解析规则。这意味着 `--config` / `DEEPSEEK_CONFIG_PATH` 会被遵循，MCP/技能检查使用解析后的 `mcp_config_path` / `skills_dir`（包括环境变量覆盖）。

要引导创建缺失的 MCP/技能路径，运行 `deepseek-tui setup --all`。也可以运行 `deepseek-tui setup --skills --local` 创建工作区本地的 `./skills` 目录。

`deepseek-tui doctor --json` 打印机器可读报告，跳过实时 API 连接探测。顶层键：`version`、`config_path`、`config_present`、`workspace`、`api_key.source`、`base_url`、`default_text_model`、`mcp`、`skills`、`tools`、`plugins`、`sandbox`、`platform`、`api_connectivity`、`capability`。CI 消费者应使用 `api_key.source`（`env`/`config`/`missing`），而非解析人类可读的 `doctor` 文本。

`capability` 键包含基于静态知识（发布文档、API 指南）而非实时 API 探测的每个提供商的能力信息。顶层子键：`resolved_provider`、`resolved_model`、`context_window`、`max_output`、`thinking_supported`、`cache_telemetry_supported`、`request_payload_mode` 和 `deprecation`。当解析出的模型是已知的旧版别名（例如 `deepseek-chat`、`deepseek-reasoner`）时，`deprecation` 子对象包含 `alias`、`replacement` 和 `notice` 字段。

在 CI 脚本中使用 `capability.context_window` 和 `capability.max_output` 进行上下文窗口预算管理。使用 `capability.thinking_supported` 决定是否配置推理强度。使用 `capability.deprecation` 警告用户有关旧版模型别名的情况。

## Setup 状态、清理和扩展目录

`deepseek-tui setup` 接受以下标志（除了已有的 `--mcp`、`--skills`、`--local`、`--all` 和 `--force`）：

- `--status` — 打印紧凑的单屏状态信息（API 密钥、base URL、模型、MCP/skills/tools/plugins 计数、沙箱、`.env` 存在情况）。只读且不需要网络；可安全在 CI 中运行。如果 `.env` 存在且工作区中有 `.env.example`，状态输出会提示执行 `cp .env.example .env`
- `--tools` — 在 `~/.deepseek/tools/` 中创建脚手架目录，包含描述自声明 frontmatter 约定的 `README.md`（`# name:` / `# description:` / `# usage:`）以及遵循该约定的 `example.sh`。该目录有意识地不会自动加载；通过 MCP、钩子或技能将各个脚本接入代理
- `--plugins` — 在 `~/.deepseek/plugins/` 中创建脚手架目录，包含 `README.md` 和使用与 `SKILL.md` 相同 frontmatter 形状的 `example/PLUGIN.md` 占位文件。插件也不会自动加载；通过技能或 MCP 包装器引用以激活
- `--all` 现在同时搭建 MCP + skills + tools + plugins
- `--clean` — 列出 `~/.deepseek/sessions/checkpoints/latest.json` 和 `offline_queue.json` 如果存在。传入 `--force` 以实际删除它们。此操作永远不会触及真实会话历史或任务队列

`--status` 和 `--clean` 与脚手架标志互斥。

## 为什么引擎会剥离 XML/`[TOOL_CALL]` 文本

DeepSeek TUI 仅通过 API 工具通道（结构化的 `tool_use` / `tool_call` 条目）发送和接收工具调用。`crates/tui/src/core/engine.rs` 中的流式循环识别一组固定的虚假包装器起始标记 —— `[TOOL_CALL]`、`<deepseek:tool_call`、`<tool_call`、`<invoke `、`<function_calls>` —— 并将它们从可见的助手文本中清除，同时绝不会将其转换为结构化工具调用。当包装器被剥离时，循环会为每个轮次发出一个紧凑的 `status` 通知，以便用户看到可见文本为何缩短。任何重新启用基于文本的工具执行的变更都应被视为回归；`crates/tui/tests/protocol_recovery.rs` 中的协议恢复测试锁定了此约定。
