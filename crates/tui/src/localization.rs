//! Lightweight localization registry for high-visibility TUI strings.
//!
//! This intentionally covers UI chrome only. It does not change model prompts,
//! model output language, provider behavior, or media payload semantics.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaleCoverage {
    English,
    V076Core,
    PlannedQa,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleSpec {
    pub tag: &'static str,
    pub display_name: &'static str,
    pub script: &'static str,
    pub direction: TextDirection,
    pub fallback: &'static str,
    pub coverage: LocaleCoverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    Ja,
    ZhHans,
    PtBr,
}

impl Locale {
    pub fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ja => "ja",
            Self::ZhHans => "zh-Hans",
            Self::PtBr => "pt-BR",
        }
    }

    #[allow(dead_code)]
    pub fn spec(self) -> LocaleSpec {
        match self {
            Self::En => LocaleSpec {
                tag: "en",
                display_name: "English",
                script: "Latin",
                direction: TextDirection::Ltr,
                fallback: "en",
                coverage: LocaleCoverage::English,
            },
            Self::Ja => LocaleSpec {
                tag: "ja",
                display_name: "Japanese",
                script: "Jpan",
                direction: TextDirection::Ltr,
                fallback: "en",
                coverage: LocaleCoverage::V076Core,
            },
            Self::ZhHans => LocaleSpec {
                tag: "zh-Hans",
                display_name: "Chinese Simplified",
                script: "Hans",
                direction: TextDirection::Ltr,
                fallback: "en",
                coverage: LocaleCoverage::V076Core,
            },
            Self::PtBr => LocaleSpec {
                tag: "pt-BR",
                display_name: "Portuguese (Brazil)",
                script: "Latin",
                direction: TextDirection::Ltr,
                fallback: "en",
                coverage: LocaleCoverage::V076Core,
            },
        }
    }

    #[allow(dead_code)]
    pub fn shipped() -> &'static [Self] {
        &[Self::En, Self::Ja, Self::ZhHans, Self::PtBr]
    }
}

