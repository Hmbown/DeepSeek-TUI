# Tencent Lighthouse 香港手機設定

[English](TENCENT_LIGHTHOUSE_HK.md) | [简体中文](TENCENT_LIGHTHOUSE_HK.zh-CN.md)

本手冊將在 Tencent Cloud 香港地區設定一個 Lighthouse 執行個體，
作為可從手機上的 Feishu/Lark 控制的常駐 DeepSeek TUI 主機。

若您將此作為 Tencent 原生預設路徑進行教學，請從
[docs/TENCENT_CLOUD_REMOTE_FIRST.md](TENCENT_CLOUD_REMOTE_FIRST.md) 開始。
本檔案是 Lighthouse 主機本身的實作手冊。

## 目標架構

```text
CNB 鏡像或 GitHub 分支
  -> /opt/whalebro/deepseek-tui

Feishu/Lark 行動應用程式
  -> Feishu/Lark 長連線機器人
  -> deepseek-feishu-bridge systemd 服務
  -> http://127.0.0.1:7878 deepseek serve --http
  -> /opt/whalebro
       -> deepseek-tui/

可選公開邊緣：
EdgeOne -> Lighthouse 上的 Caddy/Nginx 公開網站
```

執行階段 API 必須保持在 `127.0.0.1`。橋接器是唯一面向手機的
控制介面。EdgeOne 為可選，且僅應放在刻意公開的
HTTP 服務之前，而非執行階段 API。

## 遠端 Whalebro 工作空間

使用 `/opt/whalebro` 作為 VPS 工作空間根目錄。主要工作副本為
`/opt/whalebro/deepseek-tui`。

先建立以下路徑：

- `/opt/whalebro/deepseek-tui`
- `/opt/whalebro/worktrees`

Linux 足以應付 Rust、Node 和服務工作。僅限 Mac 的發佈工作，
例如 iOS 模擬器執行、`.app`/DMG 檢查、公證和 Apple 簽署，
仍應在 Mac 上進行。

## Lighthouse 執行個體

建議的旅行用方案：

- 地區：香港（中國）
- 映像檔：純 Ubuntu 24.04 LTS 或最新 Ubuntu LTS
- 規格：第一個月購買 HK 2 vCPU / 4 GB / 70 GB 方案
- 登入：SSH 金鑰，非密碼
- 防火牆：SSH 開啟；執行階段 API 僅限 localhost

Tencent 的 Lighthouse 文件說明 Linux 執行個體可使用 SSH 金鑰，
且 Lighthouse 防火牆預設開放 SSH/HTTP/HTTPS。

使用 4 GB RAM 以舒適地編譯 Rust 並執行橋接器。4 vCPU /
8 GB 方案更適合多個並行代理工作者。

## Feishu / Lark 應用程式

在以下平台建立企業自建應用：

- Feishu 中國：`https://open.feishu.cn/app`
- Lark 國際：`https://open.larksuite.com/app`

設定：

1. 啟用機器人功能。
2. 複製 App ID 和 App Secret。
3. 新增訊息收發權限。最小實用集合為：
   - `im:message`
   - `im:message:send_as_bot`
   - 您租戶的直接訊息讀取權限
   - 僅在您日後刻意啟用群組控制時才加入群組 @訊息讀取權限
4. 新增事件訂閱 `im.message.receive_v1`。
5. 使用長連線 / WebSocket 模式。
6. 發布應用程式並將機器人加入您的 Feishu/Lark 聊天。

## 伺服器引導

SSH 進入 Lighthouse 執行個體並執行：

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

若想從 VPS 推送存取，請改用 SSH 儲存庫 URL。若 CNB
鏡像不可用，請退回使用：

```bash
export DEEPSEEK_REPO_URL=https://github.com/Hmbown/DeepSeek-TUI.git
```

對於穩定發佈文件，在使用前確認 CNB 鏡像具有該分支或標籤：

```bash
export DEEPSEEK_REPO_URL=https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
git ls-remote "$DEEPSEEK_REPO_URL" \
  refs/heads/main \
  refs/tags/v0.8.37
```

CNB 鏡像接收 `main` 和發佈標籤。CNB 是此 Lighthouse 路徑的預設
原始碼；僅在 CNB 工作流程或憑證異常時才使用 GitHub 作為備援。

若此部署設定尚未推送至 Git，請先推送分支
或將此工作副本複製到 VPS 後再執行這些指令。全新的
VPS 複製無法看到未提交的本機檔案。

為 `deepseek` 使用者安裝 Rust 1.88+，然後建置兩個發布的二進位檔：

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

複製並安裝橋接器/服務檔案：

```bash
cd /opt/whalebro/deepseek-tui
sudo bash scripts/tencent-lighthouse/install-services.sh
```

編輯兩個 env 檔案後，驗證橋接器/執行階段配對：

```bash
sudo -u deepseek node /opt/deepseek/bridge/scripts/validate-config.mjs \
  --env /etc/deepseek/feishu-bridge.env \
  --runtime-env /etc/deepseek/runtime.env \
  --workspace-root /opt/whalebro \
  --check-filesystem
```

## 密鑰

