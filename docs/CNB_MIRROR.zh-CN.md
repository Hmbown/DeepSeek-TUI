# CNB Cool 镜像

[English](CNB_MIRROR.md) | [繁體中文](CNB_MIRROR.zh-TW.md)

`cnb.cool/deepseek-tui.com/DeepSeek-TUI` 是本 GitHub 仓库的单向镜像，面向 GitHub 速度慢或被屏蔽（主要是中国大陆）网络环境中的用户。该镜像接收每次对 `main` 的推送、每个 `v*` 版本 tag 以及 Lighthouse/Feishu 设置所使用的 Tencent 候选发布分支。

## 工作原理

该镜像由 [`Sync to CNB`](../.github/workflows/sync-cnb.yml) GitHub Actions 工作流维护：

- **触发器：** 推送到 `main`、推送任何 `v*` tag、匹配 `work/v*-feishu-*` 或 `work/v*-lighthouse*` 的 Tencent 设置分支，或用于手动恢复的 `workflow_dispatch`。
- **认证：** HTTP 基本认证，用户名为 `cnb`，密码为仓库 secret `CNB_GIT_TOKEN`。
- **范围：** 仅推送触发运行的那个 ref。Tag 推送只推送该 tag。分支推送镜像 `main` 或一个显式匹配的 Tencent 设置分支。其他功能分支和 dependabot ref **不会**被镜像。
- **并发：** 通过一个 `cnb-sync` 并发组进行序列化运行，因此来自 `auto-tag.yml` 的连续 `main` 推送和 tag 推送不会竞争。
- **重试：** 每次推送在工作流放弃前最多重试三次，采用线性退避（5s、10s）。

CNB 流水线配置也在 GitHub 中进行源码控制，位于 [`/.cnb.yml`](../.cnb.yml)。这是有意为之：同步工作流将 GitHub ref 强制镜像到 CNB，因此仅在 CNB 侧创建的流水线文件将会被覆盖。请通过 GitHub PR 提交 `.cnb.yml` 更改，由单向镜像将其带到 CNB。

## CNB tag 发布

当 CNB 接收到一个 `v*` tag 时，根目录下的 `.cnb.yml` tag 流水线会从源码构建 Linux x64 发布资产，并发布一个 CNB release，包含：

- `deepseek-linux-x64`
- `deepseek-tui-linux-x64`
- `deepseek-artifacts-sha256.txt`

这使得能够访问 CNB 但无法访问 GitHub 的用户拥有一条 CNB 原生的发布路径。GitHub 仍是规范的全平台发布矩阵；CNB tag 流水线是中国友好的 Linux x64 回退方案。

## 发布后验证镜像

在 `vX.Y.Z` tag 的 `release.yml` 完成后，CNB 镜像应同时拥有 `main` 上的新提交和新的 tag：

```bash
# 快速检查：新 tag 是否存在于 CNB？
git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git \
    refs/tags/vX.Y.Z

# 快速检查：CNB 的 main 是否与 origin/main 在同一个 commit？
gh_main=$(git ls-remote https://github.com/Hmbown/DeepSeek-TUI.git refs/heads/main | awk '{print $1}')
cnb_main=$(git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git refs/heads/main | awk '{print $1}')
test "$gh_main" = "$cnb_main" && echo "同步" || echo "分歧: gh=$gh_main cnb=$cnb_main"
```

或直接检查工作流运行：

```bash
gh run list --workflow=sync-cnb.yml --repo Hmbown/DeepSeek-TUI --limit 5
```

如果与发布 tag 对应的最新运行状态为 `success`，说明镜像已捕获。如果是 `failure`，请按以下手动回退方案操作。

## 手动回退

如果工作流因任何原因失败（CNB 限流、token 过期、GitHub 宕机等），维护者可以从其本地 checkout 手动推送到 CNB。这是可行的，因为 CNB token 是一个个人 PAT——工作流使用的同一 token 保存在维护者的密码管理器中。

### 一次性设置

```bash
# 将 CNB remote 添加在 origin 旁边。
git remote add cnb https://cnb:${CNB_TOKEN}@cnb.cool/deepseek-tui.com/DeepSeek-TUI.git

# 或者，如果你不希望 token 出现在 shell 历史中：
git remote add cnb https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
# （在首次推送时，会提示你输入用户名 `cnb` 和密码 ${CNB_TOKEN}；
#  后续推送将使用凭证助手。）
```

