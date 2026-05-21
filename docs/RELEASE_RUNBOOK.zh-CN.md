# DeepSeek TUI 发布 Runbook

[English](RELEASE_RUNBOOK.md) | [繁體中文](RELEASE_RUNBOOK.zh-TW.md)

本 runbook 是发布 Rust crates、GitHub release 资产和 `deepseek-tui` npm 包装的权威指南。

当前打包说明：
- `deepseek-tui` 是当前面向用户发布的实时运行时和 TUI 包。
- `deepseek-tui-core` 是一个支持性的工作区 crate，用于提取/对等工作，并非当前运行时的替代品。

## 官方发布目标

- 最终用户 crate：
  - `deepseek-tui`
  - `deepseek-tui-cli`
- 从本工作区发布的支持性 crate：
  - `deepseek-secrets`
  - `deepseek-config`
  - `deepseek-protocol`
  - `deepseek-state`
  - `deepseek-agent`
  - `deepseek-execpolicy`
  - `deepseek-hooks`
  - `deepseek-mcp`
  - `deepseek-tools`
  - `deepseek-core`
  - `deepseek-app-server`
  - `deepseek-tui-core`
- `deepseek-cli` 在 crates.io 上是一个不相关的 crate，不属于此发布流程。

## 版本协调

- Rust crate 从 [Cargo.toml](../Cargo.toml) 继承共享的工作区版本。
- 内部路径依赖版本应与共享的工作区版本匹配；一旦工作区版本更新，过时的旧版本号将成为发布阻塞项。
- npm 包装版本位于 [npm/deepseek-tui/package.json](../npm/deepseek-tui/package.json) 中。
- `deepseekBinaryVersion` 控制 npm 包装下载哪些 GitHub release 二进制文件。
- 允许仅打包的 npm 发布：
  - 升级 npm 包版本
  - 将 `deepseekBinaryVersion` 固定到之前发布的 Rust 二进制文件版本
  - 在 `npm publish` 之前重新运行 `npm pack` 烟雾测试

## 预检

在打标签之前，从仓库根目录运行以下命令：

```bash
./scripts/release/check-versions.sh   # 检查 workspace、npm、lockfile 之间的版本漂移
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo publish --dry-run --locked --allow-dirty -p deepseek-tui
./scripts/release/publish-crates.sh dry-run
```

`check-versions.sh` 也会在 CI 的每次推送/PR 中运行（`.github/workflows/ci.yml` 中的 `versions` 任务），因此 `Cargo.toml`、各个 crate 的清单文件、`npm/deepseek-tui/package.json` 和 `Cargo.lock` 之间的版本漂移会在发布前而非发布时被发现。

`publish-crates.sh dry-run` 对没有未发布工作区依赖的 crate 执行完整的 `cargo publish --dry-run`，对有依赖关系的工作区 crate 执行打包预检。这样可以避免因 crates.io 尚未包含新工作区版本而导致的误报，同时在发布前仍能验证包内容。

对于 npm 包装验证，构建两个发布的二进制文件并运行跨平台烟雾测试工具。该工具打包 npm 包装，将其安装到一个干净的临时项目中，通过 HTTP 提供本地 release 资产，并检查调度器到 TUI 路径（`deepseek doctor --help`）和直接 TUI 入口点（`deepseek-tui --help`）。

```bash
cargo build --release --locked -p deepseek-tui-cli -p deepseek-tui
node scripts/release/npm-wrapper-smoke.js
```

设置 `DEEPSEEK_TUI_KEEP_SMOKE_DIR=1` 以保留临时打包/安装目录供检查。

要同时在本地执行 `npm run release:check`，在启动服务器前使用完整的资产矩阵 fixtures 重新生成本地资产目录：

```bash
DEEPSEEK_TUI_PREPARE_ALL_ASSETS=1 node scripts/release/prepare-local-release-assets.js
cd npm/deepseek-tui
DEEPSEEK_TUI_VERSION=X.Y.Z DEEPSEEK_TUI_RELEASE_BASE_URL=http://127.0.0.1:8123/ npm run release:check
```

将 `DEEPSEEK_TUI_VERSION` 设置为该次本地运行中您要验证的 npm 包版本。

CI 工作流在 Linux、macOS 和 Windows 上运行相同的 tarball 安装 + 委托入口点烟雾测试。

发布后，验证版本在两个注册表中均可见：

```bash
./scripts/release/check-published.sh X.Y.Z
```

在该命令在 npm 上看到 `deepseek-tui@X.Y.Z` 并在 crates.io 上看到每个 `deepseek-*` crate 的 `X.Y.Z` 之前，不要标记 Rust 发布为完成。对于罕见的仅 npm 打包发布，使用 `--allow-npm-binary-mismatch` 运行，并在发布说明中明确说明没有新的 Rust 二进制版本发布。

## Rust Crates 发布

Crate 发布到 crates.io 是**手动的**——没有自动化的 `crates-publish` GitHub 工作流。操作员在已配置 `cargo login` 的开发工作站上运行 `scripts/release/` 中的辅助脚本。

