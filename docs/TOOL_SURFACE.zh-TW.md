# 工具介面

[English](TOOL_SURFACE.md) | [简体中文](TOOL_SURFACE.zh-CN.md)

為何是這些特定工具、以此分組方式，以及每個工具預計如何
在可用的 shell 等價物中被選擇。配合 `crates/tui/src/prompts/agent.txt`。

## 設計立場

- **當專用工具回傳結構化輸出時，優先使用專用工具而非 `exec_shell`。**
  Bash 跳脫容易出錯且平台行為各異（GNU vs BSD `grep`、`rg` 不一定已安裝）。
  結構化輸出也使模型免於重新解析自由格式文字。
- **`exec_shell` 用於其他所有情況。** 建置、測試、格式化、lint、臨時
  命令、任何平台特定的操作。我們不嘗試包裝長尾部分。
- **捨棄不比其 shell 等價物更好的工具。** 針對相同底層操作
  提供兩個工具別名是模型陷阱——LLM 會在兩者之間交替使用，
  快取命中率會受到影響。

## 當前介面 (v0.8.35)

### 檔案操作

| 工具 | 利基 |
|---|---|
| `read_file` | 讀取 UTF-8 檔案。PDF 可透過 `pdftotext` (poppler) 自動擷取（當可用時）；`pages: "1-5"` 可切片大型文件。 |
| `list_dir` | 結構化、感知 gitignore 的目錄列表。優先於 `exec_shell("ls")`。 |
| `write_file` | 建立或覆寫一個檔案。 |
| `edit_file` | 在單一檔案內搜尋並取代。比完整重寫更便宜。 |
| `apply_patch` | 套用 unified diff。適合多區塊編輯的正確工具。 |
| `retrieve_tool_result` | 讀取先前大型工具輸出溢寫至 `~/.deepseek/tool_outputs/` 的摘要或切片；使用 `summary`、`head`、`tail`、`lines` 或 `query`，而非重播整個結果。 |
| `handle_read` | 從即時工具環境持有的 `var_handle` 酬載中讀取有界投影。這是 RLM 工作階段、子 agent 文字紀錄及其他大型符號酬載的基礎。 |

### 搜尋

| 工具 | 利基 |
|---|---|
| `grep_files` | 在工作區內以 Regex 搜尋檔案內容；結構化匹配 + context 行。純 Rust（`regex` crate），無 `rg`/`grep` shell-out。 |
| `file_search` | 模糊匹配檔名（非內容）。當你大致知道名稱時使用。 |
| `web_search` | 預設為 Bing；DuckDuckGo、Tavily 和 Bocha 可在設定中選擇。排名摘要 + `ref_id` 供引用。 |
| `fetch_url` | 對已知 URL 進行直接 HTTP GET。當已知連結時比 `web_search` 更快。HTML 預設剝除為文字。 |

### Shell

| 工具 | 利基 |
|---|---|
| `exec_shell` | 執行 shell 命令。前景執行可取消，但僅用於有界命令；逾時會終止處理程序並回傳背景重新執行提示。 |
| `exec_shell_wait` | 輪詢背景任務以取得增量輸出。取消回合會停止等待而不終止任務。 |
| `exec_shell_interact` | 將 stdin 傳送至執行中的背景任務並讀取增量輸出。 |
| `exec_shell_cancel` | 依 ID 取消一個執行中的背景 shell 任務，或當明確請求時取消所有執行中的背景 shell 任務。 |
| `task_shell_start` | 在背景啟動長時間執行的命令並立即回傳。對於可能需要執行數分鐘的診斷、測試、搜尋及伺服器，優於前景 shell。 |
| `task_shell_wait` | 輪詢背景命令。若完成後提供 `gate`，則在活躍的持久性任務上記錄結構化 gate 證據。 |

當前景 shell 命令逾時，處理程序不會被默默地繼續執行。
工具結果會告知模型使用
`task_shell_start` 或附帶 `background = true` 的 `exec_shell` 重新執行長時間工作，
然後使用 `task_shell_wait` 或 `exec_shell_wait` 進行輪詢。

