# 貢獻 DeepSeek TUI

[English](CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md)

感謝您對貢獻 DeepSeek TUI 的興趣！本文件提供貢獻的指引與說明。

## 入門

### 先決條件

- Rust 1.88 或更新版本（edition 2024）
- Cargo 套件管理器
- Git

### 設定開發環境

1. Fork 並 clone 儲存庫：
   ```bash
   git clone https://github.com/YOUR_USERNAME/DeepSeek-TUI.git
   cd DeepSeek-TUI
   ```

2. 建置專案：
   ```bash
   cargo build
   ```

3. 執行測試：
   ```bash
   cargo test
   ```

4. 使用開發設定執行：
   ```bash
   cargo run
   ```

## 開發工作流程

### 程式碼風格

- 提交前執行 `cargo fmt` 以確保一致的格式化
- 執行 `cargo clippy` 並處理所有警告
- 遵循 Rust 命名慣例（函式/變數使用 snake_case，型別使用 CamelCase）
- 為公開 API 新增文件註解

### 測試

- 為新功能撰寫測試
- 確保所有現有測試通過：`cargo test --workspace --all-features`
- 將單元測試放在其涵蓋的程式碼旁邊（標準 Rust `#[cfg(test)]`
  模組），並將整合測試放在所屬 crate 的 `tests/`
  目錄下（例如 `crates/tui/tests/` 或 `crates/state/tests/`）。儲存庫
  根目錄的 `tests/` 目錄不會被使用

### 提交訊息

使用清晰、描述性的提交訊息，遵循 conventional commits：

- `feat:` 新功能
- `fix:` 錯誤修復
- `docs:` 文件變更
- `refactor:` 程式碼重構
- `test:` 新增或更新測試
- `chore:` 維護任務

範例：`feat: add doctor subcommand for system diagnostics`

當提交內容從社群 PR 中擷取程式碼時（請參閱下方的「您的貢獻如何落地」），
請在提交訊息內文中包含一行 `Harvested from PR #N by @author`。
自動關閉工作流程會監控此模式，並關閉被引用的 PR 並給予致謝，
讓貢獻者清楚知道他們的工作已發布。

## 您的貢獻如何落地

我們遵循一個審慎的「落地有用的部分，致謝貢獻者」模式，
這偶爾會讓新貢獻者感到意外。有兩種路徑：

### 路徑 1 — 直接合併

如果您的 PR 範圍明確、通過 CI、未觸及信任邊界範圍
（認證 / 沙箱 / 發布 / 品牌），且與 main 沒有衝突，
維護者會直接合併。這是小型錯誤修復和經過良好測試的功能新增最常見的結果。

### 路徑 2 — 擷取（Harvest）

如果您的 PR 規模較大、範圍混雜、與 main 衝突，或需要潤飾
而由維護者直接處理比與貢獻者來回溝通更快速，維護者可能會**擷取**
有用的提交或程式碼區塊，整合成一個新的提交到 `main` 上，而不是直接合併 PR。
這**不是拒絕** — 這表示您的程式碼已落地。

當發生此情況時：

- 擷取的提交訊息包含 `Harvested from PR #N by
  @your-handle`。這是合約：該行是您的致謝，也是
  您的貢獻已發布的訊號。
- 下一個版本的 `CHANGELOG.md` 條目會以您的 handle 致謝。
- 自動關閉工作流程會以範本感謝訊息關閉您的 PR，
  並附上指向 `main` 上提交的連結。

若要讓未來的貢獻透過更快速的直接合併路徑而非擷取路徑落地，
您可以做的最有效事項是：

1. **保持 PR 單一目的。** 每個 PR 一個錯誤修復；每個 PR 一個功能。
   不要將重構與功能混在一起。
2. **在開啟 PR 前 rebase 到最新的 `main`**，並在 CI 回饋後
   也進行 rebase。衝突即使變更很小也會強制走擷取路徑。
3. **包含測試**與新行為。維護者經常擷取沒有測試的 PR，
   因為新增測試比要求貢獻者提供測試更快速。
