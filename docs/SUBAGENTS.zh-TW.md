# 子 Agent

[English](SUBAGENTS.md)

子 agent 是 agent 迴圈的持久性背景執行個體。父 agent
以一個聚焦的任務開啟一個子 agent，立即取得 `agent_id` 和
工作階段名稱，然後繼續工作，而子 agent 執行至完成。
子 agent 預設繼承父 agent 的工具登錄，並以
`CancellationToken::child_token()` 執行，因此取消父 agent
會取消所有後代。

此文件涵蓋角色分類法。主動的編排介面為
`agent_open`、`agent_eval` 和 `agent_close`；請參閱 `prompts/base.md`
的「Sub-Agent Strategy」及內嵌的工具說明。

## 角色分類法

`agent_open` 上的 `type` 欄位為子 agent 選擇一個 system-prompt 姿態
（`agent_type` 作為相容性別名被接受）。每個角色都是對工作的
一個獨特立場——不僅僅是不同的標籤。

| 角色           | 立場                                     | 寫入？ | 執行 shell？ | 典型用途                                   |
|---------------|----------------------------------------|--------|-------------|----------------------------------------------|
| `general`     | 靈活；執行父 agent 所說的任何事           | 是     | 是          | 預設值；多步驟任務                            |
| `explore`     | 唯讀；快速對應相關程式碼                  | 否     | 是（讀取）   | 「找出 `Foo` 的每個呼叫點」                    |
| `plan`        | 分析並產出策略                            | 最少   | 最少        | 「設計遷移方案；不要執行」                     |
| `review`      | 讀取並以嚴重性分數評分                     | 否     | 否          | 「稽核此 PR 是否有錯誤」                      |
| `implementer` | 以最少編輯實作特定變更                    | 是     | 是          | 「重寫 `bar.rs::Foo::bar` 以執行 X」           |
| `verifier`    | 執行測試/驗證，回報結果                    | 否     | 是（測試）   | 「執行 cargo test --workspace，回報」          |
| `custom`      | 明確的狹窄工具允許清單                     | 視情況 | 視情況      | 以手選工具進行鎖定的派送                      |

每個角色的完整 system prompt 位於
`crates/tui/src/tools/subagent/mod.rs`（搜尋
`*_AGENT_PROMPT`）。prompt 前綴在子 agent 啟動時自動載入；
父 agent 的指派 prompt 成為第一回合的使用者訊息。

## Context Forking

`agent_open` 預設以全新狀態啟動：子 agent 取得其角色 prompt 加上
你傳遞的任務。當子 agent 應從父 agent 的當前請求前綴繼續時，
請使用 `fork_context: true`。在 fork 模式中，執行階段會盡可能
保持父 agent 的 prefill/prompt 前綴位元組完全相同，附加一個
結構化狀態快照，然後在尾部加入子 agent 角色指令和任務。
這樣可以保留 DeepSeek 的 prefix-cache 重複使用，同時為子 agent
提供延續、審查、摘要或壓縮工作所需的 context。

對獨立探索使用全新工作階段。當任務依賴於父 agent 文字紀錄中
已有的決策、檔案、待辦事項或計畫狀態時，使用 fork 後的工作階段。

### 何時選擇哪個角色

- **`general`**——當任務是「完成整件事」，而非「去查看」、
  「設計」或「驗證」。這是正確的預設值；僅在姿態重要時
  才選擇更具體的角色。
- **`explore`**——當父 agent 在決定下一步之前需要證據時。
  探索者便宜且快速；可平行開啟 2–3 個來處理獨立的區域。
  它們應先定位：確認專案根目錄，在不熟悉的目錄樹中閱讀相關的
  `AGENTS.md`/`README.md` 指引，僅搜尋可能的範圍，
  並回傳 `path:line-range` 證據而非敘事導覽。
  要使用的角色名稱為 `explore` 或 `explorer`。
- **`plan`**——當父 agent 有目標但沒有可執行的分解時。
  規劃者寫入產出物（`update_plan` 行、`checklist_write` 條目）
  但不會執行它們。
- **`review`**——當已有變更且父 agent 希望對其進行評分時。
  審查者不修補——他們在發現中描述修復方式，以便父 agent
  在判決為「修復它」時可以派遣一個 Implementer。
- **`implementer`**——當變更已明確指定且只需要落實時。
  實作者保持嚴格範圍：最少編輯、不進行順手重構、
  在交還前執行快速驗證。
- **`verifier`**——當父 agent 需要對測試套件或其他驗證
  給出權威性的通過/失敗時。驗證者不修復失敗；
  他們擷取失敗的斷言 + 堆疊，並將修復候選方案放在 RISKS 下。
