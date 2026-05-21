# 工具表面

[English](TOOL_SURFACE.md) | [繁體中文](TOOL_SURFACE.zh-TW.md)

为什么选择这些特定的工具、按这些分组，以及每个工具相对于可用的 shell 等效工具应如何选择。与 `crates/tui/src/prompts/agent.txt` 配套使用。

## 设计立场

- **只要专用工具能返回结构化输出，就优先使用它们而非 `exec_shell`。** Bash 转义容易出错，且平台行为各异（GNU 与 BSD 的 `grep`，`rg` 并非始终安装）。结构化输出也让模型无需重新解析自由格式文本。
- **其他情况全部使用 `exec_shell`。** 构建、测试、格式化、lint、临时命令以及任何平台特定的操作。我们不试图封装长尾需求。
- **淘汰那些不优于 shell 等效工具的工具。** 两个工具别名指向同一个底层操作是模型的陷阱——LLM 会在它们之间交替使用，缓存命中率会下降。

## 当前表面（v0.8.35）

### 文件操作

| 工具 | 适用场景 |
|---|---|
| `read_file` | 读取 UTF-8 文件。PDF 在可用时通过 `pdftotext`（poppler）自动提取；`pages: "1-5"` 可切片读取大型文档。 |
| `list_dir` | 结构化的、感知 gitignore 的目录列表。优先于 `exec_shell("ls")`。 |
| `write_file` | 创建或覆盖文件。 |
| `edit_file` | 在单个文件内进行搜索替换。比完全重写更经济。 |
| `apply_patch` | 应用统一差异格式（unified diff）补丁。适合多块编辑。 |
| `retrieve_tool_result` | 读取之前溢出到 `~/.deepseek/tool_outputs/` 的大型工具输出的摘要或切片；使用 `summary`、`head`、`tail`、`lines` 或 `query` 而非重放整个结果。 |
| `handle_read` | 从实时工具环境持有的 `var_handle` 载荷中读取有界投影。这是 RLM 会话、子代理转录本和其他大型符号载荷的基础。 |

### 搜索

| 工具 | 适用场景 |
|---|---|
| `grep_files` | 在工作区内对文件内容进行正则表达式搜索；返回结构化匹配结果及上下文行。纯 Rust 实现（`regex` crate），无需通过 shell 调用 `rg`/`grep`。 |
| `file_search` | 模糊匹配文件名（而非内容）。当您大致知道文件名时使用。 |
| `web_search` | 默认使用 Bing；可在配置中选择 DuckDuckGo、Tavily 和 Bocha。返回带排名的摘要及用于引用的 `ref_id`。 |
| `fetch_url` | 对已知 URL 直接发起 HTTP GET 请求。当链接已知时比 `web_search` 更快。默认将 HTML 转换为纯文本。 |

### Shell

| 工具 | 适用场景 |
|---|---|
| `exec_shell` | 执行 shell 命令。前台运行可取消，但仅用于有界命令；超时会终止进程并返回后台重运行的提示。 |
| `exec_shell_wait` | 轮询后台任务的增量输出。取消当前轮次会停止等待而不会杀死任务。 |
| `exec_shell_interact` | 向正在运行的后台任务发送 stdin 并读取增量输出。 |
| `exec_shell_cancel` | 按 ID 取消一个正在运行的后台 shell 任务，或在明确要求时取消所有正在运行的后台 shell 任务。 |
| `task_shell_start` | 在后台启动一个长时间运行的 command 并立即返回。对于可能运行数分钟的诊断、测试、搜索和服务器，优先于前台 shell。 |
| `task_shell_wait` | 轮询后台 command。如果在完成后提供了 `gate`，则在活动的持久任务上记录结构化的门控证据。 |

当前台 shell command 超时时，进程不会静默继续。工具结果会告诉模型使用 `task_shell_start` 或带 `background = true` 的 `exec_shell` 重新运行长时间工作，然后使用 `task_shell_wait` 或 `exec_shell_wait` 进行轮询。

