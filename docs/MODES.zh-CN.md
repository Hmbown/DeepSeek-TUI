# 模式与审批

DeepSeek TUI 有两条相关的概念：

- **TUI 模式**：你在哪种可见交互模式下（Plan/Agent/YOLO）。
- **审批模式**：UI 在执行工具前的审批严格程度。

## TUI 模式

按 `Tab` 可在输入框空闲时补全输入框菜单、将草稿作为下一轮跟进排队（当轮次运行中时），或循环切换可见模式：**Plan → Agent → YOLO → Plan**。按 `Shift+Tab` 循环切换推理强度。

- **Plan（规划模式）**：设计优先的提示词交互。只读调查工具保持可用；shell 和补丁执行保持关闭。当你想要出声思考并产出一个可以交给人类（未来的自己，或审查者）的方案时使用此模式。
- **Agent（代理模式）**：多步骤工具使用。shell 和付费工具需要审批（文件写入无需提示即可执行）。
- **YOLO 模式**：启用 shell + 信任模式，自动批准所有工具。仅在受信任的仓库中使用。

三种模式均可使用 `rlm` 工具。在其 Python REPL 中，`llm_query_batched` 可扇形展开 1–16 个低成本并行子调用，固定使用 `deepseek-v4-flash`。当任务可分解时，模型会自行使用该工具。

## 兼容性说明

- `/normal` 是一个隐藏的兼容性别名，切换到 `Agent` 模式。
- 旧版设置文件中 `default_mode = "normal"` 仍然加载为 `agent`；保存时会写入标准化后的值。

## Escape 键行为

`Esc` 是一个取消栈，而非模式切换键。

- 首先关闭斜杠菜单或临时 UI。
- 如有活动请求且正在运行中，取消该请求。
- 如果输入框为空，丢弃已排队的草稿。
- 如果输入框有文本，清除当前输入。
- 否则不执行任何操作。

## 审批模式

你可以在运行时覆盖审批行为：

```text
/config
# 将 approval_mode 行编辑为：suggest | auto | never
```

旧版说明：`/set approval_mode ...` 已废弃，改用 `/config`。

- `suggest`（默认）：使用上述的按模式规则。
- `auto`：自动批准所有工具（类似于 YOLO 审批行为，但不会强制切换为 YOLO 模式）。
- `never`：阻止任何被认为不安全/非只读的工具。

## 小屏幕状态行为

当终端高度受限时，状态区域会优先压缩，以保证头部/聊天区/输入框/页脚保持可见：

- 加载和排队状态行根据可用高度进行预算分配。
- 队列预览在完整预览放不下时折叠为紧凑摘要。
- `/queue` 工作流仍然可用；紧凑状态仅影响渲染密度。

## 工作区边界与信任模式

默认情况下，文件工具限制在 `--workspace` 目录内。启用信任模式以允许在工作区外进行文件访问：

```text
/trust
```

YOLO 模式自动启用信任模式。

## MCP 行为

MCP 工具以 `mcp_<server>_<tool>` 形式暴露，使用与内置工具相同的审批流程。只读 MCP 辅助工具可能在建议性审批模式下自动运行；可能产生副作用的 MCP 工具需要审批。

见 `MCP.md`。

## 相关 CLI 参数

运行 `deepseek --help` 查看正式列表。常用参数：

- `-p, --prompt <TEXT>`：一次性提示模式（打印并退出）
- `--model <MODEL>`：当使用 `deepseek` 门面程序时，向 TUI 转发 DeepSeek 模型覆盖
- `--workspace <DIR>`：文件工具的工作区根目录
- `--yolo`：以 YOLO 模式启动
- `-r, --resume <ID|PREFIX|latest>`：恢复已保存的会话
- `-c, --continue`：恢复此工作区最近一次会话
- `--max-subagents <N>`：限制在 `1..=20`
- `--no-alt-screen`：不使用交替屏幕缓冲区运行（内联模式）
- `--mouse-capture` / `--no-mouse-capture`：启用或禁用内部鼠标滚动、对话记录选择和右键上下文操作。鼠标捕获在非 Windows 终端上默认启用，拖拽选择仅复制用户/助手的对话记录文本；按住 Shift 拖拽或使用 `--no-mouse-capture` 以使用原始终端选择。它在 Windows（CMD/终端中鼠标转义垃圾字符会写入提示符）和 JetBrains JediTerm（PyCharm/IDEA/CLion 等）中默认关闭 —— 这些环境中终端声称支持鼠标，但实际上将 SGR 鼠标事件作为原始文本转发（#878, #898）。使用 `--mouse-capture` 在默认关闭的环境中手动启用
- `--profile <NAME>`：选择配置 profile
- `--config <PATH>`：配置文件路径
- `-v, --verbose`：详细日志
