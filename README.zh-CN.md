# DeepSeek TUI

> **面向 [DeepSeek V4](https://platform.deepseek.com) 模型的终端原生编程智能体，支持 100 万 token 上下文、思考模式推理流和完整工具调用。**

[English README](README.md)

```bash
npm i -g deepseek-tui
```

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/deepseek-tui)](https://www.npmjs.com/package/deepseek-tui)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui-cli?label=crates.io)](https://crates.io/crates/deepseek-tui-cli)

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

预构建二进制覆盖 **Linux x64**、**Linux ARM64**（v0.8.8 起）、**macOS x64**、
**macOS ARM64**、**Windows x64**。其他平台（musl、riscv64、FreeBSD 等）请见
下方的 [从源码安装](#从源码安装) 章节，或参考完整的
[docs/INSTALL.md](docs/INSTALL.md)。

首次启动时会提示输入 [DeepSeek API key](https://platform.deepseek.com/api_keys)。TUI 会把它保存到用户配置 `~/.deepseek/config.toml`，因此在任意目录、IDE 终端和脚本里都能读到，并且不会触发系统密钥环弹窗。

也可以提前配置：

```bash
# 推荐 —— 保存到 ~/.deepseek/config.toml；适用于所有场景
# （交互式 shell、IDE 终端、脚本、cron）：
deepseek auth set --provider deepseek

# 环境变量备选 —— 注意 zsh 中 ~/.zshrc 的 export 只对交互式 shell 生效。
# 需要在登录 shell / IDE / 脚本中也生效，请放进 ~/.zshenv：
export DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY"
deepseek

# 检查二进制实际读到的源：
deepseek doctor
```

> 轮换或移除已保存的密钥：`deepseek auth clear --provider deepseek`
> （旧别名 `deepseek logout` 也可用），然后重新运行
> `deepseek auth set --provider deepseek`。

### Linux ARM64（HarmonyOS 轻薄本、openEuler、Kylin、树莓派、Graviton 等）

从 **v0.8.8** 起，`npm i -g deepseek-tui` 直接支持 glibc 系的 ARM64 Linux。
如果你停留在 v0.8.7 或更早版本，会看到 `Unsupported architecture: arm64`
错误。升级到最新版即可，或直接用 `cargo install`：

```bash
# 需要 Rust 1.85+（https://rustup.rs）
cargo install deepseek-tui-cli --locked   # 提供 `deepseek`
cargo install deepseek-tui     --locked   # 提供 `deepseek-tui`
```

也可以从 [Releases 页面](https://github.com/Hmbown/DeepSeek-TUI/releases) 下载
`deepseek-linux-arm64` 与 `deepseek-tui-linux-arm64`，放到同一个 `PATH` 目录里。
从 x64 主机交叉编译到 ARM64 的步骤见
[docs/INSTALL.md](docs/INSTALL.md#cross-compiling-from-x64-to-arm64-linux)。

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

适用于任何 Tier-1 Rust 目标，包括 musl、riscv64、FreeBSD，以及早于
v0.8.8、还没有官方预编译包的 ARM64 发行版。

```bash
# Linux 构建依赖（Debian/Ubuntu/openEuler/Kylin）：
#   sudo apt-get install -y build-essential pkg-config libdbus-1-dev
#   # RHEL 系：sudo dnf install -y gcc make pkgconf-pkg-config dbus-devel

git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI

cargo install --path crates/cli --locked   # 需要 Rust 1.85+；提供 `deepseek`
cargo install --path crates/tui --locked   # 提供 `deepseek-tui`

deepseek --version
```

两个二进制都需要安装：`deepseek` 是入口调度器，运行时会调用 `deepseek-tui`。
跨平台编译、镜像、平台特定故障排查见 [docs/INSTALL.md](docs/INSTALL.md)。

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
deepseek auth set --provider deepseek          # 保存 API key 到 ~/.deepseek/config.toml
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

UI 语言与模型输出语言相互独立。

### 切换为中文界面

如果界面是英文的，可以在 TUI 内一键切换为简体中文：

1. 在 Composer 里输入 `/config`，按 Tab 或 Enter 打开配置面板。

   ![输入 /config 命令](assets/locale-config-step1.jpg)

2. 选择 **Edit locale**，在 `New:` 字段输入 `zh-Hans`，按 Enter 应用。

   ![编辑 locale 设为 zh-Hans](assets/locale-config-step2.jpg)

可选语言：`auto` | `en` | `ja` | `zh-Hans` | `pt-BR`

也可以直接在 `~/.deepseek/settings.toml` 里设置 `locale = "zh-Hans"`，或者通过 `LC_ALL` / `LANG` 环境变量自动选择。

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

## v0.8.8 新功能

以稳定性为主的发版：在 v0.8.6 / v0.8.7 基础上做了一层较厚的 UX 打磨，以及生产环境里暴露出来的运行时修复。模型和 API 没有改动，配置和会话保持向后兼容。

- **TUI 打磨**：限速 / 5xx 时显示倒计时重试横幅、MCP 服务器健康指示灯、工具输出溢出预览（`~/.deepseek/tool_outputs/<id>.txt`，详情面板里完整查看）、多日时长显示（`2d 3h` 而不是 `188415s`）、`worked Nh Mm` 累计计时器、终端可点击 OSC 8 超链接、`edit_file` / `write_file` 输出渲染为带行号 `+`/`-` 槽位的内联 unified diff。
- **斜杠菜单不再抖动聊天区**（Windows 10 PowerShell + WSL 用户反馈）：在斜杠 / 提及菜单打开时锁定 composer 高度，不再因为匹配项数量变化重绘上方区域。
- **撰写区暂存** — Ctrl+S 把当前草稿停到 `~/.deepseek/composer_stash.jsonl`；`/stash list` / `pop` / `clear` 全程 LIFO，最多保留 200 条。
- **可访问性**：`NO_ANIMATIONS=1`（或 `1` / `true` / `yes` / `on`）启动时强制 `low_motion = true` 与 `fancy_animations = false`，无视已保存设置；新增 `docs/ACCESSIBILITY.md`。键盘增强标志在崩溃 / Ctrl+Z 挂起 / 外部编辑器路径上都会回弹，崩溃后终端不会卡在 raw 模式。
- **子智能体**：默认并发上限 `5 → 10`（可通过 `[subagents].max_concurrent` 配置，硬上限 20）；已完成的子智能体不再占用并发名额。多智能体 fan-out 时 UI 卡死的问题已修复（`SharedSubAgentManager` 改为 `Arc<RwLock<…>>`）。子智能体输出会先做摘要再注入父上下文。新增 `Implementer` / `Verifier` 角色。`agent_swarm` / `/swarm` 老接口在 v0.8.5 已删除。
- **工作流 / 扩展**：`load_skill` 模型可调用工具；技能发现器跨工具走 `.agents/skills` / `skills` / `.opencode/skills` / `.claude/skills` / `~/.deepseek/skills`。`/hooks` 列出生命周期钩子配置；所有 `HookEvent` 现在都有运行时生产者。`instructions = [...]` 配置数组允许把多个项目级文件叠加进系统提示。`deepseek pr <N>` 子命令直接抓取 PR 元数据并在撰写区里预填评审 prompt。
- **用户记忆 MVP（默认关闭）**：开启后 `~/.deepseek/memory.md` 注入到系统提示的 `<user_memory>` 块；撰写区输入 `# foo` 直接追加一条带时间戳的记忆，不触发回合；`/memory show|path|clear|edit` 用于查看与编辑。`[memory] enabled = true` 或 `DEEPSEEK_MEMORY=on` 启用。
- **安全**：项目级配置不能再通过覆盖 `api_key` / `base_url` / `provider` / `mcp_config_path` 做权限提升，也不能把 `approval_policy` 设为 `auto` 或 `sandbox_mode` 设为 `danger-full-access`。`SSL_CERT_FILE` 环境变量在 HTTPS 客户端被识别（PEM bundle + DER 兜底），方便企业 CA / MITM 代理用户。execpolicy 在 shlex 之前剥离 heredoc 主体，所以 `auto_allow = ["cat > file.txt"]` 也能匹配 heredoc 形式 `cat <<EOF > file.txt\nbody\nEOF`。
- **打包**：新增 **Linux ARM64** 预编译包（鲲鹏、HarmonyOS PC、Kylin、openEuler、树莓派、Graviton 等），`npm i -g deepseek-tui` 直接可用；新增 `docs/INSTALL.md` 覆盖所有安装方式。`deepseek update` 修复了 v0.8.7 在所有平台都失败的架构名映射 bug。CI 删了三个重复 / 失效的 workflow。
- **Bug 修复**：composer 的 `Option+Backspace` 现在按词删除；离线撰写队列绑定到当前会话 ID，老的无作用域队列会被关闭式回退；`display_path` 测试不再竞争 `$HOME`，并在 Windows 用 `\` 拼接。
- **认证与引导**：v0.8.8 默认不会触发系统凭据弹窗；启动、`doctor` 和普通请求路径使用 CLI 参数 → `~/.deepseek/config.toml` → 环境变量。`deepseek auth set --provider deepseek` 与 TUI 内引导现在都写入同一个用户配置文件。引导页文案改写为 "Step 1: 打开 https://platform.deepseek.com/api_keys / Step 2: 粘贴并回车"。"DeepSeek API key not found" 错误会优先给出 `deepseek auth set`，再说明环境变量方案（含 `~/.zshrc` vs `~/.zshenv` 提示）。规范入口 `deepseek` 现在接受 TUI 已声明支持的 `fireworks` / `sglang` provider；旧的 `deepseek login --api-key` / `deepseek logout` 也走同一套配置文件路径。

完整列表见 [CHANGELOG.md](CHANGELOG.md)。

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