交互式 shell 任务也可通过 `/jobs` 查看。TUI 任务中心由与 `exec_shell`/`task_shell_start` 相同的 shell 管理器提供支持，显示 command、cwd、运行时间、状态、输出尾部、进程本地 shell ID 以及（可用时）关联的持久任务 ID。`/jobs show`、`/jobs poll`、`/jobs wait`、`/jobs stdin` 和 `/jobs cancel` 提供对实时任务的检查、轮询、stdin 和取消控制。任务属于进程本地；重启后，实时进程状态不会重新附加，任何记住的分离条目必须标记为过期，而非作为实时进程呈现。

### MCP 管理和命令面板发现

MCP 服务器配置通过 `/mcp` 和 `/config` 中的 `mcp_config_path` 行在 TUI 中显示。`/mcp` 显示已解析的配置路径、服务器启用/禁用状态、传输方式、command 或 URL、超时设置、连接错误以及已发现的工具/资源/prompt。它支持有限的管理操作，包括 init、add、enable、disable、remove、validate 以及 reload/reconnect。配置编辑会立即写入，但模型可见的 MCP 工具池需要重启后才生效。

命令面板包含按服务器分组的 MCP 条目。禁用和失败的服务器保持可见，已发现的工具/prompt 使用运行时名称显示，例如 `mcp_<server>_<tool>`。

### Git / 诊断 / 测试

| 工具 | 适用场景 |
|---|---|
| `git_status` | 检查仓库状态，无需运行 shell。 |
| `git_diff` | 检查工作树或暂存区的差异。 |
| `diagnostics` | 一次性获取工作区、git、沙箱和工具链信息。 |
| `run_tests` | 运行 `cargo test`，可附带可选参数。 |

### 任务管理和持久工作

| 工具 | 适用场景 |
|---|---|
| `update_plan` | 用于复杂多步骤工作的结构化检查清单。 |
| `update_plan` | 用于复杂多步骤工作的结构化检查清单。 |
| `task_create` | 通过 `TaskManager` 创建/入队一个持久后台任务。这是长时间运行代理工作的实际可执行工作对象。 |
| `task_list` | 列出持久任务及其状态和关联的运行时 ID。 |
| `task_read` | 读取持久任务详情：线程/轮次关联、时间线、检查清单、门控、工件、PR 尝试、GitHub 事件。 |
| `task_cancel` | 取消一个已排队或正在运行的持久任务。需要批准。 |
| `checklist_write` | 当前线程/任务下的细粒度进度。检查清单状态从属于持久任务。 |
| `checklist_add` / `checklist_update` / `checklist_list` | 单项检查清单操作。 |
| `todo_write` / `todo_add` / `todo_update` / `todo_list` | 检查清单工具的兼容性别名。现有会话可继续使用，但新 prompt 应使用 `checklist_*`。 |
| `note` | 一次性重要事实，供后续参考。 |

### 验证门控和工件

| 工具 | 适用场景 |
|---|---|
| `task_gate_run` | 运行一个已批准的验证 command，并将结构化证据附加到活动的持久任务上：command、cwd、退出码、持续时间、分类、摘要和日志工件。 |

大型日志和 command 输出应作为工件保存，在转录本中保留简洁摘要。`task_gate_run` 对活动的持久任务自动处理此操作。

### GitHub 上下文和受保护写入

| 工具 | 适用场景 |
|---|---|
| `github_issue_context` | 通过 `gh issue view` 获取只读 issue 上下文；可能时大型正文成为任务工件。 |
| `github_pr_context` | 通过 `gh pr view` 获取只读 PR 上下文；通过 `gh pr diff --patch` 可选捕获差异；可能时大型正文/差异成为任务工件。 |
| `github_comment` | 需要批准的 issue/PR 评论，附带结构化证据。 |
| `github_close_issue` | 需要批准的 issue 关闭。需要非空的验收标准和证据；除非明确允许，否则拒绝脏工作树。切勿仅仅因为代理正在停止而关闭 issue。 |

### PR 尝试

