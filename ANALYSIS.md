# DeepSeek-TUI 项目分析文档

> 分析时间：2026-05-06
> 项目版本：v0.8.14
> 仓库地址：https://github.com/Hmbown/DeepSeek-TUI

---

## 一、项目概述

DeepSeek-TUI 是一个**完全在终端运行的 AI 编程助手（coding agent）**，专为 DeepSeek V4 模型打造。它用纯 Rust 编写，无需 Node.js 或 Python 运行时，提供：

- 1M token 上下文窗口
- Thinking-mode（链式推理）流式输出
- 完整工具套件（shell、文件操作、git、web 搜索、MCP 服务器、子代理等）
- 三种工作模式：Plan（只读探索）、Agent（交互审批）、YOLO（自动审批）
- 持久任务队列、会话保存/恢复、工作区回滚
- HTTP/SSE Runtime API 用于无头代理工作流
- MCP 协议支持外部工具服务器
- LSP 诊断集成（rust-analyzer、pyright、gopls、clangd、typescript-language-server）

---

## 二、项目架构

### 2.1 整体分层

```
┌─────────────────────────────────────────────────────────────┐
│                    用户界面层 (User Interface)                │
│  TUI (ratatui + crossterm)  |  One-shot Mode  |  CLI/REPL   │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│                    核心引擎层 (Core Engine)                    │
│  Agent Loop ─ Session ─ Turn Mgmt ─ Tool Orchestration       │
│  Capacity Controller ─ Context Compaction ─ LSP Hooks        │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│                 工具与扩展层 (Tool & Extension)                │
│  Tools (shell/file/git/web) | Skills | Hooks | MCP Servers   │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│              运行时 API + 任务管理层                            │
│  HTTP/SSE Runtime API  |  Persistent Task Manager            │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│                 LLM 客户端层 (LLM Layer)                       │
│  LlmClient trait → DeepSeekClient (reqwest + SSE streaming)  │
│  支持 DeepSeek / NVIDIA NIM / Fireworks / SGLang / OpenRouter │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Workspace Crates（14 个）

| Layer | Crate | 用途 |
|-------|-------|------|
| **叶子** | `protocol` | 所有跨 crate 的数据类型定义（Thread、EventFrame、ToolPayload 等） |
| **叶子** | `config` | 配置加载、profiles、环境变量优先级 |
| **叶子** | `state` | SQLite 会话/线程持久化层 |
| **叶子** | `tui-core` | TUI 状态机脚手架 |
| **中间层** | `tools` | 工具注册表、ToolHandler trait、ToolSpec |
| **中间层** | `mcp` | MCP 服务器管理 + JSON-RPC stdio 传输 |
| **中间层** | `hooks` | 生命周期钩子（stdout/jsonl/webhook） |
| **中间层** | `execpolicy` | 审批/沙箱策略引擎 |
| **中间层** | `agent` | ModelRegistry 模型注册与解析 |
| **中间层** | `secrets` | OS keyring API key 存储 |
| **集成层** | `core` | Runtime 运行时（组合所有子系统） |
| **应用层** | `app-server` | HTTP/SSE + JSON-RPC 应用服务器 |
| **应用层** | `tui` | TUI 二进制（主体，243 个 .rs 文件） |
| **入口层** | `cli` | `deepseek` 命令行分发器 |

### 2.3 依赖图

```
Layer 0 (叶子):  protocol, config, state, tui-core
Layer 1:         tools, mcp, hooks, execpolicy
Layer 2:         agent
Layer 3:         core
Layer 4:         app-server, tui
Layer 5:         cli (最终入口)
```

### 2.4 核心源文件说明（crates/tui/src/）

| 文件/目录 | 大小 | 用途 |
|-----------|------|------|
| `main.rs` | 162KB | CLI 入口、参数解析、模式路由 |
| `client.rs` | 66KB | DeepSeekClient 结构体、HTTP 客户端、重试、限流 |
| `client/chat.rs` | 60KB | Chat Completions API 核心：请求构建、SSE 流解析 |
| `llm_client/mod.rs` | 34KB | LlmClient trait 定义、重试逻辑 |
| `core/engine.rs` | 74KB | 引擎状态机、操作处理 |
| `core/engine/turn_loop.rs` | 82KB | 主流式推理循环 |
| `core/engine/streaming.rs` | 5KB | 流超时/重试常量和状态 |
| `core/engine/tool_execution.rs` | 14KB | 工具执行编排 |
| `core/engine/lsp_hooks.rs` | 5KB | LSP 编辑后诊断注入 |
| `config.rs` | 152KB | 配置系统 |
| `compaction.rs` | 89KB | 上下文压缩算法 |
| `runtime_threads.rs` | 172KB | 运行时线程/轮次/事件持久化 |
| `runtime_api.rs` | 103KB | HTTP/SSE 运行时 API 服务器 |
| `task_manager.rs` | 65KB | 持久任务队列 |
| `tools/registry.rs` | 45KB | 工具注册表 |
| `tools/shell.rs` | 85KB | Shell 命令执行 |
| `tools/subagent/mod.rs` | 130KB | 子代理系统 |
| `mcp.rs` | 70KB | MCP 客户端实现 |
| `tui/app.rs` | 168KB | TUI 应用状态与消息处理 |
| `tui/ui.rs` | 295KB | TUI 渲染逻辑 |

---

## 三、API 梳理

### 3.1 DeepSeek Chat Completions API（核心调用）

#### 端点

| 端点 | 方法 | 用途 |
|------|------|------|
| `{base_url}/chat/completions` | POST | 正常和流式模型推理 |
| `{base_url}/models` | GET | 模型发现与健康检查 |

默认 `base_url`：`https://api.deepseek.com`