4. **避免觸及信任邊界範圍**而沒有事先取得維護者
   同意。這包括認證/憑證流程、沙箱策略、
   發布/發行管線，以及 `prompts/` 內容。未經事先討論就觸及
   這些範圍的 PR，即使實作良好也不太可能直接合併。

## 專案結構

DeepSeek TUI 是一個 Cargo workspace。執行時期和大多數 TUI、
引擎以及工具程式碼目前位於 `crates/tui/src/`。較小的 workspace
crate 提供正在逐步提取的共用抽象。

```
crates/
├── tui/           deepseek-tui 二進位檔（互動式 TUI + 執行時期 API）
├── cli/           deepseek 二進位檔（調度器 facade）
├── app-server/    HTTP/SSE + JSON-RPC 傳輸
├── core/          Agent 迴圈 / 工作階段 / 回合管理
├── protocol/      請求/回應框架
├── config/        設定載入、設定檔、環境變數優先順序
├── state/         SQLite 對話串/工作階段持續性
├── tools/         型別化工具規格與生命週期
├── mcp/           MCP 客戶端 + stdio 伺服器
├── hooks/         生命週期掛鉤（stdout/jsonl/webhook）
├── execpolicy/    審批/沙箱策略引擎
├── agent/         模型/提供者註冊
└── tui-core/      事件驅動 TUI 狀態機腳手架
```

請參閱 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) 了解這些 crate
之間的即時資料流，包括由下而上的建置順序。

## 提交變更

1. 從 `main` 建立功能分支：
   ```bash
   git checkout -b feat/your-feature
   ```

2. 進行您的變更並提交

3. 確保 CI 通過：
   ```bash
   cargo fmt --check
   cargo clippy
   cargo test
   ```

4. 推送您的分支並建立 Pull Request

5. 在 PR 描述中清楚地描述您的變更

## Pull Request 指引

- 保持 PR 專注於單一變更
- 必要時更新文件
- 為新功能新增測試
- 在請求審查前確保 CI 通過

## 典型 PR 的樣貌

一個結構良好的 PR 遵循一致的模式。近期的範例包括：

- **#386** — `/init` 指令：新的 `crates/tui/src/commands/init.rs` 模組、專案類型偵測、
  AGENTS.md 產生、`commands/mod.rs` 中的指令註冊、在地化字串。
- **#389** — 內嵌 LSP 診斷：`crates/tui/src/lsp/` 中的 LSP 子系統、
  `core/engine/lsp_hooks.rs` 中的引擎掛鉤、設定開關、測試覆蓋。
- **#387** — 自我更新：新的 `crates/cli/src/update.rs` 模組、CLI 子指令註冊、
  HTTP 下載 + SHA256 驗證 + 原子二進位檔替換。
- **#393** — `/share` 工作階段 URL：新的 `crates/tui/src/commands/share.rs`、HTML 渲染、
  `gh gist create` 整合、指令註冊。
- **#343/#346** —（v0.8.5）執行時期對話串/回合時間軸和持久任務管理器重構。

通常每個 PR 觸及 1–3 個新檔案，修改 2–5 個現有檔案以進行接線
（註冊表、分派匹配、在地化），並新增或更新測試。變更
範圍限制在單一功能或修復 — 如果您發現需要做的相關工作，
請開啟一個單獨的 issue，而不是擴大 PR 範圍。

提交前，請執行：
```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features 2>&1 | head -50
cargo check
```

## 回報 Issue

回報 issue 時，請包含：

- 作業系統和版本
- Rust 版本（`rustc --version`）
- DeepSeek TUI 版本（`deepseek --version`）
- 重現 issue 的步驟
- 預期行為 vs 實際行為
- 相關的錯誤訊息或日誌

## 行為準則

保持尊重和包容。我們歡迎各種背景和經驗水準的貢獻者。

## 授權

透過貢獻 DeepSeek TUI，您同意您的貢獻將以 MIT License 授權。

## 有疑問？

歡迎針對任何有關貢獻的疑問開啟一個 issue。
