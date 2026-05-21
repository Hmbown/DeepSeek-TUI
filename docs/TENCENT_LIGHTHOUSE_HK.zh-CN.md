# 腾讯云 Lighthouse 香港手机端设置

[English](TENCENT_LIGHTHOUSE_HK.md) | [繁體中文](TENCENT_LIGHTHOUSE_HK.zh-TW.md)

本 runbook 用于在香港设置一台腾讯云 Lighthouse 实例，作为始终在线的 DeepSeek TUI 主机，通过飞书/Lark 从手机端进行控制。

如果您将此作为腾讯云原生默认路径进行教学，请从 [docs/TENCENT_CLOUD_REMOTE_FIRST.md](TENCENT_CLOUD_REMOTE_FIRST.md) 开始。本文档是 Lighthouse 主机本身的实施 runbook。

## 目标架构

```text
CNB 镜像或 GitHub 分支
  -> /opt/whalebro/deepseek-tui

飞书/Lark 移动应用
  -> 飞书/Lark 长连接机器人
  -> deepseek-feishu-bridge systemd 服务
  -> http://127.0.0.1:7878 deepseek serve --http
  -> /opt/whalebro
       -> deepseek-tui/

可选公网边缘：
EdgeOne -> Lighthouse 上的 Caddy/Nginx 公网站点
```

运行时 API 必须保持在 `127.0.0.1` 上。bridge 是唯一面向手机的控制面。EdgeOne 是可选的，仅应前置在有意公开的 HTTP 服务前，而非运行时 API。

## 远程 Whalebro 工作区

使用 `/opt/whalebro` 作为 VPS 工作区根目录。首要的检出目录是 `/opt/whalebro/deepseek-tui`。

首先创建这些路径：

- `/opt/whalebro/deepseek-tui`
- `/opt/whalebro/worktrees`

Linux 足以完成 Rust、Node 和服务相关工作。仅 Mac 专属的发布工作，如 iOS 模拟器运行、`.app`/DMG 检查、公证和 Apple 签名，仍应在 Mac 上完成。

## Lighthouse 实例

出行推荐套餐：

- 区域：香港（中国）
- 镜像：标准 Ubuntu 24.04 LTS 或最新的 Ubuntu LTS
- 规格：首月购买香港 2 vCPU / 4 GB / 70 GB 套餐
- 登录：SSH 密钥，而非密码
- 防火墙：SSH 开放；运行时 API 仅在 localhost 上

腾讯云 Lighthouse 文档说明 Linux 实例可以使用 SSH 密钥，且 Lighthouse 防火墙默认开放 SSH/HTTP/HTTPS 端口。

使用 4 GB 内存可流畅编译 Rust 和运行 bridge。4 vCPU / 8 GB 套餐更适合多个并行 agent 工作进程。

## 飞书 / Lark 应用

在以下位置创建企业自建应用：

- 飞书中国：`https://open.feishu.cn/app`
- Lark 国际版：`https://open.larksuite.com/app`

配置：

1. 启用机器人能力。
2. 复制 App ID 和 App Secret。
3. 添加消息发送/接收权限。最低实用权限集为：
   - `im:message`
   - `im:message:send_as_bot`
   - 您租户的私信读取权限
   - 仅在您有意启用群组控制后，才需要群组 @消息 读取权限
4. 添加事件订阅 `im.message.receive_v1`。
5. 使用长连接 / WebSocket 模式。
6. 发布应用并将机器人添加到您的飞书/Lark 聊天中。

## 服务器引导

SSH 登录 Lighthouse 实例并执行：

```bash
sudo apt-get update
sudo apt-get install -y git
export DEEPSEEK_BRANCH=main
export DEEPSEEK_REPO_URL=https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
git clone --branch "$DEEPSEEK_BRANCH" "$DEEPSEEK_REPO_URL" /tmp/deepseek-tui
cd /tmp/deepseek-tui
sudo DEEPSEEK_REPO_URL="$DEEPSEEK_REPO_URL" \
  DEEPSEEK_REPO_BRANCH="$DEEPSEEK_BRANCH" \
  bash scripts/tencent-lighthouse/bootstrap-ubuntu.sh
```

如果您希望从 VPS 获得推送权限，请改用 SSH 仓库 URL。如果 CNB 镜像不可用，回退到：

```bash
export DEEPSEEK_REPO_URL=https://github.com/Hmbown/DeepSeek-TUI.git
```

对于稳定版本文档，在使用 CNB 镜像前确认其包含该分支或标签：

```bash
export DEEPSEEK_REPO_URL=https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
git ls-remote "$DEEPSEEK_REPO_URL" \
  refs/heads/main \
  refs/tags/v0.8.37
```

CNB 镜像接收 `main` 分支和发布标签。CNB 是此 Lighthouse 路径的默认源；GitHub 仅在 CNB 工作流或凭据异常时作为回退。

如果此部署配置尚未推送到 Git，请先推送分支或将此检出目录复制到 VPS，然后再运行这些命令。全新的 VPS 克隆无法看到未提交的本地文件。

为 `deepseek` 用户安装 Rust 1.88+，然后构建两个发布的二进制文件：

```bash
sudo -iu deepseek
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
sed -n '1,120p' /tmp/rustup-init.sh
sh /tmp/rustup-init.sh -y --profile minimal
. "$HOME/.cargo/env"
rustup default stable
cd /opt/whalebro/deepseek-tui
cargo install --path crates/cli --locked --force
cargo install --path crates/tui --locked --force
exit
```