#### 流式请求示例

```json
{
  "model": "deepseek-v4-pro",
  "messages": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "..." },
    { "role": "assistant", "content": "...", "tool_calls": [...] },
    { "role": "tool", "content": "...", "tool_call_id": "..." }
  ],
  "max_tokens": 8192,
  "stream": true,
  "stream_options": { "include_usage": true },
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "exec_shell",
        "description": "...",
        "parameters": { ... }
      }
    }
  ],
  "tool_choice": "auto",
  "reasoning_effort": "max"
}
```

#### 认证

```
Authorization: Bearer <DEEPSEEK_API_KEY>
Content-Type: application/json
```

#### Reasoning Effort 映射

| 用户设置 | API 字段 |
|----------|----------|
| `off` | `thinking.type = "disabled"` |
| `high` | `reasoning_effort = "high"` |
| `max` | `reasoning_effort = "max"` |

#### 支持的提供商

| Provider | base_url | 模型 ID 格式 |
|----------|----------|-------------|
| DeepSeek | `https://api.deepseek.com` | `deepseek-v4-pro` / `deepseek-v4-flash` |
| NVIDIA NIM | `https://integrate.api.nvidia.com/v1` | `deepseek-ai/deepseek-v4-pro` |
| Fireworks | `https://api.fireworks.ai/inference/v1` | `accounts/fireworks/models/deepseek-v4-pro` |
| SGLang | `http://localhost:30000/v1` | `deepseek-ai/DeepSeek-V4-Pro` |
| OpenRouter | `https://openrouter.ai/api/v1` | `deepseek/deepseek-v4-pro` |
| Novita | — | `deepseek/deepseek-v4-pro` |

### 3.2 Runtime HTTP/SSE API（`deepseek serve --http`）

本地 HTTP 服务，供外部 UI/自动化脚本调用（默认绑定 `127.0.0.1`）：

| 路径 | 方法 | 用途 |
|------|------|------|
| `/v1/threads` | POST | 创建/启动对话线程 |
| `/v1/threads/{id}` | GET | 读取线程详情 |
| `/v1/threads/{id}/turns` | POST | 启动推理轮次 |
| `/v1/threads/{id}/events?since_seq=N` | GET (SSE) | 实时事件流回放 |
| `/v1/tasks` | POST | 创建后台任务 |
| `/v1/tasks/{id}` | GET | 查询任务状态 |
| `/v1/app/status` | GET | 应用状态和作业列表 |

### 3.3 MCP JSON-RPC stdio API

`deepseek mcp-server` 启动的 stdio 服务，遵循 JSON-RPC 2.0：