#[allow(dead_code)]
pub const PLANNED_QA_LOCALES: &[LocaleSpec] = &[
    LocaleSpec {
        tag: "ar",
        display_name: "Arabic",
        script: "Arab",
        direction: TextDirection::Rtl,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "hi",
        display_name: "Hindi",
        script: "Deva",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "bn",
        display_name: "Bengali",
        script: "Beng",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "id",
        display_name: "Indonesian",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "vi",
        display_name: "Vietnamese",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "sw",
        display_name: "Swahili",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "ha",
        display_name: "Hausa",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "yo",
        display_name: "Yoruba",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "es-419",
        display_name: "Spanish (Latin America)",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "fr",
        display_name: "French",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
    LocaleSpec {
        tag: "fil",
        display_name: "Filipino/Tagalog",
        script: "Latin",
        direction: TextDirection::Ltr,
        fallback: "en",
        coverage: LocaleCoverage::PlannedQa,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageId {
    ComposerPlaceholder,
    HistorySearchPlaceholder,
    HistorySearchTitle,
    HistoryHintMove,
    HistoryHintAccept,
    HistoryHintRestore,
    HistoryNoMatches,
    ConfigTitle,
    ConfigModalTitle,
    ConfigSearchPlaceholder,
    ConfigNoSettings,
    ConfigNoMatchesPrefix,
    ConfigFilteredSettings,
    ConfigShowing,
    ConfigFooterDefault,
    ConfigFooterScrollable,
    ConfigFooterFiltered,
    HelpTitle,
    HelpFilterPlaceholder,
    HelpFilterPrefix,
    HelpNoMatches,
    HelpSlashCommands,
    HelpKeybindings,
    HelpFooterTypeFilter,
    HelpFooterMove,
    HelpFooterJump,
    HelpFooterClose,
    CmdAgentDescription,
    CmdAttachDescription,
    CmdCacheDescription,
    CmdClearDescription,
    CmdCompactDescription,
    CmdConfigDescription,
    CmdContextDescription,
    CmdCostDescription,
    CmdCycleDescription,
    CmdCyclesDescription,
    CmdExitDescription,
    CmdExportDescription,
    CmdHelpDescription,
    CmdHomeDescription,
    CmdInitDescription,
    CmdJobsDescription,
    CmdLinksDescription,
    CmdLoadDescription,
    CmdLogoutDescription,
    CmdMcpDescription,
    CmdModelDescription,
    CmdModelsDescription,
    CmdNoteDescription,
    CmdPlanDescription,
    CmdProviderDescription,
    CmdQueueDescription,
    CmdRecallDescription,
    CmdRestoreDescription,
    CmdRetryDescription,
    CmdReviewDescription,
    CmdRlmDescription,
    CmdSaveDescription,
    CmdSessionsDescription,
    CmdSettingsDescription,
    CmdSkillDescription,
    CmdSkillsDescription,
    CmdStatuslineDescription,
    CmdSubagentsDescription,
    CmdSystemDescription,
    CmdTaskDescription,
    CmdTokensDescription,
    CmdTrustDescription,
    CmdUndoDescription,
    CmdYoloDescription,
}

#[allow(dead_code)]
pub const ALL_MESSAGE_IDS: &[MessageId] = &[
    MessageId::ComposerPlaceholder,
    MessageId::HistorySearchPlaceholder,
    MessageId::HistorySearchTitle,
    MessageId::HistoryHintMove,
    MessageId::HistoryHintAccept,
    MessageId::HistoryHintRestore,
    MessageId::HistoryNoMatches,
    MessageId::ConfigTitle,
    MessageId::ConfigModalTitle,
    MessageId::ConfigSearchPlaceholder,
    MessageId::ConfigNoSettings,
    MessageId::ConfigNoMatchesPrefix,
    MessageId::ConfigFilteredSettings,
    MessageId::ConfigShowing,
    MessageId::ConfigFooterDefault,
    MessageId::ConfigFooterScrollable,
    MessageId::ConfigFooterFiltered,
    MessageId::HelpTitle,
    MessageId::HelpFilterPlaceholder,
    MessageId::HelpFilterPrefix,
    MessageId::HelpNoMatches,
    MessageId::HelpSlashCommands,
    MessageId::HelpKeybindings,
    MessageId::HelpFooterTypeFilter,
    MessageId::HelpFooterMove,
    MessageId::HelpFooterJump,
    MessageId::HelpFooterClose,
    MessageId::CmdAgentDescription,
    MessageId::CmdAttachDescription,
    MessageId::CmdCacheDescription,
    MessageId::CmdClearDescription,
    MessageId::CmdCompactDescription,
    MessageId::CmdConfigDescription,
    MessageId::CmdContextDescription,
    MessageId::CmdCostDescription,
    MessageId::CmdCycleDescription,
    MessageId::CmdCyclesDescription,
    MessageId::CmdExitDescription,
    MessageId::CmdExportDescription,
    MessageId::CmdHelpDescription,
    MessageId::CmdHomeDescription,
    MessageId::CmdInitDescription,
    MessageId::CmdJobsDescription,
    MessageId::CmdLinksDescription,
    MessageId::CmdLoadDescription,
    MessageId::CmdLogoutDescription,
    MessageId::CmdMcpDescription,
    MessageId::CmdModelDescription,
    MessageId::CmdModelsDescription,
    MessageId::CmdNoteDescription,
    MessageId::CmdPlanDescription,
    MessageId::CmdProviderDescription,
    MessageId::CmdQueueDescription,
    MessageId::CmdRecallDescription,
    MessageId::CmdRestoreDescription,
    MessageId::CmdRetryDescription,
    MessageId::CmdReviewDescription,
    MessageId::CmdRlmDescription,
    MessageId::CmdSaveDescription,
    MessageId::CmdSessionsDescription,
    MessageId::CmdSettingsDescription,
    MessageId::CmdSkillDescription,
    MessageId::CmdSkillsDescription,
    MessageId::CmdStatuslineDescription,
    MessageId::CmdSubagentsDescription,
    MessageId::CmdSystemDescription,
    MessageId::CmdTaskDescription,
    MessageId::CmdTokensDescription,
    MessageId::CmdTrustDescription,
    MessageId::CmdUndoDescription,
    MessageId::CmdYoloDescription,
];

pub fn tr(locale: Locale, id: MessageId) -> &'static str {
    fallback_translation(translation(locale, id), id)
}

#[allow(dead_code)]
pub fn missing_message_ids(locale: Locale) -> Vec<MessageId> {
    ALL_MESSAGE_IDS
        .iter()
        .copied()
        .filter(|id| translation(locale, *id).is_none())
        .collect()
}

pub fn normalize_configured_locale(input: &str) -> Option<&'static str> {
    let normalized = normalize_locale_input(input);
    if matches!(normalized.as_str(), "" | "auto" | "system") {
        return Some("auto");
    }
    parse_locale(&normalized).map(Locale::tag)
}

pub fn resolve_locale(setting: &str) -> Locale {
    resolve_locale_with_env(setting, |key| std::env::var(key).ok())
}

pub fn resolve_locale_with_env<F>(setting: &str, env: F) -> Locale
where
    F: Fn(&str) -> Option<String>,
{
    let normalized = normalize_locale_input(setting);
    if !matches!(normalized.as_str(), "" | "auto" | "system") {
        return parse_locale(&normalized).unwrap_or(Locale::En);
    }

    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(value) = env(key)
            && let Some(locale) = parse_locale(&normalize_locale_input(&value))
        {
            return locale;
        }
    }

    Locale::En
}

#[allow(dead_code)]
pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text.width() <= max_width {
        return text.to_string();
    }

    let ellipsis_width = '…'.width().unwrap_or(1);
    if max_width <= ellipsis_width {
        return "…".to_string();
    }

    let limit = max_width - ellipsis_width;
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > limit {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push('…');
    out
}

fn normalize_locale_input(input: &str) -> String {
    input
        .split('.')
        .next()
        .unwrap_or(input)
        .split('@')
        .next()
        .unwrap_or(input)
        .trim()
        .replace('_', "-")
        .to_lowercase()
}

