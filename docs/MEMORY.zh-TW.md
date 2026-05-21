# 使用者記憶體

[English](MEMORY.md)

使用者記憶體功能提供模型一個小型持久化筆記檔案，
會在每一回合注入到 system prompt 中。這是用來存放
應跨工作階段保留的偏好設定與慣例的地方——
「我偏好 pytest 而非 unittest」、「此程式碼庫使用
4 格縮排」、「提交前一律執行 `cargo fmt`」——
無需在每次對話中重複說明。

記憶體是**選擇性加入**的。當停用時（預設值），不會載入任何內容，
不會攔截任何內容，且 `remember` 工具不會暴露給模型。
這為未要求此功能的使用者保持零額外負擔的行為。

## 啟用記憶體

可設定環境變數：

```bash
export DEEPSEEK_MEMORY=on
```

接受的 truthy 值為 `1`、`on`、`true`、`yes`、`y` 以及
`enabled`。

…或新增至 `~/.deepseek/config.toml`：

```toml
[memory]
enabled = true
```

切換後請重新啟動 TUI。停用方式與上述相反。

記憶體檔案預設位於 `~/.deepseek/memory.md`；可透過
`config.toml` 中的 `memory_path` 或環境變數 `DEEPSEEK_MEMORY_PATH`
覆寫。當兩者皆設定時，`DEEPSEEK_MEMORY_PATH` 優先於設定檔。

## 快速範例

```text
# remember that this repo prefers cargo fmt before commits
/memory
/memory path
/memory edit
/memory help
```

- 在編輯器中輸入 `# remember that this repo prefers cargo fmt before commits`
  會附加一個帶時間戳記的項目符號，而不觸發回合。
- 執行 `/memory` 以確認功能寫入位置及當前儲存的內容。
- 執行 `/memory edit` 以在編輯器中手動整理檔案。

## 注入的內容

當記憶體啟用且檔案存在時，每一回合的 system
prompt 會攜帶一個額外區塊：

```xml
<user_memory source="/Users/you/.deepseek/memory.md">
- (2026-05-03 22:14 UTC) prefer pytest over unittest
- (2026-05-03 22:31 UTC) this codebase uses 4-space indentation
…
</user_memory>
```

該區塊位於 prompt 組合中 volatile-content 邊界之上，
因此會保留在 DeepSeek 的 prefix cache 中跨回合使用。
檔案在每次 prompt 建構呼叫時讀取——透過 `/memory`
或外部編輯器的編輯會在下一個回合生效，無需重新啟動。

大於 100 KiB 的檔案仍會載入但會被截斷，並附加一個標記
以便檢視截斷位置。

## 三種新增至記憶體的方式

### 1. `# ` 編輯器前綴 (#492)

在編輯器中輸入以 `#` 開頭（但非 `##` 或 `#!`）的單行：

```
# remember to use 4-space indentation in this repo
```

TUI 會攔截該輸入並將帶時間戳記的項目符號附加到
你的記憶體檔案。**不會觸發回合**——你的輸入已被使用，
狀態列會確認寫入的路徑，你可以繼續輸入真正的問題。

多個 `#` 前綴會刻意正常遞送至回合提交，
以便貼上 Markdown 標題時不會意外觸發。

### 2. `/memory` 斜線命令 (#491)

檢視、清除或取得編輯檔案的提示：

| 子命令               | 效果                                                   |
|---------------------|--------------------------------------------------------|
| `/memory`           | 顯示解析後的路徑及當前內容                              |
| `/memory show`      | 無參數形式的別名                                        |
| `/memory path`      | 僅輸出解析後的路徑                                      |
| `/memory clear`     | 以空白標記取代檔案內容                                  |
| `/memory edit`      | 輸出 `${VISUAL:-${EDITOR:-vi}} <path>` shell 命令列     |
| `/memory help`      | 顯示命令專用說明及當前路徑                              |

`/memory edit` 形式刻意僅輸出命令而非在處理程序中啟動編輯器——
這樣可以保持斜線命令處理器簡單且一致，無論你使用哪種編輯器。

你也可以從一般說明介面探索此功能：

- `/help memory` 顯示斜線命令摘要及使用方式。
- `/memory help` 輸出記憶體專用的子命令及解析後的路徑。

### 3. `remember` 工具（自動更新，#489）

當記憶體啟用時，模型會獲得一個 `remember` 工具，形狀如下：

