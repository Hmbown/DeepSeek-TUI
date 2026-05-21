# 模式与审批

[English](MODES.md) | [繁體中文](MODES.zh-TW.md)

DeepSeek TUI 有两个相关概念：

- **TUI 模式**：你处于何种可见交互类型（Plan/Agent/YOLO）。
- **审批模式**：UI 在执行工具前的请求审批力度。

## TUI 模式

按下 `Tab` 完成编辑器菜单、在轮次运行时将草稿排入下一轮次的后续操作，或在编辑器空闲时循环切换可见模式：**Plan → Agent → YOLO → Plan**。
按下 `Shift+Tab` 循环推理强度。
运行 `/mode` 打开模式选择器，或使用 `/mode agent`、`/mode plan`、`/mode yolo`、`/mode 1`、`/mode 2` 或 `/mode 3` 直接切换。

- **Plan**：设计优先的提示。只读调查工具保持可用；shell 和补丁执行保持关闭。当你想要出声思考并生成计划交给人类（未来的自己或审阅者）时使用此模式。
- **Agent**：多步骤工具使用。shell 和付费工具需要审批（文件写入无需提示即可执行）。
- **YOLO**：启用 shell + 信任模式并自动批准所有工具。仅在受信任的仓库中使用。

所有三种模式都可以通过 `rlm_open`、`rlm_eval`、`rlm_configure` 和 `rlm_close` 访问持久化 RLM 会话。在 RLM Python REPL 内部，`sub_query_batch` 可以扇出 1-16 个低成本的并行子调用，固定到 `deepseek-v4-flash`。当工作量太大或重复性过高不适合父级对话记录时，模型会自动使用它。

## 兼容性说明

- 带有 `default_mode = "normal"` 的旧设置文件仍然会作为 `agent` 加载；保存时会重写为标准化值。

## Escape 键行为

`Esc` 是一个取消栈，而不是模式切换。

- 首先关闭斜杠菜单或临时 UI。
- 如果轮次正在运行，取消活动请求。
- 如果编辑器为空，丢弃排队的草稿。
- 如果有文本存在，清除当前输入。
- 否则为无操作。

## 审批模式

你可以在运行时覆盖审批行为：

```text
/config
# 编辑 approval_mode 行：suggest | auto | never
```

旧版说明：`/set approval_mode ...` 已被 `/config` 替代。

- `suggest`（默认）：使用上述每种模式的规则。
- `auto`：自动批准所有工具（类似于 YOLO 审批行为，但不强制 YOLO 模式）。
- `never`：阻止任何不被视为安全/只读的工具。

## 小屏幕状态行为

当终端高度受限时，状态区域会首先压缩，以便标题/聊天/编辑器/页脚保持可见：

- 加载和排队状态行按可用高度预算。
- 当完整预览无法容纳时，排队预览会折叠为紧凑摘要。
- `/queue` 工作流仍然可用；紧凑状态仅影响渲染密度。

## 工作区边界和信任模式

默认情况下，文件工具被限制在 `--workspace` 目录内。启用信任模式以允许在工作区之外的任何位置访问文件：

```text
/trust
```

YOLO 模式自动启用信任模式。

## MCP 行为

MCP 工具暴露为 `mcp_<server>_<tool>`，并使用与内置工具相同的审批流程。只读 MCP 辅助工具可以在建议性审批模式下自动运行；可能有副作用的 MCP 工具需要审批。

请参阅 `MCP.md`。

## 相关 CLI 标志

运行 `deepseek --help` 获取权威列表。常用标志：

- `-p, --prompt <TEXT>`：一次性 prompt 模式（打印并退出）
- `deepseek exec --output-format stream-json <PROMPT>`：为测试工具和后端包装器每行输出一个 JSON 对象
- `deepseek exec --resume <ID|PREFIX> <PROMPT>` / `--session-id <ID|PREFIX>`：非交互式地继续已保存的会话
- `deepseek exec --continue <PROMPT>`：非交互式地继续此工作区最近的已保存会话
- `--model <MODEL>`：使用 `deepseek` facade 时，将 DeepSeek 模型覆盖转发到 TUI
- `--workspace <DIR>`：文件工具的工作区根目录
- `--yolo`：以 YOLO 模式启动
- `-r, --resume <ID|PREFIX|latest>`：恢复已保存的会话
- `-c, --continue`：恢复此工作区中最近的会话
- `--max-subagents <N>`：限制在 `1..=20`
- `--mouse-capture` / `--no-mouse-capture`：选择启用或禁用内部鼠标滚动、对话记录选择、右键上下文操作和对话记录滚动条拖动。鼠标捕获在非 Windows 终端和 Windows Terminal/ConEmu/Cmder 上默认启用，因此拖动选择仅复制对话记录文本并保持范围限定在对话记录面板内；按住 Shift 拖动或使用 `--no-mouse-capture` 可进行原始终端选择。它在旧版 Windows 控制台（没有 `WT_SESSION` / `ConEmuPID` 的 CMD）和 JetBrains JediTerm（PyCharm/IDEA/CLion 等）内部默认关闭——这些终端声明支持鼠标但将 SGR 鼠标事件转发为原始文本（#878、#898）。在默认关闭的地方使用 `--mouse-capture` 可选择加入。原始终端选择可能跨越右侧边栏，因为终端而非 TUI 拥有选择权。
- `--profile <NAME>`：选择配置 Profile
- `--config <PATH>`：配置文件路径
- `-v, --verbose`：详细日志
