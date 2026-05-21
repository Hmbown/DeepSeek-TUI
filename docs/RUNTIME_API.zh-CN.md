# 运行时 API 与集成合约

[English](RUNTIME_API.md) | [繁體中文](RUNTIME_API.zh-TW.md)

DeepSeek TUI 通过 `deepseek serve --http` 暴露本地运行时 API，并通过 `deepseek doctor --json` 提供机器可读的健康检查。它还通过 `deepseek serve --acp` 为使用 Agent Client Protocol（ACP）通过 stdio 通信的编辑器客户端提供服务。本文档是适用于原生 macOS 工作台应用程序（以及其他本地监管程序）的稳定集成合约，这些程序嵌入 DeepSeek 引擎而无需通过屏幕抓取终端输出。

## 架构

```
macOS 工作台（或任何本地监管程序）
        │
        ├─ deepseek doctor --json   → 机器可读的健康与能力信息
        ├─ deepseek serve --http    → HTTP/SSE 运行时 API
        ├─ deepseek serve --acp     → 面向编辑器（如 Zed）的 ACP stdio 代理
        ├─ deepseek serve --mcp     → MCP stdio 服务器
        └─ deepseek [args]          → 交互式 TUI 会话
```

引擎作为仅限本地的进程运行。所有 API 默认绑定到 `localhost`。无托管中继、无 provider token 托管、无密钥泄露。

## ACP stdio 适配器：`deepseek serve --acp`

`deepseek serve --acp` 通过以换行符分隔的 stdio 使用 JSON-RPC 2.0 与 ACP 兼容的编辑器客户端通信。初始适配器实现了 ACP 基线：

- `initialize`
- `session/new`
- `session/prompt`
- `session/cancel`

Prompt 请求通过已配置的 DeepSeek 客户端和当前默认模型进行路由。响应以 `session/update` 代理消息块的形式发出，后跟带有 `stopReason: "end_turn"` 的 `session/prompt` 响应。

该适配器有意保持保守：它尚未通过 ACP 暴露 shell 工具、文件写入工具、检查点重放或会话加载。如需完整的本地运行时 API，请使用 `deepseek serve --http`；当其他客户端需要将 DeepSeek 的工具作为 MCP 工具使用时，请使用 `deepseek serve --mcp`。

## 能力端点：`deepseek doctor --json`

返回一个 JSON 对象，描述当前安装的就绪状态。适用于 macOS 工作台的健康检查轮询。

```bash
deepseek doctor --json
```

### 响应模式（关键字段）

| 字段 | 类型 | 描述 |
|---|---|---|
| `version` | string | 已安装版本（例如 `"0.8.9"`） |
| `config_path` | string | 已解析的配置文件路径 |
| `config_present` | bool | 配置文件是否存在 |
| `workspace` | string | 默认工作区目录 |
| `api_key.source` | string | `env`、`config` 或 `missing` |
| `base_url` | string | API 基础 URL |
| `default_text_model` | string | 默认模型 |
| `memory.enabled` | bool | 记忆功能是否启用 |
| `memory.path` | string | 记忆文件路径 |
| `memory.file_present` | bool | 记忆文件是否存在 |
| `mcp.config_path` | string | MCP 配置文件路径 |
| `mcp.present` | bool | MCP 配置是否存在 |
| `mcp.servers` | array | 每个服务器的健康状态：`{name, enabled, status, detail}` |
| `skills.selected` | string | 已解析的技能目录 |
| `skills.global.path` / `.present` / `.count` | — | DeepSeek 全局技能目录（`~/.deepseek/skills`） |
| `skills.agents.path` / `.present` / `.count` | — | 工作区 `.agents/skills/` 目录 |
| `skills.agents_global.path` / `.present` / `.count` | — | agentskills.io 全局技能目录（`~/.agents/skills`） |
| `skills.local.path` / `.present` / `.count` | — | `skills/` 目录 |
| `skills.opencode.path` / `.present` / `.count` | — | `.opencode/skills/` 目录 |
| `skills.claude.path` / `.present` / `.count` | — | `.claude/skills/` 目录 |
| `tools.path` / `.present` / `.count` | — | 全局工具目录 |
| `plugins.path` / `.present` / `.count` | — | 全局插件目录 |
| `sandbox.available` | bool | 此操作系统是否支持沙箱 |
| `sandbox.kind` | string 或 null | 沙箱类型（例如 `"macos_seatbelt"`） |
| `storage.spillover.path` / `.present` / `.count` | — | 工具输出溢出目录 |
| `storage.stash.path` / `.present` / `.count` | — | Composer 暂存区 |

### 示例

```json
{
  "version": "0.8.9",
  "config_path": "/Users/you/.deepseek/config.toml",
  "config_present": true,
  "workspace": "/Users/you/projects/deepseek-tui",
  "api_key": {
    "source": "env"
  },
  "base_url": "https://api.deepseek.com/beta",
  "default_text_model": "deepseek-v4-pro",
  "memory": {
    "enabled": false,
    "path": "/Users/you/.deepseek/memory.md",
    "file_present": true
  },
  "mcp": {
    "config_path": "/Users/you/.deepseek/mcp.json",
    "present": true,
    "servers": [
      {"name": "filesystem", "enabled": true, "status": "ok", "detail": "ready"}
    ]
  },
  "sandbox": {
    "available": true,
    "kind": "macos_seatbelt"
  }
}
```