```json
{
  "name": "remember",
  "description": "Append a durable note to the user memory file...",
  "input_schema": {
    "type": "object",
    "properties": {
      "note": { "type": "string", ... }
    },
    "required": ["note"]
  }
}
```

當模型注意到值得跨工作階段保留的持久性偏好、慣例或
事實時，會使用此工具。此工具為自動核准，因為寫入範圍
限定於使用者自己的記憶體檔案——若將其置於標準寫入核准
流程之後，將失去自動記憶體捕捉的意義。

若模型將 `remember` 用於暫時性任務狀態（「我目前正在
編輯 foo.rs」），結果無害但會浪費 context。工具的說明明確
告知模型**不要**這樣做——僅限持久性的單句筆記。

## 檔案格式

記憶體為純 Markdown 格式，包含帶時間戳記的項目符號：

```markdown
- (2026-05-03 22:14 UTC) prefer pytest over unittest
- (2026-05-03 22:31 UTC) this codebase uses 4-space indentation
- (2026-05-04 09:02 UTC) all PRs need 2 reviewers before merge
```

你可以在任何編輯器中手動編輯檔案——載入器不關心
時間戳記格式；它僅將整個檔案作為記憶體區塊讀取。
時間戳記是一種慣例，方便你在整理檔案時知道每條筆記
是何時新增的。

## 層級與匯入

記憶體刻意設定為**使用者範圍**而非儲存庫範圍。它
位於——而非內部——專案指令來源旁邊，例如
`AGENTS.md`、`.deepseek/instructions.md` 及 `instructions = [...]`。

- 使用**記憶體**存放應跟隨你跨儲存庫和工作階段的持久性
  個人偏好。
- 使用**專案指令**存放應隨程式碼庫一起傳遞的儲存庫專用
  慣例。

記憶體載入器目前逐字讀取一個解析後的檔案路徑。
**不**支援 `@path` 匯入/包含；如果你需要更大的可重複使用
指令組合，請改為放入專案指令檔案或 skill 中。

## 不應放入記憶體的內容

記憶體是用於**持久性**訊號。不應放入的內容：

- **機密**——無 API 金鑰、token、密碼。檔案是磁碟上的純文字，
  且會逐字注入到 system prompt 中。
- **暫時性任務狀態**——「我目前正在處理解析器」
  每個工作階段都會變更；不屬於跨工作階段記憶體。
- **對話片段**——引用式筆記應屬於筆記工具 (`note`)，
  而非記憶體。
- **長篇指令**——超過幾句話的內容應放在 `AGENTS.md`
  （專案層級）或 [skill](../crates/tui/src/skills/mod.rs)
  （可重複使用的指令包）中。

## 隱私與範圍

記憶體檔案完全位於你機器上的 `~/.deepseek/`。
永不上傳至任何雲端服務——TUI 僅在記憶體啟用時，
將其內嵌於 LLM 提供者收到的 system prompt 中。
若你切換提供者（DeepSeek / NVIDIA NIM / Fireworks / 等），
會使用相同的記憶體檔案；該檔案與提供者無關。

該檔案為每位使用者獨立，而非每個專案獨立。如果你需要
專案特定的記憶體，請改用專案層級的 `AGENTS.md` 或
`.deepseek/instructions.md`——這些由 `project_context` 載入，
且位於儲存庫中（或你提交它們的任何位置）。

## 設定參考

```toml
# ~/.deepseek/config.toml
[memory]
enabled = true                    # 預設 false；或設定 DEEPSEEK_MEMORY=on
# 路徑設定於頂層（位於 skills_dir、notes_path 旁邊）：
memory_path = "~/.deepseek/memory.md"
```

| 設定                   | 預設值                        | 覆寫方式                              |
|-----------------------|-------------------------------|---------------------------------------|
| 記憶體啟用             | `false`                       | `[memory] enabled = true` 或 `DEEPSEEK_MEMORY=on` |
| 記憶體檔案路徑          | `~/.deepseek/memory.md`       | `memory_path = "..."` 或 `DEEPSEEK_MEMORY_PATH=`  |
| 最大檔案大小            | 100 KiB                       | （目前無；截斷標記會顯示截斷位置）               |

## 相關連結

- `docs/SUBAGENTS.md` — 子 agent 會繼承記憶體，也可以使用
  `remember` 工具。
- `docs/CONFIGURATION.md` — 完整設定參考。
- Issue [#489](https://github.com/Hmbown/DeepSeek-TUI/issues/489)
  — phase-1 EPIC 追蹤此工作。
