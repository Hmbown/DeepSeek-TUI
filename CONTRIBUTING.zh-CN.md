# 为 DeepSeek TUI 贡献代码

感谢你对 DeepSeek TUI 的贡献兴趣！本文档提供贡献的指南和说明。

## 快速开始

### 前提条件

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

- 提交前运行 `cargo fmt` 以保持格式一致
- 运行 `cargo clippy` 并处理所有警告
- 遵循 Rust 命名规范（函数/变量使用 snake_case，类型使用 CamelCase）
- 为公开 API 添加文档注释

### 测试

- 为新功能编写测试
- 确保所有现有测试通过：`cargo test --workspace --all-features`
- 将单元测试放在与所覆盖代码同文件的 `#[cfg(test)]` 模块中；将集成测试放在所属 crate 的 `tests/` 目录下（例如 `crates/tui/tests/` 或 `crates/state/tests/`）。仓库根目录的 `tests/` 目录未被使用

### 提交信息

使用清晰、描述性的提交信息，遵循 conventional commits 规范：

- `feat:` 新功能
- `fix:` Bug 修复
- `docs:` 文档变更
- `refactor:` 代码重构
- `test:` 添加或更新测试
- `chore:` 维护任务

示例：`feat: add doctor subcommand for system diagnostics`

## 项目结构

DeepSeek TUI 是一个 Cargo workspace。运行时和大部分 TUI、引擎、工具代码位于 `crates/tui/src/`。较小的 workspace crate 提供正在逐步提取的共享抽象。

```
crates/
├── tui/           deepseek-tui 二进制（交互式 TUI + 运行时 API）
├── cli/           deepseek 二进制（调度器门面）
├── app-server/    HTTP/SSE + JSON-RPC 传输
├── core/          代理循环 / 会话 / 轮次管理
├── protocol/      请求/响应帧格式
├── config/        配置加载、profiles、环境变量优先级
├── state/         SQLite 线程/会话持久化
├── tools/         类型化工具规格与生命周期
├── mcp/           MCP 客户端 + stdio 服务器
├── hooks/         生命周期钩子（stdout/jsonl/webhook）
├── execpolicy/    审批/沙箱策略引擎
├── agent/         模型/提供商注册
└── tui-core/      事件驱动的 TUI 状态机脚手架
```

关于这些 crate 之间的实时数据流，请参阅 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)；关于构建顺序，请参阅 [DEPENDENCY_GRAPH.md](DEPENDENCY_GRAPH.md)。

## 提交变更

1. 从 `main` 分支创建功能分支：
   ```bash
   git checkout -b feat/your-feature
   ```

2. 进行修改并提交

3. 确保 CI 通过：
   ```bash
   cargo fmt --check
   cargo clippy
   cargo test
   ```

4. 推送分支并创建 Pull Request

5. 在 PR 描述中清晰说明你的变更

## Pull Request 指南

- PR 应专注于单一变更
- 如有需要，更新文档
- 为新功能添加测试
- 确保 CI 通过后再请求审查

## 典型 PR 的结构

结构良好的 PR 遵循一致的模式。近期的优秀示例包括：

- **#386** — `/init` 命令：新增 `crates/tui/src/commands/init.rs` 模块，项目类型检测，AGENTS.md 生成，命令注册于 `commands/mod.rs`，本地化字符串。
- **#389** — 内联 LSP 诊断：LSP 子系统在 `crates/tui/src/lsp/`，引擎钩子在 `core/engine/lsp_hooks.rs`，配置开关，测试覆盖。
- **#387** — 自更新：新增 `crates/cli/src/update.rs` 模块，CLI 子命令注册，HTTP 下载 + SHA256 校验 + 原子二进制替换。
- **#393** — `/share` 会话 URL：新增 `crates/tui/src/commands/share.rs`，HTML 渲染，`gh gist create` 集成，命令注册。
- **#343/#346** —（v0.8.5）运行时线程/轮次时间线和持久化任务管理器重构。

通常每个 PR 涉及 1-3 个新文件，修改 2-5 个现有文件用于连接（注册表、dispatch 匹配、本地化），并添加或更新测试。变更范围限于单一功能或修复 —— 如果发现需要做的相关工作，请单独开 issue，而不是扩大 PR 范围。

提交前，运行：
```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features 2>&1 | head -50
cargo check
```

## 报告问题

报告问题时，请包含：

- 操作系统和版本
- Rust 版本（`rustc --version`）
- DeepSeek TUI 版本（`deepseek --version`）
- 复现问题的步骤
- 预期行为与实际行为
- 相关的错误消息或日志

## 行为准则

保持尊重和包容。我们欢迎各种背景和经验水平的贡献者。

## 许可证

通过为 DeepSeek TUI 贡献代码，你同意你的贡献将基于 MIT 许可证进行许可。

## 有问题？

欢迎就任何关于贡献的问题提交 issue。