复制并安装 bridge/服务文件：

```bash
cd /opt/whalebro/deepseek-tui
sudo bash scripts/tencent-lighthouse/install-services.sh
```

编辑两个 env 文件后，验证 bridge/运行时配对：

```bash
sudo -u deepseek node /opt/deepseek/bridge/scripts/validate-config.mjs \
  --env /etc/deepseek/feishu-bridge.env \
  --runtime-env /etc/deepseek/runtime.env \
  --workspace-root /opt/whalebro \
  --check-filesystem
```

## 密钥

生成一个运行时 token，并在两个 env 文件中使用相同的值：

```bash
openssl rand -hex 32
sudoedit /etc/deepseek/runtime.env
sudoedit /etc/deepseek/feishu-bridge.env
```

必需的值：

- `/etc/deepseek/runtime.env`
  - `DEEPSEEK_API_KEY`
  - `DEEPSEEK_RUNTIME_TOKEN`
- `/etc/deepseek/feishu-bridge.env`
  - `FEISHU_APP_ID`
  - `FEISHU_APP_SECRET`
  - `FEISHU_DOMAIN=feishu`（飞书使用），`lark`（Lark 使用）
  - `DEEPSEEK_RUNTIME_TOKEN`
  - `FEISHU_ALLOW_GROUPS=false`（首次部署时使用）

首次配对时，选择以下任一方式：

1. 临时设置 `DEEPSEEK_ALLOW_UNLISTED=true`，向机器人发送消息，复制返回的 `chat_id`，然后设置 `DEEPSEEK_CHAT_ALLOWLIST=<chat_id>` 并关闭未列出的访问权限。
2. 或者从飞书/Lark 事件日志中获取聊天 ID，并在首次启动前设置 allowlist。

## 启动服务

```bash
sudo systemctl start deepseek-runtime
sudo systemctl status deepseek-runtime --no-pager
curl -s http://127.0.0.1:7878/health

sudo systemctl start deepseek-feishu-bridge
sudo journalctl -u deepseek-feishu-bridge -f
```

配置完两个服务后，运行 Lighthouse doctor：

```bash
cd /opt/whalebro/deepseek-tui
sudo bash scripts/tencent-lighthouse/doctor.sh
```

开机自启由 `install-services.sh` 完成；如需手动设置：

```bash
sudo systemctl enable deepseek-runtime deepseek-feishu-bridge
```

## 手机端命令

私信可以是纯文本，是预期的首选控制路径：

```text
check git status and summarize what needs attention
```

群聊默认禁用。如果您之后设置 `FEISHU_ALLOW_GROUPS=true`，群聊提示必须以 `/ds` 开头。

有用的命令：

- `/status`
- `/threads`
- `/new`
- `/resume <thread_id>`
- `/interrupt`
- `/compact`
- `/allow <approval_id>`
- `/deny <approval_id>`
- `/allow <approval_id> remember`

仅在您有意让运行时线程在未来转向自动批准工具时使用 `remember`。

## CNB 部署按钮

手动 Lighthouse 设置通过后，CNB 可以成为可重复的部署按钮：

1. 将 `deploy/tencent-lighthouse/cnb/cnb.yml.example` 复制为 CNB 仓库中的 `.cnb.yml`。
2. 将 `deploy/tencent-lighthouse/cnb/tag_deploy.yml.example` 复制为 `.cnb/tag_deploy.yml`。
3. 配置 `deploy/tencent-lighthouse/cnb/README.md` 中记录的 CNB 部署密钥。
4. 触发 `lighthouse-hk` 部署环境。

在服务器稳定之前保持手动操作。每次推送时自动部署虽然方便，但会消耗 CNB 配额，并可能在手机端对话活跃时重启 bridge。

## EdgeOne

首次设置飞书/Lark 长连接时不需要 EdgeOne。仅在您需要在 Lighthouse 主机上的有意公开服务前放置一个公网 HTTPS 域名时才添加。

EdgeOne 的良好用途：

- 公开文档或教程站点
- 小型运维状态页面
- 未来的 webhook 模式 bridge 端点
- 托管在同一 Lighthouse 实例上的演示 Web 应用

不要使用 EdgeOne 暴露：

- `http://127.0.0.1:7878`
- `/v1/*` 运行时端点
- 任何接受 `DEEPSEEK_RUNTIME_TOKEN` 的端点

## 端到端验证

从手机私信发送给机器人：

1. 发送 `/status` 确认运行时版本、localhost 绑定、认证状态、工作区、git 仓库、分支和脏文件计数。
2. 发送一个无害的提示，如 `summarize git status`。
3. 在对话活跃时发送 `/interrupt` 并确认对话停止。
4. 发送 `/threads`，然后对列出的某个线程发送 `/resume <thread_id>`。
5. 触发工具审批，验证 `/allow <approval_id>` 和 `/deny <approval_id>` 两个路径。
6. 重启两个服务并重新运行 `/status`。
7. 重启实例，然后确认 `systemctl status deepseek-runtime` 和 `systemctl status deepseek-feishu-bridge` 恢复为活跃状态。

## 运维说明

- 将 `deepseek serve --http` 绑定到 `127.0.0.1`。
- 保持 Lighthouse 防火墙在此设置中仅关注 SSH。
- 使用 SSH 密钥认证。
- 使用 `tmux` 从 Blink/Termius 进行紧急终端操作。
- 在手机端工作时，将 `/opt/whalebro/deepseek-tui` 保留在个人分支上。
