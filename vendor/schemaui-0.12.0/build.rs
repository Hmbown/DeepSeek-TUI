use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use unicode_width::UnicodeWidthStr;

const SHORTCUTS_BEGIN: &str = "<!-- AUTO-GENERATED:SHORTCUTS:BEGIN -->";
const SHORTCUTS_END: &str = "<!-- AUTO-GENERATED:SHORTCUTS:END -->";

const CONTEXT_ORDER: [&str; 6] = [
    "default",
    "collection",
    "overlay",
    "help",
    "text",
    "numeric",
];

fn default_dispatch() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct KeymapDocEntry {
    id: String,
    description: String,
    #[serde(rename = "descriptionZh")]
    description_zh: Option<String>,
    contexts: Vec<String>,
    #[serde(default = "default_dispatch")]
    dispatch: bool,
    combos: Vec<String>,
}

#[derive(Clone, Copy)]
enum Locale {
    En,
    Zh,
}

impl Locale {
    fn table_headers(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Locale::En => ("Shortcut", "Action", "Kind"),
            Locale::Zh => ("快捷键", "动作", "类型"),
        }
    }

    fn context_heading(self, context: &str) -> &'static str {
        match (self, context) {
            (Locale::En, "default") => "Default context",
            (Locale::En, "collection") => "Collection context",
            (Locale::En, "overlay") => "Overlay context",
            (Locale::En, "help") => "Help context",
            (Locale::En, "text") => "Text field context",
            (Locale::En, "numeric") => "Numeric field context",
            (Locale::Zh, "default") => "默认上下文",
            (Locale::Zh, "collection") => "集合上下文",
            (Locale::Zh, "overlay") => "覆盖层上下文",
            (Locale::Zh, "help") => "帮助上下文",
            (Locale::Zh, "text") => "文本字段上下文",
            (Locale::Zh, "numeric") => "数值字段上下文",
            _ => "Unknown context",
        }
    }

    fn description(self, entry: &KeymapDocEntry) -> &str {
        match self {
            Locale::En => &entry.description,
            Locale::Zh => entry
                .description_zh
                .as_deref()
                .expect("missing descriptionZh while rendering zh docs"),
        }
    }

    fn kind(self, dispatch: bool) -> &'static str {
        match (self, dispatch) {
            (Locale::En, true) => "command",
            (Locale::En, false) => "local edit",
            (Locale::Zh, true) => "命令",
            (Locale::Zh, false) => "局部编辑",
        }
    }
}

fn main() {
    for path in [
        "build.rs",
        "README.md",
        "README.ZH.md",
        "keymap/default.keymap.json",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is missing"));
    if let Err(error) = sync_shortcut_docs(&manifest_dir) {
        panic!("failed to sync README shortcut docs: {error}");
    }
}

fn sync_shortcut_docs(root: &Path) -> Result<(), String> {
    let keymap_path = root.join("keymap/default.keymap.json");
    let keymap_text =
        fs::read_to_string(&keymap_path).map_err(|err| format!("read {keymap_path:?}: {err}"))?;
    let entries: Vec<KeymapDocEntry> =
        serde_json::from_str(&keymap_text).map_err(|err| format!("parse keymap JSON: {err}"))?;

    validate_entries(&entries)?;

    update_marked_block(
        &root.join("README.md"),
        &render_shortcut_block(&entries, Locale::En),
    )?;
    update_marked_block(
        &root.join("README.ZH.md"),
        &render_shortcut_block(&entries, Locale::Zh),
    )?;

    Ok(())
}

fn validate_entries(entries: &[KeymapDocEntry]) -> Result<(), String> {
    for entry in entries {
        if entry.combos.is_empty() {
            return Err(format!(
                "keymap entry {} must declare at least one combo",
                entry.id
            ));
        }
        if entry.contexts.is_empty() {
            return Err(format!(
                "keymap entry {} must declare at least one context",
                entry.id
            ));
        }
        if entry
            .description_zh
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Err(format!(
                "keymap entry {} must provide non-empty descriptionZh",
                entry.id
            ));
        }
        for context in &entry.contexts {
            if !CONTEXT_ORDER.contains(&context.as_str()) {
                return Err(format!(
                    "keymap entry {} uses unsupported context {}",
                    entry.id, context
                ));
            }
        }
    }

    Ok(())
}

