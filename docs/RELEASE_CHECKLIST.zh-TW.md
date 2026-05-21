# 發佈檢查清單

[English](RELEASE_CHECKLIST.md) | [简体中文](RELEASE_CHECKLIST.zh-CN.md)

一個標籤前檢查清單，v0.8.21/v0.8.22 CHANGELOG 的遺漏證明我們需要它。
在發佈分支（`work/vX.Y.Z-...`）的乾淨工作樹上按順序逐步執行。
將任何未勾選的方塊視為發佈阻擋項。

關於底層工具（預檢腳本、npm 煙霧測試、
publish-crates）的更多背景，請參閱 [`RELEASE_RUNBOOK.md`](RELEASE_RUNBOOK.md)。

## 1. CHANGELOG 條目存在於該版本

- [ ] `CHANGELOG.md` 頂部有一個 `## [X.Y.Z] - YYYY-MM-DD` 標題
- [ ] 該條目標記了每位其提交進入本版本的
      外部貢獻者。使用以下指令取得清單：
      ```
      git log vPREV..HEAD --no-merges --format="%h %an <%ae> %s" \
        | grep -v '<your-email@…>'
      ```
      對於每位貢獻者，連結其顯示名稱和（已知時）
      `@github-handle`。
- [ ] 該條目使用 Keep a Changelog 標題 — `Added`、`Changed`、
      `Fixed`、`Security`、`Removed`、`Deprecated`。僅在
      有使用者必須繞過的實質問題時才新增 `Known issues`。
- [ ] 該條目將所有引用的 issue/PR 編號標記為 `#NNNN`，
      以便 GitHub 的自動連結功能擷取到。

## 2. 版本固定值同步

- [ ] `Cargo.toml` 工作區 `version` 已遞增。
- [ ] 所有各 crate `crates/*/Cargo.toml` 路徑依賴 `version = "..."`
      固定值與新工作區版本一致。
- [ ] `npm/deepseek-tui/package.json` `version` 和 `deepseekBinaryVersion`
      兩者皆已遞增。
- [ ] `Cargo.lock` 已刷新（`cargo update --workspace --offline`）。
- [ ] `./scripts/release/check-versions.sh` 回報
      `Version state OK: workspace=X.Y.Z, npm=X.Y.Z, lockfile in sync.`

## 3. 預檢閘門

從儲存庫根目錄依序執行：

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo check --workspace --all-targets --locked`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --all-features --locked`
      （在宣告為不穩定測試之前，使用
      `cargo test -p PKG --bin BIN -- TEST_NAME` 隔離重新執行任何單一失敗。
      會變更跨處理序狀態的測試 — `HOME`、`cwd`、`RUST_LOG` —
      可能在平行執行時競爭。在 `Known issues` 中記錄已確認的不穩定測試。）
- [ ] `./scripts/release/publish-crates.sh dry-run`

## 4. npm 包裝煙霧測試

- [ ] `cargo build --release --locked -p deepseek-tui-cli -p deepseek-tui`
- [ ] `node scripts/release/npm-wrapper-smoke.js`
      （若稍後需要檢查臨時安裝目錄，請設定
      `DEEPSEEK_TUI_KEEP_SMOKE_DIR=1`。）

## 5. 分支和 PR

- [ ] 分支已推送：`git push -u origin work/vX.Y.Z-...`
- [ ] 已使用 `gh pr create --base main --title "chore(release): prepare vX.Y.Z"` 開啟 PR
- [ ] PR 內文包含：
  - 一段發佈主題摘要
  - 自上次發佈以來的新提交清單
  - 明確標示任何 **Security** 項目，以便審查者注意到
  - 貢獻者感謝清單
  - CHANGELOG 中的 `Known issues` 區塊（若有）
- [ ] PR 標題為**中性** — 不要在標題中使用 CVE 風格語言或特定
      攻擊細節。將這些保留到標籤推送後的 GitHub 發佈說明中。

## 6. CI 通過且審查完成

- [ ] 所有必要的 CI 作業皆為綠色。`versions` 作業應反映
      預檢 `check-versions.sh`，是您的最後一道防線。
- [ ] PR 已經審查。

## 7. 標籤與發佈（審查後）

- [ ] `git tag -s vX.Y.Z -m "vX.Y.Z"`
- [ ] `git push origin vX.Y.Z`
- [ ] `release.yml` 工作流程已建置資產並上傳至
      此標籤的 GitHub 發佈版本。
- [ ] `npm view deepseek-tui@X.Y.Z version deepseekBinaryVersion --json`
      在 npm 註冊表中回報新版本。
- [ ] `crates.io` 有新版本（或 `publish-crates.sh` 作業已
      推送）。
- [ ] `ghcr.io/hmbown/deepseek-tui:vX.Y.Z` 和 `:latest` 已更新。

## 8. 標籤後

- [ ] 編輯 GitHub 發佈說明以展開任何刻意從 PR 標題/內文中
      省略的 CVE 風格或攻擊細節。
- [ ] 在下個發佈的追蹤 issue 中記錄任何延期項目。
- [ ] 關閉此發佈修復的所有 issue。

---

若某步驟失敗，**修復根本原因**而不是跳過它。預提交
勾點、簽署和 CI 都在此捕捉真正的問題。`--no-verify`、
`--no-gpg-sign` 以及強制推送發佈分支覆蓋審查者應
透過慣例保持強制停用。
