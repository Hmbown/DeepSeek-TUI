# 参与贡献 DeepSeek TUI

[English](CONTRIBUTING.md) | [繁體中文](CONTRIBUTING.zh-TW.md)

感谢你有兴趣为 DeepSeek TUI 做出贡献！本文档提供了贡献的指南和说明。

## 快速入门

### 先决条件

- Rust 1.88 或更高版本（edition 2024）
- Cargo 包管理器
- Git

### 搭建开发环境

1. Fork 并克隆仓库：
   ```bash
   git clone https://github.com/YOUR_USERNAME/DeepSeek-TUI.git
   cd DeepSeek-TUI
   ```

2. 构建项目：
   ```bash
   cargo build
   ```

3. 运行测试：
   ```bash
   cargo test
   ```

4. 使用开发配置运行：
   ```bash
   cargo run
   ```

## 开发工作流

### 代码风格

- 提交前运行 `cargo fmt` 以确保格式一致
- 运行 `cargo clippy` 并处理所有警告
- 遵循 Rust 命名规范（函数/变量使用 snake_case，类型使用 CamelCase）
- 为公共 API 添加文档注释

### 测试

- 为新功能编写测试
- 确保所有现有测试通过：`cargo test --workspace --all-features`
- 将单元测试放在所覆盖代码的同一文件中（标准的 Rust `#[cfg(test)]`
  模块），并将集成测试放在所属 crate 的 `tests/`
  目录下（例如 `crates/tui/tests/` 或 `crates/state/tests/`）。仓库
  根目录的 `tests/` 目录不使用

### 提交信息

使用清晰、描述性的提交信息，遵循 conventional commits 规范：

- `feat:` 新功能
- `fix:` Bug 修复
- `docs:` 文档更改
- `refactor:` 代码重构
- `test:` 添加或更新测试
- `chore:` 维护任务

示例：`feat: add doctor subcommand for system diagnostics`

当提交内容来自社区 PR 的 Harvest（参见下方"你的贡献如何合入"）时，请在提交正文中包含
`Harvested from PR #N by @author` 一行。一个自动关闭工作流会监听此模式，并关闭所引用的
PR 并给予致谢，以便贡献者获得其工作已发布的明确信号。

## 你的贡献如何合入

我们遵循一种经过深思熟虑的「合入有用的部分，致谢贡献者」模式，
这有时会让新贡献者感到意外。共有两条路径：

### 路径 1 — 直接合并

如果你的 PR 范围合理、通过了 CI、未触及信任边界范围
（认证/沙箱/发布/品牌），且与 main 分支没有冲突，
维护者会直接合并。这是小型 bug 修复和经过充分测试的功能添加最常见的结果。

### 路径 2 — Harvest

如果你的 PR 规模较大、范围混杂、与 main 分支有冲突，或者需要润色且
维护者自行处理比与贡献者来回沟通更快，维护者可能会将有用的提交或
代码块 **harvest** 到 `main` 分支上的新提交中，而不是直接合并 PR。
这**不是拒绝**——这意味着你的代码已经合入了。

当这种情况发生时：

- harvest 后的提交信息中包含 `Harvested from PR #N by
  @your-handle`。这是约定：该行就是你的致谢标记，也是你的贡献已发布的信号。
- 下一个版本的 `CHANGELOG.md` 条目会以你的 handle 致谢。
- 自动关闭工作流会以模板化的感谢信息关闭你的 PR，并附上指向 `main` 分支上该提交的链接。

为了让未来的贡献走更快的直接合并路径而非 Harvest 路径，你能做的最高杠杆的事情是：

1. **保持 PR 单一用途。** 每个 PR 一个 bug 修复；每个 PR 一个功能。
   不要将重构与功能混在一起。
2. **在打开 PR 之前以及收到 CI 反馈后，rebase 到当前的 `main` 分支**。
   冲突即使变更很小也会强制走 Harvest 路径。
3. **为新行为包含测试**。维护者通常会对没有测试的 PR 执行 harvest，
   因为添加测试比要求贡献者补充测试更快。