## HTTP/SSE 运行时 API：`deepseek serve --http`

```bash
deepseek serve --http [--host 127.0.0.1] [--port 7878] [--workers 2] [--auth-token TOKEN]
```

默认值：host `127.0.0.1`，port `7878`，2 个 worker（限制在 1–8 之间）。

服务器默认绑定到 `localhost`。通过 CLI 标志进行配置——没有 `[app_server]` 配置节。

默认情况下，现有本地行为保持不变，`/v1/*` 路由不进行身份验证。要为 `/v1/*` 路由要求 bearer token，请传递 `--auth-token TOKEN` 或在启动服务器前设置 `DEEPSEEK_RUNTIME_TOKEN=TOKEN`。`/health` 保持公开，用于本地进程监管和就绪检查。

已认证的客户端可以通过 `Authorization: Bearer TOKEN`、`X-DeepSeek-Runtime-Token: TOKEN` 或 `?token=TOKEN`（适用于无法设置自定义 header 的 EventSource 类型客户端）提供 token。

### 端点

**健康检查**
- `GET /health`

**会话**（旧版会话管理器）
- `GET /v1/sessions?limit=50&search=<子字符串>`
- `GET /v1/sessions/{id}`
- `DELETE /v1/sessions/{id}`
- `POST /v1/sessions/{id}/resume-thread`

**线程**（持久运行时数据模型）
- `GET /v1/threads?limit=50&include_archived=false&archived_only=false`
- `GET /v1/threads/summary?limit=50&search=<可选>&include_archived=false&archived_only=false`
- `POST /v1/threads`
- `GET /v1/threads/{id}`
- `PATCH /v1/threads/{id}`（见下方请求体格式）
- `POST /v1/threads/{id}/resume`
- `POST /v1/threads/{id}/fork`

`archived_only=true` 仅返回已归档的线程（与 `include_archived` 互斥并覆盖其设置）。默认行为不变：`include_archived=false` 和 `archived_only=false` 返回活动线程。于 v0.8.10（#563）添加。

`PATCH /v1/threads/{id}` 请求体——每个字段都是可选的，缺失表示"不变"。至少需要一个字段存在。`title` 和 `system_prompt` 接受空字符串以清除之前设置的值。于 v0.8.10（#562）添加：

```json
{
  "archived": true,
  "allow_shell": false,
  "trust_mode": false,
  "auto_approve": false,
  "model": "deepseek-v4-pro",
  "mode": "agent",
  "title": "用户设置的线程标题",
  "system_prompt": "你是一个有用的助手。"
}
```

**轮次**（线程内部）
- `POST /v1/threads/{id}/turns`
- `POST /v1/threads/{id}/turns/{turn_id}/steer`
- `POST /v1/threads/{id}/turns/{turn_id}/interrupt`
- `POST /v1/threads/{id}/compact`（手动压缩）

**事件**（SSE 重放 + 实时流）
- `GET /v1/threads/{id}/events?since_seq=<u64>`

**兼容性流**（一次性，向后兼容）
- `POST /v1/stream`

**任务**（持久后台工作）
- `GET /v1/tasks`
- `POST /v1/tasks`
- `GET /v1/tasks/{id}`
- `POST /v1/tasks/{id}/cancel`

**自动化**（定时重复工作）
- `GET /v1/automations`
- `POST /v1/automations`
- `GET /v1/automations/{id}`
- `PATCH /v1/automations/{id}`
- `DELETE /v1/automations/{id}`
- `POST /v1/automations/{id}/run`
- `POST /v1/automations/{id}/pause`
- `POST /v1/automations/{id}/resume`
- `GET /v1/automations/{id}/runs?limit=20`

**内省**
- `GET /v1/workspace/status`
- `GET /v1/skills`
- `GET /v1/apps/mcp/servers`
- `GET /v1/apps/mcp/tools?server=<可选>`

**用量**（跨线程的 token/成本聚合）
- `GET /v1/usage?since=<rfc3339>&until=<rfc3339>&group_by=<day|model|provider|thread>`

`since` / `until` 是包含性的 RFC 3339 时间戳，可以省略（无边界）。`group_by` 默认为 `day`。桶按升序键排序。空时间范围产生空的 `buckets`（不会返回 404）。成本通过模型→定价映射计算；模型没有定价条目的轮次会贡献 token 但成本为 `0.0`。于 v0.8.10（#564）添加。

```json
{
  "since": "2026-04-01T00:00:00Z",
  "until": "2026-04-30T23:59:59Z",
  "group_by": "day",
  "totals": {
    "input_tokens": 12345,
    "output_tokens": 6789,
    "cached_tokens": 0,
    "reasoning_tokens": 0,
    "cost_usd": 0.012,
    "turns": 42
  },
  "buckets": [
    {
      "key": "2026-04-30",
      "input_tokens": 1234,
      "output_tokens": 678,
      "cached_tokens": 0,
      "reasoning_tokens": 0,
      "cost_usd": 0.001,
      "turns": 3
    }
  ]
