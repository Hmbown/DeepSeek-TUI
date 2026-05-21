# 執行時期 API 和整合合約

[English](RUNTIME_API.md) | [简体中文](RUNTIME_API.zh-CN.md)

DeepSeek TUI 透過 `deepseek serve --http` 暴露本地執行時期 API，並透過 `deepseek doctor --json` 提供機器可讀的健康狀態。它也透過 `deepseek serve --acp` 暴露給透過 stdio 使用 Agent Client Protocol 的編輯器客戶端。本文件是針對原生 macOS 工作台應用程式（以及其他本地監督器）嵌入 DeepSeek 引擎而無需擷取終端機輸出的穩定整合合約。

## 架構

```
macOS 工作台（或任何本地監督器）
        │
        ├─ deepseek doctor --json   → 機器可讀的健康狀態和能力
        ├─ deepseek serve --http    → HTTP/SSE 執行時期 API
        ├─ deepseek serve --acp     → 供 Zed 等編輯器使用的 ACP stdio 代理
        ├─ deepseek serve --mcp     → MCP stdio 伺服器
        └─ deepseek [args]          → 互動式 TUI 工作階段
```

引擎作為僅限本地的處理程序執行。所有 API 預設繫結到 `localhost`。沒有託管中繼、沒有提供者 token 保管、沒有密鑰洩漏。

## ACP stdio 轉接器：`deepseek serve --acp`

`deepseek serve --acp` 透過換行分隔的 stdio 使用 JSON-RPC 2.0，供相容 ACP 的編輯器客戶端使用。初始轉接器實作了 ACP 基準：

- `initialize`
- `session/new`
- `session/prompt`
- `session/cancel`

提示請求透過已設定的 DeepSeek 客戶端和目前預設模型進行路由。回應以 `session/update` 代理訊息區塊形式發出，隨後是帶有 `stopReason: "end_turn"` 的 `session/prompt` 回應。

此轉接器刻意保持保守：它尚未透過 ACP 暴露 shell 工具、檔案寫入工具、檢查點重播或工作階段載入。使用 `deepseek serve --http` 取得完整的本地執行時期 API，使用 `deepseek serve --mcp` 在其他客戶端需要將 DeepSeek 工具作為 MCP 工具使用時。

## 能力端點：`deepseek doctor --json`

返回描述目前安裝就緒狀態的 JSON 物件。適合從 macOS 工作台進行健康檢查輪詢。

```bash
deepseek doctor --json
```

### 回應綱要（關鍵欄位）

| 欄位 | 類型 | 說明 |
|---|---|---|
| `version` | 字串 | 已安裝的版本（例如 `"0.8.9"`） |
| `config_path` | 字串 | 解析後的設定檔案路徑 |
| `config_present` | 布林 | 設定檔案是否存在 |
| `workspace` | 字串 | 預設工作區目錄 |
| `api_key.source` | 字串 | `env`、`config` 或 `missing` |
| `base_url` | 字串 | API 基礎 URL |
| `default_text_model` | 字串 | 預設模型 |
| `memory.enabled` | 布林 | 記憶體功能是否啟用 |
| `memory.path` | 字串 | 記憶體檔案路徑 |
| `memory.file_present` | 布林 | 記憶體檔案是否存在 |
| `mcp.config_path` | 字串 | MCP 設定檔案路徑 |
| `mcp.present` | 布林 | MCP 設定是否存在 |
| `mcp.servers` | 陣列 | 每個伺服器的健康狀態：`{name, enabled, status, detail}` |
| `skills.selected` | 字串 | 解析後的 skills 目錄 |
| `skills.global.path` / `.present` / `.count` | — | DeepSeek 全域 skills 目錄 (`~/.deepseek/skills`) |
| `skills.agents.path` / `.present` / `.count` | — | 工作區 `.agents/skills/` 目錄 |
| `skills.agents_global.path` / `.present` / `.count` | — | agentskills.io 全域 skills 目錄 (`~/.agents/skills`) |
| `skills.local.path` / `.present` / `.count` | — | `skills/` 目錄 |
| `skills.opencode.path` / `.present` / `.count` | — | `.opencode/skills/` 目錄 |
| `skills.claude.path` / `.present` / `.count` | — | `.claude/skills/` 目錄 |
| `tools.path` / `.present` / `.count` | — | 全域工具目錄 |
| `plugins.path` / `.present` / `.count` | — | 全域外掛程式目錄 |
| `sandbox.available` | 布林 | 此作業系統是否支援沙箱 |
| `sandbox.kind` | 字串或 null | 沙箱類型（例如 `"macos_seatbelt"`） |
| `storage.spillover.path` / `.present` / `.count` | — | 工具輸出溢位目錄 |
| `storage.stash.path` / `.present` / `.count` | — | 撰寫器暫存 |