fn parse_locale(value: &str) -> Option<Locale> {
    if value == "c" || value == "posix" || value.starts_with("en") {
        return Some(Locale::En);
    }
    if value.starts_with("ja") {
        return Some(Locale::Ja);
    }
    if value.starts_with("zh") {
        if value.contains("hant")
            || value.contains("-tw")
            || value.contains("-hk")
            || value.contains("-mo")
        {
            return None;
        }
        return Some(Locale::ZhHans);
    }
    if value.starts_with("pt") || value == "br" {
        return Some(Locale::PtBr);
    }
    None
}

fn fallback_translation(candidate: Option<&'static str>, id: MessageId) -> &'static str {
    candidate.unwrap_or_else(|| english(id))
}

fn english(id: MessageId) -> &'static str {
    match id {
        MessageId::ComposerPlaceholder => "Write a task or use /.",
        MessageId::HistorySearchPlaceholder => "Search prompt history...",
        MessageId::HistorySearchTitle => "History Search",
        MessageId::HistoryHintMove => "Up/Down move",
        MessageId::HistoryHintAccept => "Enter accept",
        MessageId::HistoryHintRestore => "Esc restore",
        MessageId::HistoryNoMatches => "  No matches",
        MessageId::ConfigTitle => "Session Configuration",
        MessageId::ConfigModalTitle => " Config ",
        MessageId::ConfigSearchPlaceholder => "type to filter",
        MessageId::ConfigNoSettings => "  No settings available.",
        MessageId::ConfigNoMatchesPrefix => "  No settings match ",
        MessageId::ConfigFilteredSettings => "  Filtered settings",
        MessageId::ConfigShowing => "  Showing",
        MessageId::ConfigFooterDefault => {
            " type=filter, Up/Down=select, Enter/e=edit, Esc/q=close "
        }
        MessageId::ConfigFooterScrollable => {
            " type=filter, Up/Down=select, Enter/e=edit, PgUp/PgDn=scroll, Esc/q=close "
        }
        MessageId::ConfigFooterFiltered => {
            " type=filter, Backspace=delete, Ctrl+U/Esc=clear, Enter=edit "
        }
        MessageId::HelpTitle => "Help",
        MessageId::HelpFilterPlaceholder => "Type to filter",
        MessageId::HelpFilterPrefix => "Filter: ",
        MessageId::HelpNoMatches => "  No matches.",
        MessageId::HelpSlashCommands => "Slash commands",
        MessageId::HelpKeybindings => "Keybindings",
        MessageId::HelpFooterTypeFilter => " type to filter ",
        MessageId::HelpFooterMove => "  Up/Down move ",
        MessageId::HelpFooterJump => " PgUp/PgDn jump ",
        MessageId::HelpFooterClose => " Esc close ",
        MessageId::CmdAgentDescription => "Switch to agent mode",
        MessageId::CmdAttachDescription => {
            "Attach image/video media; use @path for text files or directories"
        }
        MessageId::CmdCacheDescription => {
            "Show DeepSeek prefix-cache hit/miss stats for the last N turns"
        }
        MessageId::CmdClearDescription => "Clear conversation history",
        MessageId::CmdCompactDescription => {
            "Trigger context compaction to free up space (legacy; v0.6.6 prefers cycle restart)"
        }
        MessageId::CmdConfigDescription => "Open interactive configuration editor",
        MessageId::CmdContextDescription => "Open compact session context inspector",
        MessageId::CmdCostDescription => "Show session cost breakdown",
        MessageId::CmdCycleDescription => "Show the carry-forward briefing for a specific cycle",
        MessageId::CmdCyclesDescription => "List checkpoint-restart cycle handoffs in this session",
        MessageId::CmdExitDescription => "Exit the application",
        MessageId::CmdExportDescription => "Export conversation to markdown",
        MessageId::CmdHelpDescription => "Show help information",
        MessageId::CmdHomeDescription => "Show home dashboard with stats and quick actions",
        MessageId::CmdInitDescription => "Generate AGENTS.md for project",
        MessageId::CmdJobsDescription => "Inspect and control background shell jobs",
        MessageId::CmdLinksDescription => "Show DeepSeek dashboard and docs links",
        MessageId::CmdLoadDescription => "Load session from file",
        MessageId::CmdLogoutDescription => "Clear API key and return to setup",
        MessageId::CmdMcpDescription => "Open or manage MCP servers",
        MessageId::CmdModelDescription => "Switch or view current model",
        MessageId::CmdModelsDescription => "List available models from API",
        MessageId::CmdNoteDescription => {
            "Append note to persistent notes file (.deepseek/notes.md)"
        }
        MessageId::CmdPlanDescription => {
            "Switch to plan mode and review suggested implementation steps"
        }
        MessageId::CmdProviderDescription => {
            "Switch or view the active LLM backend (deepseek | nvidia-nim)"
        }
        MessageId::CmdQueueDescription => "View or edit queued messages",
        MessageId::CmdRecallDescription => "Search prior cycle archives (BM25 over message text)",
        MessageId::CmdRestoreDescription => {
            "Roll back the workspace to a prior pre/post-turn snapshot. With no arg, lists recent snapshots."
        }
        MessageId::CmdRetryDescription => "Retry the last request",
        MessageId::CmdReviewDescription => "Run a structured code review on a file, diff, or PR",
        MessageId::CmdRlmDescription => {
            "Recursive Language Model (RLM) turn — store the prompt in a Python REPL and let the model write code to process it, with `llm_query()` / `sub_rlm()` for sub-LLM calls."
        }
        MessageId::CmdSaveDescription => "Save session to file",
        MessageId::CmdSessionsDescription => "Open session picker",
        MessageId::CmdSettingsDescription => "Show persistent settings",
        MessageId::CmdSkillDescription => {
            "Activate a skill, or install/update/uninstall/trust a community skill"
        }
        MessageId::CmdSkillsDescription => {
            "List local skills (or --remote to browse the curated registry)"
        }
        MessageId::CmdStatuslineDescription => "Configure which items appear in the footer",
        MessageId::CmdSubagentsDescription => "List sub-agent status",
        MessageId::CmdSystemDescription => "Show current system prompt",
        MessageId::CmdTaskDescription => "Manage background tasks",
        MessageId::CmdTokensDescription => "Show token usage for session",
        MessageId::CmdTrustDescription => {
            "Manage workspace trust and per-path allowlist (`/trust add <path>`, `/trust list`, `/trust on|off`)"
        }
        MessageId::CmdUndoDescription => "Remove last message pair",
        MessageId::CmdYoloDescription => "Enable YOLO mode (shell + trust + auto-approve)",
    }
}

