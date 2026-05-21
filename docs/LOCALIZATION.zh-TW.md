# 在地化矩陣

[English](LOCALIZATION.md) | [简体中文](LOCALIZATION.zh-CN.md)

狀態日期：2026-04-29

此檔案僅追蹤 UI 在地化。它不會變更模型輸出語言、提供者行為或 DeepSeek 酬載支援。除非單獨新增原生媒體酬載支援，否則媒體附件仍為本機路徑文字參考。

## 來源稽核

v0.7.6 同位檢查使用即時 GitHub 來源及 `/opt/homebrew/bin/gh`。

| 專案 | 參考 | 證據 | 結果 |
|---|---:|---|---|
| Codex CLI | `openai/codex@df966996a75333add031fca47b72655e9ee504fd` | `gh repo view openai/codex`；對 `locale`、`i18n`、`l10n`、`translation`、`messages` 的遞迴目錄掃描；README 語言掃描 | 在已稽核的目錄樹中未找到已簽入的 CLI UI 在地化登錄。將 Codex CLI 同位視為英文優先的終端機 UI 行為，而非已發布的 locale 標籤來源。 |
| opencode | `anomalyco/opencode@00bb9836a60f1dcdd0ce5078b05d12f749fdde66` | `packages/console/app/src/lib/language.ts`、`packages/app/src/context/language.tsx`、`packages/web/src/i18n/locales.ts`、`packages/app/src/i18n/parity.test.ts` | opencode 發布了應用程式/檔案的 locale 基礎設施，包含語言偵測、locale 標籤、文件 locale 別名、阿拉伯語的 RTL 方向，以及針對目標鍵值的同位測試。 |

## v0.7.6 已發布核心套件

這些 locale 受 `settings.toml` 中的 `locale` 以及 `LANG` / `LC_ALL` 自動偵測支援。

| Locale | 顯示名稱 | 書寫系統 | 方向 | 回退 | 優先層級 | v0.7.6 範圍 | 備註 |
|---|---|---|---|---|---|---|---|
| `en` | English | Latin | LTR | `en` | 基準 | 來源字串保持標準形式。 | 英文始終可用。 |
| `ja` | Japanese | Jpan | LTR | `en` | v0.7.6 必須具備 | 核心 TUI chrome | 涵蓋編輯器佔位符/歷史搜尋、說明 chrome 及 `/config` chrome。 |
| `zh-Hans` | Chinese Simplified | Hans | LTR | `en` | v0.7.6 必須具備 | 核心 TUI chrome | `zh`、`zh-CN` 及 `zh-Hans` 解析至此。繁體中文未發布。 |
| `pt-BR` | Portuguese (Brazil) | Latin | LTR | `en` | v0.7.6 必須具備 | 核心 TUI chrome | `pt` 及 `pt-PT` 目前回退至巴西葡萄牙語；歐洲葡萄牙語未單獨發布。 |

選擇方式：

```toml
locale = "auto"     # 預設值；檢查 LC_ALL、LC_MESSAGES，然後 LANG
locale = "ja"
locale = "zh-Hans"
locale = "pt-BR"
```

回退機制：

- 遺失或不支援的已設定 locale 會回退至英文。
- 當未偵測到受支援的環境 locale 時，`auto` 會回退至英文。
- 解析後的 locale 會包含在 system prompt 中，作為 V4 推理和回覆的回退自然語言。最新的使用者訊息具有優先權，包括 `reasoning_content`，因此即使解析後的 locale 為英文，中文回合仍應保持中文。

## 規劃中的全球南方 QA 矩陣

除非後續變更新增完整的訊息覆蓋範圍及 QA 證據，否則這些不聲稱為 v0.7.6 中已發布的翻譯。

| Locale | 顯示名稱 | 書寫系統 | 方向 | 優先層級 | 覆蓋狀態 | 回退 | QA 狀態 | 版面風險 |
|---|---|---|---|---|---|---|---|---|
| `ar` | Arabic | Arab | RTL | 後續 | 已規劃 | `en` | 僅自動化渲染器範例；發布前需要母語者審查 | RTL 排序、標點符號、快捷鍵混合 |
| `hi` | Hindi | Deva | LTR | 後續 | 已規劃 | `en` | 僅自動化渲染器範例；發布前建議母語者審查 | 組合標記、游標寬度、截斷 |
| `bn` | Bengali | Beng | LTR | 後續 | 已規劃 | `en` | 僅矩陣；發布前需要母語者審查 | 組合標記、換行 |
| `id` | Indonesian | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；需要自動化窄寬度快照及審查者通過 | 標籤比英文長 |
| `vi` | Vietnamese | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；需要自動化寬度快照及審查者通過 | 變音符號及換行標籤 |
| `sw` | Swahili | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；發布前需要母語或流利程度審查 | 翻譯品質、較長的命令說明 |
| `ha` | Hausa | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；發布前需要母語或流利程度審查 | 變音符號及術語 |
| `yo` | Yoruba | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；發布前需要母語或流利程度審查 | 聲調標記及術語 |
| `fil` | Filipino/Tagalog | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；發布前需要來源字串 | 術語一致性 |
| `es-419` | Spanish (Latin America) | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；發布前需要審查者通過 | 區域術語 |
| `fr` | French | Latin | LTR | 後續 | 已規劃 | `en` | 僅矩陣；發布前需要審查者通過 | 非洲 locale 術語有所不同 |

## 訊息覆蓋範圍

首次登錄遍歷涵蓋高可見性終端機 chrome 的穩定訊息 ID：

- 編輯器佔位符
- 編輯器歷史搜尋標題、佔位符、提示及無符合狀態
- `/config` 標題、篩選佔位符、無符合狀態、已篩選計數及頁尾提示
- 說明覆蓋層標題、篩選佔位符、無符合狀態、區段標籤及頁尾提示

v0.7.6 中尚未翻譯：

- 模型/系統 prompt 及人格
- 提供者或工具 schema
- 完整的斜線命令說明及每個狀態/toast/錯誤路徑
- 此設定備註以外的 README/檔案內容

## 翻譯者備註

除非後續詞彙表明確變更，否則請保持以下技術術語穩定：
`Plan`、`Agent`、`YOLO`、`/config`、`/mcp`、`@path`、`/attach`、`DeepSeek`、
`MCP`、`CLI`、`TUI`，以及快捷鍵如 `Enter`、`Esc`、`Tab`、`PgUp` 和
`PgDn`。

## QA 檢查清單

在將規劃中的 locale 提升為已發布之前：

1. 在 `crates/tui/src/localization.rs` 中新增完整的訊息覆蓋範圍。
2. 新增 locale 解析測試及遺失鍵值測試。
3. 至少為編輯器、說明及 `/config` 新增窄寬度渲染覆蓋範圍。
4. 驗證 CJK 寬度、RTL 標點符號、組合標記及截斷。
5. 記錄母語/流利審查狀態，或將 locale 標記為僅自動化 QA。
