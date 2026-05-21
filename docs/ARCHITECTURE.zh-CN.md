# DeepSeek TUI 架构

[English](ARCHITECTURE.md) | [繁體中文](ARCHITECTURE.zh-TW.md)

本文档为开发者和贡献者提供 DeepSeek TUI 架构概览。

当前边界说明（v0.8.6）：
- `crates/tui` 仍然是 TUI、运行时 API、任务管理器和工具执行循环的活跃最终用户运行时。
- 其他工作区 crate 正在逐步拆分，但它们尚不是唯一的运行时真实来源。
- LSP 子系统（`crates/tui/src/lsp/`）已完全接入引擎的工具后执行路径（`core/engine/lsp_hooks.rs`），在每次 `edit_file`/`apply_patch`/`write_file` 之后提供内联诊断信息。
- Swarm agent 系统已在 v0.8.5 中移除。当前活跃的 v0.8.35 编排界面是持久化 sub-agent 会话（`agent_open` / `agent_eval` / `agent_close`）和持久化 RLM 会话（`rlm_open` / `rlm_eval` / `rlm_configure` / `rlm_close`）。活跃代码库中没有模型可见的 swarm 工具残留。

## 高层概览

```
┌─────────────────────────────────────────────────────────────────┐
│                         用户界面                                 │
│  ┌─────────────────┐  ┌─────────────────┐  ┌────────────────┐  │
│  │   TUI (ratatui) │  │  一次性模式     │  │  配置/CLI      │  │
│  └────────┬────────┘  └────────┬────────┘  └────────┬───────┘  │
└───────────┼─────────────────────┼────────────────────┼──────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                        核心引擎                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Agent 循环 (core/engine.rs)           │   │
│  │  ┌─────────┐  ┌─────────────┐  ┌──────────────────────┐ │   │
│  │  │ 会话    │  │ 轮次管理    │  │ 工具编排             │ │   │
│  │  └─────────┘  └─────────────┘  └──────────────────────┘ │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                     工具与扩展层                                  │
│  ┌──────────┐  ┌──────────┐  ┌─────────┐  ┌────────────────┐   │
│  │  工具    │  │  技能    │  │  钩子   │  │  MCP 服务器    │   │
│  │ (shell,  │  │ (插件)   │  │ (前置/  │  │  (外部)        │   │
│  │  file)   │  │          │  │  后置)  │  │                │   │
│  └──────────┘  └──────────┘  └─────────┘  └────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                  运行时 API + 任务管理                            │
│  ┌─────────────────────────────┐  ┌──────────────────────────┐  │
│  │ HTTP/SSE 运行时 API         │  │ 持久化任务管理器         │  │
│  │ (runtime_api.rs)            │  │ (task_manager.rs)        │  │
│  └─────────────────────────────┘  └──────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
            │                     │
            ▼                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                        LLM 层                                    │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              LLM 客户端抽象 (llm_client.rs)               │  │
│  │  ┌─────────────────┐  ┌─────────────────────────────┐    │  │
│  │  │  DeepSeek 客户端 │  │  兼容客户端 (DeepSeek)        │    │  │
│  │  │   (client.rs)   │  │       (client.rs)           │    │  │
│  │  └─────────────────┘  └─────────────────────────────┘    │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## 模块组织

### 入口点

- **`main.rs`** - CLI 参数解析（clap）、配置加载、入口路由

### 核心组件

- **`core/`** - 主引擎组件
  - `engine.rs` - 引擎状态、操作处理、消息处理
  - `engine/turn_loop.rs` - 流式轮次循环和工具执行编排
  - `engine/capacity_flow.rs` - 容量护栏检查点和干预
  - `session.rs` - 会话状态管理
  - `turn.rs` - 基于轮次的对话处理
  - `events.rs` - UI 更新的事件系统
  - `ops.rs` - 核心操作

### 配置

- **`config.rs`** - 配置加载、配置文件、环境变量
- **`settings.rs`** - 运行时设置管理

### 工作区 Crate

- **`crates/tools`** - 共享工具调用原语，包括 TUI 运行时使用的工具结果/错误/能力类型。
- **`crates/agent`** - 模型/提供商注册表（ModelRegistry），用于将模型 ID 解析为提供商端点。
- **`crates/app-server`** - 用于无头 agent 工作流的 HTTP/SSE + JSON-RPC 应用服务器传输。
- **`crates/config`** - 配置加载、配置文件、环境变量优先级、CLI 运行时覆盖。
- **`crates/core`** - Agent 循环、会话管理、轮次编排、容量流护栏。
- **`crates/execpolicy`** - 工具执行决策的审批/沙箱策略引擎。
- **`crates/hooks`** - 生命周期钩子（stdout、jsonl、webhook），用于工具事件的前置/后置处理。
- **`crates/mcp`** - MCP 客户端 + stdio 服务器，用于 Model Context Protocol 工具服务器。
- **`crates/protocol`** - 请求/响应帧和协议类型。
- **`crates/secrets`** - 操作系统密钥链集成，用于 API 密钥存储。
- **`crates/state`** - SQLite 线程/会话持久化层。
- **`crates/tui-core`** - 事件驱动的 TUI 状态机脚手架。

### LLM 集成

- **`client.rs`** - DeepSeek 官方文档所述的 OpenAI 兼容 Chat Completions API 的 HTTP 客户端
- **`llm_client.rs`** - 带重试逻辑的抽象 LLM 客户端 trait
- **`models.rs`** - API 请求/响应的数据结构

#### DeepSeek API 端点

DeepSeek 暴露 OpenAI 兼容的端点。CLI 使用：
- `https://api.deepseek.com/beta/chat/completions` - 默认 v0.8.16 DeepSeek 模型轮次
- `https://api.deepseek.com/beta/models` - 默认 v0.8.16 实时模型发现和健康检查

