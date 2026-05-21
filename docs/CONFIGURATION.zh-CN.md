# 配置

[English](CONFIGURATION.md)

DeepSeek TUI 从 TOML 文件和环境变量中读取配置。进程启动时，如果存在工作区本地的 `.env` 文件，也会加载它。使用受追踪的 `.env.example` 作为模板；将其复制为 `.env`，然后根据需要编辑 provider 和安全开关。

## 查找位置

默认配置路径：

- `~/.deepseek/config.toml`

覆盖方式：

- CLI：`deepseek --config /path/to/config.toml`
- 环境变量：`DEEPSEEK_CONFIG_PATH=/path/to/config.toml`

如果同时设置，`--config` 优先。环境变量覆盖在文件加载后应用。

### 项目级覆盖（#485）

当 TUI 在包含 `<workspace>/.deepseek/config.toml` 文件的工作区中启动时，该文件中声明的值会合并到全局配置之上。这样仓库可以锁定自己的 provider、model、沙箱策略或审批策略，而无需修改用户的 `~/.deepseek/config.toml`。传递 `--no-project-config` 可跳过此次启动的覆盖。

项目覆盖中支持的键（仅顶层字段）：

| 键 | 效果 |
|---|---|
| `provider` | 切换后端（例如企业仓库使用 `"nvidia-nim"`） |
| `model` | 覆盖 `default_text_model` |
| `api_key` | 使用每个仓库的密钥（通常从 `.env` 中读取，**不要提交**） |
| `base_url` | 指向自托管端点 |
| `reasoning_effort` | 对复杂仓库强制设为 `"high"` / `"max"` |
| `approval_policy` | 对于有意见的仓库设为 `"never"` / `"on-request"` / `"untrusted"` |
| `sandbox_mode` | `"read-only"` / `"workspace-write"` / `"danger-full-access"` |
| `mcp_config_path` | 每个仓库的 MCP 服务器集合 |
| `notes_path` | 将笔记保存在仓库内 |
| `max_subagents` | 为受限仓库限制并发数（限制在 1 到 20） |
| `allow_shell` | 控制 shell 工具访问权限（设为 `false` 关闭） |

覆盖范围故意保持狭窄——它只包含仓库维护者最可能希望跨贡献者标准化的字段。其他设置（skills_dir、hooks、capacity、retry 等）保持用户全局。如果你的仓库需要更多，请提交 issue 描述具体用例。

`deepseek` 外观命令和 `deepseek-tui` 二进制文件共享同一个 DeepSeek 认证和模型默认配置。`deepseek auth set --provider deepseek`（以及旧的 `deepseek login --api-key ...` 别名）将密钥保存到 `~/.deepseek/config.toml`，`deepseek --model deepseek-v4-flash` 会作为 `DEEPSEEK_MODEL` 转发给 TUI。

凭据查找顺序为 `config -> keyring -> env`，在任何显式 CLI `--api-key` 之后。运行 `deepseek auth status` 可检查当前活动的 provider 的配置文件、OS 密钥环后端、环境变量、获胜来源和最后四位标签，而不会打印密钥本身。该命令仅探测当前活动 provider 的密钥环条目。

对于托管的通用 OpenAI 兼容或自托管 provider，设置 `provider = "nvidia-nim"`、`"openai"`、`"atlascloud"`、`"fireworks"`、`"sglang"`、`"vllm"` 或 `"ollama"`，或传递 `deepseek --provider <name>`。外观命令将 provider 凭据保存到共享用户配置，并将解析后的密钥、base URL、provider 和 model 转发给 TUI 进程。使用 `deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"` 或 `deepseek auth set --provider openai --api-key "YOUR_OPENAI_COMPATIBLE_API_KEY"` 或 `deepseek auth set --provider atlascloud --api-key "YOUR_ATLASCLOUD_API_KEY"` 或 `deepseek auth set --provider fireworks --api-key "YOUR_FIREWORKS_API_KEY"` 通过外观命令保存 provider 密钥。通用 `openai` provider 默认使用 `https://api.openai.com/v1`，接受 `OPENAI_BASE_URL`，并按原样传递 model ID 给 OpenAI 兼容网关。`atlascloud` 默认使用 `https://api.atlascloud.ai/v1`，接受 `ATLASCLOUD_BASE_URL`，并使用 `deepseek-ai/deepseek-v4-flash` 作为默认 model。SGLang、vLLM 和 Ollama 是自托管的，默认可以无 API 密钥运行。Ollama 默认使用 `http://localhost:11434/v1`，并按原样发送 model 标签如 `deepseek-coder:1.3b` 或 `qwen2.5-coder:7b`。自托管 provider 和回环自定义 URL（`localhost`、`127.0.0.1`、`[::1]`、`0.0.0.0`）不会读取密钥存储，除非显式请求 API 密钥认证；当本地服务器确实需要 Bearer 认证时，请使用环境变量或配置文件中的密钥。

### 自定义 OpenAI 兼容网关

对于实现 OpenAI Chat Completions API 的第三方服务，使用内置的 `openai` provider 名称并将其 provider 表指向网关：

```toml
provider = "openai"
default_text_model = "your-model-id"

[providers.openai]
api_key = "YOUR_OPENAI_COMPATIBLE_API_KEY"
base_url = "https://your-gateway.example/v1"
```

不要发明自定义 provider 名称；`provider` 必须是上面列出的已知 provider 之一。将端点放在 `[providers.openai]` 下，而不是旧的顶层 `base_url`