use std::sync::Arc;

use serde_json::{Map, Value};

use crate::tui::model::FormSchema;
use crate::tui::state::LayoutNavModel;

use super::{
    error::FieldCoercionError,
    field::{FieldState, components::ComponentPalette},
    section::SectionState,
    ui_store::UiStores,
};

#[derive(Debug, Clone)]
pub struct RootSectionState {
    #[allow(dead_code)]
    pub id: String,
    pub title: String,
    #[allow(dead_code)]
    pub description: Option<String>,
    pub sections: Vec<SectionState>,
}

#[derive(Debug, Clone)]
pub struct FormState {
    pub roots: Vec<RootSectionState>,
    ui: UiStores,
    layout_nav: Option<LayoutNavModel>,
}

impl FormState {
    #[allow(dead_code)]
    pub fn from_schema(schema: &FormSchema) -> Self {
        Self::from_schema_with_palette(schema, Arc::new(ComponentPalette::default()))
    }

    pub fn from_schema_with_palette(schema: &FormSchema, palette: Arc<ComponentPalette>) -> Self {
        let mut roots = Vec::new();
        if schema.roots.is_empty() {
            roots.push(empty_root_state());
        } else {
            for root in &schema.roots {
                let mut sections = Vec::new();
                for section in &root.sections {
                    SectionState::collect(section, 0, &palette, &mut sections);
                }
                if sections.is_empty() {
                    sections.push(empty_section_state());
                }
                roots.push(RootSectionState {
                    id: root.id.clone(),
                    title: root.title.clone(),
                    description: root.description.clone(),
                    sections,
                });
            }
        }
        let mut state = Self {
            roots,
            ui: UiStores::new(),
            layout_nav: None,
        };
        state.normalize_focus();
        state
    }

    pub fn from_sections(
        root_id: impl Into<String>,
        root_title: impl Into<String>,
        root_description: Option<String>,
        sections: Vec<SectionState>,
    ) -> Self {
        let mut state = Self {
            roots: vec![RootSectionState {
                id: root_id.into(),
                title: root_title.into(),
                description: root_description,
                sections,
            }],
            ui: UiStores::new(),
            layout_nav: None,
        };
        state.normalize_focus();
        state
    }

    #[cfg(all(test, feature = "json"))]
    pub(crate) fn from_roots_for_test(roots: Vec<RootSectionState>) -> Self {
        let mut state = Self {
            roots,
            ui: UiStores::new(),
            layout_nav: None,
        };
        state.normalize_focus();
        state
    }

    pub fn with_layout_nav(mut self, layout_nav: LayoutNavModel) -> Self {
        self.layout_nav = Some(layout_nav);
        self
    }

    pub fn set_layout_nav(&mut self, layout_nav: LayoutNavModel) {
        self.layout_nav = Some(layout_nav);
    }

    pub fn layout_nav(&self) -> Option<&LayoutNavModel> {
        self.layout_nav.as_ref()
    }

    pub fn root_index(&self) -> usize {
        self.ui.root.current()
    }

    pub fn section_index(&self) -> usize {
        self.ui.sections.current()
    }

    pub fn field_index(&self) -> usize {
        self.ui.fields.current()
    }

    pub fn set_root_index(&mut self, index: usize) -> bool {
        let changed = self.ui.root.set(index, self.roots.len());
        if changed {
            self.ui.sections.reset();
            self.ui.fields.reset();
            self.normalize_focus();
        }
        changed
    }

    pub fn set_section_index(&mut self, index: usize) -> bool {
        let len = self
            .roots
            .get(self.root_index())
            .map(|root| root.sections.len())
            .unwrap_or(0);
        let changed = self.ui.sections.set(index, len);
        if changed {
            self.ui.fields.reset();
            self.normalize_focus();
        }
        changed
    }

    pub fn set_field_index(&mut self, index: usize) -> bool {
        let len = self
            .active_section()
            .map(|section| section.fields.len())
            .unwrap_or(0);
        let changed = self.ui.fields.set(index, len);
        if changed {
            self.normalize_focus();
        }
        changed
    }