4. **避免在未获得维护者事先同意的情况下触及信任边界范围**。
   这包括认证/凭据流程、沙箱策略、
   发布/版本管道以及 `prompts/` 内容。触及这些内容且未经事先讨论的 PR，
   即使变更实现得很好，也不太可能直接合并。

## 项目结构

DeepSeek TUI 是一个 Cargo workspace。运行时和大部分 TUI、
引擎以及工具代码目前位于 `crates/tui/src/` 中。较小的 workspace
crate 提供了正在逐步抽取的共享抽象。

```
crates/
├── tui/           deepseek-tui 二进制文件（交互式 TUI + 运行时 API）
├── cli/           deepseek 二进制文件（调度器外观）
├── app-server/    HTTP/SSE + JSON-RPC 传输层
├── core/          Agent 循环 / 会话 / 轮次管理
├── protocol/      请求/响应帧
├── config/        配置加载、profile、环境变量优先级
├── state/         SQLite 线程/会话持久化
├── tools/         类型化工具规范与生命周期
├── mcp/           MCP 客户端 + stdio 服务器
├── hooks/         生命周期钩子（stdout/jsonl/webhook）
├── execpolicy/    审批/沙箱策略引擎
├── agent/         模型/提供商注册表
└── tui-core/      事件驱动的 TUI 状态机脚手架
```

参见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) 了解这些 crate
之间的实时数据流，包括自底向上的构建顺序。

## 提交更改

1. 从 `main` 分支创建功能分支：
   ```bash
   git checkout -b feat/your-feature
   ```

2. 进行更改并提交

3. 确保 CI 通过：
   ```bash
   cargo fmt --check
   cargo clippy
   cargo test
   ```

4. 推送你的分支并创建 Pull Request

5. 在 PR 描述中清晰地说明你的更改

## Pull Request 指南

- 保持 PR 专注于单一更改
- 如有需要请更新文档
- 为新功能添加测试
- 在请求审查前确保 CI 通过

## 典型 PR 的形态

一个结构良好的 PR 遵循一致的模式。最近的范例包括：

- **#386** — `/init` 命令：新增 `crates/tui/src/commands/init.rs` 模块，项目类型检测，
  AGENTS.md 生成，命令注册于 `commands/mod.rs`，本地化字符串。
- **#389** — 内联 LSP 诊断：`crates/tui/src/lsp/` 中的 LSP 子系统，`core/engine/lsp_hooks.rs`
  中的引擎钩子，配置开关，测试覆盖。
- **#387** — 自更新：新增 `crates/cli/src/update.rs` 模块，CLI 子命令注册，
  HTTP 下载 + SHA256 校验 + 原子化二进制替换。
- **#393** — `/share` 会话 URL：新增 `crates/tui/src/commands/share.rs`，HTML 渲染，
  `gh gist create` 集成，命令注册。
- **#343/#346** — (v0.8.5) 运行时线程/轮次时间线以及持久化任务管理器重构。

典型情况下，每个 PR 涉及 1-3 个新文件，修改 2-5 个现有文件以完成接线（注册表、分发匹配、
本地化），并添加或更新测试。更改范围限定为单个功能或修复——如果你发现了需要处理的相关工作，
请打开单独的 issue 而不是扩大 PR 范围。

提交前，运行：
```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features 2>&1 | head -50
cargo check
```

## 报告问题

报告问题时，请包含以下信息：

- 操作系统及版本
- Rust 版本（`rustc --version`）
- DeepSeek TUI 版本（`deepseek --version`）
- 复现问题的步骤
- 预期行为与实际行为
- 相关的错误消息或日志

## 行为准则

请保持尊重和包容。我们欢迎来自各种背景和经验水平的贡献者。

## 许可证

通过向 DeepSeek TUI 贡献代码，你同意你的贡献将按照 MIT 许可证进行许可。

## 有问题？

如有任何关于贡献的问题，欢迎随时开启一个 issue。
