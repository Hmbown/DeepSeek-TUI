# v0.7.6 舊版 Rust 稽核

[English](LEGACY_RUST_AUDIT_0_7_6.md)

狀態日期：2026-04-29

此稽核刻意為非破壞性。在 v0.7.6 中，除非測試證明公開的 CLI、已儲存工作階段、工具結構描述和已記錄指令路徑不再依賴它們，否則不會移除任何相容性程式碼。

## 摘要

| 介面 | 所屬模組 | 當前消費者 | 參考檢查 | 相容性原因 | 當前警告 | 建議動作 |
|---|---|---|---|---|---|---|
| 舊版 MCP 同步 API（`McpServerInput`、`list`、`add`、`remove`、`call_tool`、`load_legacy`） | `crates/tui/src/mcp.rs` | 未接入當前 `/mcp` 指令路徑；保留在 `#[allow(dead_code)]` 之後 | 已檢查直接 Rust 參考和當前 MCP 指令路徑；已儲存/設定 JSON 相容性仍需要專用的煙霧測試 | 保留舊 JSON 形狀，包含 `mcpServers` 別名和同步呼叫輔助程式，而非同步 MCP 管理器為活躍路徑 | 僅程式碼 TODO | 在明確的舊版模組後方設閘，或在 CLI/執行階段平齊測試證明沒有呼叫者使用後移除。由 #218 追蹤。 |
| 舊版提示常數/函數（`AGENT_PROMPT`、`YOLO_PROMPT`、`PLAN_PROMPT`、`base_system_prompt`、`normal_system_prompt` 等） | `crates/tui/src/prompts.rs` | 測試和仍直接匯入提示常數的舊呼叫者 | 直接 Rust 參考仍然存在；無法證明公開 crate 和舊測試工具的匯入已不存在 | 分層提示 API 取代了單體提示，但舊呼叫點可能仍會針對常數編譯 | 無 | v0.7.6 保留；僅在內部呼叫者遷移後才新增棄用註解。由 #219 追蹤。 |
| `/compact` 斜線指令定位 | `crates/tui/src/commands/mod.rs` | 公開斜線指令註冊表和說明覆蓋層 | 公開指令註冊表/文件路徑仍參考它 | 當前循環/接縫政策偏好重啟/循環流程，但使用者仍可手動執行 `/compact` | 說明標記為舊版並指向循環重啟 | 保留為手動相容性指令；在上下文/token 問題解決前勿移除。 |
| `todo_*` 相容性工具 | `crates/tui/src/tools/todo.rs` | 仍使用 `todo_add`、`todo_update`、`todo_list`、`todo_write` 的工具註冊表/模型呼叫 | 工具註冊表相容性和已儲存工具呼叫風險仍然存在 | `checklist_*` 為標準名稱，但舊工具名稱可能出現在已儲存提示、追蹤或模型先驗值中 | 中繼資料標記 `compat_alias: true`；說明標記為相容性別名 | 新增帶有目標版本的明確棄用中繼資料，然後僅在工具結構描述遷移證據出現後移除。由 #220 追蹤。 |
| 已棄用的子代理別名工具（`spawn_agent`、`send_input`、委派別名） | `crates/tui/src/tools/subagent/mod.rs` | 工具註冊表和模型/工具呼叫相容性 | 工具註冊表相容性和已儲存工具呼叫風險仍然存在 | 標準名稱為 `agent_spawn`、`agent_send_input` 等；別名保留舊工具呼叫相容性 | `_deprecation` 中繼資料和追蹤警告；移除目標為 `v0.8.0` | 在 v0.7.x 期間保留；移除已有中繼資料。由 #221 追蹤。 |
| 舊版根目錄/提供者 TOML `api_key` 相容性 | `crates/tui/src/config.rs`、`crates/config/src/lib.rs` | 設定解析器；設定檔案中有現有 `api_key` 的使用者 | 公開設定載入和文件仍提及遷移行為 | 偏好 Keyring 遷移，但破壞現有設定會阻擋啟動/認證 | 追蹤警告指向 `deepseek auth set` / `deepseek auth migrate` | 保留；警告對使用者可操作。移除應等到遷移指令和發佈說明窗口出現。 |
| 模型別名標準化（`deepseek-chat`、`deepseek-reasoner`、舊版 V3/R1 別名） | `crates/tui/src/config.rs`、`crates/config/src/lib.rs` | 設定/環境變數/模型選擇器正規化 | 公開文件和現有設定可能仍使用別名 | 保留舊有已記錄的 DeepSeek 別名並映射至 `deepseek-v4-flash` | 設計上為靜默別名 | 保留；移除別名會破壞設定且無實質效益。 |
| 已棄用的調色盤常數和別名 | `crates/tui/src/palette.rs`、`crates/tui/tests/palette_audit.rs` | 現有呼叫點加上稽核測試 | 調色盤稽核強制執行剩餘的允許清單 | 偏好語義別名，但保留舊常數以防止廣泛的樣式變動 | 調色盤稽核在允許清單外阻擋直接棄用使用 | 保留別名；持續機會性地將呼叫點移至語義角色。 |

## 後續移除候選項

以下在 v0.7.6 中移除不安全：

1. #218 舊版 MCP 同步 API：需要呼叫圖檢查和針對 `/mcp`、`deepseek mcp` 和 MCP 伺服器驗證流程的明確 CLI/執行階段平齊測試。
2. #219 舊版提示常數/函數：需要證明沒有公開 crate 或舊測試工具匯入它們。
3. #220 `todo_*` 工具別名：需要棄用中繼資料和已儲存追蹤/工具結構描述遷移窗口。
4. #221 已棄用的子代理別名工具：移除目標已編碼為 `v0.8.0`，但實際移除應單獨追蹤和測試。

## 驗證檢查清單

移除任何相容性介面之前：

1. 使用 `rg` 搜尋直接 Rust 參考。
2. 搜尋文件和 README 指令範例。
3. 使用所有功能執行工作區測試。
4. 若介面影響工具結構描述或持久化歷史記錄，執行已儲存工作階段/工具呼叫相容性煙霧測試。
5. 保留發佈說明條目，且對於使用者可見的設定/工具變更，至少保留一個次要版本的遷移提示。