互動式 shell 作業也可透過 `/jobs` 檢視。TUI 作業中心由
與 `exec_shell`/`task_shell_start` 相同的 shell 管理員提供支援，並顯示
命令、cwd、經過時間、狀態、輸出尾部、處理程序本地 shell ID，以及
當可用時連結的持久性任務 ID。`/jobs show`、`/jobs poll`、`/jobs wait`、
`/jobs stdin` 及 `/jobs cancel` 為即時作業提供檢視、輪詢、stdin 和取消
控制。作業是處理程序本地的；重新啟動後，即時處理程序
狀態不會重新附加，且任何已記憶的分離條目必須標記為
過時，而非顯示為即時處理程序。

### MCP 管理員及調色盤探索

MCP 伺服器設定透過 `/mcp` 和 `/config` 中的 `mcp_config_path` 行
在 TUI 中呈現。`/mcp` 顯示解析後的設定路徑、
伺服器啟用/停用狀態、傳輸方式、命令或 URL、逾時、連線
錯誤及已發現的工具/資源/prompt。它支援用於 init、add、enable、disable、remove、validate 及 reload/reconnect 的狹窄管理員
動作。設定編輯會立即寫入，但模型可見的 MCP 工具池在編輯後
需要重新啟動。

命令調色盤包含按伺服器分組的 MCP 條目。已停用和失敗的
伺服器保持可見，且已發現的工具/prompt 使用顯示給模型的執行階段名稱，
例如 `mcp_<server>_<tool>`。

### Git / 診斷 / 測試

| 工具 | 利基 |
|---|---|
| `git_status` | 檢視儲存庫狀態而不執行 shell。 |
| `git_diff` | 檢視工作樹或暫存的 diff。 |
| `diagnostics` | 在單一呼叫中取得工作區、git、沙箱及工具鏈資訊。 |
| `run_tests` | 附帶可選參數的 `cargo test`。 |

### 任務管理與持久性工作

| 工具 | 利基 |
|---|---|
| `update_plan` | 用於複雜多步驟工作的結構化檢查清單。 |
| `task_create` | 透過 `TaskManager` 建立/排入持久性背景任務。這是長時間執行 agent 工作的真實可執行工作物件。 |
| `task_list` | 列出持久性任務及其狀態和連結的執行階段 ID。 |
| `task_read` | 讀取持久性任務詳細資訊：執行緒/回合連結、時間軸、檢查清單、gates、產出物、PR 嘗試、GitHub 事件。 |
| `task_cancel` | 取消已排入佇列或執行中的持久性任務。需要核准。 |
| `checklist_write` | 在活躍的執行緒/任務下進行細粒度進度追蹤。檢查清單狀態隸屬於持久性任務。 |
| `checklist_add` / `checklist_update` / `checklist_list` | 單一條目檢查清單操作。 |
| `todo_write` / `todo_add` / `todo_update` / `todo_list` | 檢查清單工具的相容性别名。現有工作階段繼續運作，但新的 prompt 應使用 `checklist_*`。 |
| `note` | 供日後使用的一次性重要事實。 |

### 驗證 Gates 與產出物

| 工具 | 利基 |
|---|---|
| `task_gate_run` | 執行已核准的驗證命令，並將結構化證據附加到活躍的持久性任務：命令、cwd、退出碼、持續時間、分類、摘要及記錄產出物。 |

大型記錄和命令輸出應為產出物，並在文字紀錄中包含精簡摘要。`task_gate_run` 會自動為活躍的持久性任務處理此操作。

### GitHub Context 與受保護寫入

| 工具 | 利基 |
|---|---|
| `github_issue_context` | 透過 `gh issue view` 取得唯讀 issue context；大型內文在可能時成為任務產出物。 |
| `github_pr_context` | 透過 `gh pr view` 取得唯讀 PR context；可選的 diff 擷取透過 `gh pr diff --patch`；大型內文/diff 在可能時成為任務產出物。 |
| `github_comment` | 需要核准的 issue/PR 評論，附帶結構化證據。 |
| `github_close_issue` | 需要核准的 issue 關閉。需要非空白的驗收條件與證據；除非明確允許，否則拒絕髒工作樹。切勿僅因 agent 正在停止就關閉 issue。 |