`https://api.deepseek.com/v1` 被接受用于 OpenAI SDK 兼容性，并且仍可显式配置以退出仅限 beta 的功能，如严格工具模式、聊天前缀补全和 FIM 补全。DeepSeek 公开文档未为此工作流记录 Responses API 路径；引擎通过 Chat Completions 驱动轮次。

### 工具系统

- **`tools/`** - 内置工具实现
  - `mod.rs` - 工具注册表和通用类型
  - `shell.rs` - Shell 命令执行
  - `file.rs` - 文件读/写操作
  - `todo.rs` - 清单工具以及旧版 todo 别名
  - `tasks.rs` - 模型可见的持久化任务、门控、后台 shell 和 PR 尝试工具
  - `github.rs` - 只读 GitHub 上下文和由 `gh` 支持的受限评论/关闭工具
  - `automation.rs` - 通过 `AutomationManager` 的模型可见调度工具
  - `plan.rs` - 规划工具
  - `subagent.rs` - 持久化 sub-agent 会话（替代已移除的 `agent_swarm` 界面）
  - `spec.rs` - 工具规范
  - `rlm.rs` - 持久化递归语言模型（RLM）会话 — 带有语义辅助调用和 `var_handle` 输出支持的沙盒化 Python REPL

### 扩展系统

- **`mcp.rs`** - 用于外部工具服务器的 Model Context Protocol 客户端
- **`skills.rs`** - 插件/技能加载和执行
- **`hooks.rs`** - 带条件的前置/后置执行钩子

### 用户界面

- **`tui/`** - 终端 UI 组件（基于 ratatui）
  - `app.rs` - 应用状态和消息处理
  - `ui.rs` - 事件处理、流式状态和渲染逻辑
  - `approval.rs` - 工具审批对话框
  - `clipboard.rs` - 剪贴板处理
  - `streaming.rs` - 流式文本收集器

- **`ui.rs`** - 旧版/简单 UI 实用工具

### LSP 集成