| 方法 | 用途 |
|------|------|
| `initialize` / `capabilities` | 初始化和能力查询 |
| `healthz` | 健康检查 |
| `tools/list` | 列出可用工具（支持 `server` 过滤） |
| `tools/call` | 调用工具（支持 qualified name `mcp__server__tool`） |
| `resources/list` / `resources/read` | 资源管理 |
| `server/list` | 列出注册的 MCP 服务器 |
| `server/register` | 注册新服务器 |
| `server/start` / `server/stop` | 启停服务器 |
| `server/unregister` | 注销服务器 |
| `shutdown` | 关闭 |

### 3.4 内置工具清单

| 工具名称 | 实现文件 | 功能说明 |
|----------|----------|----------|
| `exec_shell` | `tools/shell.rs` | Shell 命令执行（带超时、cwd 限制、PTY） |
| `read_file` | `tools/file.rs` | 读取文件内容（支持 offset/limit） |
| `write_file` | `tools/file.rs` | 写入文件 |
| `edit_file` | `tools/file.rs` | 基于搜索替换的文件编辑 |
| `apply_patch` | `tools/apply_patch.rs` | 应用统一 diff 补丁 |
| `grep_files` | `tools/search.rs` | 正则搜索文件内容 |
| `file_search` | `tools/file_search.rs` | 按文件名模式搜索 |
| `web_search` / `web.run` | `tools/web_search.rs` / `web_run.rs` | Web 搜索 |
| `fetch_url` | `tools/fetch_url.rs` | 获取 URL 内容（带 SSRF 防护） |
| `git_status` / `git_diff` / `git_log` | `tools/git.rs` | Git 操作 |
| `git_history` | `tools/git_history.rs` | Git 历史查询 |
| `github_pr_*` / `github_issue_*` | `tools/github.rs` | GitHub 操作（通过 `gh` CLI） |
| `agent_spawn` | `tools/subagent/mod.rs` | 子代理生成（最多 10-20 并发） |
| `rlm_query` | `tools/rlm.rs` | RLM 并行推理（1-16 个 flash 子代理） |
| `update_plan` | `tools/plan.rs` | 更新工作计划 |
| `checklist_write` / `checklist_update` | `tools/todo.rs` | 清单管理 |
| `task_create` / `task_gate_run` | `tools/tasks.rs` | 持久任务管理 |
| `remember` | `tools/remember.rs` | 用户记忆持久化 |
| `load_skill` | `tools/skill.rs` | 加载技能包 |
| `revert_turn` | `tools/revert_turn.rs` | 工作区回滚 |
| `automation_*` | `tools/automation.rs` | 自动化调度 |
| `mcp__*` | 通过 `mcp.rs` 路由 | MCP 外部工具服务器 |

### 3.5 CLI 命令接口

```bash
deepseek                              # 交互式 TUI
deepseek "explain this function"      # 一次性提示
deepseek --model deepseek-v4-flash    # 模型覆盖
deepseek --yolo                       # 自动审批模式
deepseek auth set --provider deepseek # 保存 API key
deepseek doctor                       # 诊断检查
deepseek models                       # 列出可用模型
deepseek sessions                     # 列出已保存会话
deepseek resume --last                # 恢复最近会话
deepseek fork <SESSION_ID>            # 分叉会话
deepseek serve --http                 # 启动 HTTP/SSE API
deepseek mcp list                     # 列出 MCP 服务器
deepseek mcp-server                   # 运行 MCP stdio 服务器
deepseek pr <N>                       # PR 审查
deepseek config get/set/list/path     # 配置管理
deepseek model list/resolve           # 模型解析
deepseek thread list/read/archive     # 线程管理
deepseek metrics                      # 使用量统计
deepseek update                       # 自动更新
```

---

## 四、核心实现原理

### 4.1 双二进制分发架构

```
deepseek (dispatcher/CLI, crates/cli)
    │
    ├─ 轻量命令（auth/config/model/thread/sandbox）→ 就地处理
    │
    └─ 交互命令 → delegate_to_tui()
         → 查找同级 deepseek-tui 二进制
         → 传递环境变量 (DEEPSEEK_API_KEY, DEEPSEEK_MODEL, DEEPSEEK_PROVIDER...)
         → 启动子进程
```