- **`custom`**——僅在父 agent 需要明確限制工具集時。
  透過 `agent_open` 上的 `allowed_tools` 欄位傳遞允許清單。

### 別名

模型可以用多種方式拼寫每個角色：

| 正式名稱       | 別名                                                              |
|---------------|------------------------------------------------------------------|
| `general`     | `worker`、`default`、`general-purpose`                           |
| `explore`     | `explorer`、`exploration`                                        |
| `plan`        | `planning`、`awaiter`                                            |
| `review`      | `reviewer`、`code-review`                                        |
| `implementer` | `implement`、`implementation`、`builder`                         |
| `verifier`    | `verify`、`verification`、`validator`、`tester`                  |
| `custom`      | （無；需要明確的 `allowed_tools` 陣列）                           |

所有比對不區分大小寫。未知值會產生一個類型化錯誤，
列出可接受的集合，讓模型可以在下一個回合自我修正。

## 並行上限

派送器預設將並行子 agent 上限設為 10
（可透過 `~/.deepseek/config.toml` 中的 `[subagents].max_concurrent` 設定，
硬性上限為 20）。當父 agent 達到上限時，`agent_open` 會回傳
包含上限值的錯誤；父 agent 應使用 `agent_eval` 等待完成，
或使用 `agent_close` 釋放一個槽位後再重試。

該上限僅計算**執行中**的 agent——已完成/失敗/
已取消的記錄會保留以供檢視，但不佔用槽位。
失去其 `task_handle` 的 agent（例如跨處理程序重新啟動後）
也不計入上限。

## 生命週期

每個開啟的工作階段會產生一個記錄，經歷以下過程：

```
Pending → Running → (Completed | Failed(reason) | Cancelled | Interrupted(reason))
```

當管理員偵測到一個 `Running` 的 agent 其任務 handle 已遺失時，
會觸發 `Interrupted`——通常是處理程序重新啟動後，
從 `~/.deepseek/subagents.v1.json` 載入了 agent。
父 agent 可以以相同的指派開啟一個替代工作階段，或將其視為
終止狀態。

### 工作階段邊界 (#405)

每個 `SubAgentManager` 執行個體在建構時會為自己指派一個新的
`session_boot_id`。每個新工作階段會用該 ID 標記 agent；
持久化的狀態檔案會跨重新啟動攜帶它。

`agent_eval` 和側邊欄/狀態投影預設聚焦於當前工作階段的
agent。不是仍在執行的先前工作階段 agent 會被視為
已封存記錄，這樣模型就不會將過時的工作誤認為即時工作。

從 pre-#405 持久化狀態檔案載入的記錄（無
`session_boot_id` 欄位）會被分類為先前工作階段，因為
管理員無法將其與當前啟動匹配。

## 輸出合約

每個子 agent 產生一個最終結果字串，包含五個部分，
按順序排列：

```
SUMMARY:    一段文字；你做了什麼以及發生了什麼
CHANGES:    已修改的檔案，附帶一行描述；若為唯讀則為 "None."
EVIDENCE:   path:line-range 引用和關鍵發現；每項一個項目符號
RISKS:      可能出錯的地方/父 agent 應再次確認的內容
BLOCKERS:   阻礙你的事項；若順利完成則為 "None."
```

確切格式位於 `crates/tui/src/prompts/subagent_output_format.md`。
父 agent 將 `EVIDENCE` 作為下一回合的工作集來閱讀，因此
探索者和審查者應在此處精確引用。

## 記憶體與 `remember` 工具 (#489)

子 agent 在記憶體啟用時會繼承父 agent 的記憶體檔案
（`[memory] enabled = true` 或 `DEEPSEEK_MEMORY=on`）。它們可以
透過 `remember` 工具附加持久性筆記——對於發現值得跨工作階段
攜帶的專案慣例的探索者，或學到「這個測試不穩定」的驗證者
來說很方便。

記憶體寫入範圍限定於使用者自己的 `memory.md` 檔案；
它們不經過標準的寫入核准流程。

## 實作備註

- 原始碼：`crates/tui/src/tools/subagent/mod.rs`（約 3500 LOC）。
- 持久化狀態：`~/.deepseek/subagents.v1.json`。Schema 版本
  `1`（向前相容——新的可選欄位使用
  `#[serde(default)]`）。
- `is_running` 檢查會忽略 `task_handle` 為
  `None` 的 agent；這樣可以避免將已持久化但已分離的記錄
  計入並行上限 (#509)。
- `SharedSubAgentManager` 是 `Arc<RwLock<...>>`——讀取路徑使用
  讀取鎖，因此 `/agents` 和側邊欄投影不會在
  多 agent 展開期間阻塞主迴圈 (#510)。