- **`lsp/`** - 编辑后诊断注入（#136）
  - `mod.rs` - `LspManager` — 每种语言的延迟传输池 + 配置
  - `client.rs` - `StdioLspTransport` — 通过 stdio 的 JSON-RPC，支持 `didOpen`/`didChange`/`publishDiagnostics`
  - `diagnostics.rs` - 诊断类型、严重程度和 HTML 块渲染器
  - `registry.rs` - 语言检测和默认服务器映射（rust-analyzer、pyright、gopls、clangd、typescript-language-server）
  - 通过 `core/engine/lsp_hooks.rs` 接入引擎 — 在每次成功编辑后调用

### 安全

- **`sandbox/`** - 平台沙箱策略准备和拒绝报告
  - `mod.rs` - 沙箱类型定义
  - `policy.rs` - 沙箱策略配置
  - `seatbelt.rs` - macOS Seatbelt 配置文件生成
  - `landlock.rs` - Linux Landlock 检测和未来辅助合约
  - `windows.rs` - Windows 辅助合约；在有 Job Object 进程隔离辅助程序之前不做广告

### 实用工具

- **`utils.rs`** - 通用实用工具
- **`logging.rs`** - 日志基础设施
- **`compaction.rs`** - 长对话的上下文压缩
- **`pricing.rs`** - 成本估算
- **`prompts.rs`** - 系统 prompt 模板
- **`project_doc.rs`** - 项目文档处理
- **`session.rs`** - 会话序列化
- **`runtime_api.rs`** - HTTP/SSE 运行时 API（`deepseek serve --http`）
- **`runtime_threads.rs`** - 持久化线程/轮次/项存储 + 可重放事件时间线
- **`task_manager.rs`** - 持久化队列、工作池、任务时间线和产物

## 数据流

### 交互式会话

1. 用户在 TUI 中输入
2. 输入由 `core/engine.rs` 处理
3. 消息通过 `llm_client.rs` 发送到 LLM
4. 响应流式返回，在 `client.rs` 中解析
5. 工具调用被提取并通过 `tools/` 执行
6. 工具执行前后触发钩子
7. 结果聚合并发回 LLM
8. 最终响应在 TUI 中渲染

### 崩溃恢复 + 离线队列

1. 在发送用户输入之前，TUI 将检查点快照写入 `~/.deepseek/sessions/checkpoints/latest.json`
2. 启动默认是全新的；之前的会话通过 `--resume`/`--continue`（或 TUI 中的 `Ctrl+R`）显式恢复
3. 在降级/离线时，新 prompt 在内存中排队并镜像到 `~/.deepseek/sessions/checkpoints/offline_queue.json`
4. 队列编辑（`/queue ...`）持续持久化，因此草稿和排队的 prompt 可以在重启后存活
5. 成功的轮次完成会清除活动检查点并写入持久化会话快照
6. Agent/YOLO 轮次还在 `~/.deepseek/snapshots/<project_hash>/<worktree_hash>/.git` 下创建前置/后置轮次的 side-git 工作区快照；`/restore N` 和 `revert_turn` 恢复文件状态而不更改对话历史或用户的 `.git`

### 工具执行

1. LLM 通过 `tool_use` 内容块请求工具
2. 工具注册表查找处理程序
3. 执行前置钩子
4. 如需要则请求审批（非 YOLO 模式）
5. 执行工具（在 macOS 上可能被沙箱化）
6. 执行后置钩子
7. 结果元数据保留在运行时项记录上
8. **LSP 编辑后钩子**（v0.8.6）：如果工具是 `edit_file`/`apply_patch`/`write_file` 且 LSP 已启用，引擎运行 `run_post_edit_lsp_hook()` 收集诊断信息
9. **诊断刷新**（v0.8.6）：在下一次 API 请求之前，`flush_pending_lsp_diagnostics()` 将收集到的错误作为合成用户消息注入
10. 结果返回到 agent 循环

### 后台任务