1. 更新 [Cargo.toml](../Cargo.toml) 中的工作区版本。
2. 在本地运行 `./scripts/release/check-versions.sh` 和 `./scripts/release/publish-crates.sh dry-run`；两者都必须通过。
3. 将发布标记为 `vX.Y.Z`（通常通过将版本更新推送到 `main` 并让 `auto-tag.yml` 创建标签——请参阅下面的 npm 包装发布部分了解 `RELEASE_TAG_PAT` 要求）。
4. 使用 `./scripts/release/publish-crates.sh publish` 按以下顺序发布 crate：
   - `deepseek-secrets`
   - `deepseek-config`
   - `deepseek-protocol`
   - `deepseek-state`
   - `deepseek-agent`
   - `deepseek-execpolicy`
   - `deepseek-hooks`
   - `deepseek-mcp`
   - `deepseek-tools`
   - `deepseek-core`
   - `deepseek-app-server`
   - `deepseek-tui-core`
   - `deepseek-tui-cli`
   - `deepseek-tui`
5. 在发布依赖项之前，等待每个已发布 crate 版本出现在 crates.io 上。

发布辅助脚本对于重新运行是幂等的：已发布的 crate 版本将被跳过。

## GitHub Release 资产

`.github/workflows/release.yml` 构建以下二进制文件：

- `deepseek-linux-x64`
- `deepseek-macos-x64`
- `deepseek-macos-arm64`
- `deepseek-windows-x64.exe`
- `deepseek-tui-linux-x64`
- `deepseek-tui-macos-x64`
- `deepseek-tui-macos-arm64`
- `deepseek-tui-windows-x64.exe`

发布任务还会上传 `deepseek-artifacts-sha256.txt`。npm 安装程序和发布验证脚本都依赖此校验和清单。

## npm 包装发布

**npm 发布步骤是手动的。** `release.yml` 不再运行 `npm publish`，因为 npm 账户在每次发布时需要 2FA OTP，并且尚未配置可绕过 2FA 的自动化 token。GitHub Release 流程保持完全自动化；仅 npm 包装发布需要开发者在配有 `npm login` 和认证器应用的工作站上操作。

### 步骤

1. 在 [npm/deepseek-tui/package.json](../npm/deepseek-tui/package.json) 中设置 npm 包版本以匹配工作区 `Cargo.toml`。CI 的版本漂移检查将在打标签前捕获不匹配。
2. 将 `deepseekBinaryVersion` 设置为应提供二进制文件的 GitHub release 标签。
3. 将版本更新推送到 `main`。`auto-tag.yml` 创建匹配的 `vX.Y.Z` 标签，`release.yml` 构建二进制矩阵并草拟 GitHub Release。
4. **等待 GitHub Release 完成**，包含所有八个已签名二进制文件及 `deepseek-artifacts-sha256.txt`。npm 的 `prepublishOnly` 钩子（`scripts/verify-release-assets.js`）要求每个资产都存在。
5. 从开发者机器上手动发布 npm 包装：

```bash
cd npm/deepseek-tui
npm publish --access public
# （系统会提示您从认证器输入 npm OTP）
```

### 为什么不自动化？

- `release.yml` 之前的 `publish-npm` 任务使用了 `secrets.NPM_TOKEN`，但 npm 的 2FA 默认策略意味着发布 token 必须是启用了"Bypass 2FA for token authentication"的自动化 token，或者账户级别禁用 2FA。我们两者都未配置。
- 独立的 `publish-npm.yml` 和 `crates-publish.yml` 工作流已被移除；不再保留无用的自动化管道。未来转向 npm Trusted Publishing（OIDC）时会在那时重新引入专用的工作流。

### 如果您之后修复了 token

要重新启用自动化发布：配置一个启用了"Bypass 2FA for token authentication"的 npm 自动化 token（或通过 OIDC 设置 npm Trusted Publishing），在仓库中存储相应的 secret，然后重新向 `release.yml` 添加 `publish-npm` 任务（或一个专用工作流），同时还原此部分的"手动"描述。

## CNB Cool 镜像

每次推送到 `main` 和每个 `v*` 标签都会通过 `Sync to CNB` 工作流镜像到 `cnb.cool/deepseek-tui.com/DeepSeek-TUI`，以便处于 GitHub 封锁网络环境中的用户可以获取源代码。发布标签后，在宣布发布完成之前**验证镜像已捕获该标签**：

```bash
git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git refs/tags/vX.Y.Z
```

如果工作流在发布标签时失败，手动回退方法记录在 [docs/CNB_MIRROR.md](CNB_MIRROR.md) 中（一次性 `git remote add cnb …`，然后 `git push cnb vX.Y.Z`）。

## 恢复与回滚

- Crate 发布部分完成：
  - 重新运行 `./scripts/release/publish-crates.sh publish`
  - 已发布的 crate 版本将被跳过
- GitHub 资产缺失或校验和清单不完整：
  - 修复 `.github/workflows/release.yml`
  - 重新打标签或在 `npm publish` 之前上传修正后的资产
- npm 仅打包问题：
  - 仅升级 npm 包版本
  - 将 `deepseekBinaryVersion` 保持在最后一个已知良好的 Rust 版本上
  - 重新打包并重新发布包装
- 错误的 npm 发布无法覆盖：
  - 发布一个带有修正元数据或安装逻辑的新 npm 版本
- CNB 镜像在发布标签时失败：
  - 通过 `gh run list --workflow=sync-cnb.yml` 检查运行情况
  - 使用 `gh workflow run sync-cnb.yml` 重新触发，或按照 [docs/CNB_MIRROR.md](CNB_MIRROR.md#manual-fallback) 手动推送标签
