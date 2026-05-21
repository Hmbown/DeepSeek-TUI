# 容量控制器

[English](capacity_controller.md)

`deepseek-tui` 包含一個可選加入的容量感知上下文控制器。在
預設的 V4 路徑中它是停用的，因為其主動介入可能會重寫
即時提示並破壞前綴快取親和性。除非明確設定 `capacity.enabled = true`，
否則請將其視為遙測資料或實驗性防護欄。

## 政策概覽

每個檢查點計算：

- `H_hat`（執行階段壓力代理值）
- `C_hat`（模型容量先驗值）
- `slack = C_hat - H_hat`
- 最近 `N=8` 次觀測的動態 slack 設定檔

### 執行階段壓力代理值 (`H_hat`)

- `action_complexity_bits = log2(1 + action_count_this_turn)`
- `tool_complexity_bits = log2(1 + tool_calls_recent_window)`
- `ref_complexity_bits = log2(1 + unique_reference_ids_recent_window)`
- `context_pressure_bits = 6.0 * context_used_ratio`

公式：

`H_hat = 0.35*action_complexity_bits + 0.30*tool_complexity_bits + 0.20*ref_complexity_bits + 0.15*context_pressure_bits`

### 容量先驗值 (`C_hat`)

各模型先驗值：

- `deepseek_v3_2_chat = 3.9`
- `deepseek_v3_2_reasoner = 4.1`
- `deepseek_v4_pro = 3.5`
- `deepseek_v4_flash = 4.2`
- 備援 `3.8`（用於其他 DeepSeek ID，包含未來發佈版本）

### 失敗機率

使用滾動設定檔欄位：

- `final_slack`
- `min_slack`
- `violation_ratio`
- `slack_volatility`
- `slack_drop`

公式：

`z = -1.65*final_slack -0.85*min_slack +1.35*violation_ratio +0.70*slack_volatility +0.28*slack_drop -0.12`

`p_fail = sigmoid(z)`，限制在 `[0,1]` 範圍內。

風險區間：

- 低：`p_fail <= low_risk_max`
- 中：`p_fail <= medium_risk_max`
- 高：否則

控制器明確啟用時的動作對應：

- 低 -> `NoIntervention`（無介入）
- 中 -> `TargetedContextRefresh`（定向上下文刷新）
- 高 + 嚴重動態（`min_slack <= severe_min_slack` 或 `violation_ratio >= severe_violation_ratio`） -> `VerifyAndReplan`（驗證並重新規劃）
- 否則高 -> `VerifyWithToolReplay`（透過工具重播驗證）

## 檢查點

啟用時，引擎會在以下時機評估控制器政策：

1. 請求前檢查點（`MessageRequest` 組裝之前）。
2. 工具後檢查點（工具結果附加之後）。
3. 錯誤升級檢查點（工具錯誤連續路徑）。

## 介入措施

介入措施不屬於預設的 v0.7.5 V4 路徑。預設路徑為：
附加訊息、保留前綴快取重用、在接近實際模型壓力時建議手動 `/compact`，
僅在請求將超過模型輸入預算時使用溢位復原。

### `TargetedContextRefresh`（定向上下文刷新）

- 在可能時執行壓縮（`compact_messages_safe`）。
- 若壓縮路徑失敗則退回本機修剪。
- 持久化標準狀態。
- 以精簡的標準提示 + 記憶體指標替換長尾活躍上下文。

### `VerifyWithToolReplay`（透過工具重播驗證）

- 從最近回合上下文中重播一個唯讀關鍵工具呼叫。
- 附加驗證說明，包含通過/失敗 + 差異摘要。
- 在重播衝突/錯誤時，標記升級候選並停用當前回合的重播。

### `VerifyAndReplan`（驗證並重新規劃）

- 持久化標準快照。
- 清除揮發性提示尾部，同時保留最新的使用者提問和最新的驗證說明。
- 將標準重新規劃指令注入系統提示。
- 從精簡的標準狀態繼續回合迴圈。

## 安全控制

- 每回合最多一次介入。
- 刷新和重新規劃有冷卻時間。
- 每回合重播預算（`max_replay_per_turn`）。
- 控制器輸入不可用時的故障開放行為。
- 壓縮/重播失敗會記錄；回合繼續進行。

## 記憶體儲存

路徑：

- `DEEPSEEK_CAPACITY_MEMORY_DIR`（若已設定）
- 否則 `~/.deepseek/memory/<session_id>.jsonl`
- 備援：當家目錄路徑不可用/無法寫入時使用 `<workspace>/.deepseek/memory/<session_id>.jsonl`

記錄欄位：

- `id`、`ts`、`turn_index`、`action_trigger`
- `h_hat`、`c_hat`、`slack`、`risk_band`
- `canonical_state`
- `source_message_ids`
- 可選 `replay_info`

載入器工具支援取得最後 `K` 個快照以進行還原。

## 設定

`[capacity]` 索引鍵：

- `enabled`（預設 `false`）
- `low_risk_max`（預設 `0.50`）
- `medium_risk_max`（預設 `0.62`）
- `severe_min_slack`（預設 `-0.25`）
- `severe_violation_ratio`（預設 `0.40`）
- `refresh_cooldown_turns`（預設 `6`）
- `replan_cooldown_turns`（預設 `5`）
- `max_replay_per_turn`（預設 `1`）
- `min_turns_before_guardrail`（預設 `4`）
- `profile_window`（預設 `8`）
- `deepseek_v3_2_chat_prior`（預設 `3.9`）
- `deepseek_v3_2_reasoner_prior`（預設 `4.1`）
- `deepseek_v4_pro_prior`（預設 `3.5`）
- `deepseek_v4_flash_prior`（預設 `4.2`）
- `fallback_default_prior`（預設 `3.8`）

等效的環境變數覆寫可透過 `DEEPSEEK_CAPACITY_*` 使用。