### 範例

```json
{
  "version": "0.8.9",
  "config_path": "/Users/you/.deepseek/config.toml",
  "config_present": true,
  "workspace": "/Users/you/projects/deepseek-tui",
  "api_key": {
    "source": "env"
  },
  "base_url": "https://api.deepseek.com/beta",
  "default_text_model": "deepseek-v4-pro",
  "memory": {
    "enabled": false,
    "path": "/Users/you/.deepseek/memory.md",
    "file_present": true
  },
  "mcp": {
    "config_path": "/Users/you/.deepseek/mcp.json",
    "present": true,
    "servers": [
      {"name": "filesystem", "enabled": true, "status": "ok", "detail": "ready"}
    ]
  },
  "sandbox": {
    "available": true,
    "kind": "macos_seatbelt"
  }
}
```

## HTTP/SSE 執行時期 API：`deepseek serve --http`

```bash
deepseek serve --http [--host 127.0.0.1] [--port 7878] [--workers 2] [--auth-token TOKEN]
```

預設值：主機 `127.0.0.1`，埠 `7878`，2 個工作者（限制在 1–8）。

伺服器預設繫結到 `localhost`。設定透過 CLI 旗標 — 沒有 `[app_server]` 設定區段。

預設情況下，現有的本地行為保持不變，`/v1/*` 路由不需要認證。要對 `/v1/*` 路由要求 bearer token，請傳遞 `--auth-token TOKEN` 或在啟動伺服器前設定 `DEEPSEEK_RUNTIME_TOKEN=TOKEN`。`/health` 保持公開，用於本地處理程序監督和就緒檢查。

已認證的客戶端可以將 token 提供為 `Authorization: Bearer TOKEN`、`X-DeepSeek-Runtime-Token: TOKEN`，或對於無法設定自訂標頭的 EventSource 風格客戶端，使用 `?token=TOKEN`。

### 端點

**健康檢查**
- `GET /health`

**工作階段**（舊版工作階段管理員）
- `GET /v1/sessions?limit=50&search=<substring>`
- `GET /v1/sessions/{id}`
- `DELETE /v1/sessions/{id}`
- `POST /v1/sessions/{id}/resume-thread`

**對話串**（持久性執行時期資料模型）
- `GET /v1/threads?limit=50&include_archived=false&archived_only=false`
- `GET /v1/threads/summary?limit=50&search=<optional>&include_archived=false&archived_only=false`
- `POST /v1/threads`
- `GET /v1/threads/{id}`
- `PATCH /v1/threads/{id}`（請參閱下方的請求體形狀）
- `POST /v1/threads/{id}/resume`
- `POST /v1/threads/{id}/fork`