| 工具 | 适用场景 |
|---|---|
| `pr_attempt_record` | 将当前 git 差异捕获为尝试元数据以及在持久任务上的补丁工件。 |
| `pr_attempt_list` | 列出任务上记录的尝试。 |
| `pr_attempt_read` | 检查一条记录的尝试及其工件引用。 |
| `pr_attempt_preflight` | 对尝试补丁运行 `git apply --check`。不会修改工作树。 |

### 自动化

| 工具 | 适用场景 |
|---|---|
| `automation_create` | 创建定时自动化任务。需要批准。 |
| `automation_list` / `automation_read` | 检查持久自动化任务及最近的运行记录。 |
| `automation_update` | 更新 prompt、调度、cwd 或状态。需要批准。 |
| `automation_pause` / `automation_resume` / `automation_delete` | 生命周期控制。需要批准。 |
| `automation_run` | 立即运行一个自动化任务；运行会入队一个普通的持久任务。需要批准。 |

### 子代理

v0.8.33 开始将大型工具输出迁移到符号句柄：工具返回小的 `var_handle` 对象，`handle_read` 从后端运行环境中检索有界切片、计数或 JSON 投影。这使父级转录本保持小巧，同时保留恢复完整载荷的路径。

当前面向模型的子代理表面是持久且有意保持小巧的：

| 工具 | 适用场景 |
|---|---|
| `agent_open` | 打开一个命名的子代理会话用于独立工作。立即返回会话投影，使父级可以继续协调工作。 |
| `agent_eval` | 发送后续输入、阻塞等待完成，或获取现有会话的当前投影/转录本句柄。 |
| `agent_close` | 按名称或 ID 取消或释放子代理会话。 |

参见 `agent.txt` 了解委托协议，以及 [`SUBAGENTS.md`](SUBAGENTS.md) 了解角色分类（`general` / `explore` / `plan` / `review` / `implementer` / `verifier` / `custom`）。

`agent_open` 默认为新的子对话。传递 `fork_context: true` 用于延续式工作或多角度审查，以继承父级上下文。在 fork 模式下，运行时尽可能字节一致地保留父级 prefill/prompt 前缀，以便 DeepSeek 的前缀缓存可以重用，然后追加子角色指令和任务。

### 递归 LM 会话

RLM 现在也是持久化的：

| 工具 | 适用场景 |
|---|---|
| `rlm_open` | 打开一个针对文件、内联内容或 URL 的命名 Python REPL 会话。 |
| `rlm_eval` | 在该会话中运行有界的 Python 代码，使用确定性代码和 REPL 内部的语义辅助工具，如 `sub_query_batch`。 |
| `rlm_configure` | 调整输出反馈、子查询超时/深度以及会话共享设置。 |
| `rlm_close` | 关闭 Python 运行时并返回最终会话统计信息。 |

大型 RLM 输出应以 `var_handle` 形式返回。使用 `handle_read` 获取有界的文本切片、行范围、计数或 JSONPath 投影，而非将完整值重放到父级转录本中。

在 `rlm_eval` 内部，加载的源码可作为 `_context` 使用；`content` 也作为便捷别名绑定，因为代理在进行 Python 分析时自然会使用它。较短的 `context` 和 `ctx` 名称有意不绑定，以免用户变量与之冲突。

子调用超时是会话级别的策略：在运行大规模扇出之前，使用带有 `sub_query_timeout_secs` 的 `rlm_configure`。辅助工具 `sub_query`、`sub_query_batch`、`sub_query_map` 和 `sub_rlm` 接受 `timeout_secs` 关键字以兼容常见的代理猜测，但实际超时仍由 RLM 会话级别配置决定。

`finalize(value, confidence=...)` 保留可 JSON 序列化的值。字符串成为文本句柄；字典、列表、数字、布尔值和 null 成为 JSON 句柄，`handle_read` 可通过 JSONPath 进行投影。

### 会话接力

`/relay [focus]` 要求当前代理将 `.deepseek/handoff.md` 写为一个紧凑的 `# Session relay` 工件，供下一个线程使用。文件名保持兼容现有 prompt 加载和旧会话；可见的心智模型是中继/接力。

