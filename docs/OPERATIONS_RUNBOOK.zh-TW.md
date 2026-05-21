# DeepSeek TUI 營運手冊

[English](OPERATIONS_RUNBOOK.md) | [简体中文](OPERATIONS_RUNBOOK.zh-CN.md)

本手冊涵蓋本機 CLI/TUI 執行階段的實用除錯和事件回應。

## 快速分類

1. 確認二進位檔 + 設定：
   - `cargo run -- --version`
   - `cat ~/.deepseek/config.toml`（或檢查已設定的設定檔）
2. 啟用詳細記錄：
   - `RUST_LOG=deepseek_cli=debug cargo run`
   - 對於 HTTP 重試/重新連線：`RUST_LOG=deepseek_cli::client=debug cargo run`
3. 擷取當前狀態：
   - `ls ~/.deepseek/sessions`
   - `ls ~/.deepseek/sessions/checkpoints`
   - `ls ~/.deepseek/tasks`

## 事件：回合卡住或串流停止

症狀：
- TUI 保持在載入狀態
- 部分助理輸出且未完成

檢查：
1. 檢查重試/健康狀態記錄（`deepseek_cli::client`）
2. 驗證端點連線：
   - `curl -sS https://api.deepseek.com/beta/models -H "Authorization: Bearer $DEEPSEEK_API_KEY"`
3. 確認工具輸出中沒有本機沙箱/權限死鎖

動作：
1. 若前景 shell 指令正在執行，按下 `Ctrl+B` 並選擇將其設為背景或取消當前回合。
2. 若指令是在背景啟動的，要求助理使用 `exec_shell_cancel` 和返回的任務 ID 取消它。
3. 當您想停止請求本身時，使用 `Esc` 或 `Ctrl+C` 中斷當前回合。
4. 重試提示；若仍然失敗，重新啟動 TUI。
5. 重新啟動時，驗證先前的佇列/進行中的執行階段回合顯示為已中斷，而非保留在執行狀態。

## 事件：網路中斷 / 離線行為

預期行為：
- 離線模式啟用時，新提示會被佇列
- 佇列狀態持久化至 `~/.deepseek/sessions/checkpoints/offline_queue.json`

檢查：
1. 在 TUI 中開啟佇列：`/queue list`
2. 確保持久化的佇列檔案存在且更新時間戳記

動作：
1. 恢復連線
2. 重新發送佇列條目（從 `/queue edit <n>` + Enter，或一般輸入流程）
3. 當佇列為空時確保佇列檔案被清除

## 事件：需要崩潰復原

預期行為：
- 檢查點儲存於 `~/.deepseek/sessions/checkpoints/latest.json`
- 啟動時開始一個新的工作階段，除非提供 `--resume`/`--continue`

動作：
1. 透過 `deepseek --resume <id>` 或 TUI 中的 `Ctrl+R` 明確恢復先前工作
2. 若需要檢查點檢查，檢查 `latest.json` 以確認結構描述不符/詳細資訊
3. 若結構描述比二進位檔支援的更新，升級二進位檔或移除過時的檢查點

## 事件：持久化狀態結構描述錯誤

症狀：
- 如 `schema vX is newer than supported vY` 的錯誤

受影響的儲存：
- 工作階段（`~/.deepseek/sessions/*.json`）
- 執行階段執行緒/回合/項目記錄
- 任務（`~/.deepseek/tasks/tasks/*.json`）

動作：
1. 確認二進位版本和遷移預期
2. 編輯前備份狀態目錄
3. 選擇以下之一：
   - 使用較新的相容二進位檔執行，或
   - 封存不相容的記錄並重新產生狀態

## 事件：MCP/工具執行失敗

檢查：
1. 驗證 `~/.deepseek/mcp.json` 結構描述和伺服器指令路徑
2. 確認伺服器處理程序可手動啟動
3. 檢查 TUI 歷史記錄/記錄中的沙箱拒絕

動作：
1. 以必要的核准重試（或僅在適當時使用 YOLO）
2. 暫時停用失敗的 MCP 伺服器並隔離問題
3. 透過 `/mcp` 診斷驗證後重新啟用

## 事件後檢查清單

1. 保留記錄和相關狀態檔案
2. 記錄觸發條件、影響和緩解措施
3. 新增或更新回歸測試（重試/復原/結構描述）
4. 若行為有所變更，更新本手冊和架構文件
