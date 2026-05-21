# MCP（外部工具服务器）

[English](MCP.md) | [繁體中文](MCP.zh-TW.md)

DeepSeek TUI 可以通过 MCP（模型上下文协议）加载额外的工具。MCP 服务器是 TUI 启动并通过 stdio 与之通信的本地进程。

浏览说明：
- `web.run` 是规范的内置浏览工具。
- `web_search` 作为兼容性别名仍然可用，服务于旧版 prompt 和集成。

服务器模式说明：
- `deepseek-tui serve --mcp` 运行 MCP stdio 服务器。
- `deepseek-tui serve --http` 运行运行时 HTTP/SSE API（独立模式）。
- `deepseek` 调度器将 `deepseek mcp-server` 作为等效的 stdio 入口点暴露，供分离式 CLI 使用。

## 初始化 MCP 配置

在你的解析后的 MCP 路径创建初始 MCP 配置：

```bash
deepseek-tui mcp init
```

`deepseek-tui setup --mcp` 在执行 skills 设置的同时执行相同的 MCP 初始化。

常用管理命令：

```bash
deepseek-tui mcp list
deepseek-tui mcp tools [server]
deepseek-tui mcp add <name> --command "<cmd>" --arg "<arg>"
deepseek-tui mcp add <name> --url "http://localhost:3000/mcp"
deepseek-tui mcp enable <name>
deepseek-tui mcp disable <name>
deepseek-tui mcp remove <name>
deepseek-tui mcp validate
```

## TUI 内管理器

在交互式 TUI 中，`/mcp` 会打开一个紧凑的管理器，显示解析后的 MCP 配置路径。它会展示每个已配置的服务器、其启用还是禁用状态、传输方式、命令或 URL、超时值、连接错误以及发现运行后发现的工具/资源/prompt。

支持的 TUI 内操作：

```text
/mcp init
/mcp init --force
/mcp add stdio <name> <command> [args...]
/mcp add http <name> <url>
/mcp enable <name>
/mcp disable <name>
/mcp remove <name>
/mcp validate
/mcp reload
```

`/mcp validate` 和 `/mcp reload` 重新连接以进行 UI 发现并刷新管理器快照。从 TUI 进行的配置编辑会立即写入，但模型可见的 MCP 工具池不会热重载；管理器会将其标记为需要重启，直到 TUI 重启为止。

## 配置文件位置

默认路径：

- `~/.deepseek/mcp.json`

覆盖方式：

- 配置：`mcp_config_path = "/path/to/mcp.json"`
- 环境变量：`DEEPSEEK_MCP_CONFIG=/path/to/mcp.json`

`deepseek-tui mcp init`（以及 `deepseek-tui setup --mcp`）会写入此解析后的路径。

交互式 `/config` 编辑器也会暴露 `mcp_config_path`。在 TUI 中更改它会更新 `/mcp` 使用的路径，并需要在模型可见的 MCP 工具池重建之前重启。

编辑文件或更改 `mcp_config_path` 后，请重启 TUI。

## 工具命名

发现的 MCP 工具以以下格式暴露给模型：

- `mcp_<server>_<tool>`

示例：名为 `git` 的服务器，其中包含名为 `status` 的工具，将变成 `mcp_git_status`。

命令面板包含按服务器分组的 MCP 条目。它会显示禁用和失败的服务器，而不是隐藏它们，并使用与展示给模型的相同的运行时工具名称。

## 资源和 Prompt 辅助工具

CLI 在启用 MCP 时也会暴露辅助工具：

- `list_mcp_resources`（可选的 `server` 过滤器）
- `list_mcp_resource_templates`（可选的 `server` 过滤器）
- `mcp_read_resource` / `read_mcp_resource`（别名）
- `mcp_get_prompt`

## 最小示例

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

你还可以使用 `mcpServers` 而不是 `servers`，以与其他客户端兼容。

## 将 DeepSeek 作为 MCP 服务器运行

你可以将本地 DeepSeek 二进制文件注册为 MCP 服务器，以便其他 DeepSeek 会话（或任何 MCP 客户端）调用其工具。

### 快速设置