1. 客户端入队任务（`/task add ...` 或 `POST /v1/tasks`）
2. `task_manager.rs` 将任务 + 队列条目持久化到 `~/.deepseek/tasks`
3. 工作线程取出排队的任务（有界池），转换到 `running`
4. 任务创建/使用运行时线程并启动运行时轮次
5. `runtime_threads.rs` 持久化线程/轮次/项记录 + 单调事件序列
6. 时间线/工具摘要/产物引用增量持久化
7. 清单状态、验证器门控、PR 尝试和受限 GitHub 事件从工具元数据应用到活动任务
8. 最终状态（`completed|failed|canceled`）是持久的，可通过 TUI/API 查询

模型可见的持久化任务工具是同一管理器的表层。它们不引入并行工作系统：`task_create` 入队普通任务，`checklist_*` 更新任务本地进度，`task_gate_run` 和完成的 `task_shell_wait` 附加验证证据，自动化运行入队普通的持久化任务。

### 运行时线程/轮次时间线

1. API/TUI 创建或恢复线程（`/v1/threads*`）
2. 在线程上开始轮次（`/v1/threads/{id}/turns`）
3. 引擎事件映射到项生命周期事件（`item.started|item.delta|item.completed`）
4. 中断/引导操作仅适用于活动轮次
5. 压缩（自动/手动）作为 `context_compaction` 项生命周期事件发出
6. 客户端使用 `/v1/threads/{id}/events?since_seq=<n>` 重放历史并恢复

### 持久化模式门控

- `session_manager.rs`、`runtime_threads.rs` 和 `task_manager.rs` 在持久化记录上嵌入 `schema_version`。
- 加载时，较新的模式版本会因显式错误而被拒绝，而不是静默截断/覆盖数据。
- 这允许安全的前向迁移，并防止二进制文件和存储状态不同步时的数据损坏。

## 扩展点

### 添加新工具

1. 在 `tools/` 中创建处理程序
2. 在 `tools/registry.rs` 中注册
3. 添加工具规范（名称、描述、输入模式）

### 添加 MCP 服务器

1. 在 `~/.deepseek/mcp.json` 中配置
2. 服务器在启动时自动发现
3. 工具自动暴露给 LLM

### 创建技能（Skill）

1. 创建带有 `SKILL.md` 的技能目录
2. 定义技能 prompt 和可选脚本
3. 放置在 `~/.deepseek/skills/` 中

### 添加钩子

在 `~/.deepseek/config.toml` 中配置：

```toml
[[hooks]]
event = "tool_call_before"
command = "echo 'Running tool: $TOOL_NAME'"
```

## 关键设计决策

1. **流式优先**：所有 LLM 响应都流式传输以确保响应性
2. **工具安全**：非 YOLO 模式需要对破坏性操作进行审批，包括有副作用的 MCP 工具
3. **可扩展性**：MCP、技能和钩子允许无需更改代码的自定义
4. **跨平台**：核心在 Linux/macOS/Windows 上运行。沙箱保证是平台特定的：macOS Seatbelt 是活动策略路径；Linux 和 Windows 需要辅助执行才能被视为完整的操作系统沙箱。
5. **最小依赖**：精心选择依赖以确保构建速度
6. **本地优先运行时 API**：HTTP/SSE 端点旨在用于受信任的本地主机访问，目前由 `crates/tui` 运行时提供

## 配置文件

- `~/.deepseek/config.toml` - 主配置
- `/etc/deepseek/managed_config.toml` - 可选托管默认层（Unix）
- `/etc/deepseek/requirements.toml` - 可选允许策略约束（Unix）
- `~/.deepseek/mcp.json` - MCP 服务器配置
- `~/.deepseek/skills/` - 用户技能目录
- `~/.deepseek/sessions/` - 会话历史
- `~/.deepseek/sessions/checkpoints/` - 崩溃检查点 + 离线队列持久化
- `~/.deepseek/snapshots/` - `/restore` 和 `revert_turn` 的 side-git 前置/后置轮次工作区快照
- `~/.deepseek/tasks/` - 后台任务记录、队列、时间线、产物
- `~/.deepseek/audit.log` - 仅追加的审计事件，记录凭证 + 审批/提权操作
