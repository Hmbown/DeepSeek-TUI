# 安全性政策

[English](SECURITY.md) | [简体中文](SECURITY.zh-CN.md)

DeepSeek TUI 是一個編碼代理，可直接存取檔案操作、Shell 執行和網路。安全性揭露將被認真對待。

## 支援的版本

僅最新穩定版本會收到安全性修補程式。不對舊版本進行回溯移植。

| 版本 | 支援 |
|---|---|
| 最新穩定版 | :white_check_mark: |
| < 最新版 | :x: |

請查看[發布頁面](https://github.com/Hmbown/DeepSeek-TUI/releases)了解目前版本。

## 回報漏洞

**請勿為安全性漏洞開啟公開的 GitHub issue。**

透過以下方式之一私下回報：

- **GitHub 私人通報**：[github.com/Hmbown/DeepSeek-TUI/security/advisories/new](https://github.com/Hmbown/DeepSeek-TUI/security/advisories/new)
- **Email**：[security@deepseek-tui.com](mailto:security@deepseek-tui.com) — 主旨行請包含 `[SECURITY]`

報告中請包含：

- 漏洞描述以及若被利用的影響
- 重現步驟或概念驗證
- 受影響的版本和設定詳細資訊
- 任何建議的緩解措施（可選）

## 回應時間表

| 階段 | 目標 |
|---|---|
| 確認收到 | 收到後 48 小時內 |
| 評估 | 5 天內 — 分類嚴重性、範圍和修復方式 |
| 修補（嚴重） | 評估後 14 天內 |
| 修補（中等/低） | 下一個功能版本或依維護者時間表 |
| 揭露 | 修補程式發布且使用者有時間更新後 |

您將在每個階段收到狀態更新。若時間表延遲，我們將溝通原因和修訂後的預估。

## 範圍

### 範圍內（列入計算的項目）

- 透過精心設計的提示或模型回應進行的遠端程式碼執行
- 沙箱逃逸 — 突破 YOLO 模式工作區邊界或 Shell `cwd` 限制
- 憑證洩漏 — 外洩 API 金鑰、權杖或環境密鑰
- 預期工作區外的任意檔案讀取/寫入（`PathEscape` 繞過）
- 透過 `fetch_url` 或 `web_search` 針對內部網路端點的 SSRF
- 未經授權的 MCP 伺服器存取或工具呼叫

### 範圍外

- 對維護者或貢獻者的社交工程
- 針對 DeepSeek API 的阻斷服務 / 速率限制耗盡
- 第三方依賴中的漏洞（請回報給上游專案）
- 需要對受害者機器進行實體存取的攻擊
- 未在 DeepSeek TUI 情境中展示的理論性 ML 模型注入攻擊

如果您不確定某個錯誤是否在範圍內，請仍然回報。我們將進行分類並回應。

## 名人堂

我們為提交經過驗證的安全性漏洞的回報者維護一個名人堂。若要獲得致謝，請在報告中包含您偏好的名稱 / handle。

*目前尚無條目 — 成為第一人。*
