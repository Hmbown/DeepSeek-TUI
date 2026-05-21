# DeepSeek TUI 架構

[English](ARCHITECTURE.md) | [简体中文](ARCHITECTURE.zh-CN.md)

本文件為開發者和貢獻者提供 DeepSeek TUI 架構的概覽。

目前邊界說明（v0.8.6）：
- `crates/tui` 仍然是 TUI、執行階段 API、任務管理器和工具執行迴圈的正式終端使用者執行階段。
- 其他工作區 crate 正逐步拆分，但尚未成為唯一的執行階段權威來源。
- LSP 子系統（`crates/tui/src/lsp/`）已完全接入引擎的工具執行後路徑
  （`core/engine/lsp_hooks.rs`），在每次 edit_file/apply_patch/write_file 後提供內嵌診斷。
- Swarm agent 系統已在 v0.8.5 中移除。目前 v0.8.35 的主動協作介面為持久子代理工作階段（`agent_open` / `agent_eval` / `agent_close`）和持久 RLM 工作階段（`rlm_open` / `rlm_eval` / `rlm_configure` / `rlm_close`）。
  目前程式碼庫中不存在模型可見的 swarm 工具。

## 高層概覽

```
┌─────────────────────────────────────────────────────────────────┐
│                         使用者介面                               │
│  ┌─────────────────┐  ┌─────────────────┐  ┌────────────────┐  │
│  │   TUI (ratatui) │  │  單次模式       │  │  設定/CLI      │  │
│  └────────┬────────┘  └────────┬────────┘  └────────┬───────┘  │
└───────────┼─────────────────────┼────────────────────┼──────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                        核心引擎                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Agent 迴圈 (core/engine.rs)           │   │
│  │  ┌─────────┐  ┌─────────────┐  ┌──────────────────────┐ │   │
│  │  │ 工作階段 │  │ 回合管理    │  │ 工具協作編排         │ │   │
│  │  └─────────┘  └─────────────┘  └──────────────────────┘ │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                     工具與擴充層                                 │
│  ┌──────────┐  ┌──────────┐  ┌─────────┐  ┌────────────────┐   │
│  │  工具    │  │  技能    │  │  鉤子   │  │  MCP 伺服器    │   │
│  │ (shell,  │  │ (外掛)   │  │ (前/    │  │  (外部)        │   │
│  │  file)   │  │          │  │  後)    │  │                │   │
│  └──────────┘  └──────────┘  └─────────┘  └────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                  執行階段 API + 任務管理                         │
│  ┌─────────────────────────────┐  ┌──────────────────────────┐  │
│  │ HTTP/SSE 執行階段 API       │  │ 持久任務管理器           │  │
│  │ (runtime_api.rs)            │  │ (task_manager.rs)        │  │
│  └─────────────────────────────┘  └──────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
            │                     │
            ▼                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                        LLM 層                                    │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              LLM 客戶端抽象 (llm_client.rs)               │  │
│  │  ┌─────────────────┐  ┌─────────────────────────────┐    │  │
│  │  │  DeepSeek 客戶端 │  │  相容客戶端 (DeepSeek)       │    │  │
│  │  │   (client.rs)   │  │       (client.rs)           │    │  │
│  │  └─────────────────┘  └─────────────────────────────┘    │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## 模組組織

### 進入點

- **`main.rs`** - CLI 參數解析（clap）、設定載入、進入點路由

### 核心元件

- **`core/`** - 主要引擎元件
  - `engine.rs` - 引擎狀態、操作處理、訊息處理
  - `engine/turn_loop.rs` - 串流回合迴圈與工具執行協作編排
  - `engine/capacity_flow.rs` - 容量防護檢查點與介入
  - `session.rs` - 工作階段狀態管理
  - `turn.rs` - 基於回合的對話處理
  - `events.rs` - UI 更新的事件系統
  - `ops.rs` - 核心操作

### 設定

- **`config.rs`** - 設定載入、設定檔、環境變數
- **`settings.rs`** - 執行階段設定管理

### 工作區 Crate

- **`crates/tools`** - 共用的工具呼叫基本元素，包含 TUI 執行階段使用的工具結果/錯誤/能力型別。
- **`crates/agent`** - 模型/提供者登錄（ModelRegistry），用於將模型 ID 解析為提供者端點。
- **`crates/app-server`** - HTTP/SSE + JSON-RPC 應用伺服器傳輸，用於無頭 agent 工作流程。
- **`crates/config`** - 設定載入、設定檔、環境變數優先順序、CLI 執行階段覆寫。
- **`crates/core`** - Agent 迴圈、工作階段管理、回合協作編排、容量流程防護。
- **`crates/execpolicy`** - 用於工具執行決策的核准/沙箱策略引擎。
- **`crates/hooks`** - 生命週期鉤子（stdout、jsonl、webhook），用於工具事件的前/後處理。
- **`crates/mcp`** - MCP 客戶端 + stdio 伺服器，用於 Model Context Protocol 工具伺服器。
- **`crates/protocol`** - 請求/回應框架與協定型別。
- **`crates/secrets`** - OS 鑰匙圈整合，用於 API 金鑰儲存。
- **`crates/state`** - SQLite 執行緒/工作階段持久層。
- **`crates/tui-core`** - 事件驅動的 TUI 狀態機鷹架。

### LLM 整合

- **`client.rs`** - 用於 DeepSeek 已記錄的 OpenAI 相容 Chat Completions API 的 HTTP 客戶端
- **`llm_client.rs`** - 具有重試邏輯的抽象 LLM 客戶端特徵
- **`models.rs`** - API 請求/回應的資料結構

#### DeepSeek API 端點

DeepSeek 公開 OpenAI 相容端點。CLI 使用：
- `https://api.deepseek.com/beta/chat/completions` - 預設 v0.8.16 DeepSeek 模型回合
- `https://api.deepseek.com/beta/models` - 預設 v0.8.16 即時模型探索與健康檢查