别名：`/batonpass`、`/接力`。

在长时间休息、压缩或将工作移至新会话之前使用它。接力应保留目标、当前 Work 检查清单项、更改的文件、决策、验证状态以及一个具体的下一步操作。

### 并行扇出：成本等级上限

两个工具提供并行扇出功能，具有反映完全不同成本等级的不同并发限制：

| 工具 | 每个子任务执行的操作 | 实际耗时 | Token 成本 | 上限 |
|---|---|---|---|---|---|
| `agent_open` | 完整的子代理循环（规划、工具调用、多轮流式输出、可打开子代理） | 数分钟 | 数千个 token | 默认 10 个并发（`[subagents].max_concurrent`，硬上限 20） |
| `rlm_eval` 辅助工具 `sub_query_batch` | 在实时 RLM 会话内，一次性非流式 Chat Completions 调用固定在 `deepseek-v4-flash` 上 | 数秒 | ~数百个 token | 每次调用 16 个 |

上限出现在每个工具的描述和错误消息中，以便模型（和用户）选择合适的工具。如果一个子代理足够但需要对相同加载上下文进行并行语义查询，优先使用 `sub_query_batch` 的 `rlm_eval`；如果每个任务需要自己的携带工具的子代理循环，使用 `agent_open` 并关闭已完成的会话以释放槽位。

## 已移除的旧别名和表面

v0.8.33 从活动 prompt 和工具目录中移除了旧的面向模型的子代理扇出表面。不要在活动指导中使用以下名称：`agent_spawn`、`agent_wait`、`agent_result`、`agent_send_input`、`agent_assign`、`agent_resume`、`agent_list`、`spawn_agent`、`delegate_to_agent`、`send_input` 和 `close_agent`。

旧的单次 `rlm` 面向模型工具也被持久化的 `rlm_open` / `rlm_eval` / `rlm_configure` / `rlm_close` 会话所取代。

历史兼容性结果可能包含类似这样的 `_deprecation` 块：

```json
{
  "_deprecation": {
    "this_tool": "spawn_agent",
    "use_instead": "agent_open",
    "removed_in": "0.8.33",
    "message": "Tool 'spawn_agent' is deprecated; switch to 'agent_open'."
  }
}
```

这是遗留/兼容性说明，并非当前推荐的表面。

## 发布验证：确认实际名称

在验证发布时，直接检查模型可见的注册表名称。不要 grep 随机的处理函数名称；处理函数名称可以变化，而注册表合约保持稳定。

版本验证：

```bash
deepseek --version
deepseek-tui --version
```

工具表面验证：

```bash
rg -n '"handle_read"|"rlm_open"|"rlm_eval"|"rlm_configure"|"rlm_close"|"agent_open"|"agent_eval"|"agent_close"' crates/tui/src
rg -n 'handle_read|rlm_open|rlm_eval|rlm_configure|rlm_close|agent_open|agent_eval|agent_close' docs crates/tui/src/prompts crates/tui/src/tools
```

v0.8.35 的规范实际名称如下：

- `handle_read`
- `rlm_open`、`rlm_eval`、`rlm_configure`、`rlm_close`
- `agent_open`、`agent_eval`、`agent_close`

注册表不应在遗留/移除说明之外主动宣传旧的单次名称 `agent_spawn`、`agent_wait`、`agent_result` 或旧的 `rlm` 前台工具。历史变更日志条目和兼容性代码仍可能提及它们。

## 为什么我们不提供一个单一的 `bash` 工具

单一的 `bash` 代理（如 Claude Code 的设计）功能强大，但将 shell 脚本的所有陷阱交给了模型：引号问题、平台差异、由于错误读取 cwd 导致的副作用、`cd` 在调用之间不持久等。我们的文件工具在转录本中渲染也显著更经济（结构化的 JSON 输出比 `ls -la` 的文本墙压缩效果更好）。

当缺少某些功能时，模型始终可以回退到 `exec_shell`。专用工具只是将常见的 80% 场景从 shell 逃生口中解放出来。
