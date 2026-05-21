# MCP（外部工具伺服器）

[English](MCP.md) | [简体中文](MCP.zh-CN.md)

DeepSeek TUI 可以透過 MCP（Model Context Protocol）載入額外的工具。MCP 伺服器是 TUI 啟動並透過 stdio 與之通訊的本地處理程序。

瀏覽注意事項：
- `web.run` 是標準的內建瀏覽工具。
- `web_search` 作為舊版提示和整合的相容性別名仍然可用。

伺服器模式注意事項：
- `deepseek-tui serve --mcp` 執行 MCP stdio 伺服器。
- `deepseek-tui serve --http` 執行執行時期 HTTP/SSE API（獨立模式）。
- `deepseek` 發派器將 `deepseek mcp-server` 作為等同的 stdio 進入點，供分離式 CLI 使用。

## 引導 MCP 設定

在解析後的 MCP 路徑建立入門 MCP 設定：

```bash
deepseek-tui mcp init
```

`deepseek-tui setup --mcp` 在 skills 設定的同時執行相同的 MCP 引導。

常用管理命令：

```bash
deepseek-tui mcp list
deepseek-tui mcp tools [server]
deepseek-tui mcp add <name> --command "<cmd>" --arg "<arg>"
deepseek-tui mcp add <name> --url "http://localhost:3000/mcp"
deepseek-tui mcp enable <name>
deepseek-tui mcp disable <name>
deepseek-tui mcp remove <name>
deepseek-tui mcp validate
```

## TUI 內管理員

在互動式 TUI 中，`/mcp` 會為解析後的 MCP 設定路徑開啟一個精簡管理員。它顯示每個已設定的伺服器、啟用或停用狀態、傳輸方式、命令或 URL、逾時值、連線錯誤以及在執行探索時發現的工具/資源/提示。

支援的 TUI 內操作：

```text
/mcp init
/mcp init --force
/mcp add stdio <name> <command> [args...]
/mcp add http <name> <url>
/mcp enable <name>
/mcp disable <name>
/mcp remove <name>
/mcp validate
/mcp reload
```

`/mcp validate` 和 `/mcp reload` 重新連線以進行 UI 探索並更新管理員快照。從 TUI 進行的設定編輯會立即寫入，但模型可見的 MCP 工具池不會熱重載；管理員會將其標記為需要重新啟動，直到 TUI 被重啟。

## 設定檔案位置

預設路徑：

- `~/.deepseek/mcp.json`

覆蓋：

- 設定：`mcp_config_path = "/path/to/mcp.json"`
- 環境變數：`DEEPSEEK_MCP_CONFIG=/path/to/mcp.json`

`deepseek-tui mcp init`（以及 `deepseek-tui setup --mcp`）寫入此解析後的路徑。

互動式 `/config` 編輯器也暴露 `mcp_config_path`。在 TUI 中變更它會更新 `/mcp` 使用的路徑，並需要重新啟動才能重建模型可見的 MCP 工具池。

編輯檔案或變更 `mcp_config_path` 後，請重新啟動 TUI。

## 工具命名

發現的 MCP 工具向模型暴露為：

- `mcp_<server>_<tool>`

範例：名為 `git` 的伺服器且工具名為 `status` 會變成 `mcp_git_status`。

命令面板包含按伺服器分組的 MCP 項目。它顯示已停用和失敗的伺服器而非隱藏它們，並使用與模型顯示相同的執行時期工具名稱。

## 資源和提示輔助工具

CLI 在 MCP 啟用時也暴露輔助工具：

- `list_mcp_resources`（可選 `server` 過濾器）
- `list_mcp_resource_templates`（可選 `server` 過濾器）
- `mcp_read_resource` / `read_mcp_resource`（別名）
- `mcp_get_prompt`

## 最簡範例

```json
{
  "timeouts": {
    "connect_timeout": 10,
    "execute_timeout": 60,
    "read_timeout": 120
  },
  "servers": {
    "example": {
      "command": "node",
      "args": ["./path/to/your-mcp-server.js"],
      "env": {},
      "disabled": false
    }
  }
}
```

您也可以使用 `mcpServers` 而非 `servers`，以與其他客戶端相容。

## 將 DeepSeek 作為 MCP 伺服器執行

