# DeepSeek TUI 發佈手冊

[English](RELEASE_RUNBOOK.md) | [简体中文](RELEASE_RUNBOOK.zh-CN.md)

本手冊是發布 Rust crate、GitHub 發佈資產
以及 `deepseek-tui` npm 包裝的真實來源。

當前封裝說明：
- `deepseek-tui` 是目前發布給使用者的即時執行階段和 TUI 套件。
- `deepseek-tui-core` 是用於萃取/平齊工作的支援工作區 crate，並非發布執行階段的替代品。

## 標準發布目標

- 終端使用者 crate：
  - `deepseek-tui`
  - `deepseek-tui-cli`
- 從此工作區發布的支援 crate：
  - `deepseek-secrets`
  - `deepseek-config`
  - `deepseek-protocol`
  - `deepseek-state`
  - `deepseek-agent`
  - `deepseek-execpolicy`
  - `deepseek-hooks`
  - `deepseek-mcp`
  - `deepseek-tools`
  - `deepseek-core`
  - `deepseek-app-server`
  - `deepseek-tui-core`
- `deepseek-cli` 在 crates.io 上是一個不相關的 crate，不屬於此發佈流程。

## 版本協調

- Rust crate 從 [Cargo.toml](../Cargo.toml) 繼承共享的工作區版本。
- 內部路徑依賴版本應與共享工作區版本一致；一旦工作區版本變動，過時的舊版固定即為發佈阻擋項。
- npm 包裝版本存在於 [npm/deepseek-tui/package.json](../npm/deepseek-tui/package.json)。
- `deepseekBinaryVersion` 控制 npm 包裝下載哪個 GitHub 發佈二進位檔。
- 允許僅封裝的 npm 發佈：
  - 遞增 npm 套件版本
  - 將 `deepseekBinaryVersion` 保持固定在先前已發佈的 Rust 二進位檔
  - 在 `npm publish` 前重新執行 `npm pack` 煙霧檢查

## 預檢

在建立標籤之前從儲存庫根目錄執行以下指令：

```bash
./scripts/release/check-versions.sh   # 工作區、npm、鎖定檔案之間的版本漂移
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo publish --dry-run --locked --allow-dirty -p deepseek-tui
./scripts/release/publish-crates.sh dry-run
```

`check-versions.sh` 也會在每次推送/PR 時於 CI 中執行（`.github/workflows/ci.yml` 中的 `versions` 作業），
因此 `Cargo.toml`、各 crate 清單、`npm/deepseek-tui/package.json` 和 `Cargo.lock` 之間的漂移
會在發佈時間之前而非發佈時被擷取到。

`publish-crates.sh dry-run` 對沒有未發布工作區依賴的 crate 執行完整的 `cargo publish --dry-run`，
並對相依的工作區 crate 執行封裝預檢。這樣可以避免因 crates.io 尚未包含
新工作區版本而產生的假陰性，同時仍在發布前驗證套件內容。

對於 npm 包裝驗證，建置兩個發布的二進位檔並執行跨平台煙霧測試工具。
這會封裝 npm 包裝、將其安裝到一個乾淨的臨時專案中、
透過 HTTP 提供本機發佈資產，並檢查調度器到 TUI 路徑（`deepseek doctor --help`）
和直接的 TUI 進入點（`deepseek-tui --help`）。

```bash
cargo build --release --locked -p deepseek-tui-cli -p deepseek-tui
node scripts/release/npm-wrapper-smoke.js
```

設定 `DEEPSEEK_TUI_KEEP_SMOKE_DIR=1` 以保留臨時封裝/安裝
目錄供檢查。

若也要在本機演練 `npm run release:check`，請在啟動伺服器前
使用完整的資產矩陣固定裝置重新產生本機資產目錄：

```bash
DEEPSEEK_TUI_PREPARE_ALL_ASSETS=1 node scripts/release/prepare-local-release-assets.js
cd npm/deepseek-tui
DEEPSEEK_TUI_VERSION=X.Y.Z DEEPSEEK_TUI_RELEASE_BASE_URL=http://127.0.0.1:8123/ npm run release:check
```

將 `DEEPSEEK_TUI_VERSION` 設定為您要在該本機執行中驗證的 npm 套件版本。

CI 工作流程在 Linux、macOS 和 Windows 上執行相同的 tarball 安裝 + 委派進入點煙霧測試。

發布後，證明發佈版本在兩個註冊表中都可見：

```bash
./scripts/release/check-published.sh X.Y.Z
```

在該指令看到 npm 上的 `deepseek-tui@X.Y.Z` 和 crates.io 上每個 `deepseek-*` crate 的 `X.Y.Z` 之前，
不要將 Rust 發佈標記為完成。對於罕見的僅 npm 封裝發佈，使用 `--allow-npm-binary-mismatch` 執行，
並在發佈說明中明確說明沒有發布新的 Rust 二進位版本。

## Rust Crates 發佈

Crate 發布至 crates.io 是**手動的** — 沒有自動化的
`crates-publish` GitHub 工作流程。操作者從已設定 `cargo login`
的開發工作站執行 `scripts/release/` 中的輔助程式。

1. 更新 [Cargo.toml](../Cargo.toml) 中的工作區版本。
2. 在本機執行 `./scripts/release/check-versions.sh` 和
   `./scripts/release/publish-crates.sh dry-run`；兩者都必須乾淨通過。
3. 將發佈標記為 `vX.Y.Z`（通常是將版本遞增推送到
   `main` 並讓 `auto-tag.yml` 建立標籤 — 關於 `RELEASE_TAG_PAT` 要求，
   請參閱下方 npm 包裝發佈章節）。