fn translation(locale: Locale, id: MessageId) -> Option<&'static str> {
    match locale {
        Locale::En => Some(english(id)),
        Locale::Ja => japanese(id),
        Locale::ZhHans => chinese_simplified(id),
        Locale::PtBr => portuguese_brazil(id),
    }
}

fn japanese(id: MessageId) -> Option<&'static str> {
    Some(match id {
        MessageId::ComposerPlaceholder => "タスクを書くか / を使う。",
        MessageId::HistorySearchPlaceholder => "プロンプト履歴を検索...",
        MessageId::HistorySearchTitle => "履歴検索",
        MessageId::HistoryHintMove => "Up/Down 移動",
        MessageId::HistoryHintAccept => "Enter 確定",
        MessageId::HistoryHintRestore => "Esc 復元",
        MessageId::HistoryNoMatches => "  一致なし",
        MessageId::ConfigTitle => "セッション設定",
        MessageId::ConfigModalTitle => " 設定 ",
        MessageId::ConfigSearchPlaceholder => "入力して絞り込み",
        MessageId::ConfigNoSettings => "  設定がありません。",
        MessageId::ConfigNoMatchesPrefix => "  一致する設定なし: ",
        MessageId::ConfigFilteredSettings => "  絞り込み後の設定",
        MessageId::ConfigShowing => "  表示",
        MessageId::ConfigFooterDefault => {
            " 入力=絞り込み, Up/Down=選択, Enter/e=編集, Esc/q=閉じる "
        }
        MessageId::ConfigFooterScrollable => {
            " 入力=絞り込み, Up/Down=選択, Enter/e=編集, PgUp/PgDn=スクロール, Esc/q=閉じる "
        }
        MessageId::ConfigFooterFiltered => {
            " 入力=絞り込み, Backspace=削除, Ctrl+U/Esc=クリア, Enter=編集 "
        }
        MessageId::HelpTitle => "ヘルプ",
        MessageId::HelpFilterPlaceholder => "入力して絞り込み",
        MessageId::HelpFilterPrefix => "絞り込み: ",
        MessageId::HelpNoMatches => "  一致なし。",
        MessageId::HelpSlashCommands => "スラッシュコマンド",
        MessageId::HelpKeybindings => "キー操作",
        MessageId::HelpFooterTypeFilter => " 入力して絞り込み ",
        MessageId::HelpFooterMove => "  Up/Down 移動 ",
        MessageId::HelpFooterJump => " PgUp/PgDn ジャンプ ",
        MessageId::HelpFooterClose => " Esc 閉じる ",
        MessageId::CmdAgentDescription => "Agent モードに切り替え",
        MessageId::CmdAttachDescription => {
            "画像・動画メディアを添付（テキストファイルやディレクトリは @path）"
        }
        MessageId::CmdCacheDescription => {
            "直近 N ターンの DeepSeek プレフィックスキャッシュのヒット/ミス統計を表示"
        }
        MessageId::CmdClearDescription => "会話履歴をクリア",
        MessageId::CmdCompactDescription => {
            "コンテキスト圧縮で容量を確保（旧式：v0.6.6 以降はサイクル再起動を推奨）"
        }
        MessageId::CmdConfigDescription => "インタラクティブな設定エディタを開く",
        MessageId::CmdContextDescription => "コンパクトなセッションコンテキスト検査ツールを開く",
        MessageId::CmdCostDescription => "セッションのコスト内訳を表示",
        MessageId::CmdCycleDescription => "指定したサイクルの引き継ぎブリーフィングを表示",
        MessageId::CmdCyclesDescription => {
            "セッション内のチェックポイント再起動サイクルの引き継ぎを一覧表示"
        }
        MessageId::CmdExitDescription => "アプリを終了",
        MessageId::CmdExportDescription => "会話を Markdown にエクスポート",
        MessageId::CmdHelpDescription => "ヘルプを表示",
        MessageId::CmdHomeDescription => "統計とクイックアクション付きのホームダッシュボードを表示",
        MessageId::CmdInitDescription => "プロジェクト用に AGENTS.md を生成",
        MessageId::CmdJobsDescription => "バックグラウンドのシェルジョブを確認・制御",
        MessageId::CmdLinksDescription => "DeepSeek ダッシュボードとドキュメントへのリンクを表示",
        MessageId::CmdLoadDescription => "ファイルからセッションを読み込み",
        MessageId::CmdLogoutDescription => "API キーを消去してセットアップに戻る",
        MessageId::CmdMcpDescription => "MCP サーバを開く・管理する",
        MessageId::CmdModelDescription => "現在のモデルを切り替え・確認",
        MessageId::CmdModelsDescription => "API から利用可能なモデルを一覧表示",
        MessageId::CmdNoteDescription => "永続ノートファイル（.deepseek/notes.md）に追記",
        MessageId::CmdPlanDescription => "Plan モードに切り替え、推奨される実装手順を確認",
        MessageId::CmdProviderDescription => {
            "現在の LLM バックエンドを切り替え・確認（deepseek | nvidia-nim）"
        }
        MessageId::CmdQueueDescription => "キューされたメッセージを確認・編集",
        MessageId::CmdRecallDescription => {
            "過去のサイクルアーカイブを検索（メッセージ本文への BM25 検索）"
        }
        MessageId::CmdRestoreDescription => {
            "ワークスペースを以前のターン前/後スナップショットへロールバック。引数なしで最近のスナップショットを一覧表示。"
        }
        MessageId::CmdRetryDescription => "直前のリクエストを再試行",
        MessageId::CmdReviewDescription => "ファイル・diff・PR に対して構造化コードレビューを実行",
        MessageId::CmdRlmDescription => {
            "再帰言語モデル（RLM）ターン — プロンプトを Python REPL に格納し、モデルが処理コードを記述。サブ LLM 呼び出しは `llm_query()` / `sub_rlm()`。"
        }
        MessageId::CmdSaveDescription => "セッションをファイルに保存",
        MessageId::CmdSessionsDescription => "セッションピッカーを開く",
        MessageId::CmdSettingsDescription => "永続化された設定を表示",
        MessageId::CmdSkillDescription => {
            "スキルを有効化、またはコミュニティスキルをインストール／更新／アンインストール／信頼"
        }
        MessageId::CmdSkillsDescription => {
            "ローカルスキルを一覧表示（--remote で精選レジストリを参照）"
        }
        MessageId::CmdStatuslineDescription => "フッターに表示する項目を設定",
        MessageId::CmdSubagentsDescription => "サブエージェントの状態を一覧表示",
        MessageId::CmdSystemDescription => "現在のシステムプロンプトを表示",
        MessageId::CmdTaskDescription => "バックグラウンドタスクを管理",
        MessageId::CmdTokensDescription => "セッションのトークン使用量を表示",
        MessageId::CmdTrustDescription => {
            "ワークスペースの信頼設定とパス別許可リストを管理（`/trust add <path>`、`/trust list`、`/trust on|off`）"
        }
        MessageId::CmdUndoDescription => "最後のメッセージ対を削除",
        MessageId::CmdYoloDescription => "YOLO モードを有効化（shell + 信頼 + 自動承認）",
    })
}