优势：dispatcher 轻量快速，TUI 可独立编译/更新。

### 4.2 事件驱动引擎

```
┌─────────────┐    Op (Submit/Cancel/Steer)    ┌──────────────┐
│  TUI Layer  │ ──────────────────────────────→ │    Engine    │
│  (app.rs)   │ ←────────────────────────────── │ (engine.rs)  │
└─────────────┘    Event (Status/Delta/Tool)    └──────────────┘
```

- **Op 枚举**：UI → Engine 的操作
  - `Submit { input, mode, attachments }`
  - `Cancel`
  - `Steer { input }` — 中途补充信息
  - `ApprovalResponse { decision }`
- **Event 枚举**：Engine → UI 的事件
  - `Status { message }`
  - `ContentDelta { text }`
  - `ThinkingDelta { text }`
  - `ToolCallStart { name, args }`
  - `ToolCallResult { name, output }`
  - `TurnComplete { status, cost }`
  - `Error { message }`

Engine 运行在 tokio 后台任务中，通过 `mpsc` channel 双向通信。

### 4.3 流式推理循环（turn_loop.rs）

```rust
loop {
    // 1. 检查取消
    if cancel_token.is_cancelled() → return Interrupted

    // 2. 处理 steer 输入（用户中途补充信息）
    while rx_steer.try_recv() → inject as user message

    // 3. 刷新系统提示（含模式、技能、记忆、计划状态等）
    refresh_system_prompt(mode)

    // 4. 检查 max_steps
    if turn.at_max_steps() → return MaxStepsReached

    // 5. 容量守卫检查点
    capacity_checkpoint() → may trigger compaction/replan

    // 6. 构建 MessageRequest
    let request = build_request(messages, tools, system_prompt, model, reasoning_effort)

    // 7. 调用 LLM 流式 API
    let stream = client.create_message_stream(request).await?

    // 8. 逐事件处理
    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::ContentDelta { text } → emit to UI
            StreamEvent::ThinkingDelta { text } → emit thinking
            StreamEvent::ContentBlockStart { tool_use } → collect tool call
            StreamEvent::MessageDelta { stop_reason } → finalize
        }
    }

    // 9. 如果有工具调用：
    for tool_call in pending_tool_calls {
        // 审批检查
        // 执行工具
        // LSP post-edit hook（编辑类工具后收集诊断）
        // 注入结果到消息历史
    }

    // 10. 如果无工具调用 → 轮次结束
    if no_tool_calls → return Completed
}
```

### 4.4 SSE 流解析机制（client/chat.rs）

**核心流程**：

1. **发送请求**：`POST {base_url}/chat/completions`，body 含 `"stream": true`
2. **获取字节流**：`response.bytes_stream()` → 原始字节流
3. **行分割**：手动按 `\n` 分割，处理 `\r\n`
4. **SSE 解析**：
   - 空行 = 事件边界
   - `data:` 行累积到 `line_buf`
   - `[DONE]` 标记流结束
5. **JSON 解析**：每个 data chunk 通过 `parse_sse_chunk()` 转为 `StreamEvent`
6. **特殊处理**：
   - `reasoning_content` 字段 → thinking-mode 支持
   - `tool_calls` 增量 JSON → 工具调用收集
   - `usage` → token 计费统计

**保护机制**：

| 机制 | 参数 | 说明 |
|------|------|------|
| 空闲超时 | 300s（可配置） | 无数据时判定流卡死 |
| Buffer 上限 | 10MB | 防止内存溢出 |
| 背压检测 | 8MB 高水位 | 暂停 10ms |
| 透明重试 | 最多 2 次 | 仅在无内容收到时 |
| 总时长上限 | 30 分钟 | 极端情况兜底 |
| 连续错误上限 | 5 次 | 超过则放弃 |

### 4.5 DeepSeekClient 结构