您可以將本地的 DeepSeek 二進位檔註冊為 MCP 伺服器，以便其他 DeepSeek 工作階段（或任何 MCP 客戶端）可以呼叫其工具。

### 快速設定

```bash
deepseek-tui mcp add-self
```

這會解析目前的二進位檔路徑，產生一個執行 `deepseek-tui serve --mcp` 的設定項目，並將其寫入您的 MCP 設定檔案。預設伺服器名稱為 `deepseek`。

選項：

- `--name <NAME>` — 自訂伺服器名稱（預設：`deepseek`）
- `--workspace <PATH>` — 伺服器的工作區目錄

### 手動設定

在 `~/.deepseek/mcp.json` 中的等效手動項目：

```json
{
  "servers": {
    "deepseek": {
      "command": "/path/to/deepseek",
      "args": ["serve", "--mcp"],
      "env": {}
    }
  }
}
```

`deepseek-tui` 二進位檔直接支援 `serve --mcp`。`deepseek` 發派器提供等效的 `deepseek mcp-server` stdio 進入點。使用 PATH 上可用的那個（執行 `which deepseek` 或 `which deepseek-tui` 來尋找完整路徑）。`mcp add-self` 命令會自動解析正確的二進位檔。

### 先決條件

- `command` 中引用的二進位檔必須存在且可執行。
- MCP 伺服器作為子處理程序透過 stdio 執行 — 不需要網路埠。
- 每個 MCP 客戶端工作階段會產生自己的伺服器處理程序。

### 工具命名

來自自託管 DeepSeek 伺服器的工具遵循標準命名慣例：

- `mcp_deepseek_<tool>`（如果伺服器命名為 `deepseek`）

例如，`shell` 工具會變成 `mcp_deepseek_shell`。

### MCP 伺服器 vs HTTP/SSE API vs ACP

| | `deepseek-tui serve --mcp` | `deepseek-tui serve --http` | `deepseek-tui serve --acp` |
|---|---|---|---|
| **協定** | MCP stdio | HTTP/SSE JSON-RPC | ACP stdio |
| **使用案例** | MCP 客戶端的工具伺服器 | 應用程式的執行時期 API | Zed/自訂 ACP 客戶端的編輯器代理 |
| **設定** | `~/.deepseek/mcp.json` 項目 | 直接 URL 連線 | 編輯器 `agent_servers` 自訂命令 |
| **生命週期** | 每個客戶端工作階段產生 | 長期執行守護程序 | 每個編輯器代理工作階段產生 |

當您希望 DeepSeek 工具可供其他 MCP 客戶端使用時，使用 `mcp add-self`。
當建構直接使用 API 的應用程式時，使用 `serve --http`。
當編輯器希望以 ACP 代理身份與 DeepSeek 對話時，使用 `serve --acp`。

### 驗證

新增後，測試連線：

```bash
deepseek-tui mcp validate
deepseek-tui mcp tools deepseek
```

## 伺服器欄位

每個伺服器的設定：

- `command`（字串，必要）
- `args`（字串陣列，可選）
- `env`（物件，可選）
- `connect_timeout`、`execute_timeout`、`read_timeout`（秒，可選）
- `disabled`（布林，可選）
- `enabled`（布林，可選，預設 `true`）
- `required`（布林，可選）：如果此伺服器無法初始化，啟動/連線驗證會失敗。
- `enabled_tools`（陣列，可選）：此伺服器的工具允許清單。
- `disabled_tools`（陣列，可選）：在 `enabled_tools` 之後套用的拒絕清單。

## 安全性注意事項

MCP 工具現在透過與內建工具相同的工具審核框架進行。唯讀 MCP 輔助工具（資源/提示列出和讀取）可以在建議性審核模式下無需提示執行，而有副作用的 MCP 工具則需要審核。

您仍然應該只設定您信任的 MCP 伺服器，並將 MCP 伺服器設定視為等同於在您的機器上執行程式碼。

## 疑難排解

- 執行 `deepseek-tui doctor` 確認解析的 MCP 設定路徑及其是否存在。
- 在 TUI 中，執行 `/mcp validate` 來更新可見的伺服器/工具快照。
- 如果 MCP 設定遺失，執行 `deepseek-tui mcp init --force` 重新產生。
- 如果工具沒有出現，請確認伺服器命令在您的 shell 中可以執行，並且伺服器支援 MCP `tools/list`。
