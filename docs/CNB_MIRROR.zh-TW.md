# CNB Cool 鏡像

[English](CNB_MIRROR.md) | [简体中文](CNB_MIRROR.zh-CN.md)

`cnb.cool/deepseek-tui.com/DeepSeek-TUI` 是此 GitHub 儲存庫的單向鏡像，
供 GitHub 連線緩慢或遭封鎖的網路使用者（主要為中國大陸）使用。
鏡像會接收每次對 `main`、每個 `v*` 發佈標籤，以及 Lighthouse/Feishu
設定所使用的 Tencent 發佈候選分支的推送。

## 運作原理

鏡像由 [`Sync to CNB`](../.github/workflows/sync-cnb.yml)
GitHub Actions 工作流程維護：

- **觸發條件：** 推送至 `main`、推送任何 `v*` 標籤、
  符合 `work/v*-feishu-*` 或 `work/v*-lighthouse*` 的 Tencent 設定分支，
  或手動復原用的 `workflow_dispatch`。
- **認證：** 以使用者 `cnb` 的 HTTPS 基本認證，密碼為 `CNB_GIT_TOKEN`
  儲存庫密鑰。
- **範圍：** 僅推送觸發執行的 ref。標籤推送僅推送該標籤。
  分支推送鏡像 `main` 或明確匹配的 Tencent 設定分支。
  其他功能分支和 dependabot ref 刻意*不予*鏡像。
- **並行控制：** 執行透過 `cnb-sync` 並行群組序列化，
  因此 `auto-tag.yml` 接連發生的 `main` 推送和標籤推送不會互相競爭。
- **重試：** 每次推送在流程放棄前最多重試三次，採用線性退避（5 秒、10 秒）。

CNB 管線設定也在 GitHub 中受原始碼管控，位於
[`/.cnb.yml`](../.cnb.yml)。這是刻意為之：同步工作流程會強制鏡像
GitHub ref 到 CNB，因此僅在 CNB 端建立的管線檔案會被覆寫。
請透過 GitHub PR 提交 `.cnb.yml` 變更，讓單向鏡像將它們帶到 CNB。

## CNB 標籤發佈

當 CNB 收到 `v*` 標籤時，根目錄的 `.cnb.yml` 標籤管線會從原始碼建置 Linux x64
發佈資產，並發佈一個 CNB 發佈版本，包含：

- `deepseek-linux-x64`
- `deepseek-tui-linux-x64`
- `deepseek-artifacts-sha256.txt`

這讓能存取 CNB 但無法存取 GitHub 的使用者擁有一條 CNB 原生的發佈途徑。
GitHub 仍是標準的完整發佈矩陣；CNB 標籤管線是中國友善的 Linux x64 備援方案。

## 發佈後驗證鏡像

在 `release.yml` 針對 `vX.Y.Z` 標籤完成後，CNB 鏡像
應同時擁有 `main` 上的新提交和新標籤：

```bash
# 快速檢查：新標籤是否存在於 CNB？
git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git \
    refs/tags/vX.Y.Z

# 快速檢查：CNB 的 main 是否與 origin/main 指向相同提交？
gh_main=$(git ls-remote https://github.com/Hmbown/DeepSeek-TUI.git refs/heads/main | awk '{print $1}')
cnb_main=$(git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git refs/heads/main | awk '{print $1}')
test "$gh_main" = "$cnb_main" && echo "已同步" || echo "分歧：gh=$gh_main cnb=$cnb_main"
```

或直接檢查工作流程執行記錄：

```bash
gh run list --workflow=sync-cnb.yml --repo Hmbown/DeepSeek-TUI --limit 5
```

若發佈標籤的最新執行為 `success`，代表鏡像已擷取到。若為 `failure`，
請依照下方手動備援方案操作。

## 手動備援方案

若工作流程因任何原因失敗（CNB 速率限制、token 過期、
GitHub 中斷等），維護者可從其本機工作副本手動推送至 CNB。
這是可行的，因為 CNB token 是個人 PAT — 工作流程使用的
同一 token 存放於維護者的密碼管理器中。

### 一次性設定

```bash
# 在 origin 旁邊新增 CNB 遠端。
git remote add cnb https://cnb:${CNB_TOKEN}@cnb.cool/deepseek-tui.com/DeepSeek-TUI.git

# 或者，若不想讓 token 出現在 shell 歷史記錄中：
git remote add cnb https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
# （首次推送時會提示輸入使用者名稱 `cnb` 和密碼 ${CNB_TOKEN}；
#  後續推送會使用憑證輔助程式。）
```