`https://api.deepseek.com/v1` 為 OpenAI SDK 相容性而被接受，且
仍可明確設定以選擇退出僅限 beta 的功能，例如
strict tool mode、chat prefix completion 和 FIM completion。公開的
DeepSeek 文件未記錄此工作流程的 Responses API 路徑；引擎
透過 Chat Completions 驅動回合。

### 工具系統

- **`tools/`** - 內建工具實作
  - `mod.rs` - 工具登錄與通用型別
  - `shell.rs` - Shell 指令執行
  - `file.rs` - 檔案讀取/寫入操作
  - `todo.rs` - 檢查清單工具及舊版 todo 別名
  - `tasks.rs` - 模型可見的持久任務、閘門、背景 shell 和 PR-attempt 工具
  - `github.rs` - 由 `gh` 支援的唯讀 GitHub 上下文及受保護的評論/關閉工具
  - `automation.rs` - 基於 `AutomationManager` 的模型可見排程工具
  - `plan.rs` - 規劃工具
  - `subagent.rs` - 持久子代理工作階段（取代已移除的 `agent_swarm` 介面）
  - `spec.rs` - 工具規格
  - `rlm.rs` - 持久遞迴語言模型（RLM）工作階段 — 沙箱化的 Python REPL，具備語義輔助呼叫和 `var_handle` 輸出支援

### 擴充系統

- **`mcp.rs`** - 用於外部工具伺服器的 Model Context Protocol 客戶端
- **`skills.rs`** - 外掛/技能載入與執行
- **`hooks.rs`** - 帶有條件的前/後執行鉤子

### 使用者介面

- **`tui/`** - 終端機 UI 元件（基於 ratatui）
  - `app.rs` - 應用程式狀態與訊息處理
  - `ui.rs` - 事件處理、串流狀態與渲染邏輯
  - `approval.rs` - 工具核准對話框
  - `clipboard.rs` - 剪貼簿處理
  - `streaming.rs` - 串流文字收集器

- **`ui.rs`** - 舊版/簡易 UI 工具

### LSP 整合

