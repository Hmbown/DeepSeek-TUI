# DeepSeek TUI

> **面向 [DeepSeek V4](https://platform.deepseek.com) 模型的终端原生编程智能体，支持 100 万 token 上下文、思考模式推理流和完整工具调用。**

[English README](README.md)

```bash
npm i -g deepseek-tui
```

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/deepseek-tui)](https://www.npmjs.com/package/deepseek-tui)

![DeepSeek TUI screenshot](assets/screenshot.png)

---

## 这是什么？

DeepSeek TUI 是一个完全运行在终端里的编程智能体。它可以让 DeepSeek 前沿模型直接访问你的工作区：读取和编辑文件、运行 shell 命令、搜索和浏览网页、管理 git、调度子智能体，并通过快速的键盘驱动 TUI 完成多步开发任务。

它面向 **DeepSeek V4**（`deepseek-v4-pro` / `deepseek-v4-flash`）构建，默认支持 100 万 token 上下文窗口和原生思考模式流式输出。模型推理、工具调用和最终回答会在终端里实时呈现。

### 主要功能

- **原生 RLM**（`rlm_query` 工具）：用现有 DeepSeek 客户端并行调度 1 到 16 个低成本 `deepseek-v4-flash` 子任务，用于批量分析、任务拆解或并行推理。
- **思考模式流式输出**：实时显示 DeepSeek 的推理过程。
- **完整工具集**：文件操作、shell 执行、git、网页搜索/浏览、apply-patch、子智能体、MCP 服务器。
- **100 万 token 上下文**：上下文接近上限时自动进行智能压缩。
- **三种交互模式**：Plan（只读探索）、Agent（默认交互并带审批）、YOLO（可信工作区内自动批准工具）。
- **推理强度档位**：用 `Shift+Tab` 在 `off -> high -> max` 之间切换。
- **会话保存和恢复**：适合长任务的断点续作。
- **工作区回滚**：通过 side-git 记录每轮前后快照，支持 `/restore` 和 `revert_turn`，不修改项目自己的 `.git`。
- **HTTP/SSE 运行时 API**：`deepseek serve --http` 可用于无界面智能体流程。
- **MCP 协议支持**：连接 Model Context Protocol 服务器扩展工具，见 [docs/MCP.md](docs/MCP.md)。
- **实时成本跟踪**：按轮次和会话统计 token 用量与成本估算。
- **深色主题**：DeepSeek 蓝色系终端界面。

---

## 快速开始

```bash
npm install -g deepseek-tui
deepseek
```

首次启动时会提示输入 [DeepSeek API key](https://platform.deepseek.com/api_keys)。也可以提前配置：

```bash
# 通过 CLI 保存
deepseek login --api-key "YOUR_DEEPSEEK_API_KEY"

# 或通过环境变量
export DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY"
deepseek
```

### 中国大陆 / 镜像友好安装

如果在中国大陆访问 GitHub 或 npm 下载较慢，可以通过 Cargo 注册表镜像安装 Rust crate：

```toml
# ~/.cargo/config.toml
[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
```

然后从对应的包安装：

```bash
cargo install deepseek-tui-cli --locked   # 提供推荐入口 `deepseek`
cargo install deepseek-tui     --locked   # 可选：提供 TUI 伴随二进制 `deepseek-tui`
deepseek --version
deepseek doctor --json
```

从 `v0.8.2` 起回到分包安装：

- `deepseek-tui-cli`：推荐使用的调度器入口（`deepseek`）。
- `deepseek-tui`：交互式 TUI 伴随二进制。

也可以直接从 [GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases) 下载预编译二进制。如果你有镜像后的 release 资产目录，也可以配合 `DEEPSEEK_TUI_RELEASE_BASE_URL` 使用 TUNA、rsproxy、腾讯云 COS 或阿里云 OSS 等镜像。

### 从源码安装

```bash
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path crates/tui --locked   # 需要 Rust 1.85+
deepseek --version
```

---

## 其他模型提供方

### NVIDIA NIM

```bash
deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"
deepseek --provider nvidia-nim

# 或仅对当前进程生效：
DEEPSEEK_PROVIDER=nvidia-nim NVIDIA_API_KEY="..." deepseek
```

### Fireworks 和自托管 SGLang

```bash
deepseek auth set --provider fireworks --api-key "YOUR_FIREWORKS_API_KEY"
deepseek --provider fireworks --model deepseek-v4-pro

# SGLang 通常是自托管；localhost 部署可以不配置鉴权。
SGLANG_BASE_URL="http://localhost:30000/v1" deepseek --provider sglang --model deepseek-v4-flash
```

---

## 使用方式

```bash
deepseek                                       # 交互式 TUI
deepseek "explain this function"              # 一次性提示
deepseek --model deepseek-v4-flash "summarize" # 指定模型
deepseek --yolo                                # YOLO 模式，自动批准工具
deepseek login --api-key "..."                 # 保存 API key
deepseek doctor                                # 检查配置和连接
deepseek doctor --json                         # 机器可读诊断
deepseek setup --status                        # 只读安装状态检查
deepseek setup --tools --plugins               # 创建本地工具和插件目录
deepseek models                                # 列出可用 API 模型
deepseek sessions                              # 列出已保存会话
deepseek resume --last                         # 恢复最近会话
deepseek serve --http                          # HTTP/SSE API 服务
deepseek mcp list                              # 列出已配置 MCP 服务器
deepseek mcp validate                          # 校验 MCP 配置和连接
deepseek mcp-server                            # 启动 dispatcher MCP stdio 服务器
```

### 常用快捷键

| 按键 | 功能 |
|---|---|
| `Tab` | 补全 `/` 或 `@`；运行中则把草稿排队为后续消息；否则切换模式 |
| `Shift+Tab` | 切换推理强度：off -> high -> max |
| `F1` | 帮助 |
| `Esc` | 返回 / 关闭 |
| `Ctrl+K` | 命令面板 |
| `Ctrl+R` | 恢复旧会话 |
| `Alt+R` | 搜索提示历史和恢复草稿 |
| `@path` | 在输入框中附加文件或目录上下文 |
| `Alt+↑` | 编辑最后一条排队消息 |
| `/attach <path>` | 附加图片或视频路径引用 |

---

## 模式

| 模式 | 行为 |
|---|---|
| **Plan** | 只读调查；模型先探索并提出拆解计划，再进行更改 |
| **Agent** | 默认交互模式；多步工具调用带审批门禁 |
| **YOLO** | 在可信工作区自动批准工具；仍会保留计划和清单以便追踪 |

---

## 配置

主配置文件是 `~/.deepseek/config.toml`。完整选项见 [config.example.toml](config.example.toml) 和 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

常用环境变量：

| 变量 | 用途 |
|---|---|
| `DEEPSEEK_API_KEY` | DeepSeek API key |
| `DEEPSEEK_BASE_URL` | API base URL |
| `DEEPSEEK_MODEL` | 默认模型 |
| `DEEPSEEK_PROVIDER` | 提供方：`deepseek`、`nvidia-nim`、`fireworks` 或 `sglang` |
| `DEEPSEEK_PROFILE` | 配置 profile 名称 |
| `NVIDIA_API_KEY` | NVIDIA NIM API key |
| `FIREWORKS_API_KEY` | Fireworks AI API key |
| `SGLANG_BASE_URL` | 自托管 SGLang 端点 |
| `SGLANG_API_KEY` | 可选 SGLang bearer token |

快速诊断：

```bash
deepseek setup --status
deepseek doctor --json
```

UI 语言与模型输出语言相互独立。可以在 `settings.toml` 里设置 `locale`，也可以通过 `LC_ALL` / `LANG` 环境变量自动选择。支持 `en`、`zh-Hans`、`ja`、`pt-BR` 等界面语言。

DeepSeek 上下文缓存是自动的；当 API 返回 cache hit/miss token 字段时，TUI 会把它们纳入用量和成本统计。

---

## 模型和价格

DeepSeek TUI 默认面向带 100 万 token 上下文窗口的 **DeepSeek V4** 模型。

| 模型 | 上下文 | 输入（缓存命中） | 输入（缓存未命中） | 输出 |
|---|---|---|---|---|
| `deepseek-v4-pro` | 1M | $0.003625 / 1M* | $0.435 / 1M* | $0.87 / 1M* |
| `deepseek-v4-flash` | 1M | $0.0028 / 1M | $0.14 / 1M | $0.28 / 1M |

旧别名 `deepseek-chat` 和 `deepseek-reasoner` 会自动映射到 `deepseek-v4-flash`。

**NVIDIA NIM** 托管变体（`deepseek-ai/deepseek-v4-pro`、`deepseek-ai/deepseek-v4-flash`）使用你的 NVIDIA 账号条款，不走 DeepSeek 平台计费。

*DeepSeek 标注的 Pro 价格是限时 75% 折扣，有效期到 2026-05-05 15:59 UTC；该时间之后 TUI 成本估算会回退到 Pro 基础价格。*

---

## 文档

| 文档 | 主题 |
|---|---|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | 代码库内部结构 |
| [CONFIGURATION.md](docs/CONFIGURATION.md) | 完整配置参考 |
| [MODES.md](docs/MODES.md) | Plan / Agent / YOLO 模式 |
| [MCP.md](docs/MCP.md) | Model Context Protocol 集成 |
| [RUNTIME_API.md](docs/RUNTIME_API.md) | HTTP/SSE API 服务 |
| [RELEASE_RUNBOOK.md](docs/RELEASE_RUNBOOK.md) | 发布流程 |
| [OPERATIONS_RUNBOOK.md](docs/OPERATIONS_RUNBOOK.md) | 运维和恢复 |

完整更新历史见 [CHANGELOG.md](CHANGELOG.md)。

---

## 创建和安装技能

DeepSeek-TUI 会从当前技能目录发现技能。优先级是：工作区
`.agents/skills`、工作区 `./skills`、全局目录（默认
`~/.deepseek/skills`）。每个技能都是一个包含 `SKILL.md` 的目录：

```text
~/.deepseek/skills/my-skill/
└── SKILL.md
```

`SKILL.md` 需要以 YAML frontmatter 开头：

```markdown
---
name: my-skill
description: 当 DeepSeek 需要遵循我的自定义工作流时使用这个技能。
---

# My Skill

这里写给智能体的指令。
```

常用命令：

```bash
/skills
/skill my-skill
/skill new
/skill install github:<owner>/<repo>
/skill update my-skill
/skill uninstall my-skill
/skill trust my-skill
```

`/skills` 列出已发现技能，`/skill <name>` 会把技能应用到下一条消息，
`/skill new` 会调用内置的 skill-creator 辅助创建新技能。已安装技能也会
进入模型可见的会话上下文；当用户点名某个技能，或任务明显匹配技能描述时，
智能体可以主动读取对应的 `SKILL.md` 并使用它。

社区技能可以直接从 GitHub 安装。安装过程受 `[network]` 策略约束，并会校验
压缩包大小、路径穿越和符号链接。`/skill trust <name>` 只在你希望技能内置脚本
可被执行时才需要。

---

## 贡献

欢迎提交 pull request。请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。

*本项目与 DeepSeek Inc. 无隶属关系。*

## 许可证

[MIT](LICENSE)