產生一個執行階段 token 並將相同的值放入兩個 env 檔案：

```bash
openssl rand -hex 32
sudoedit /etc/deepseek/runtime.env
sudoedit /etc/deepseek/feishu-bridge.env
```

必要值：

- `/etc/deepseek/runtime.env`
  - `DEEPSEEK_API_KEY`
  - `DEEPSEEK_RUNTIME_TOKEN`
- `/etc/deepseek/feishu-bridge.env`
  - `FEISHU_APP_ID`
  - `FEISHU_APP_SECRET`
  - `FEISHU_DOMAIN=feishu`（Feishu），`lark`（Lark）
  - `DEEPSEEK_RUNTIME_TOKEN`
  - `FEISHU_ALLOW_GROUPS=false`（首次部署）

首次配對時，選擇以下方式之一：

1. 暫時設定 `DEEPSEEK_ALLOW_UNLISTED=true`，向機器人發送訊息，複製返回的
   `chat_id`，然後設定 `DEEPSEEK_CHAT_ALLOWLIST=<chat_id>` 並將
   未列入清單的存取關閉。
2. 或從 Feishu/Lark 事件記錄取得聊天 ID，並在
   首次啟動前設定允許清單。

## 啟動服務

```bash
sudo systemctl start deepseek-runtime
sudo systemctl status deepseek-runtime --no-pager
curl -s http://127.0.0.1:7878/health

sudo systemctl start deepseek-feishu-bridge
sudo journalctl -u deepseek-feishu-bridge -f
```

兩個服務設定完成後執行 Lighthouse doctor：

```bash
cd /opt/whalebro/deepseek-tui
sudo bash scripts/tencent-lighthouse/doctor.sh
```

開機啟動由 `install-services.sh` 完成；若需要：

```bash
sudo systemctl enable deepseek-runtime deepseek-feishu-bridge
```

## 手機指令

私訊可以是純文字，且是預期的第一個控制路徑：

```text
檢查 git 狀態並摘要需要注意的事項
```

群組聊天預設停用。若您日後設定
`FEISHU_ALLOW_GROUPS=true`，群組提示必須以 `/ds` 開頭。

實用指令：

- `/status`
- `/threads`
- `/new`
- `/resume <thread_id>`
- `/interrupt`
- `/compact`
- `/allow <approval_id>`
- `/deny <approval_id>`
- `/allow <approval_id> remember`

僅在您刻意想要執行階段執行緒轉向
未來工具自動核准時才使用 `remember`。

## CNB 部署按鈕

手動 Lighthouse 設定驗證通過後，CNB 可成為可重複的部署
按鈕：

1. 將 `deploy/tencent-lighthouse/cnb/cnb.yml.example` 複製到 CNB
   儲存庫中的 `.cnb.yml`。
2. 將 `deploy/tencent-lighthouse/cnb/tag_deploy.yml.example` 複製到
   `.cnb/tag_deploy.yml`。
3. 設定 `deploy/tencent-lighthouse/cnb/README.md` 中記載的
   CNB 部署密鑰。
4. 觸發 `lighthouse-hk` 部署環境。

在伺服器穩定之前保持手動。每次推送的自動部署
日後會很方便，但它們可能消耗 CNB 配額並在手機回合
活躍時重啟橋接器。

## EdgeOne

首次 Feishu/Lark 長連線設定不需要 EdgeOne。僅在您
需要將公開 HTTPS 網域放置在 Lighthouse 主機上刻意公開的
服務前時才加入。

適合 EdgeOne 的用途：

- 公開文件或教學網站
- 小型營運者狀態頁面
- 未來的 webhook 模式橋接器端點
- 在同一 Lighthouse 執行個體上託管的展示網頁應用程式

請勿使用 EdgeOne 公開：

- `http://127.0.0.1:7878`
- `/v1/*` 執行階段端點
- 任何接受 `DEEPSEEK_RUNTIME_TOKEN` 的端點

## 端對端驗證

從手機私訊發送給機器人：

1. 發送 `/status` 並確認執行階段版本、localhost 繫結、認證狀態、
   工作空間、git 儲存庫、分支和髒污計數。
2. 發送一個無害提示，例如 `摘要 git 狀態`。
3. 在回合活躍時發送 `/interrupt` 並確認回合停止。
4. 發送 `/threads`，然後對列出的執行緒發送 `/resume <thread_id>`。
5. 觸發工具核准並驗證 `/allow <approval_id>` 和
   `/deny <approval_id>` 兩個路徑。
6. 重啟兩個服務並重新執行 `/status`。
7. 重新啟動執行個體，然後確認 `systemctl status deepseek-runtime` 和
   `systemctl status deepseek-feishu-bridge` 恢復為活躍狀態。

## 營運注意事項

- 將 `deepseek serve --http` 繫結至 `127.0.0.1`。
- 在此設定中保持 Lighthouse 防火牆專注於 SSH。
- 使用 SSH 金鑰認證。
- 使用 `tmux` 進行來自 Blink/Termius 的緊急終端機工作。
- 從手機工作時，保持 `/opt/whalebro/deepseek-tui` 在個人分支上。