`archived_only=true` 僅返回已封存的對話串（會互相覆蓋 `include_archived`）。預設行為不變：`include_archived=false` 且 `archived_only=false` 返回活躍的對話串。新增於 v0.8.10 (#563)。

`PATCH /v1/threads/{id}` 請求體 — 每個欄位皆為可選，缺失表示「不變更」。至少必須存在一個欄位。`title` 和 `system_prompt` 接受空字串來清除先前設定的值。新增於 v0.8.10 (#562)：

```json
{
  "archived": true,
  "allow_shell": false,
  "trust_mode": false,
  "auto_approve": false,
  "model": "deepseek-v4-pro",
  "mode": "agent",
  "title": "使用者設定的對話串標題",
  "system_prompt": "You are a useful assistant."
}
```

**回合**（對話串內）
- `POST /v1/threads/{id}/turns`
- `POST /v1/threads/{id}/turns/{turn_id}/steer`
- `POST /v1/threads/{id}/turns/{turn_id}/interrupt`
- `POST /v1/threads/{id}/compact`（手動壓縮）

**事件**（SSE 重播 + 即時串流）
- `GET /v1/threads/{id}/events?since_seq=<u64>`

**相容性串流**（單次，向後相容）
- `POST /v1/stream`

**任務**（持久性背景工作）
- `GET /v1/tasks`
- `POST /v1/tasks`
- `GET /v1/tasks/{id}`
- `POST /v1/tasks/{id}/cancel`

**自動化**（排程的重複性工作）
- `GET /v1/automations`
- `POST /v1/automations`
- `GET /v1/automations/{id}`
- `PATCH /v1/automations/{id}`
- `DELETE /v1/automations/{id}`
- `POST /v1/automations/{id}/run`
- `POST /v1/automations/{id}/pause`
- `POST /v1/automations/{id}/resume`
- `GET /v1/automations/{id}/runs?limit=20`

**自省**
- `GET /v1/workspace/status`
- `GET /v1/skills`
- `GET /v1/apps/mcp/servers`
- `GET /v1/apps/mcp/tools?server=<optional>`

**用量**（跨對話串的 token/成本彙總）
- `GET /v1/usage?since=<rfc3339>&until=<rfc3339>&group_by=<day|model|provider|thread>`

`since` / `until` 是包含邊界的 RFC 3339 時間戳記，可以省略（無邊界）。`group_by` 預設為 `day`。分組桶按遞增鍵排序。空的時間範圍會產生空的 `buckets`（永遠不會是 404）。成本透過模型→定價對應計算；模型沒有定價項目的回合會貢獻 token 但成本為 `0.0`。新增於 v0.8.10 (#564)。

```json
{
  "since": "2026-04-01T00:00:00Z",
  "until": "2026-04-30T23:59:59Z",
  "group_by": "day",
  "totals": {
    "input_tokens": 12345,
    "output_tokens": 6789,
    "cached_tokens": 0,
    "reasoning_tokens": 0,
    "cost_usd": 0.012,
    "turns": 42
  },
  "buckets": [
    {
      "key": "2026-04-30",
      "input_tokens": 1234,
      "output_tokens": 678,
      "cached_tokens": 0,
      "reasoning_tokens": 0,
      "cost_usd": 0.001,
      "turns": 3
    }
  ]
}
```

## 執行時期資料模型

執行時期使用持久性的 Thread/Turn/Item 生命週期。

- **ThreadRecord** — `id`、`created_at`、`updated_at`、`model`、`workspace`、`mode`、`task_id`、`coherence_state`、`system_prompt`、`latest_turn_id`、`latest_response_bookmark`、`archived`
- **TurnRecord** — `id`、`thread_id`、`status`（`queued|in_progress|completed|failed|interrupted|canceled`）、時間戳記、持續時間、用量、錯誤摘要
- **TurnItemRecord** — `id`、`turn_id`、`kind`（`user_message|agent_message|tool_call|file_change|command_execution|context_compaction|status|error`）、生命週期 `status`、`metadata`

事件為僅附加，具有用於重播/繼續的全域單調 `seq`。

### 重啟語義

- 如果處理程序在回合或項目處於 `queued` 或 `in_progress` 狀態時重啟，恢復的記錄會被標記為 `interrupted` 並附帶 `"Interrupted by process restart"` 錯誤。
- 任務執行會在相同的持久性對話串/回合儲存之上執行自己的恢復。

### 審核模型

- `auto_approve` 旗標適用於執行時期審核橋接器和引擎工具上下文。當為對話串/回合/任務啟用時，需要審核的工具會在非互動式執行時期路徑中自動核准，shell 安全檢查在自動核准模式下執行，產生的子代理也會繼承該設定。
- 省略時，`auto_approve` 預設為 `false`。

### SSE 事件串流

SSE 事件負載形狀：

```json
{
  "seq": 42,
  "timestamp": "2026-02-11T20:18:49.123Z",
  "thread_id": "thr_1234abcd",
  "turn_id": "turn_5678efgh",
  "item_id": "item_90ab12cd",
  "event": "item.delta",
  "payload": {
    "delta": "partial output",
    "kind": "agent_message"
  }
}
```

常見事件名稱：`thread.started`、`thread.forked`、`turn.started`、`turn.lifecycle`、`turn.steered`、`turn.interrupt_requested`、`turn.completed`、`item.started`、`item.delta`、`item.completed`、`item.failed`、`item.interrupted`、`approval.required`、`sandbox.denied`、`coherence.state`。

## 安全性邊界

- **僅限 localhost**。伺服器預設繫結到 `127.0.0.1`。僅當您有進行認證的反向代理 / VPN 時才設定 `--host 0.0.0.0`。執行時期不提供使用者隔離或 TLS。
- **可選的 token 防護**。`--auth-token` 或 `DEEPSEEK_RUNTIME_TOKEN` 需要對 `/v1/*` 路由提供相符的 bearer token。這是一個本地便利防護，而非在公共網路上替代 TLS、VPN 或受信任反向代理的方案。
- **不保管提供者 token**。伺服器絕不返回 API 金鑰。`api_key.source` 能力欄位報告 `env`、`config` 或 `missing` — 絕不報告金鑰本身。
- **無託管中繼**。應用程式伺服器是使用者控制下的本地處理程序。沒有雲端元件。
- **能力回應**絕不洩漏密鑰、檔案內容或工作階段訊息主體。它們報告*中繼資料*：存在性、計數、狀態旗標。

### CORS 允許清單

執行時期 API 內建開發來源允許清單：`http://localhost:3000`、`http://127.0.0.1:3000`、`http://localhost:1420`、`http://127.0.0.1:1420`、`tauri://localhost`。要新增額外的來源（例如在 Vite 的預設 `:5173` 上開發 UI 時），使用以下任一方式：

- CLI 旗標（可重複）：`deepseek serve --http --cors-origin http://localhost:5173`
- 環境變數（逗號分隔）：`DEEPSEEK_CORS_ORIGINS="http://localhost:5173,http://localhost:8080"`
- 設定（`~/.deepseek/config.toml`）：
  ```toml
  [runtime_api]
  cors_origins = ["http://localhost:5173"]
  ```

使用者提供的來源會**疊加在**內建預設值之上；它們不會取代預設值。不支援萬用字元來源 — 保留明確的允許清單模型。新增於 v0.8.10 (#561)。

## 工作階段生命週期（原生 UI 監督）

| 操作 | 端點 |
|---|---|
| 列出工作階段 | `GET /v1/sessions` |
| 取得工作階段 | `GET /v1/sessions/{id}` |
| 刪除工作階段 | `DELETE /v1/sessions/{id}` |
| 繼續至對話串 | `POST /v1/sessions/{id}/resume-thread` |
| 建立對話串 | `POST /v1/threads` |
| 列出對話串 | `GET /v1/threads` |
| 附加事件 | `GET /v1/threads/{id}/events?since_seq=0` |
| 傳送訊息 | `POST /v1/threads/{id}/turns` |
| 操控 | `POST /v1/threads/{id}/turns/{turn_id}/steer` |
| 中斷 | `POST /v1/threads/{id}/turns/{turn_id}/interrupt` |
| 壓縮 | `POST /v1/threads/{id}/compact` |

## 相容性測試

合約快照位於 `crates/protocol/tests/`。執行：

```bash
cargo test -p deepseek-protocol --test parity_protocol --locked
```

這會驗證應用程式伺服器的事件綱要未偏離記錄的合約。CI 在每次推送到 `main` 和發佈標籤時執行此測試。