```bash
deepseek-tui mcp add-self
```

这将解析当前的二进制文件路径，生成一个运行 `deepseek-tui serve --mcp` 的配置条目，并将其写入你的 MCP 配置文件。默认服务器名称为 `deepseek`。

选项：

- `--name <NAME>` — 自定义服务器名称（默认：`deepseek`）
- `--workspace <PATH>` — 服务器的工作空间目录

### 手动配置

在 `~/.deepseek/mcp.json` 中的等效手动条目：

```json
{
  "servers": {
    "deepseek": {
      "command": "/path/to/deepseek",
      "args": ["serve", "--mcp"],
      "env": {}
    }
  }
}
```

`deepseek-tui` 二进制文件直接支持 `serve --mcp`。`deepseek` 调度器提供等效的 `deepseek mcp-server` stdio 入口点。使用你 `PATH` 上任意可用的（运行 `which deepseek` 或 `which deepseek-tui` 来查找完整路径）。`mcp add-self` 命令会自动解析正确的二进制文件。

### 前提条件

- `command` 中引用的二进制文件必须存在且可执行。
- MCP 服务器作为子进程通过 stdio 运行 — 不需要网络端口。
- 每个 MCP 客户端会话都会生成自己的服务器进程。

### 工具命名

来自自托管 DeepSeek 服务器的工具遵循标准命名约定：

- `mcp_deepseek_<tool>`（如果服务器命名为 `deepseek`）

例如，`shell` 工具变为 `mcp_deepseek_shell`。

### MCP 服务器 vs HTTP/SSE API vs ACP

| | `deepseek-tui serve --mcp` | `deepseek-tui serve --http` | `deepseek-tui serve --acp` |
|---|---|---|---|
| **协议** | MCP stdio | HTTP/SSE JSON-RPC | ACP stdio |
| **用途** | MCP 客户端的工具服务器 | 应用程序的运行时 API | Zed/自定义 ACP 客户端的编辑器 agent |
| **配置** | `~/.deepseek/mcp.json` 条目 | 直接 URL 连接 | 编辑器 `agent_servers` 自定义命令 |
| **生命周期** | 按客户端会话生成 | 长期运行的守护进程 | 按编辑器 agent 会话生成 |

当你希望 DeepSeek 工具对其他 MCP 客户端可用时，使用 `mcp add-self`。
当你构建直接消费 API 的应用程序时，使用 `serve --http`。
当编辑器想要作为 ACP agent 与 DeepSeek 通信时，使用 `serve --acp`。

### 验证

添加后，测试连接：

```bash
deepseek-tui mcp validate
deepseek-tui mcp tools deepseek
```

## 服务器字段

每个服务器的设置：

- `command`（字符串，必需）
- `args`（字符串数组，可选）
- `env`（对象，可选）
- `connect_timeout`、`execute_timeout`、`read_timeout`（秒，可选）
- `disabled`（布尔值，可选）
- `enabled`（布尔值，可选，默认 `true`）
- `required`（布尔值，可选）：如果此服务器无法初始化，启动/连接验证将失败。
- `enabled_tools`（数组，可选）：此服务器工具名称的允许列表。
- `disabled_tools`（数组，可选）：在 `enabled_tools` 之后应用的拒绝列表。

## 安全说明

MCP 工具现在通过与内置工具相同的工具审批框架运行。只读 MCP 辅助工具（资源/prompt 列出和读取）可以在建议性审批模式下无需 prompt 运行，而有副作用的 MCP 工具需要审批。

你仍然应该只配置你信任的 MCP 服务器，并将 MCP 服务器配置视为等同于在你的机器上运行代码。

## 故障排除

- 运行 `deepseek-tui doctor` 来确认它解析的 MCP 配置路径以及该路径是否存在。
- 在 TUI 中，运行 `/mcp validate` 来刷新可见的服务器/工具快照。
- 如果 MCP 配置缺失，运行 `deepseek-tui mcp init --force` 重新生成它。
- 如果工具没有出现，请验证服务器命令在你的 shell 中是否正常工作，以及服务器是否支持 MCP `tools/list`。