- **`lsp/`** - 編輯後診斷注入（#136）
  - `mod.rs` - `LspManager` — 延遲的每個語言傳輸池 + 設定
  - `client.rs` - `StdioLspTransport` — 透過 stdio 的 JSON-RPC，具備 `didOpen`/`didChange`/`publishDiagnostics`
  - `diagnostics.rs` - 診斷型別、嚴重性與 HTML 區塊渲染器
  - `registry.rs` - 語言偵測與預設伺服器對應（rust-analyzer、pyright、gopls、clangd、typescript-language-server）
  - 透過 `core/engine/lsp_hooks.rs` 接入引擎 — 在每次成功編輯後呼叫

### 安全性

- **`sandbox/`** - 平台沙箱策略準備與拒絕回報
  - `mod.rs` - 沙箱型別定義
  - `policy.rs` - 沙箱策略設定
  - `seatbelt.rs` - macOS Seatbelt 設定檔產生
  - `landlock.rs` - Linux Landlock 偵測與未來輔助合約
  - `windows.rs` - Windows 輔助合約；在 Job Object 程序隔離輔助程式存在之前不進行公告

### 工具

- **`utils.rs`** - 通用工具
- **`logging.rs`** - 日誌基礎設施
- **`compaction.rs`** - 長對話的上下文壓縮
- **`pricing.rs`** - 成本估算
- **`prompts.rs`** - 系統提示範本
- **`project_doc.rs`** - 專案文件處理
- **`session.rs`** - 工作階段序列化
- **`runtime_api.rs`** - HTTP/SSE 執行階段 API（`deepseek serve --http`）
- **`runtime_threads.rs`** - 持久執行緒/回合/項目儲存 + 可重播事件時間軸
- **`task_manager.rs`** - 持久佇列、工作池、任務時間軸與產出物

## 資料流

### 互動式工作階段

1. 使用者在 TUI 中輸入
2. 輸入由 `core/engine.rs` 處理
3. 訊息透過 `llm_client.rs` 傳送至 LLM
4. 回應串流回傳，在 `client.rs` 中解析
5. 工具呼叫被提取並透過 `tools/` 執行
6. 工具執行前/後觸發鉤子
7. 結果彙總並傳回 LLM
8. 最終回應在 TUI 中渲染

### 崩潰復原 + 離線佇列

1. 在傳送使用者輸入之前，TUI 將檢查點快照寫入 `~/.deepseek/sessions/checkpoints/latest.json`
2. 啟動預設為全新狀態；先前的工作階段透過 `--resume`/`--continue`（或在 TUI 中按 `Ctrl+R`）明確恢復
3. 在降級/離線狀態下，新提示會加入記憶體佇列並鏡像到 `~/.deepseek/sessions/checkpoints/offline_queue.json`
4. 佇列編輯（`/queue ...`）會持續持久化，使草稿和佇列提示在重新啟動後仍能保留
5. 成功完成回合會清除活動檢查點並寫入持久工作階段快照
6. Agent/Yolo 回合也會在 `~/.deepseek/snapshots/<project_hash>/<worktree_hash>/.git` 下進行前/後回合的側 git 工作區快照；`/restore N` 和 `revert_turn` 可恢復檔案狀態，而不會變更對話歷史或使用者的 `.git`

### 工具執行

1. LLM 透過 `tool_use` 內容區塊請求工具
2. 工具登錄查詢處理常式
3. 執行前鉤子執行
4. 如有需要則請求核准（非 yolo 模式）
5. 工具執行（在 macOS 上可能進行沙箱化）
6. 執行後鉤子執行
7. 結果中繼資料保留在執行階段項目記錄上
8. **LSP 編輯後鉤子**（v0.8.6）：如果工具是 `edit_file`/`apply_patch`/`write_file` 且 LSP 已啟用，引擎會執行 `run_post_edit_lsp_hook()` 來收集診斷
9. **診斷刷新**（v0.8.6）：在下一次 API 請求之前，`flush_pending_lsp_diagnostics()` 將任何收集到的錯誤作為合成使用者訊息注入
10. 結果傳回 agent 迴圈

### 背景任務

