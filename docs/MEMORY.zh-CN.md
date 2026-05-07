# 用户记忆

用户记忆功能为模型提供一个轻量级的持久化笔记文件，该文件会在每一轮的 system prompt 中注入。这是存放应该在跨会话中持久保留的偏好和约定的地方 —— 例如"我习惯使用 pytest 而非 unittest"、"此代码库使用 4 空格缩进"、"提交前始终运行 `cargo fmt`" —— 无需在每次对话中重复这些内容。

记忆功能是**可选择加入的**。当禁用（默认状态）时，无任何加载、无任何拦截，且模型不会看到 `remember` 工具。这样对未申请此功能的用户保持零开销行为。

## 启用记忆功能

可设置环境变量：

```bash
export DEEPSEEK_MEMORY=on
```

接受的 truthy 值为 `1`、`on`、`true`、`yes`、`y` 和 `enabled`。

…或将以下配置添加至 `~/.deepseek/config.toml`：

```toml
[memory]
enabled = true
```

切换后需重启 TUI。禁用的方式与之相反。

记忆文件默认位于 `~/.deepseek/memory.md`；可通过 `config.toml` 中的 `memory_path` 或环境变量 `DEEPSEEK_MEMORY_PATH` 覆盖。当两者同时设置时，`DEEPSEEK_MEMORY_PATH` 优先。

## 快速示例

```text
# remember that this repo prefers cargo fmt before commits
/memory
/memory path
/memory edit
/memory help
```

- 在输入框中输入 `# 记住，该仓库提交前需运行 cargo fmt`，可将时间戳条目追加到记忆文件，而**不会触发对话轮次**。
- 运行 `/memory` 确认功能写入到何处以及当前存储的内容。
- 运行 `/memory edit` 可手动在编辑器中整理文件。

## 注入了什么

当记忆功能启用且文件存在时，每一轮的 system prompt 会携带一个额外的块：

```xml
<user_memory source="/Users/you/.deepseek/memory.md">
- (2026-05-03 22:14 UTC) 习惯使用 pytest 而非 unittest
- (2026-05-03 22:31 UTC) 此代码库使用 4 空格缩进
…
</user_memory>
```

该块位于提示词组装中的易变内容边界之上，因此会在轮次之间保持在 DeepSeek 的前缀缓存中。每次构建提示词时读取文件 —— 通过 `/memory` 或外部编辑器的编辑内容会在下一轮生效，无需重启。

超过 100 KiB 的文件仍然加载，但会被截断，并在末尾追加标记以便你看到截断位置。

## 添加记忆的三种方式

### 1. 输入框中的 `# ` 前缀（#492）

在输入框中输入以 `#` 开头（但非 `##` 或 `#!`）的单行：

```
# 记住：该仓库使用 4 空格缩进
```

TUI 拦截输入，将带时间戳的条目追加到你的记忆文件。**不会触发对话轮次** —— 你的输入被消耗，状态行确认写入路径，你可以继续输入你的实际问题。

多 `#` 前缀有意回退为普通轮次提交，以便你在粘贴 Markdown 标题时不会意外触发。

### 2. 斜杠命令 `/memory`（#491）

查看、清除或获取有关编辑文件的提示：

| 子命令 | 作用 |
|---------------------|--------------------------------------------------------|
| `/memory` | 显示解析后的路径和文件当前内容（内联） |
| `/memory show` | `/memory` 的同义别名 |
| `/memory path` | 仅输出解析后的路径 |
| `/memory clear` | 将文件替换为空标记 |
| `/memory edit` | 输出 `${VISUAL:-${EDITOR:-vi}} <path>` 的 shell 命令行 |
| `/memory help` | 显示命令专属帮助和当前路径 |

`/memory edit` 形式有意地只打印命令，而不是在进程内启动编辑器 —— 这保持了斜杠命令处理器的简单性和一致性，无论你使用哪种编辑器。

你也可以从通用帮助界面发现这一功能：

- `/help memory` 显示斜杠命令摘要和使用说明。
- `/memory help` 输出记忆专属的子命令和解析路径。

### 3. `remember` 工具（自动更新，#489）

当记忆功能启用时，模型获得一个 `remember` 工具，其形状如下：

