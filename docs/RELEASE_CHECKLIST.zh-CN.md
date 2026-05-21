# 发布检查清单

[English](RELEASE_CHECKLIST.md) | [繁體中文](RELEASE_CHECKLIST.zh-TW.md)

v0.8.21/v0.8.22 的 CHANGELOG 差距证明了我们需要一份打 tag 前的检查清单。请从发布分支（`work/vX.Y.Z-...`）上的干净 worktree 按顺序逐项执行。任何未勾选的方框均视为发布阻碍。

有关底层工具（preflight 脚本、npm smoke、publish-crates）的更多上下文，请参见 [`RELEASE_RUNBOOK.md`](RELEASE_RUNBOOK.md)。

## 1. 该版本的 CHANGELOG 条目已存在

- [ ] `CHANGELOG.md` 在顶部有一个 `## [X.Y.Z] - YYYY-MM-DD` 标题
- [ ] 条目中致谢了每个其提交包含在此版本中的外部贡献者。按以下命令获取列表：
      ```
      git log vPREV..HEAD --no-merges --format="%h %an <%ae> %s" \
        | grep -v '<your-email@…>'
      ```
      对于每位贡献者，链接其显示名称和（如已知）`@github-handle`。
- [ ] 条目使用了 Keep a Changelog 的标题格式——`Added`、`Changed`、`Fixed`、`Security`、`Removed`、`Deprecated`。仅当存在用户必须规避的重大问题时才添加 `Known issues`。
- [ ] 条目将所有引用的 issue/PR 编号以 `#NNNN` 形式列出，以便 GitHub 上的自动链接器识别。

## 2. 版本号已同步

- [ ] `Cargo.toml` 工作区 `version` 已升级。
- [ ] 所有 `crates/*/Cargo.toml` 中的每个 crate 路径依赖 `version = "..."` 值均与新工作区版本一致。
- [ ] `npm/deepseek-tui/package.json` 的 `version` **和** `deepseekBinaryVersion` 均已升级。
- [ ] `Cargo.lock` 已刷新（`cargo update --workspace --offline`）。
- [ ] `./scripts/release/check-versions.sh` 输出 `Version state OK: workspace=X.Y.Z, npm=X.Y.Z, lockfile in sync.`

## 3. Preflight 门禁

从仓库根目录按顺序运行：

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo check --workspace --all-targets --locked`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --all-features --locked`
      （在宣布某次失败为 flake 之前，先用 `cargo test -p PKG --bin BIN -- TEST_NAME` 单独重新运行该次失败的测试。会修改进程全局状态——`HOME`、`cwd`、`RUST_LOG`——的测试可能在并行执行时产生竞态。将确认的 flake 记录在 `Known issues` 中。）
- [ ] `./scripts/release/publish-crates.sh dry-run`

## 4. npm wrapper smoke

- [ ] `cargo build --release --locked -p deepseek-tui-cli -p deepseek-tui`
- [ ] `node scripts/release/npm-wrapper-smoke.js`
      （如需在之后检查临时安装目录，设置 `DEEPSEEK_TUI_KEEP_SMOKE_DIR=1`。）

## 5. 分支和 PR

- [ ] 分支已推送：`git push -u origin work/vX.Y.Z-...`
- [ ] 已使用 `gh pr create --base main --title "chore(release): prepare vX.Y.Z"` 创建 PR
- [ ] PR 正文包含：
  - 一或两段的发布主题摘要
  - 自上次发布以来的新提交要点清单
  - 显式标注所有 **Security** 项，以便审阅者看到
  - 贡献者致谢列表
  - 来自 CHANGELOG 的 `Known issues` 块（如有）
- [ ] PR 标题是**中性的**——不要在标题中放入 CVE 风格的用语或具体的攻击细节。这些内容留待推送 tag 后在 GitHub release notes 中撰写。

## 6. CI 全绿且已审阅

- [ ] 所有必须的 CI 任务均为绿色。`versions` 任务应反映 preflight `check-versions.sh` 的结果，是你的最后一道防线。
- [ ] PR 已获审阅。

## 7. Tag 和发布（审阅后）

- [ ] `git tag -s vX.Y.Z -m "vX.Y.Z"`
- [ ] `git push origin vX.Y.Z`
- [ ] `release.yml` 工作流已为该 tag 构建并上传资产到 GitHub release。
- [ ] `npm view deepseek-tui@X.Y.Z version deepseekBinaryVersion --json` 报告新版本已出现在 npm registry 上。
- [ ] `crates.io` 已有新版本（或 `publish-crates.sh` 任务已推送）。
- [ ] `ghcr.io/hmbown/deepseek-tui:vX.Y.Z` 和 `:latest` 已更新。

## 8. Post-tag

- [ ] 编辑 GitHub release notes，扩展在 PR 标题/正文中刻意省略的 CVE 风格或攻击细节。
- [ ] 在下个版本的跟踪 issue 中记录所有延期的项目。
- [ ] 关闭本发布修复的所有 issues。

---

如果某个步骤失败，**修复根本原因**，而非跳过它。Pre-commit hooks、签名和 CI 都是为了捕获真实问题而存在的。`--no-verify`、`--no-gpg-sign` 以及越过审阅者强制推送发布分支应通过约定保持硬性禁用。