1. 客戶端將任務加入佇列（`/task add ...` 或 `POST /v1/tasks`）
2. `task_manager.rs` 在 `~/.deepseek/tasks` 下持久化任務 + 佇列項目
3. 工作器選取佇列任務（有界池），轉換為 `running`
4. 任務建立/使用執行階段執行緒並開始執行階段回合
5. `runtime_threads.rs` 持久化執行緒/回合/項目記錄 + 單調事件序列
6. 時間軸/工具摘要/產出物參考會增量持久化
7. 檢查清單狀態、驗證器閘門、PR 嘗試和受保護的 GitHub 事件會從工具中繼資料套用到活動任務
8. 最終狀態（`completed|failed|canceled`）是持久的，可透過 TUI/API 查詢

模型可見的持久任務工具是此管理器的上層介面。它們
不會引入平行工作系統：`task_create` 將正常任務加入佇列，
`checklist_*` 更新任務本機進度，`task_gate_run` 和已完成的
`task_shell_wait` 附加驗證證據，而自動化執行則將
普通的持久任務加入佇列。

### 執行階段執行緒/回合時間軸

1. API/TUI 建立或恢復執行緒（`/v1/threads*`）
2. 在執行緒上開始回合（`/v1/threads/{id}/turns`）
3. 引擎事件對應到項目生命週期事件（`item.started|item.delta|item.completed`）
4. 中斷/引導操作僅套用於活動回合
5. 壓縮（自動/手動）以 `context_compaction` 項目生命週期發出
6. 客戶端使用 `/v1/threads/{id}/events?since_seq=<n>` 重播歷史並恢復

### 持久結構描述閘門

- `session_manager.rs`、`runtime_threads.rs` 和 `task_manager.rs` 在持久記錄上嵌入 `schema_version`。
- 載入時，較新的結構描述版本會以明確錯誤拒絕，而不是靜默截斷/覆寫資料。
- 這允許安全的前向遷移，並在二進位檔與儲存狀態不同步時防止損毀。

## 擴充點

### 新增工具

1. 在 `tools/` 中建立處理常式
2. 在 `tools/registry.rs` 中註冊
3. 新增工具規格（名稱、描述、輸入結構描述）

### 新增 MCP 伺服器

1. 在 `~/.deepseek/mcp.json` 中設定
2. 伺服器在啟動時自動發現
3. 工具自動公開給 LLM

### 建立技能

1. 建立包含 `SKILL.md` 的技能目錄
2. 定義技能提示與選用腳本
3. 放置於 `~/.deepseek/skills/`

### 新增鉤子

在 `~/.deepseek/config.toml` 中設定：

```toml
[[hooks]]
event = "tool_call_before"
command = "echo 'Running tool: $TOOL_NAME'"
```

## 關鍵設計決策

1. **串流優先**：所有 LLM 回應皆以串流方式傳輸以提升回應速度
2. **工具安全性**：非 YOLO 模式需要對破壞性操作進行核准，包括具有副作用的 MCP 工具
3. **可擴充性**：MCP、技能和鉤子允許在不變更程式碼的情況下進行自訂
4. **跨平台**：核心可在 Linux/macOS/Windows 上運作。沙箱保證
   是平台特定的：macOS Seatbelt 是活動策略路徑；Linux 和
   Windows 需要輔助強制執行才能被視為完整的作業系統
   沙箱化。
5. **最小依賴**：謹慎選擇依賴以加快建置速度
6. **本地優先執行階段 API**：HTTP/SSE 端點用於受信任的 localhost 存取，目前由 `crates/tui` 執行階段提供服務

## 設定檔

- `~/.deepseek/config.toml` - 主要設定
- `/etc/deepseek/managed_config.toml` - 選用的受管理預設層（Unix）
- `/etc/deepseek/requirements.toml` - 選用的允許策略約束（Unix）
- `~/.deepseek/mcp.json` - MCP 伺服器設定
- `~/.deepseek/skills/` - 使用者技能目錄
- `~/.deepseek/sessions/` - 工作階段歷史
- `~/.deepseek/sessions/checkpoints/` - 崩潰檢查點 + 離線佇列持久化
- `~/.deepseek/snapshots/` - 用於 `/restore` 和 `revert_turn` 的前/後回合側 git 工作區快照
- `~/.deepseek/tasks/` - 背景任務記錄、佇列、時間軸、產出物
- `~/.deepseek/audit.log` - 僅附加的稽核事件，用於憑證 + 核准/提升操作
