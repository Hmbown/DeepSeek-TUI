# Tencent Cloud 遠端優先快速入門

[English](TENCENT_CLOUD_REMOTE_FIRST.md)

這是為希望擁有常駐代理工作空間、手機控制介面以及
在中國大陸運作良好的技術堆疊的 DeepSeek TUI 使用者所設計的
Tencent 原生教學路徑。

它補充了本機安裝路徑。若您只想在筆記型電腦上使用 `deepseek`，
請從 README 快速入門開始。若您想要「DS-TUI 作為可從手機控制的
遠端工作臺」，請從這裡開始。

## 預設技術堆疊

```text
GitHub main/tags
  -> CNB 鏡像：cnb.cool/deepseek-tui.com/DeepSeek-TUI
  -> 可選的 CNB 建置/部署管線
  -> Tencent Lighthouse HK
       /opt/whalebro/deepseek-tui
       /opt/whalebro/worktrees
       deepseek-runtime.service 在 127.0.0.1:7878
       deepseek-feishu-bridge.service
  -> Feishu/Lark 手機私訊

EdgeOne 為可選：
  公開 HTTPS 網域 -> EdgeOne -> Lighthouse 上的 Caddy/Nginx
```

## 各部分功能

- **CNB** 是 Tencent 端的原始碼和自動化通道。現有的
  `cnb.cool` 鏡像在 GitHub 連線緩慢時可用於複製和標籤安裝。
  可選的 CNB 部署範本位於
  `deploy/tencent-lighthouse/cnb/`。
- **Lighthouse** 是私有的常駐主機。它擁有 `/opt/whalebro`、
  systemd、Rust/Node 安裝以及 `deepseek serve --http` 執行階段。
- **Feishu/Lark** 是第一個手機 UI。橋接器使用長連線模式，
  因此第一次設定不需要公開的 webhook URL。
- **EdgeOne** 僅在您刻意公開網頁介面（如文件、狀態頁面或
  未來的 webhook 端點）時才作為公開邊緣。請勿將
  執行階段 API 放在 EdgeOne 之後。

## 第一課：讓遠端代理執行起來

1. 購買或重複使用一個香港地區的 Tencent Lighthouse 執行個體。
2. 當分支或標籤存在於 CNB 時，預設從 CNB 複製：

   ```bash
   export DEEPSEEK_REPO_URL=https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git
   git ls-remote "$DEEPSEEK_REPO_URL" refs/heads/main
   ```

   符合 `work/v*-feishu-*` 或 `work/v*-lighthouse*` 的 Tencent 設定分支
   會由 GitHub CNB 同步工作流程鏡像。僅在 CNB 工作流程或
   憑證異常時才使用 GitHub URL。

3. 在伺服器上引導 `/opt/whalebro`：

   ```bash
   export DEEPSEEK_BRANCH=main
   git clone --branch "$DEEPSEEK_BRANCH" "$DEEPSEEK_REPO_URL" /tmp/deepseek-tui
   cd /tmp/deepseek-tui
   sudo DEEPSEEK_REPO_URL="$DEEPSEEK_REPO_URL" \
     DEEPSEEK_REPO_BRANCH="$DEEPSEEK_BRANCH" \
     bash scripts/tencent-lighthouse/bootstrap-ubuntu.sh
   ```

4. 為 `deepseek` 使用者安裝 Rust，建置兩個二進位檔，並使用
   `docs/TENCENT_LIGHTHOUSE_HK.md` 安裝 systemd 單元。
5. 設定 Feishu/Lark 自建應用，填寫
   `/etc/deepseek/feishu-bridge.env`，執行驗證器，然後執行 VPS
   doctor。
6. 從手機私訊驗證 `/status`、一個無害提示、`/interrupt`、
   `/threads`、`/resume`、核准允許/拒絕、服務重啟以及重開機
   持久性。

## 第二課：讓 CNB 成為部署按鈕

當手動 Lighthouse 路徑運作正常後，將非活躍範例從
`deploy/tencent-lighthouse/cnb/` 複製到 CNB 儲存庫：

- `cnb.yml.example` -> `.cnb.yml`
- `tag_deploy.yml.example` -> `.cnb/tag_deploy.yml`

預期的部署按鈕應：

1. 執行橋接器驗證/測試和輕量級發佈版本檢查。
2. 使用儲存為 CNB 密鑰的部署金鑰 SSH 至 Lighthouse。
3. 更新 `/opt/whalebro/deepseek-tui`。
4. 重新建置/安裝兩個二進位檔。
5. 重新安裝/重啟 systemd 服務。
6. 執行 `scripts/tencent-lighthouse/doctor.sh`。

在部署金鑰、目標主機、計費/配額和回溯政策明確之前，
請勿在 `main` 上啟用此功能。

## 第三課：僅為公開 HTTPS 加入 EdgeOne

Feishu/Lark 長連線橋接器無需 EdgeOne 即可運作。當您
想要在刻意公開的 HTTP 服務前放置一個公開網域時再加入 EdgeOne：

- 公開教學/文件網站
- 小型營運者狀態頁面
- 未來的 webhook 模式橋接器
- 在同一 Lighthouse 來源上執行的展示應用程式

遵守以下規則：

- `deepseek serve --http` 保持繫結至 `127.0.0.1`。
- `/v1/*` 執行階段端點絕不公開。
- `DEEPSEEK_RUNTIME_TOKEN` 絕不離開伺服器 env 檔案。
- 在設定明確的群組允許清單之前，Feishu/Lark 群組控制保持關閉。
- 除非維護者明確接受風險，否則手機橋接器的自動核准保持關閉。

## 教學順序

向新的遠端優先使用者解釋 DeepSeek TUI 時，請使用以下順序：

1. **本機心智模型：** `deepseek` 是調度器，`deepseek-tui` 是
   伴隨執行階段，兩個二進位檔都很重要。
2. **代理安全：** Plan/Agent/YOLO 與核准模式和沙箱是分開的。
3. **遠端執行階段：** `deepseek serve --http` 是一個 localhost 執行階段 API，
   不是公開網頁應用程式。
4. **手機橋接器：** Feishu/Lark 訊息透過允許清單橋接器
   轉換為執行階段請求。
5. **CNB 自動化：** 一旦手動設定驗證通過，CNB 將設定轉為
   可重複的部署按鈕。
6. **EdgeOne 邊緣：** 在您確切知道自己要公開什麼
   公開介面後再加入公開邊緣。

## 參考資料

- CNB 鏡像詳細資訊：`docs/CNB_MIRROR.md`
- Lighthouse 實作手冊：`docs/TENCENT_LIGHTHOUSE_HK.md`
- Feishu/Lark 橋接器：`integrations/feishu-bridge/README.md`
- CNB 範本：`deploy/tencent-lighthouse/cnb/`
