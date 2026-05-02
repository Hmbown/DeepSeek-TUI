use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::model::{FieldKind, FieldSchema};
use crate::tui::state::field::convert::{NumericStepValue, adjust_numeric_value};

use super::{ComponentKind, palette::ComponentPalette};

pub(crate) fn handle_text_edit(
    buffer: &mut String,
    schema: &FieldSchema,
    key: &KeyEvent,
    palette: &ComponentPalette,
) -> bool {
    match key.code {
        KeyCode::Left | KeyCode::Right => {
            let fast = key.modifiers.contains(KeyModifiers::SHIFT);
            let sign = if matches!(key.code, KeyCode::Left) {
                -1.0
            } else {
                1.0
            };
            let delta = match schema.kind {
                FieldKind::Integer => {
                    let step = palette.numeric.step_i64(fast) as f64 * sign;
                    NumericStepValue::Integer(step.round() as i64)
                }
                FieldKind::Number => {
                    let step = palette.numeric.step_f64(fast) * sign;
                    NumericStepValue::Float(step)
                }
                _ => return false,
            };
            adjust_numeric_value(buffer, &schema.kind, delta)
        }
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return false;
            }
            buffer.push(ch);
            true
        }
        KeyCode::Backspace => {
            buffer.pop();
            true
        }
        KeyCode::Delete => {
            buffer.clear();
            true
        }
        _ => false,
    }
}

pub(crate) fn format_collection_value(
    label: &str,
    len: usize,
    selection: Option<String>,
    hint: &str,
) -> String {
    if len == 0 {
        format!("{label}: empty {hint}")
    } else {
        let selected = selection.unwrap_or_else(|| "<no selection>".to_string());
        format!("{label}[{len}] • {selected} {hint}")
    }
}

pub(crate) fn list_hint_for(kind: ComponentKind, palette: &ComponentPalette) -> String {
    match kind {
        ComponentKind::CompositeList | ComponentKind::KeyValue | ComponentKind::ScalarArray => {
            palette.collection.list_hint.to_string()
        }
        _ => String::new(),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EntryPanelState {
    pub entries: Vec<String>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct OverlayContext {
    pub title: Option<String>,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub entry_panel: Option<EntryPanelState>,
}

impl OverlayContext {
    pub fn new() -> Self {
        Self {
            title: None,
            description: None,
            instructions: None,
            entry_panel: None,
        }
    }
}