fn chinese_simplified(id: MessageId) -> Option<&'static str> {
    Some(match id {
        MessageId::ComposerPlaceholder => "编写任务或使用 /。",
        MessageId::HistorySearchPlaceholder => "搜索提示历史...",
        MessageId::HistorySearchTitle => "历史搜索",
        MessageId::HistoryHintMove => "Up/Down 移动",
        MessageId::HistoryHintAccept => "Enter 接受",
        MessageId::HistoryHintRestore => "Esc 还原",
        MessageId::HistoryNoMatches => "  无匹配",
        MessageId::ConfigTitle => "会话配置",
        MessageId::ConfigModalTitle => " 配置 ",
        MessageId::ConfigSearchPlaceholder => "输入以筛选",
        MessageId::ConfigNoSettings => "  没有可用设置。",
        MessageId::ConfigNoMatchesPrefix => "  没有匹配设置: ",
        MessageId::ConfigFilteredSettings => "  已筛选设置",
        MessageId::ConfigShowing => "  显示",
        MessageId::ConfigFooterDefault => " 输入=筛选, Up/Down=选择, Enter/e=编辑, Esc/q=关闭 ",
        MessageId::ConfigFooterScrollable => {
            " 输入=筛选, Up/Down=选择, Enter/e=编辑, PgUp/PgDn=滚动, Esc/q=关闭 "
        }
        MessageId::ConfigFooterFiltered => {
            " 输入=筛选, Backspace=删除, Ctrl+U/Esc=清除, Enter=编辑 "
        }
        MessageId::HelpTitle => "帮助",
        MessageId::HelpFilterPlaceholder => "输入以筛选",
        MessageId::HelpFilterPrefix => "筛选: ",
        MessageId::HelpNoMatches => "  无匹配。",
        MessageId::HelpSlashCommands => "斜杠命令",
        MessageId::HelpKeybindings => "快捷键",
        MessageId::HelpFooterTypeFilter => " 输入以筛选 ",
        MessageId::HelpFooterMove => "  Up/Down 移动 ",
        MessageId::HelpFooterJump => " PgUp/PgDn 跳转 ",
        MessageId::HelpFooterClose => " Esc 关闭 ",
        MessageId::CmdAgentDescription => "切换到 Agent 模式",
        MessageId::CmdAttachDescription => "附加图片或视频媒体；文本文件或目录请使用 @path",
        MessageId::CmdCacheDescription => "显示最近 N 轮的 DeepSeek 前缀缓存命中/未命中统计",
        MessageId::CmdClearDescription => "清除对话历史",
        MessageId::CmdCompactDescription => {
            "触发上下文压缩以释放空间（旧版命令；v0.6.6 起建议改用循环重启）"
        }
        MessageId::CmdConfigDescription => "打开交互式配置编辑器",
        MessageId::CmdContextDescription => "打开紧凑会话上下文检查器",
        MessageId::CmdCostDescription => "显示本次会话的费用明细",
        MessageId::CmdCycleDescription => "显示指定循环的延续简报",
        MessageId::CmdCyclesDescription => "列出本次会话中的检查点重启循环交接",
        MessageId::CmdExitDescription => "退出应用",
        MessageId::CmdExportDescription => "将对话导出为 Markdown",
        MessageId::CmdHelpDescription => "显示帮助信息",
        MessageId::CmdHomeDescription => "显示主页面板，含统计与快捷操作",
        MessageId::CmdInitDescription => "为项目生成 AGENTS.md",
        MessageId::CmdJobsDescription => "查看并管理后台 shell 作业",
        MessageId::CmdLinksDescription => "显示 DeepSeek 控制台与文档链接",
        MessageId::CmdLoadDescription => "从文件加载会话",
        MessageId::CmdLogoutDescription => "清除 API 密钥并返回设置",
        MessageId::CmdMcpDescription => "打开或管理 MCP 服务器",
        MessageId::CmdModelDescription => "切换或查看当前模型",
        MessageId::CmdModelsDescription => "列出 API 中可用的模型",
        MessageId::CmdNoteDescription => "将笔记追加到持久笔记文件（.deepseek/notes.md）",
        MessageId::CmdPlanDescription => "切换到 Plan 模式并查看建议的实现步骤",
        MessageId::CmdProviderDescription => "切换或查看当前 LLM 后端（deepseek | nvidia-nim）",
        MessageId::CmdQueueDescription => "查看或编辑已排队的消息",
        MessageId::CmdRecallDescription => "搜索此前的循环归档（基于消息文本的 BM25 检索）",
        MessageId::CmdRestoreDescription => {
            "将工作区回滚到此前的轮次前/后快照。不带参数时列出最近的快照。"
        }
        MessageId::CmdRetryDescription => "重试上一次请求",
        MessageId::CmdReviewDescription => "对文件、diff 或 PR 进行结构化代码审查",
        MessageId::CmdRlmDescription => {
            "递归语言模型（RLM）轮次 —— 将提示词存入 Python REPL，让模型编写代码进行处理；可用 `llm_query()` / `sub_rlm()` 调用子 LLM。"
        }
        MessageId::CmdSaveDescription => "将会话保存到文件",
        MessageId::CmdSessionsDescription => "打开会话选择器",
        MessageId::CmdSettingsDescription => "显示持久化设置",
        MessageId::CmdSkillDescription => "激活技能，或安装/更新/卸载/信任社区技能",
        MessageId::CmdSkillsDescription => "列出本地技能（或使用 --remote 浏览精选注册表）",
        MessageId::CmdStatuslineDescription => "配置底栏要显示哪些条目",
        MessageId::CmdSubagentsDescription => "列出子代理状态",
        MessageId::CmdSystemDescription => "显示当前系统提示词",
        MessageId::CmdTaskDescription => "管理后台任务",
        MessageId::CmdTokensDescription => "显示本次会话的 token 用量",
        MessageId::CmdTrustDescription => {
            "管理工作区信任与按路径的白名单（`/trust add <path>`、`/trust list`、`/trust on|off`）"
        }
        MessageId::CmdUndoDescription => "移除最后一组消息对",
        MessageId::CmdYoloDescription => "启用 YOLO 模式（shell + 信任 + 自动批准）",
    })
}