    pub fn is_empty(&self) -> bool {
        self.roots.iter().all(|root| {
            root.sections
                .iter()
                .all(|section| section.fields.is_empty())
        })
    }

    pub fn active_root(&self) -> Option<&RootSectionState> {
        self.roots.get(self.root_index())
    }

    pub fn active_section(&self) -> Option<&SectionState> {
        self.active_root()
            .and_then(|root| root.sections.get(self.section_index()))
    }

    pub fn active_section_mut(&mut self) -> Option<(&mut SectionState, usize)> {
        self.normalize_focus();
        let root_index = self.root_index();
        let section_index = self.section_index();
        let field_index = self.field_index();
        let root = self.roots.get_mut(root_index)?;
        let section = root.sections.get_mut(section_index)?;
        let index = field_index.min(section.fields.len().saturating_sub(1));
        Some((section, index))
    }

    pub fn focused_field_mut(&mut self) -> Option<&mut FieldState> {
        let (section, index) = self.active_section_mut()?;
        section.fields.get_mut(index)
    }

    pub fn focused_field(&self) -> Option<&FieldState> {
        self.active_section()
            .and_then(|section| section.fields.get(self.field_index()))
    }

    pub fn focus_next_field(&mut self) {
        self.advance_focus_forward();
    }

    pub fn focus_prev_field(&mut self) {
        self.advance_focus_backward();
    }

    pub fn has_focusable_fields(&self) -> bool {
        !self.focus_positions().is_empty()
    }

    pub fn focus_is_first(&self) -> bool {
        let positions = self.focus_positions();
        if positions.is_empty() {
            return false;
        }
        match (self.current_focus_position(), positions.first()) {
            (Some(current), Some(first)) => current == *first,
            _ => false,
        }
    }

    pub fn focus_is_last(&self) -> bool {
        let positions = self.focus_positions();
        if positions.is_empty() {
            return false;
        }
        match (self.current_focus_position(), positions.last()) {
            (Some(current), Some(last)) => current == *last,
            _ => false,
        }
    }

    pub fn focus_first_field(&mut self) {
        if let Some((root_idx, section_idx, field_idx)) = self.focus_positions().first().copied() {
            self.set_root_index(root_idx);
            self.set_section_index(section_idx);
            self.set_field_index(field_idx);
        }
    }

    pub fn focus_last_field(&mut self) {
        if let Some((root_idx, section_idx, field_idx)) = self.focus_positions().last().copied() {
            self.set_root_index(root_idx);
            self.set_section_index(section_idx);
            self.set_field_index(field_idx);
        }
    }

    fn advance_focus_by(&mut self, delta: i32) {
        self.normalize_focus();
        if self.roots.is_empty() {
            return;
        }
        let positions = self.focus_positions();
        if positions.is_empty() {
            return;
        }
        let len = positions.len() as i32;
        let current = self.current_focus_position();
        let current_idx = current
            .and_then(|curr| positions.iter().position(|pos| *pos == curr))
            .unwrap_or(0);
        let mut next = current_idx as i32 + delta;
        next = ((next % len) + len) % len;
        let (root_idx, section_idx, field_idx) = positions[next as usize];
        self.set_root_index(root_idx);
        self.set_section_index(section_idx);
        self.set_field_index(field_idx);
        self.normalize_focus();
    }

    pub fn advance_focus_forward(&mut self) {
        self.advance_focus_by(1);
    }

    pub fn advance_focus_backward(&mut self) {
        self.advance_focus_by(-1);
    }

    pub fn focus_next_section(&mut self, delta: i32) {
        if !self.focus_next_section_with_layout(delta) {
            self.normalize_focus();
            self.advance_section(delta);
            self.normalize_focus();
        }
    }

    pub fn focus_next_root(&mut self, delta: i32) {
        if !self.focus_next_root_with_layout(delta) {
            if self.roots.is_empty() {
                return;
            }
            if self.ui.root.advance(delta, self.roots.len()) {
                self.ui.sections.reset();
                self.ui.fields.reset();
            }
            self.normalize_focus();
        }
    }