fn render_shortcut_block(entries: &[KeymapDocEntry], locale: Locale) -> String {
    let (shortcut_header, action_header, kind_header) = locale.table_headers();
    let mut lines = Vec::new();

    for context in CONTEXT_ORDER {
        let context_entries = entries
            .iter()
            .filter(|entry| entry.contexts.iter().any(|item| item == context))
            .collect::<Vec<_>>();

        if context_entries.is_empty() {
            continue;
        }

        lines.push(format!("#### {}", locale.context_heading(context)));
        lines.push(String::new());
        let rows = context_entries
            .into_iter()
            .map(|entry| {
                vec![
                    entry
                        .combos
                        .iter()
                        .map(|combo| format!("`{}`", escape_markdown_cell(combo)))
                        .collect::<Vec<_>>()
                        .join(" / "),
                    escape_markdown_cell(locale.description(entry)),
                    format!("`{}`", locale.kind(entry.dispatch)),
                ]
            })
            .collect::<Vec<_>>();
        lines.extend(render_markdown_table(
            [shortcut_header, action_header, kind_header],
            &rows,
        ));

        if context != CONTEXT_ORDER[CONTEXT_ORDER.len() - 1] {
            lines.push(String::new());
        }
    }

    lines.join("\n").trim_end().to_string()
}

fn render_markdown_table<const N: usize>(headers: [&str; N], rows: &[Vec<String>]) -> Vec<String> {
    let mut widths = headers.map(UnicodeWidthStr::width);
    for row in rows {
        for (index, cell) in row.iter().enumerate().take(N) {
            widths[index] = widths[index].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }

    let header_line = render_markdown_row(headers.iter().copied(), &widths);
    let separator_line =
        render_markdown_row(widths.iter().map(|width| "-".repeat(*width)), &widths);
    let mut lines = vec![header_line, separator_line];
    for row in rows {
        lines.push(render_markdown_row(
            row.iter().take(N).map(String::as_str),
            &widths,
        ));
    }
    lines
}

fn render_markdown_row<I, S>(cells: I, widths: &[usize]) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut row = String::new();
    for (index, cell) in cells.into_iter().enumerate() {
        let cell = cell.as_ref();
        row.push('|');
        row.push(' ');
        row.push_str(cell);
        let padding = widths[index].saturating_sub(UnicodeWidthStr::width(cell));
        row.push_str(&" ".repeat(padding));
        row.push(' ');
    }
    row.push('|');
    row
}

fn escape_markdown_cell(input: &str) -> String {
    input.replace('|', "\\|")
}

fn update_marked_block(path: &Path, generated: &str) -> Result<(), String> {
    let original = fs::read_to_string(path).map_err(|err| format!("read {path:?}: {err}"))?;
    let begin_matches = original.match_indices(SHORTCUTS_BEGIN).collect::<Vec<_>>();
    let end_matches = original.match_indices(SHORTCUTS_END).collect::<Vec<_>>();

    if begin_matches.len() != 1 || end_matches.len() != 1 {
        return Err(format!(
            "{path:?} must contain exactly one {SHORTCUTS_BEGIN} marker and one {SHORTCUTS_END} marker"
        ));
    }

    let begin = begin_matches[0].0;
    let end = end_matches[0].0;

    if begin >= end {
        return Err(format!("invalid marker order in {path:?}"));
    }

    let prefix = &original[..begin + SHORTCUTS_BEGIN.len()];
    let suffix = &original[end..];
    let updated = format!("{prefix}\n\n{generated}\n\n{suffix}");

    if updated != original {
        fs::write(path, updated).map_err(|err| format!("write {path:?}: {err}"))?;
    }

    Ok(())
}
