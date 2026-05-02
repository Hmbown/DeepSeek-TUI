use std::sync::Arc;

use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::tui::model::CompositeField;
use crate::tui::state::field::components::{
    ComponentPalette, CompositePopupData, CompositeSelectorView,
};
use crate::tui::state::{error::FieldCoercionError, value_summary::truncate_visible_text};

use super::{CompositeEditorSession, CompositeState};

#[derive(Debug, Clone)]
pub struct CompositeListEditorContext {
    pub entry_index: usize,
    #[allow(dead_code)]
    pub entry_label: String,
    pub session: CompositeEditorSession,
}

#[derive(Debug, Clone)]
pub struct CompositeListState {
    pointer: String,
    template: CompositeField,
    entries: Vec<CompositeListEntry>,
    selected: usize,
    counter: usize,
    palette: Arc<ComponentPalette>,
}

#[derive(Debug, Clone)]
struct CompositeListEntry {
    pointer: String,
    state: CompositeState,
}

impl CompositeListState {
    pub fn new(
        pointer: &str,
        template: &CompositeField,
        defaults: Option<&Value>,
        palette: Arc<ComponentPalette>,
    ) -> Self {
        let mut state = Self {
            pointer: pointer.to_string(),
            template: template.clone(),
            entries: Vec::new(),
            selected: 0,
            counter: 0,
            palette: Arc::clone(&palette),
        };

        if let Some(Value::Array(items)) = defaults {
            state.seed_entries_from_array(items);
        }

        state
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn selected_index(&self) -> Option<usize> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.selected.min(self.entries.len() - 1))
        }
    }

    fn selected_entry(&self) -> Option<&CompositeListEntry> {
        let idx = self.selected_index()?;
        self.entries.get(idx)
    }

    fn selected_entry_mut(&mut self) -> Option<&mut CompositeListEntry> {
        let idx = self.selected_index()?;
        self.entries.get_mut(idx)
    }

    pub fn select(&mut self, delta: i32) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let len = self.entries.len() as i32;
        let next = (self.selected as i32 + delta).clamp(0, len - 1);
        let changed = next as usize != self.selected;
        self.selected = next as usize;
        changed
    }

    pub fn set_selected(&mut self, index: usize) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let len = self.entries.len();
        let bounded = index.min(len.saturating_sub(1));
        let changed = bounded != self.selected;
        self.selected = bounded;
        changed
    }

    pub fn add_entry(&mut self, variant_index: Option<usize>) -> usize {
        let entry_pointer = format!("{}/entry_{}", self.pointer, self.counter);
        self.counter += 1;
        let mut state =
            CompositeState::new(&entry_pointer, &self.template, Arc::clone(&self.palette));
        state.ensure_editable_variant_with_index(variant_index);
        self.entries.push(CompositeListEntry {
            pointer: entry_pointer,
            state,
        });
        self.selected = self.entries.len().saturating_sub(1);
        self.selected
    }

    pub fn remove_selected(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let idx = self.selected.min(self.entries.len() - 1);
        self.entries.remove(idx);
        if idx >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        true
    }

    pub fn move_selected(&mut self, delta: i32) -> bool {
        if self.entries.len() < 2 {
            return false;
        }
        let len = self.entries.len() as i32;
        let next = self.selected as i32 + delta;
        if next < 0 || next >= len {
            return false;
        }
        self.entries.swap(self.selected, next as usize);
        self.selected = next as usize;
        self.refresh_entry_pointers();
        true
    }

    pub fn selected_label(&self) -> Option<String> {
        let idx = self.selected_index()?;
        let entry = self.entries.get(idx)?;
        Some(format!(
            "#{} {}",
            idx + 1,
            entry.state.summary_with_preview()
        ))
    }

    pub fn selected_compact_label(&self) -> Option<String> {
        let idx = self.selected_index()?;
        let entry = self.entries.get(idx)?;
        Some(format!("#{} {}", idx + 1, entry.state.summary()))
    }

    pub fn selected_compact_label_with_limit(&self, max_visible: usize) -> Option<String> {
        let label = self.selected_compact_label()?;
        Some(truncate_visible_text(&label, max_visible))
    }

    pub fn summaries(&self) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| format!("#{} {}", idx + 1, entry.state.summary_with_preview()))
            .collect()
    }

    pub fn summaries_with_limit(&self, max_visible: usize) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let prefix = format!("#{} ", idx + 1);
                let prefix_width = UnicodeWidthStr::width(prefix.as_str());
                format!(
                    "{prefix}{}",
                    entry.state.summary_with_preview_limit(
                        max_visible.saturating_sub(prefix_width).max(1)
                    )
                )
            })
            .collect()
    }

    pub fn selected_entry_selector(&self) -> Option<CompositeSelectorView> {
        let entry = self.selected_entry()?;
        Some(CompositeSelectorView {
            multi: entry.state.is_multi(),
            options: entry.state.option_titles(),
            active: entry.state.active_flags(),
            descriptions: entry.state.variant_descriptions(),
        })
    }

    pub fn open_selected_editor(
        &mut self,
    ) -> Result<CompositeListEditorContext, FieldCoercionError> {
        let idx = self.selected_index().ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "no entry selected".to_string(),
        })?;
        {
            let entry = self
                .entries
                .get_mut(idx)
                .ok_or_else(|| FieldCoercionError {
                    pointer: self.pointer.clone(),
                    message: "invalid entry selection".to_string(),
                })?;
            entry.state.ensure_editable_variant();
        }
        let entry = self.entries.get(idx).ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "invalid entry selection".to_string(),
        })?;
        let variant_index = entry.state.active_indices().first().copied().unwrap_or(0);
        let session = entry
            .state
            .take_editor_session(&entry.pointer, variant_index)?;
        Ok(CompositeListEditorContext {
            entry_index: idx,
            entry_label: format!("#{} {}", idx + 1, entry.state.summary_with_preview()),
            session,
        })
    }

    pub fn restore_entry_editor(&mut self, entry_index: usize, session: CompositeEditorSession) {
        if let Some(entry) = self.entries.get(entry_index) {
            entry.state.restore_editor_session(session);
        }
    }

    pub fn build_value(&self, required: bool) -> Result<Option<Value>, FieldCoercionError> {
        if self.entries.is_empty() {
            if required {
                return Ok(Some(Value::Array(Vec::new())));
            }
            return Ok(None);
        }

        let mut values = Vec::new();
        for entry in &self.entries {
            match entry.state.build_value(false)? {
                Some(value) => values.push(value),
                None => values.push(Value::Null),
            }
        }
        Ok(Some(Value::Array(values)))
    }

    fn refresh_entry_pointers(&mut self) {
        for (index, entry) in self.entries.iter_mut().enumerate() {
            let pointer = format!("{}/entry_{}", self.pointer, index);
            entry.pointer = pointer.clone();
            entry.state.rebind_pointer(&pointer);
        }
    }

    pub fn seed_entries_from_array(&mut self, items: &[Value]) {
        self.entries.clear();
        for (index, item) in items.iter().enumerate() {
            let pointer = format!("{}/entry_{}", self.pointer, index);
            let mut state =
                CompositeState::new(&pointer, &self.template, Arc::clone(&self.palette));
            let _ = state.seed_from_value(item);
            state.ensure_editable_variant();
            self.entries.push(CompositeListEntry { pointer, state });
        }
        self.counter = self.entries.len();
        if self.entries.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    pub fn popup(&self) -> Option<CompositePopupData> {
        let entry = self.selected_entry()?;
        let options = entry.state.option_titles();
        if options.is_empty() {
            return None;
        }
        let selected = entry
            .state
            .selected_index()
            .unwrap_or(0)
            .min(options.len().saturating_sub(1));
        Some(CompositePopupData {
            options,
            selected,
            multi: entry.state.is_multi(),
            active: entry.state.active_flags(),
        })
    }

    /// Get variant selector popup for adding new entry
    pub fn variant_selector_popup(&self) -> Option<CompositePopupData> {
        let options = self.template.variant_titles();
        if options.len() <= 1 {
            return None; // No need to select if only one variant
        }
        let count = options.len();
        Some(CompositePopupData {
            options,
            selected: 0,
            multi: false,
            active: vec![false; count],
        })
    }

    /// Get the number of available variants
    pub fn variant_count(&self) -> usize {
        self.template.variant_count()
    }

    pub fn apply_selection(&mut self, selection: usize, flags: Option<Vec<bool>>) -> bool {
        let Some(entry) = self.selected_entry_mut() else {
            return false;
        };

        if entry.state.is_multi() {
            let Some(flags) = flags else {
                return false;
            };

            // Multi-choice semantics for arrays of composites (e.g. deepItems):
            // treat each active variant as its own list entry instead of storing
            // multiple active variants on a single entry.
            let active_indices: Vec<usize> = flags
                .iter()
                .enumerate()
                .filter_map(|(idx, flag)| if *flag { Some(idx) } else { None })
                .collect();

            if active_indices.is_empty() {
                // No variants selected: clear entries.
                self.entries.clear();
                self.selected = 0;
                self.counter = 0;
                return true;
            }

            // Rebuild the entries collection: one entry per active variant.
            self.entries.clear();
            self.counter = 0;
            for (index, variant_index) in active_indices.iter().copied().enumerate() {
                let pointer = format!("{}/entry_{}", self.pointer, index);
                let mut state =
                    CompositeState::new(&pointer, &self.template, Arc::clone(&self.palette));
                // Ensure only this variant is active for the new entry.
                state.ensure_editable_variant_with_index(Some(variant_index));
                self.entries.push(CompositeListEntry { pointer, state });
            }
            self.selected = 0;
            true
        } else {
            entry.state.apply_single(selection)
        }
    }
}