```rust
pub struct DeepSeekClient {
    http_client: reqwest::Client,           // 带 Bearer token 的 HTTP 客户端
    api_key: String,                        // API 密钥
    base_url: String,                       // API 基础 URL
    api_provider: ApiProvider,              // 提供商标识
    retry: RetryPolicy,                     // 重试策略（指数退避）
    default_model: String,                  // 默认模型
    connection_health: Arc<ConnectionHealth>, // 连接健康状态机
    rate_limiter: Arc<TokenBucket>,         // 客户端侧令牌桶限流
}
```

**连接健康状态机**：
- `Healthy` → 正常
- `Degraded` → 连续 2 次失败后降级
- `Recovering` → 冷却 15s 后探测恢复

### 4.6 上下文管理

#### 压缩策略（Compaction）

```
Context Size →  L1 (192K)  →  L2 (384K)  →  L3 (576K)  →  Cycle (768K)
                 轻量压缩       中度压缩       深度压缩       新 Cycle
```

- **No-LLM 预处理**：先机械删除重复 read_file、裁剪冗余工具输出
- **LLM 摘要**：使用 deepseek-v4-flash 进行上下文摘要
- **Prefix-cache 感知**：保留消息前缀以命中 DeepSeek KV cache

#### Cycle Manager

超长会话自动分段：
1. 检测到上下文即将超限
2. 归档当前 cycle（保存完整历史）
3. 生成 briefing（关键发现 + 进度 + 待做事项）
4. 用 briefing 作为种子开启新 cycle

#### Working Set

跟踪会话中涉及的文件：
- 文件读写自动纳入 working set
- 提供智能上下文建议
- 支持 `@path` 手动附加文件

### 4.7 安全与审批

#### ExecPolicy 引擎

```
命令 → 前缀匹配规则 → 决策:
  - Skip (bypass_sandbox=true) — 安全命令直接执行
  - NeedsApproval — 弹出审批对话框
  - Forbidden — 拒绝执行
```

#### 审批策略

| 策略 | 行为 |
|------|------|
| `on-request` | 默认，按需审批 |
| `untrusted` | 所有操作都需审批 |
| `never` | YOLO 模式，自动通过 |

#### 沙箱模式

| 模式 | 说明 |
|------|------|
| `read-only` | 只读访问 |
| `workspace-write` | 仅允许工作区内写入 |
| `danger-full-access` | 完全访问 |
| `external-sandbox` | 通过外部 OpenSandbox API 执行 |

平台实现：
- macOS: Seatbelt profile
- Linux: Landlock
- Windows: Job Objects
- 外部: OpenSandbox HTTP API

### 4.8 工具注册与分发

```rust
// ToolRegistry 核心接口
pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
    specs: HashMap<String, ConfiguredToolSpec>,
}

// ToolHandler trait
#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn kind(&self) -> ToolKind;        // Function | Mcp
    fn is_mutating(&self) -> bool;     // 是否有副作用
    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput>;
}

// 分发流程
dispatch(call) → 查找 handler → kind 校验 → mutating 检查 → 超时包装 → 并行控制 → 执行
```

### 4.9 子代理系统

- **agent_spawn** 工具生成子代理
- 子代理类型：`researcher`、`coder`、`reviewer` 等
- 最多 10-20 并发（可配置）
- 子代理共享工具注册表但独立上下文
- 通过 Mailbox 机制与父代理通信

### 4.10 RLM（Recursive Language Model）

- `rlm_query` 工具并行扇出 1-16 个 `deepseek-v4-flash` 子请求
- 用于批量分析、并行推理
- 结果聚合后返回父上下文

---

## 五、数据流

### 5.1 交互式会话

```
1. 用户输入 → TUI composer
2. TUI 发送 Op::Submit → Engine
3. Engine 构建 MessageRequest（含完整消息历史）
4. Engine 调用 LlmClient::create_message_stream()
5. SSE 流逐 chunk 解析 → StreamEvent
6. Engine 发送 Event::ContentDelta → TUI 实时渲染
7. 如有 tool_calls：
   a. Engine 发送 Event::ToolCallStart → TUI 显示
   b. 审批检查（非 YOLO 模式）
   c. 执行工具
   d. LSP post-edit hook
   e. 注入结果到消息历史
   f. 回到步骤 3 继续循环
8. 无工具调用 → Engine 发送 Event::TurnComplete
9. TUI 显示最终回复，等待下一轮输入
```