    pub fn try_build_value(&self) -> Result<Value, FieldCoercionError> {
        let mut root = Value::Object(Map::new());
        for section in self.iter_sections() {
            for field in &section.fields {
                if let Some(value) = field.current_value()? {
                    insert_path(&mut root, &field.schema.path, value);
                }
            }
        }
        Ok(root)
    }

    pub fn seed_from_value(&mut self, value: &Value) {
        for section in self.iter_sections_mut() {
            for field in &mut section.fields {
                if let Some(subvalue) = value_at_path(value, &field.schema.path) {
                    field.seed_value(subvalue);
                }
            }
        }
    }

    pub fn clear_errors(&mut self) {
        for section in self.iter_sections_mut() {
            for field in &mut section.fields {
                field.clear_error();
            }
        }
    }

    pub fn mark_clean(&mut self) {
        for section in self.iter_sections_mut() {
            for field in &mut section.fields {
                field.dirty = false;
            }
        }
    }

    pub fn set_error(&mut self, pointer: &str, message: String) -> bool {
        for section in self.iter_sections_mut() {
            for field in &mut section.fields {
                if field.schema.pointer == pointer {
                    field.set_error(message.clone());
                    return true;
                }
            }
        }
        false
    }

    pub fn focus_first_field_with_layout(&mut self) -> bool {
        let nav = match self.layout_nav.clone() {
            Some(nav) => nav,
            None => return false,
        };

        for root in &nav.roots {
            for section in &root.sections {
                let target_pointer = section
                    .first_pointer
                    .as_ref()
                    .or_else(|| section.pointers.first())
                    .cloned();
                if let Some(ptr) = target_pointer
                    && self.focus_field_by_pointer(&ptr)
                {
                    return true;
                }
            }
        }

        false
    }

    pub fn clear_error(&mut self, pointer: &str) {
        for section in self.iter_sections_mut() {
            for field in &mut section.fields {
                if field.schema.pointer == pointer {
                    field.clear_error();
                }
            }
        }
    }

    pub fn field_mut_by_pointer(&mut self, pointer: &str) -> Option<&mut FieldState> {
        for section in self.iter_sections_mut() {
            for field in &mut section.fields {
                if field.schema.pointer == pointer {
                    return Some(field);
                }
            }
        }
        None
    }