### 手动同步一个发布

```bash
# 确保 main 是最新的。
git fetch origin
git checkout main
git reset --hard origin/main

# 先推送 main，再推送 tag。顺序很重要：CNB 在收到指向它的 tag 之前
# 应先看到 commit。
git push cnb main --force-with-lease
git push cnb vX.Y.Z
```

### 手动重新触发工作流

如果工作流是健康的，但恰好在该次发布运行中失败（例如一个已经恢复的临时性 CNB 中断），无需推送任何内容即可重新触发它：

```bash
gh workflow run sync-cnb.yml --repo Hmbown/DeepSeek-TUI
```

`workflow_dispatch` 针对工作流的默认分支（`main`）运行，因此这将把当前 `main` 同步到 CNB。要重新同步一个特定的 tag，请使用上面的手动 `git push cnb` 方式。

## 轮换 `CNB_GIT_TOKEN`

如果工作流开始因认证错误而失败且 token 已过期：

1. 登录 `cnb.cool`，生成一个具有 `repo`（推送）权限的新个人访问 token。
2. 更新 `CNB_GIT_TOKEN` 仓库 secret：
   ```bash
   gh secret set CNB_GIT_TOKEN --repo Hmbown/DeepSeek-TUI
   ```
3. 在一个最近的 commit 上重新触发工作流：
   ```bash
   gh workflow run sync-cnb.yml --repo Hmbown/DeepSeek-TUI
   ```
4. 通过 `gh run list --workflow=sync-cnb.yml` 确认运行成功。

## 二进制发布资产和 `deepseek update`

CNB 现在从源码控制的 `.cnb.yml` 流水线为 `v*` tag 构建 Linux x64 资产。GitHub 仍是规范的全平台发布矩阵。在 GitHub 被屏蔽的网络环境中的用户应使用以下路径之一：

- **`cargo install`** 从 CNB 镜像安装：
  ```bash
  cargo install --git https://cnb.cool/deepseek-tui.com/DeepSeek-TUI --tag vX.Y.Z deepseek-tui-cli
  cargo install --git https://cnb.cool/deepseek-tui.com/DeepSeek-TUI --tag vX.Y.Z deepseek-tui
  ```
  （两个二进制文件都是必需的——dispatcher 和 TUI 分别发布；参见 `AGENTS.md` 了解双二进制安装的原理。）

- **CNB 发布资产** 用于 Linux x64，当匹配的 CNB tag 流水线成功完成时。从 CNB release 下载 `vX.Y.Z` 的 `deepseek-linux-x64`、`deepseek-tui-linux-x64` 和 `deepseek-artifacts-sha256.txt`，然后对照清单验证二进制文件。

- **`DEEPSEEK_TUI_RELEASE_BASE_URL`** 环境变量，如果存在发布资产的 CDN 镜像。npm wrapper 安装器和 `deepseek update` 会读取此变量以重定向二进制下载。对于 `deepseek update`，还需设置 `DEEPSEEK_TUI_VERSION=X.Y.Z`，以便更新器可以标记镜像的 release 而无需联系 GitHub。所指向的目录必须包含 `deepseek-artifacts-sha256.txt` 和平台二进制文件；格式匹配 GitHub Release 资产目录。

## Tencent Cloud 远程优先路径

Lighthouse + Feishu/Lark 教程使用 CNB 作为 Tencent 侧的源码和自动化通道。如需稳定安装，请从以下地址克隆 `main` 或一个发布 tag：

```bash
https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
```

镜像接收 `main`、发布 tag 和 Lighthouse/Feishu 教程使用的 Tencent 设置分支模式。这些 CNB ref 是 Tencent 侧引导的默认源码；仅当 CNB 工作流或凭证不健康时，GitHub 才作为回退方案。

CNB 部署按钮示例位于 `deploy/tencent-lighthouse/cnb/` 中。它们在复制到 `.cnb.yml` 和 `.cnb/tag_deploy.yml` 之前不会激活，因为实时部署任务需要 Lighthouse 部署密钥、目标主机和显式的 CNB 配额/计费策略。