### 5.2 工具执行

```
1. LLM 返回 tool_use content block
2. 工具注册表查找 handler
3. ExecPolicy 预检查
4. Pre-execution hooks 运行
5. 审批请求（非 YOLO 模式）
6. 工具执行（可能沙箱化）
7. Post-execution hooks 运行
8. LSP post-edit hook（编辑类工具）
9. 诊断注入到上下文
10. 结果返回 agent loop
```

### 5.3 崩溃恢复

```
1. 每次发送前写入检查点到 ~/.deepseek/sessions/checkpoints/latest.json
2. 离线时队列入 offline_queue.json
3. 重启后通过 --resume / Ctrl+R 恢复
4. 成功完成清除检查点，写入持久会话快照
5. Agent/YOLO 模式还有 side-git 工作区快照用于 /restore
```

---

## 六、配置文件

| 文件 | 位置 | 用途 |
|------|------|------|
| `config.toml` | `~/.deepseek/` | 主配置（API key、模型、安全策略） |
| `<workspace>/.deepseek/config.toml` | 项目级 | 项目覆盖（禁止 api_key/base_url） |
| `mcp.json` | `~/.deepseek/` | MCP 服务器配置 |
| `skills/` | `~/.deepseek/` | 用户技能目录 |
| `sessions/` | `~/.deepseek/` | 会话历史和检查点 |
| `tasks/` | `~/.deepseek/` | 后台任务记录 |
| `snapshots/` | `~/.deepseek/` | 工作区 side-git 快照 |
| `memory.md` | `~/.deepseek/` | 用户记忆持久化 |
| `notes.txt` | `~/.deepseek/` | 用户笔记 |
| `audit.log` | `~/.deepseek/` | 审计日志 |
| `/etc/deepseek/managed_config.toml` | 系统级 | 管理员默认配置 |
| `/etc/deepseek/requirements.toml` | 系统级 | 策略约束 |

---

## 七、关键技术栈

| 类别 | 技术 |
|------|------|
| 语言 | Rust 1.88+（2024 edition） |
| 异步运行时 | tokio（full features） |
| HTTP 客户端 | reqwest（rustls、HTTP/2、streaming） |
| 流处理 | async-stream + futures-util |
| TUI 框架 | ratatui + crossterm |
| CLI 解析 | clap 4.5 |
| 序列化 | serde + serde_json + toml |
| 数据库 | rusqlite（bundled SQLite） |
| Web 框架 | axum（用于 Runtime API） |
| 文本差异 | similar |
| 正则 | regex |
| UUID | uuid v4 |

---

## 八、模型定价

| 模型 | 上下文 | 输入（缓存命中） | 输入（缓存未命中） | 输出 |
|------|--------|------------------|-------------------|------|
| `deepseek-v4-pro` | 1M | $0.003625/1M | $0.435/1M | $0.87/1M |
| `deepseek-v4-flash` | 1M | $0.0028/1M | $0.14/1M | $0.28/1M |

> Pro 价格含 75% 限时折扣（至 2026-05-31 15:59 UTC）

---

## 九、构建与运行

```bash
# 从源码安装（需要 Rust 1.88+）
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path crates/cli --locked   # 提供 `deepseek` 命令
cargo install --path crates/tui --locked   # 提供 `deepseek-tui` 命令

# 运行
deepseek auth set --provider deepseek      # 设置 API key
deepseek doctor                            # 验证配置
deepseek                                   # 启动交互式 TUI
```

---

## 十、总结

DeepSeek-TUI 本质上是一个**事件驱动的终端 AI Agent 框架**，其核心设计理念：

1. **Streaming-first**：所有 LLM 响应均为流式，保证实时性
2. **Tool safety**：多层安全机制（策略引擎 + 审批 + 沙箱）
3. **Extensibility**：MCP + Skills + Hooks 三重扩展机制
4. **Prefix-cache aware**：感知 DeepSeek 的 KV cache 优化成本
5. **Crash-resilient**：检查点 + 离线队列 + 工作区快照保证数据不丢失
6. **Local-first**：所有数据本地存储，Runtime API 仅服务 localhost
