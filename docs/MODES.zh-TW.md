# 模式與審核

[English](MODES.md) | [简体中文](MODES.zh-CN.md)

DeepSeek TUI 有兩個相關的概念：

- **TUI 模式**：您所處的可見互動類型（Plan/Agent/YOLO）。
- **審核模式**：UI 在執行工具前請求確認的積極程度。

## TUI 模式

按下 `Tab` 可完成撰寫器選單、在回合執行期間將草稿排入佇列作為下一回合的後續動作，或在撰寫器閒置時循環切換可見模式：**Plan → Agent → YOLO → Plan**。
按下 `Shift+Tab` 可循環切換推理力度。
執行 `/mode` 開啟模式選擇器，或使用 `/mode agent`、`/mode plan`、`/mode yolo`、`/mode 1`、`/mode 2` 或 `/mode 3` 直接切換。

- **Plan**：設計優先的提示。唯讀調查工具保持可用；shell 和修補程式執行保持關閉。當您想大聲思考並產出計劃交給人類（未來的自己或審查者）時使用此模式。
- **Agent**：多步驟工具使用。shell 和付費工具需要審核（檔案寫入無需提示即可進行）。
- **YOLO**：啟用 shell + 信任模式並自動核准所有工具。僅在受信任的倉庫中使用。

這三種模式都可以透過 `rlm_open`、`rlm_eval`、`rlm_configure` 和 `rlm_close` 存取持久的 RLM 工作階段。在 RLM Python REPL 中，`sub_query_batch` 可以扇出 1-16 個低成本的平行子呼叫，固定使用 `deepseek-v4-flash`。當工作對父轉錄內容來說過於龐大或重複時，模型會自動使用它。

## 相容性注意事項

- 包含 `default_mode = "normal"` 的舊版設定檔案仍以 `agent` 載入；儲存時會將值重寫為標準化形式。

## Escape 鍵行為

`Esc` 是取消堆疊，而非模式切換。

- 先關閉斜線選單或暫態 UI。
- 如果回合正在執行，則取消目前的請求。
- 如果撰寫器為空，則丟棄已排入佇列的草稿。
- 如果有文字存在，則清除目前的輸入。
- 否則為無操作。

## 審核模式

您可以在執行時期覆蓋審核行為：

```text
/config
# 將 approval_mode 列編輯為：suggest | auto | never
```

舊版注意事項：`/set approval_mode ...` 已被 `/config` 取代。

- `suggest`（預設）：使用上述的每模式規則。
- `auto`：自動核准所有工具（類似 YOLO 的審核行為，但不強制 YOLO 模式）。
- `never`：封鎖任何不被視為安全/唯讀的工具。

## 小螢幕狀態行為

當終端機高度受限時，狀態區域會優先壓縮，使標頭/聊天/撰寫器/頁尾保持可見：

- 載入中和已排入佇列的狀態列按照可用高度進行預算分配。
- 當完整預覽放不下時，已排入佇列的預覽會折疊為精簡摘要。
- `/queue` 工作流程仍然可用；精簡狀態僅影響渲染密度。

## 工作區邊界與信任模式

預設情況下，檔案工具受限於 `--workspace` 目錄。啟用信任模式以允許在工作區外存取檔案：

```text
/trust
```

YOLO 模式會自動啟用信任模式。

## MCP 行為

MCP 工具以 `mcp_<server>_<tool>` 的形式暴露，並使用與內建工具相同的審核流程。唯讀 MCP 輔助工具可以在建議性審核模式下自動執行；可能有副作用的 MCP 工具需要審核。

請參閱 `MCP.md`。

## 相關 CLI 旗標

執行 `deepseek --help` 取得權威清單。常用旗標：

- `-p, --prompt <TEXT>`：單次提示模式（輸出後退出）
- `deepseek exec --output-format stream-json <PROMPT>`：為測試框架和後端封裝程式每行發出一個 JSON 物件
- `deepseek exec --resume <ID|PREFIX> <PROMPT>` / `--session-id <ID|PREFIX>`：以非互動方式繼續已儲存的工作階段
- `deepseek exec --continue <PROMPT>`：以非互動方式繼續此工作區最近儲存的工作階段
- `--model <MODEL>`：使用 `deepseek` 外觀時，將 DeepSeek 模型覆蓋轉發給 TUI
- `--workspace <DIR>`：檔案工具的工作區根目錄
- `--yolo`：以 YOLO 模式啟動
- `-r, --resume <ID|PREFIX|latest>`：繼續已儲存的工作階段
- `-c, --continue`：繼續此工作區中最近的工作階段
- `--max-subagents <N>`：限制在 `1..=20`
- `--mouse-capture` / `--no-mouse-capture`：選擇加入或退出內部滑鼠捲動、轉錄內容選取、右鍵內容動作和轉錄內容捲軸拖曳。在非 Windows 終端機以及 Windows Terminal/ConEmu/Cmder 上，滑鼠捕捉預設為啟用，使拖曳選取僅複製轉錄文字並保持在轉錄面板範圍內；拖曳時按住 Shift 或使用 `--no-mouse-capture` 可進行原始終端機選取。在舊版 Windows 主控台（無 `WT_SESSION` / `ConEmuPID` 的 CMD）和 JetBrains JediTerm — PyCharm/IDEA/CLion 等 — 中預設為關閉，因為這些終端機宣稱支援滑鼠但將 SGR 滑鼠事件作為原始文字轉發 (#878, #898)。使用 `--mouse-capture` 在任何預設關閉的地方選擇加入。原始終端機選取可能會跨越右側邊欄，因為選取權屬於終端機而非 TUI。
- `--profile <NAME>`：選取設定檔
- `--config <PATH>`：設定檔案路徑
- `-v, --verbose`：詳細日誌記錄