### PR 嘗試

| 工具 | 利基 |
|---|---|
| `pr_attempt_record` | 將當前 git diff 擷取為嘗試中繼資料，加上持久性任務上的修補產出物。 |
| `pr_attempt_list` | 列出在任務上記錄的嘗試。 |
| `pr_attempt_read` | 檢視一個已記錄的嘗試及其產出物參考。 |
| `pr_attempt_preflight` | 對嘗試修補執行 `git apply --check`。不變更工作樹。 |

### 自動化

| 工具 | 利基 |
|---|---|
| `automation_create` | 建立排程的自動化。需要核准。 |
| `automation_list` / `automation_read` | 檢視持久性自動化及最近的執行。 |
| `automation_update` | 更新 prompt、排程、cwd 或狀態。需要核准。 |
| `automation_pause` / `automation_resume` / `automation_delete` | 生命週期控制。需要核准。 |
| `automation_run` | 立即執行自動化；該執行會排入一個正常的持久性任務。需要核准。 |

### 子 Agent

v0.8.33 開始將大型工具輸出移向符號 handle：工具回傳
小型的 `var_handle` 物件，而 `handle_read` 從支援環境中擷取有界切片、
計數或 JSON 投影。這樣可以保持父 agent 文字紀錄小巧，
同時保留回覆完整酬載的復原路徑。

活躍的面向模型的子 agent 介面是持久性的且刻意保持小巧：

| 工具 | 利基 |
|---|---|
| `agent_open` | 開啟一個命名的子 agent 工作階段以進行獨立工作。立即回傳工作階段投影，讓父 agent 可以繼續協調。 |
| `agent_eval` | 傳送後續輸入、等待完成，或為現有工作階段取得當前投影/文字紀錄 handle。 |
| `agent_close` | 依名稱或 ID 取消或釋放子 agent 工作階段。 |

請參閱 `agent.txt` 了解委派協定，以及
[`SUBAGENTS.md`](SUBAGENTS.md) 了解角色分類法
（`general` / `explore` / `plan` / `review` / `implementer` /
`verifier` / `custom`）。

`agent_open` 預設為全新的子對話。傳遞
`fork_context: true` 用於延續式工作或應繼承父 agent context 的
多視角審查。在 fork 模式中，執行階段會盡可能保持
父 agent 的 prefill/prompt 前綴位元組完全相同以重複使用 DeepSeek 的
prefix cache，然後附加子角色指令和任務。

### Recursive LM 工作階段

RLM 現在也是持久性的：

| 工具 | 利基 |
|---|---|
| `rlm_open` | 對一個檔案、內嵌內容或 URL 開啟一個命名的 Python REPL。 |
| `rlm_eval` | 對該工作階段執行有界 Python，使用確定性程式碼及 REPL 內的語義輔助工具，如 `sub_query_batch`。 |
| `rlm_configure` | 調整輸出回饋、子查詢逾時/深度及工作階段共享設定。 |
| `rlm_close` | 關閉 Python 執行階段並回傳最終工作階段統計資料。 |

大型 RLM 輸出應以 `var_handle` 形式回傳。使用 `handle_read` 取得
有界文字切片、行範圍、計數或 JSONPath 投影，
而非將完整值重播至父 agent 文字紀錄中。

在 `rlm_eval` 內部，已載入的來源可作為 `_context` 使用；`content` 也
作為便利別名被繫結，因為 agent 在 Python 分析期間自然會使用它。
較短的名稱 `context` 和 `ctx` 刻意未被繫結，因此使用者變數可以使用它們
而不會與啟動引導衝突。

子呼叫逾時是工作階段政策：在執行大型展開之前，使用 `rlm_configure`
搭配 `sub_query_timeout_secs`。輔助工具
`sub_query`、`sub_query_batch`、`sub_query_map` 及 `sub_rlm` 接受
`timeout_secs` 關鍵字