    fn focus_field_by_pointer(&mut self, pointer: &str) -> bool {
        for (root_idx, root) in self.roots.iter().enumerate() {
            for (section_idx, section) in root.sections.iter().enumerate() {
                for (field_idx, field) in section.fields.iter().enumerate() {
                    if field.schema.pointer == pointer {
                        self.set_root_index(root_idx);
                        self.set_section_index(section_idx);
                        self.set_field_index(field_idx);
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn field_by_pointer(&self, pointer: &str) -> Option<&FieldState> {
        for section in self.iter_sections() {
            for field in &section.fields {
                if field.schema.pointer == pointer {
                    return Some(field);
                }
            }
        }
        None
    }

    pub fn is_dirty(&self) -> bool {
        self.iter_sections()
            .any(|section| section.fields.iter().any(|field| field.dirty))
    }

    pub fn error_count(&self) -> usize {
        self.iter_sections()
            .map(|section| {
                section
                    .fields
                    .iter()
                    .filter(|field| field.error.is_some())
                    .count()
            })
            .sum()
    }

    pub fn error_entries(&self) -> Vec<(String, String)> {
        let mut entries = Vec::new();
        for section in self.iter_sections() {
            for field in &section.fields {
                if let Some(err) = &field.error {
                    entries.push((field.schema.pointer.clone(), err.clone()));
                }
            }
        }
        entries
    }

    fn advance_section(&mut self, delta: i32) {
        let positions = self.section_positions();
        if positions.is_empty() {
            return;
        }
        let current_idx = positions
            .iter()
            .position(|&(root, section)| {
                root == self.root_index() && section == self.section_index()
            })
            .unwrap_or(0);
        let len = positions.len() as i32;
        let mut next = current_idx as i32 + delta;
        next = ((next % len) + len) % len;
        let (root_index, section_index) = positions[next as usize];
        self.set_root_index(root_index);
        self.set_section_index(section_index);
        self.ui.fields.reset();
    }

    fn normalize_focus(&mut self) {
        if self.roots.is_empty() {
            return;
        }
        self.ui.root.clamp(self.roots.len());
        if self
            .roots
            .get(self.root_index())
            .map(|root| root.sections.is_empty())
            .unwrap_or(true)
        {
            if let Some((idx, _)) = self
                .roots
                .iter()
                .enumerate()
                .find(|(_, root)| !root.sections.is_empty())
            {
                self.ui.root.set(idx, self.roots.len());
            } else {
                self.ui.sections.reset();
                self.ui.fields.reset();
                return;
            }
        }
        let section_len = self
            .roots
            .get(self.root_index())
            .map(|root| root.sections.len())
            .unwrap_or(0);
        if section_len == 0 {
            self.ui.sections.reset();
            self.ui.fields.reset();
            return;
        }
        self.ui.sections.clamp(section_len);

        if !self.section_has_fields(self.root_index(), self.section_index()) {
            if let Some(index) = self.first_focusable_section_in_root(self.root_index()) {
                self.ui.sections.set(index, section_len);
            } else if let Some((root_idx, section_idx)) =
                self.focusable_section_positions().first().copied()
            {
                self.ui.root.set(root_idx, self.roots.len());
                let new_len = self
                    .roots
                    .get(root_idx)
                    .map(|root| root.sections.len())
                    .unwrap_or(0);
                self.ui.sections.set(section_idx, new_len);
            }
        }

        let field_len = self
            .active_section()
            .map(|section| section.fields.len())
            .unwrap_or(0);
        self.ui.fields.clamp(field_len);
    }

    fn iter_sections(&self) -> impl Iterator<Item = &SectionState> {
        self.roots.iter().flat_map(|root| root.sections.iter())
    }

    fn iter_sections_mut(&mut self) -> impl Iterator<Item = &mut SectionState> {
        self.roots
            .iter_mut()
            .flat_map(|root| root.sections.iter_mut())
    }

    fn current_focus_position(&self) -> Option<(usize, usize, usize)> {
        let root_idx = self.root_index();
        let section_idx = self.section_index();
        let root = self.roots.get(root_idx)?;
        let section = root.sections.get(section_idx)?;
        if section.fields.is_empty() {
            return None;
        }
        let field_len = section.fields.len();
        Some((
            root_idx,
            section_idx,
            self.field_index().min(field_len.saturating_sub(1)),
        ))
    }

    fn focus_positions(&self) -> Vec<(usize, usize, usize)> {
        let mut positions = Vec::new();
        for (root_idx, root) in self.roots.iter().enumerate() {
            for (section_idx, section) in root.sections.iter().enumerate() {
                for (field_idx, _field) in section.fields.iter().enumerate() {
                    positions.push((root_idx, section_idx, field_idx));
                }
            }
        }
        positions
    }

    fn section_positions(&self) -> Vec<(usize, usize)> {
        let focusable = self.focusable_section_positions();
        if focusable.is_empty() {
            self.all_section_positions()
        } else {
            focusable
        }
    }

    fn focusable_section_positions(&self) -> Vec<(usize, usize)> {
        let mut positions = Vec::new();
        for (root_idx, root) in self.roots.iter().enumerate() {
            for (section_idx, section) in root.sections.iter().enumerate() {
                if !section.fields.is_empty() {
                    positions.push((root_idx, section_idx));
                }
            }
        }
        positions
    }

    fn all_section_positions(&self) -> Vec<(usize, usize)> {
        let mut positions = Vec::new();
        for (root_idx, root) in self.roots.iter().enumerate() {
            for (section_idx, _section) in root.sections.iter().enumerate() {
                positions.push((root_idx, section_idx));
            }
        }
        positions
    }

    fn section_has_fields(&self, root_idx: usize, section_idx: usize) -> bool {
        self.roots
            .get(root_idx)
            .and_then(|root| root.sections.get(section_idx))
            .map(|section| !section.fields.is_empty())
            .unwrap_or(false)
    }

    fn first_focusable_section_in_root(&self, root_idx: usize) -> Option<usize> {
        self.roots.get(root_idx).and_then(|root| {
            root.sections
                .iter()
                .enumerate()
                .find(|(_, section)| !section.fields.is_empty())
                .map(|(idx, _)| idx)
        })
    }

    fn focus_next_section_with_layout(&mut self, delta: i32) -> bool {
        let nav = match &self.layout_nav {
            Some(nav) => nav,
            None => return false,
        };

        let current_pointer = match self.focused_field() {
            Some(field) => field.schema.pointer.clone(),
            None => return false,
        };

        let mut sections = Vec::new();
        for root in &nav.roots {
            for section in &root.sections {
                sections.push(section);
            }
        }

        if sections.is_empty() {
            return false;
        }

        let current_idx = match sections
            .iter()
            .enumerate()
            .filter(|(_, section)| section.pointers.iter().any(|p| p == &current_pointer))
            .map(|(idx, _)| idx)
            .next_back()
        {
            Some(idx) => idx,
            None => return false,
        };

        let len = sections.len() as i32;
        if len == 0 {
            return false;
        }

        let mut next = current_idx as i32 + delta;
        next = ((next % len) + len) % len;
        let target = sections[next as usize];

        let target_pointer = target
            .first_pointer
            .as_ref()
            .or_else(|| target.pointers.first())
            .cloned();

        match target_pointer {
            Some(ptr) => self.focus_field_by_pointer(&ptr),
            None => false,
        }
    }

    fn focus_next_root_with_layout(&mut self, delta: i32) -> bool {
        let nav = match &self.layout_nav {
            Some(nav) => nav,
            None => return false,
        };

        let current_pointer = match self.focused_field() {
            Some(field) => field.schema.pointer.clone(),
            None => return false,
        };

        if nav.roots.is_empty() {
            return false;
        }

        let mut current_root_idx = None;
        for (idx, root) in nav.roots.iter().enumerate() {
            if root
                .sections
                .iter()
                .any(|section| section.pointers.iter().any(|p| p == &current_pointer))
            {
                current_root_idx = Some(idx);
                break;
            }
        }

        let current_root_idx = match current_root_idx {
            Some(idx) => idx,
            None => return false,
        };

        let len = nav.roots.len() as i32;
        if len == 0 {
            return false;
        }

        let mut next = current_root_idx as i32 + delta;
        next = ((next % len) + len) % len;
        let target_root = &nav.roots[next as usize];

        let target_section = target_root
            .sections
            .iter()
            .find(|section| section.first_pointer.is_some() || !section.pointers.is_empty());

        let target_pointer = match target_section {
            Some(section) => section
                .first_pointer
                .as_ref()
                .or_else(|| section.pointers.first())
                .cloned(),
            None => None,
        };

        match target_pointer {
            Some(ptr) => self.focus_field_by_pointer(&ptr),
            None => false,
        }
    }
}

fn insert_path(root: &mut Value, path: &[String], value: Value) {
    if path.is_empty() {
        *root = value;
        return;
    }

    if !root.is_object() {
        *root = Value::Object(Map::new());
    }

    if let Value::Object(obj) = root {
        if path.len() == 1 {
            obj.insert(path[0].clone(), value);
            return;
        }

        let entry = obj
            .entry(path[0].clone())
            .or_insert_with(|| Value::Object(Map::new()));
        insert_path(entry, &path[1..], value);
    }
}

fn empty_section_state() -> SectionState {
    SectionState {
        id: "general".to_string(),
        title: "General".to_string(),
        description: None,
        path: Vec::new(),
        depth: 0,
        fields: Vec::new(),
        scroll_offset: 0,
    }
}

fn empty_root_state() -> RootSectionState {
    RootSectionState {
        id: "general".to_string(),
        title: "General".to_string(),
        description: None,
        sections: vec![empty_section_state()],
    }
}

fn value_at_path<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        match current {
            Value::Object(map) => {
                current = map.get(segment)?;
            }
            _ => return None,
        }
    }
    Some(current)
}