fn portuguese_brazil(id: MessageId) -> Option<&'static str> {
    Some(match id {
        MessageId::ComposerPlaceholder => "Escreva uma tarefa ou use /.",
        MessageId::HistorySearchPlaceholder => "Pesquisar histórico de prompts...",
        MessageId::HistorySearchTitle => "Busca no histórico",
        MessageId::HistoryHintMove => "Up/Down move",
        MessageId::HistoryHintAccept => "Enter aceita",
        MessageId::HistoryHintRestore => "Esc restaura",
        MessageId::HistoryNoMatches => "  Sem resultados",
        MessageId::ConfigTitle => "Configuração da sessão",
        MessageId::ConfigModalTitle => " Config ",
        MessageId::ConfigSearchPlaceholder => "digite para filtrar",
        MessageId::ConfigNoSettings => "  Nenhuma configuração disponível.",
        MessageId::ConfigNoMatchesPrefix => "  Nenhuma configuração corresponde a ",
        MessageId::ConfigFilteredSettings => "  Configurações filtradas",
        MessageId::ConfigShowing => "  Mostrando",
        MessageId::ConfigFooterDefault => {
            " digite=filtrar, Up/Down=selecionar, Enter/e=editar, Esc/q=fechar "
        }
        MessageId::ConfigFooterScrollable => {
            " digite=filtrar, Up/Down=selecionar, Enter/e=editar, PgUp/PgDn=rolar, Esc/q=fechar "
        }
        MessageId::ConfigFooterFiltered => {
            " digite=filtrar, Backspace=apagar, Ctrl+U/Esc=limpar, Enter=editar "
        }
        MessageId::HelpTitle => "Ajuda",
        MessageId::HelpFilterPlaceholder => "Digite para filtrar",
        MessageId::HelpFilterPrefix => "Filtro: ",
        MessageId::HelpNoMatches => "  Sem resultados.",
        MessageId::HelpSlashCommands => "Comandos com barra",
        MessageId::HelpKeybindings => "Atalhos",
        MessageId::HelpFooterTypeFilter => " digite para filtrar ",
        MessageId::HelpFooterMove => "  Up/Down move ",
        MessageId::HelpFooterJump => " PgUp/PgDn salta ",
        MessageId::HelpFooterClose => " Esc fecha ",
        MessageId::CmdAgentDescription => "Mudar para o modo agent",
        MessageId::CmdAttachDescription => {
            "Anexar imagem ou vídeo; use @path para arquivos de texto ou diretórios"
        }
        MessageId::CmdCacheDescription => {
            "Exibir estatísticas de hit/miss do cache de prefixo DeepSeek nas últimas N rodadas"
        }
        MessageId::CmdClearDescription => "Limpar o histórico da conversa",
        MessageId::CmdCompactDescription => {
            "Compactar o contexto para liberar espaço (legado; a v0.6.6 prefere o reinício de ciclo)"
        }
        MessageId::CmdConfigDescription => "Abrir o editor interativo de configuração",
        MessageId::CmdContextDescription => "Abrir o inspetor compacto de contexto da sessão",
        MessageId::CmdCostDescription => "Exibir o detalhamento de custo da sessão",
        MessageId::CmdCycleDescription => {
            "Exibir o briefing de continuidade de um ciclo específico"
        }
        MessageId::CmdCyclesDescription => {
            "Listar as transferências dos ciclos checkpoint-restart desta sessão"
        }
        MessageId::CmdExitDescription => "Sair do aplicativo",
        MessageId::CmdExportDescription => "Exportar a conversa para markdown",
        MessageId::CmdHelpDescription => "Exibir informações de ajuda",
        MessageId::CmdHomeDescription => "Exibir o painel inicial com estatísticas e ações rápidas",
        MessageId::CmdInitDescription => "Gerar AGENTS.md para o projeto",
        MessageId::CmdJobsDescription => "Inspecionar e controlar jobs de shell em segundo plano",
        MessageId::CmdLinksDescription => "Exibir links do painel e da documentação do DeepSeek",
        MessageId::CmdLoadDescription => "Carregar a sessão de um arquivo",
        MessageId::CmdLogoutDescription => "Limpar a chave de API e voltar à configuração",
        MessageId::CmdMcpDescription => "Abrir ou gerenciar servidores MCP",
        MessageId::CmdModelDescription => "Trocar ou exibir o modelo atual",
        MessageId::CmdModelsDescription => "Listar os modelos disponíveis pela API",
        MessageId::CmdNoteDescription => {
            "Adicionar nota ao arquivo persistente (.deepseek/notes.md)"
        }
        MessageId::CmdPlanDescription => {
            "Mudar para o modo plan e revisar os passos de implementação sugeridos"
        }
        MessageId::CmdProviderDescription => {
            "Trocar ou exibir o backend LLM ativo (deepseek | nvidia-nim)"
        }
        MessageId::CmdQueueDescription => "Ver ou editar mensagens enfileiradas",
        MessageId::CmdRecallDescription => {
            "Buscar arquivos de ciclos anteriores (BM25 sobre o texto das mensagens)"
        }
        MessageId::CmdRestoreDescription => {
            "Reverter o workspace a um snapshot pré/pós-turno anterior. Sem argumento, lista os snapshots recentes."
        }
        MessageId::CmdRetryDescription => "Repetir a última requisição",
        MessageId::CmdReviewDescription => {
            "Executar uma revisão de código estruturada em um arquivo, diff ou PR"
        }
        MessageId::CmdRlmDescription => {
            "Turno do Recursive Language Model (RLM) — guarda o prompt em um REPL Python e deixa o modelo escrever o código que o processa; use `llm_query()` / `sub_rlm()` para chamadas a sub-LLMs."
        }
        MessageId::CmdSaveDescription => "Salvar a sessão em arquivo",
        MessageId::CmdSessionsDescription => "Abrir o seletor de sessões",
        MessageId::CmdSettingsDescription => "Exibir as configurações persistidas",
        MessageId::CmdSkillDescription => {
            "Ativar uma skill, ou instalar/atualizar/desinstalar/confiar em uma skill da comunidade"
        }
        MessageId::CmdSkillsDescription => {
            "Listar skills locais (ou --remote para navegar pelo registro curado)"
        }
        MessageId::CmdStatuslineDescription => "Configurar quais itens aparecem no rodapé",
        MessageId::CmdSubagentsDescription => "Listar o status dos sub-agentes",
        MessageId::CmdSystemDescription => "Exibir o prompt de sistema atual",
        MessageId::CmdTaskDescription => "Gerenciar tarefas em segundo plano",
        MessageId::CmdTokensDescription => "Exibir o uso de tokens da sessão",
        MessageId::CmdTrustDescription => {
            "Gerenciar a confiança do workspace e a allowlist por caminho (`/trust add <path>`, `/trust list`, `/trust on|off`)"
        }
        MessageId::CmdUndoDescription => "Remover o último par de mensagens",
        MessageId::CmdYoloDescription => {
            "Ativar o modo YOLO (shell + confiança + aprovação automática)"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget, Wrap},
    };

    #[test]
    fn locale_setting_normalizes_supported_tags() {
        assert_eq!(normalize_configured_locale("auto"), Some("auto"));
        assert_eq!(normalize_configured_locale("ja_JP.UTF-8"), Some("ja"));
        assert_eq!(normalize_configured_locale("zh-CN"), Some("zh-Hans"));
        assert_eq!(normalize_configured_locale("pt"), Some("pt-BR"));
        assert_eq!(normalize_configured_locale("pt-PT"), Some("pt-BR"));
        assert_eq!(normalize_configured_locale("zh-TW"), None);
    }

    #[test]
    fn locale_resolution_uses_config_then_environment_then_english() {
        assert_eq!(
            resolve_locale_with_env("ja", |_| Some("pt_BR.UTF-8".to_string())),
            Locale::Ja
        );
        assert_eq!(
            resolve_locale_with_env("auto", |key| {
                (key == "LANG").then(|| "zh_CN.UTF-8".to_string())
            }),
            Locale::ZhHans
        );
        assert_eq!(resolve_locale_with_env("auto", |_| None), Locale::En);
    }

    #[test]
    fn shipped_first_pack_has_no_missing_core_messages() {
        for locale in Locale::shipped() {
            assert!(
                missing_message_ids(*locale).is_empty(),
                "{} is missing messages",
                locale.tag()
            );
        }
    }

    #[test]
    fn unsupported_locale_falls_back_to_english() {
        assert_eq!(
            resolve_locale_with_env("ar", |_| None),
            Locale::En,
            "Arabic is planned for QA but not shipped in the v0.7.6 core pack"
        );
    }

    #[test]
    fn missing_translation_falls_back_to_english() {
        assert_eq!(
            fallback_translation(None, MessageId::ComposerPlaceholder),
            english(MessageId::ComposerPlaceholder)
        );
    }

    #[test]
    fn width_truncation_handles_cjk_rtl_indic_and_latin_samples() {
        let samples = [
            ("zh-Hans", "输入以筛选配置"),
            ("ar", "تصفية الإعدادات"),
            ("hi", "सेटिंग खोजें"),
            ("pt-BR", "configurações filtradas"),
        ];

        for (tag, sample) in samples {
            let truncated = truncate_to_width(sample, 12);
            assert!(
                truncated.width() <= 12,
                "{tag} sample overflowed: {truncated:?}"
            );
        }
    }

    #[test]
    fn planned_script_samples_render_in_narrow_terminal_buffer() {
        let samples = [
            ("CJK", "输入以筛选配置"),
            ("RTL", "تصفية الإعدادات"),
            ("Indic", "सेटिंग खोजें"),
            ("Latin Global South", "configurações filtradas"),
        ];

        for (label, sample) in samples {
            let area = Rect::new(0, 0, 18, 4);
            let mut buf = Buffer::empty(area);
            Paragraph::new(sample)
                .wrap(Wrap { trim: false })
                .render(area, &mut buf);
            let dump = buffer_text(&buf, area);

            assert!(
                dump.chars().any(|ch| !ch.is_whitespace()),
                "{label} sample produced an empty render"
            );
        }
    }

    fn buffer_text(buf: &Buffer, area: Rect) -> String {
        let mut out = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }
}