### 手動同步發佈

```bash
# 確保 main 是最新的。
git fetch origin
git checkout main
git reset --hard origin/main

# 先推送 main，再推送標籤。順序很重要：CNB 應先看到
# 提交，再看到指向它的標籤。
git push cnb main --force-with-lease
git push cnb vX.Y.Z
```

### 手動重新觸發工作流程

若工作流程正常但剛好在發佈執行時失敗
（例如 CNB 暫時中斷且現已恢復），無需推送任何內容即可重新觸發：

```bash
gh workflow run sync-cnb.yml --repo Hmbown/DeepSeek-TUI
```

`workflow_dispatch` 會針對工作流程的預設分支
（`main`）執行，因此這會將當前的 `main` 同步至 CNB。若要重新同步
特定標籤，請使用上方手動 `git push cnb` 的方式。

## 輪換 `CNB_GIT_TOKEN`

若工作流程開始出現認證錯誤且 token 已過期：

1. 登入 `cnb.cool` 並產生一個具有 `repo`（推送）權限範圍的新個人存取 token。
2. 更新 `CNB_GIT_TOKEN` 儲存庫密鑰：
   ```bash
   gh secret set CNB_GIT_TOKEN --repo Hmbown/DeepSeek-TUI
   ```
3. 在最近的提交上重新觸發工作流程：
   ```bash
   gh workflow run sync-cnb.yml --repo Hmbown/DeepSeek-TUI
   ```
4. 透過 `gh run list --workflow=sync-cnb.yml` 確認執行成功。

## 二進位發佈資產與 `deepseek update`

CNB 現在會從受原始碼管控的 `.cnb.yml` 管線為 `v*` 標籤建置 Linux x64 資產。
GitHub 仍是標準的完整發佈矩陣。位於 GitHub 封鎖網路後的使用者
應使用以下途徑之一：

- **從 CNB 鏡像使用 `cargo install`：**
  ```bash
  cargo install --git https://cnb.cool/deepseek-tui.com/DeepSeek-TUI --tag vX.Y.Z deepseek-tui-cli
  cargo install --git https://cnb.cool/deepseek-tui.com/DeepSeek-TUI --tag vX.Y.Z deepseek-tui
  ```
  （兩個二進位檔皆為必要 — 調度器和 TUI 分別發布；
  關於兩個二進位檔安裝的原因，請參閱 `AGENTS.md`。）

- **CNB 發佈資產**（Linux x64），當對應的 CNB 標籤管線
  成功完成後。從 CNB 的 `vX.Y.Z` 發佈版本下載 `deepseek-linux-x64`、
  `deepseek-tui-linux-x64` 和 `deepseek-artifacts-sha256.txt`，
  然後根據清單驗證二進位檔。

- **`DEEPSEEK_TUI_RELEASE_BASE_URL`** 環境變數，若存在
  發佈資產的 CDN 鏡像。npm 包裝安裝程式和
  `deepseek update` 會讀取此變數來重新導向二進位檔下載。
  對於 `deepseek update`，也請設定
  `DEEPSEEK_TUI_VERSION=X.Y.Z`，以便更新程式能標記鏡像
  發佈版本而無需聯絡 GitHub。所指向的目錄必須包含
  `deepseek-artifacts-sha256.txt` 和平台二進位檔；格式與
  GitHub Release 資產目錄一致。

## Tencent Cloud 遠端優先途徑

Lighthouse + Feishu/Lark 教學使用 CNB 作為 Tencent 端的原始碼和
自動化通道。若要進行穩定安裝，請從以下來源複製 `main` 或發佈標籤：

```bash
https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
```

鏡像接收 `main`、發佈標籤以及 Lighthouse/Feishu 教學使用的
Tencent 設定分支模式。這些 CNB ref 是 Tencent 端引導的預設
原始碼；當 CNB 工作流程或憑證異常時，GitHub 為備援方案。

CNB 部署按鈕範例位於 `deploy/tencent-lighthouse/cnb/`。它們在
複製到 `.cnb.yml` 和 `.cnb/tag_deploy.yml` 之前不會啟用，因為實際
部署作業需要 Lighthouse 部署金鑰、目標主機以及明確的 CNB
配額/計費政策。