```json
{
  "name": "remember",
  "description": "向用户记忆文件追加持久化笔记...",
  "input_schema": {
    "type": "object",
    "properties": {
      "note": { "type": "string", ... }
    },
    "required": ["note"]
  }
}
```

当模型注意到值得跨会话保留的持久化偏好、约定或事实时，会使用此工具。该工具自动批准，因为写入范围限于用户自己的记忆文件 —— 将其纳入标准的写入审批流程会适得其反，失去自动记忆捕获的意义。

如果模型使用 `remember` 记录临时任务状态（"我正在编辑 foo.rs"），结果无害但会浪费上下文。工具的 description 明确告诉模型不要这样做 —— 只记录持久化的单句笔记。

## 文件格式

记忆文件是带时间戳条目的纯 Markdown：

```markdown
- (2026-05-03 22:14 UTC) 习惯使用 pytest 而非 unittest
- (2026-05-03 22:31 UTC) 此代码库使用 4 空格缩进
- (2026-05-04 09:02 UTC) 所有 PR 合并前需要 2 人审查
```

你可以在任何编辑器中手动编辑文件 —— 加载器不关心时间戳格式；它只将整个文件读取为记忆块。时间戳是一种约定，方便你在整理文件时了解每条笔记的添加时间。

## 层级与引用关系

记忆功能有意地是**用户范围的**（user-scoped），而非仓库范围的。它与项目指令来源（如 `AGENTS.md`、`.deepseek/instructions.md` 和 `instructions = [...]`）并列，而非包含其中。

- 使用**记忆功能**存储应跟随你在各个仓库和会话之间的持久化个人偏好。
- 使用**项目指令**存储应随代码库一起传播的仓库专属约定。

记忆加载器当前直接读取一个解析后的文件路径。`@path` 导入 / 引用支持**尚未**实现；如果你需要更大的可复用指令集，请将其放入项目指令文件或技能中。

## 哪些内容不应放入记忆

记忆功能用于**持久化**信号。不应存放的内容包括：

- **密钥** — 不含 API 密钥、令牌、密码。文件是磁盘上的纯文本，会被逐字注入到 system prompt 中。
- **临时任务状态** — "我正在编写解析器"这类内容每次会话都不同，不属于跨会话记忆。
- **对话片段** — 引用式笔记应使用笔记工具（`note`），而非记忆功能。
- **长篇指令** — 超过几句话的内容应放入 `AGENTS.md`（项目级别）或[技能](../crates/tui/src/skills.rs)（可复用指令包）中。

## 隐私与范围

记忆文件完全位于你本机的 `~/.deepseek/` 目录中。它永不会上传到任何云服务 —— TUI 仅在启用记忆功能时将其内联于 LLM 提供商接收的 system prompt 中。如果你切换提供商（DeepSeek / NVIDIA NIM / Fireworks 等），使用的是同一个记忆文件；该文件与提供商无关。

该文件是每个用户独立拥有的，而非每个项目独立拥有。如果你需要项目专属的记忆，请改用项目级别的 `AGENTS.md` 或 `.deepseek/instructions.md` 文件 —— 这些文件由 `project_context` 加载并位于仓库中（或你提交的位置）。

## 配置参考

```toml
# ~/.deepseek/config.toml
[memory]
enabled = true                    # 默认 false；或设置 DEEPSEEK_MEMORY=on
# 路径配置在顶层（与 skills_dir、notes_path 同级）：
memory_path = "~/.deepseek/memory.md"
```

| 设置项 | 默认值 | 覆盖方式 |
|-----------------------|-------------------------------|---------------------------------------|
| 记忆功能启用 | `false` | `[memory] enabled = true` 或 `DEEPSEEK_MEMORY=on` |
| 记忆文件路径 | `~/.deepseek/memory.md` | `memory_path = "..."` 或 `DEEPSEEK_MEMORY_PATH=` |
| 最大文件大小 | 100 KiB | （暂无；截断标记显示截断位置） |

## 相关文档

- `docs/SUBAGENTS.md` — 子代理继承记忆功能，也可以使用 `remember` 工具。
- `docs/CONFIGURATION.md` — 完整配置参考。
- Issue [#489](https://github.com/Hmbown/DeepSeek-TUI/issues/489) — 第一阶段 EPIC 跟踪该功能的开发。
