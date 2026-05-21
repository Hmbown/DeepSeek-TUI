# 無障礙存取

[English](ACCESSIBILITY.md) | [简体中文](ACCESSIBILITY.zh-CN.md)

DeepSeek-TUI 在終端機中執行，因此平台本身的無障礙架構（螢幕報讀軟體、放大鏡、終端機層級的主題）已處理大部分工作。TUI 提供一小組開關，可為螢幕報讀軟體和低動態使用者減少視覺動態和密度。

## 快速參考

| 開關 | 預設值 | 效果 |
| --- | --- | --- |
| `NO_ANIMATIONS=1` 環境變數 | 未設定 | 啟動時，強制 `low_motion = true` 和 `fancy_animations = false`。會覆蓋 `settings.toml` 中儲存的任何設定。 |
| `low_motion` 設定 | `false` | 抑制旋轉圖示的動態、轉錄內容淡入效果、頁尾漂移、標頭狀態指示器循環以及作用中儲存格的脈衝效果。幀率限制器也會降低閒置重繪速度，讓游標不會過於頻繁閃爍。 |
| `fancy_animations` 設定 | `false` | 頁尾水柱條和脈衝式子代理計數器。預設為關閉。 |
| `status_indicator` 設定 | `whale` | 標頭狀態晶片。設為 `dots` 使用精簡的圓點循環，或設為 `off` 隱藏。 |
| `calm_mode` 設定 | `false` | 預設折疊工具輸出詳細資訊並修剪狀態訊息。對於每次重繪都朗讀的螢幕報讀軟體很有用。 |
| `show_thinking` 設定 | `true` | 設為 `false` 可完全隱藏模型的 `reasoning_content` 區塊。 |
| `show_tool_details` 設定 | `true` | 設為 `false` 可將工具呼叫呈現為單行而不顯示展開的負載。 |

## 標準環境變數介面

在 shell 設定檔中設定這些變數，讓它們在每個工作階段都生效：

```bash
# 強制低動態 + 無花俏動畫。
export NO_ANIMATIONS=1

# 可選：遵循更廣泛的終端機色彩慣例。
export NO_COLOR=1            # 底層 ratatui 後端會遵循此設定
```

`NO_ANIMATIONS` 接受 `1`、`true`、`yes` 或 `on` 中的任一值（不區分大小寫）。任何其他值（包括 `0`、`false`、空值或未設定）則保留您已儲存的設定。

此覆蓋在啟動時套用一次。在工作階段中更改環境變數沒有效果 — 設定僅在下一次啟動時重新讀取。

## 透過 `/settings` 進行設定

相同的開關也可以從命令面板存取：

* `/settings set low_motion on`
* `/settings set fancy_animations off`
* `/settings set calm_mode on`
* `/settings set status_indicator off`

以此方式寫入的設定會持久儲存到 `~/.config/deepseek/settings.toml`。若已設定 `NO_ANIMATIONS` 環境變數，它在啟動時仍然優先，因此取消設定該環境變數是遵循已儲存選擇的方式。

Tilix 和 Terminator 工作階段會自動以低動態模式啟動，因為這些基於 VTE 的終端機在活動回合期間會出現可見的重繪閃爍。如果您的終端機版本能乾淨呈現，您仍可在啟動後覆蓋已儲存的設定。

## 螢幕報讀軟體使用者注意事項

* `low_motion` 將閒置重繪循環放慢至每幀約 120ms，使游標不會持續重新定位。結合 `calm_mode`，重繪速率保持在足夠低的水平，讓 VoiceOver / Orca 的朗讀能隨模型輸出線性跟進，而不是在每個刻度重新朗讀整個螢幕。
* 轉錄內容為純文字 — 沒有圖片或畫布渲染 — 因此任何與平台無障礙服務整合的終端機（例如 macOS Terminal.app、iTerm2、Ghostty、Windows Terminal）都會直接傳遞渲染後的內容。
* 如果您發現某個 UI 介面在 `low_motion = true` 時仍產生動態，請針對 [`PRIOR: Screen-reader / accessibility flag`](https://github.com/Hmbown/DeepSeek-TUI/issues/450) 提交 issue，並附上螢幕截圖或終端機錄影。

## 相關 issue / 歷史

* [#450](https://github.com/Hmbown/DeepSeek-TUI/issues/450) — 記錄現有旗標、新增 `NO_ANIMATIONS` 啟動覆蓋層，以及撰寫此頁面。
* [#449](https://github.com/Hmbown/DeepSeek-TUI/issues/449) — 頁尾狀態列現在使用目前主題的對比配對，而非自訂調色盤。