4. 使用 `./scripts/release/publish-crates.sh publish` 按以下順序發布 crate：
   - `deepseek-secrets`
   - `deepseek-config`
   - `deepseek-protocol`
   - `deepseek-state`
   - `deepseek-agent`
   - `deepseek-execpolicy`
   - `deepseek-hooks`
   - `deepseek-mcp`
   - `deepseek-tools`
   - `deepseek-core`
   - `deepseek-app-server`
   - `deepseek-tui-core`
   - `deepseek-tui-cli`
   - `deepseek-tui`
5. 等待每個已發布的 crate 版本出現在 crates.io 上後再發布相依的 crate。

發布輔助程式對重新執行是冪等的：已發布的 crate 版本會被跳過。

## GitHub 發佈資產

`.github/workflows/release.yml` 建置以下二進位檔：

- `deepseek-linux-x64`
- `deepseek-macos-x64`
- `deepseek-macos-arm64`
- `deepseek-windows-x64.exe`
- `deepseek-tui-linux-x64`
- `deepseek-tui-macos-x64`
- `deepseek-tui-macos-arm64`
- `deepseek-tui-windows-x64.exe`

發佈作業也會上傳 `deepseek-artifacts-sha256.txt`。npm 安裝程式和
發佈驗證腳本都依賴該校驗和清單。

## npm 包裝發佈

**npm publish 步驟是手動的。** `release.yml` 不再執行 `npm publish`，
因為 npm 帳號每次發布都需要 2FA OTP，且尚未配置可繞過 2FA
的自動化 token。GitHub Release 流程保持完全自動化；
僅 npm 包裝發布需要開發者在已設定 `npm login` 和
驗證器應用程式的工作站上執行。

### 步驟

1. 將 [npm/deepseek-tui/package.json](../npm/deepseek-tui/package.json) 中的 npm 套件版本設為與工作區 `Cargo.toml` 一致。
   CI 的版本漂移防護會在標籤建立前擷取到不一致。
2. 將 `deepseekBinaryVersion` 設為應提供二進位檔的 GitHub 發佈標籤。
3. 將版本遞增推送到 `main`。`auto-tag.yml` 建立對應的 `vX.Y.Z` 標籤，
   而 `release.yml` 建置二進位矩陣並草擬 GitHub Release。
4. **等待 GitHub Release 完成**，包含全部八個已簽署的二進位檔加上
   `deepseek-artifacts-sha256.txt`。npm 的 `prepublishOnly` 勾點
   （`scripts/verify-release-assets.js`）要求每個資產都存在。
5. 從開發者機器手動發布 npm 包裝：

```bash
cd npm/deepseek-tui
npm publish --access public
# （系統會提示您輸入驗證器中的 npm OTP）
```

### 為何不自動化？

- `release.yml` 舊有的 `publish-npm` 作業使用 `secrets.NPM_TOKEN`，
  但 npm 的預設 2FA 政策意味著發布 token 必須是已啟用「對 token 認證繞過 2FA」
  的自動化 token，或帳號層級的 2FA 停用狀態。我們兩者皆未設定。
- 獨立的 `publish-npm.yml` 和 `crates-publish.yml` 工作流程已被移除；
  沒有遺留的無效自動化管道。未來若轉移到 npm Trusted Publishing (OIDC)，
  屆時將重新引入專用工作流程。

### 若日後修復 token

要重新啟用自動化發布：配置已啟用「對 token 認證繞過 2FA」的 npm 自動化 token
（或透過 OIDC 設定 npm Trusted Publishing），將對應的密鑰儲存在儲存庫上，
並在 `release.yml`（或專用工作流程）中重新加入 `publish-npm` 作業，
同時還原本章節的「手動」框架。

## CNB Cool 鏡像

每次對 `main` 的推送和每個 `v*` 標籤都會透過 `Sync to CNB` 工作流程
鏡像到 `cnb.cool/deepseek-tui.com/DeepSeek-TUI`，
讓 GitHub 封鎖網路後的使用者能取得原始碼。在發佈標籤之後，
**驗證鏡像已擷取到**，再宣告發佈已完成：

```bash
git ls-remote https://cnb.cool/deepseek-tui.com/DeepSeek-TUI.git refs/tags/vX.Y.Z
```

若工作流程對發佈標籤失敗，手動備援方案記載於
[docs/CNB_MIRROR.md](CNB_MIRROR.md)（一次性 `git
remote add cnb …`，然後 `git push cnb vX.Y.Z`）。

## 復原與回溯

- Crate 發布部分完成：
  - 重新執行 `./scripts/release/publish-crates.sh publish`
  - 已發布的 crate 版本將被跳過
- GitHub 資產遺失或校驗和清單不完整：
  - 修復 `.github/workflows/release.yml`
  - 重新標記或在 `npm publish` 前上傳修正後的資產
- 僅 npm 封裝問題：
  - 僅遞增 npm 套件版本
  - 將 `deepseekBinaryVersion` 保持在最後一個已知正常的 Rust 發佈版本
  - 重新封裝並重新發布包裝
- 錯誤的 npm 發布無法覆寫：
  - 發布具有修正後中繼資料或安裝邏輯的新 npm 版本
- CNB 鏡像對發佈標籤失敗：
  - 透過 `gh run list --workflow=sync-cnb.yml` 檢查執行記錄
  - 使用 `gh workflow run sync-cnb.yml` 重新觸發，或依照
    [docs/CNB_MIRROR.md](CNB_MIRROR.md#manual-fallback) 手動推送標籤
