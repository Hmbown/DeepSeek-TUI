use std::{
    cell::{RefCell, RefMut},
    sync::Arc,
};

use serde_json::{Map, Value, json};

use crate::{
    tui::model::{CompositeField, CompositeMode, form_schema_from_ui_ast},
    ui_ast::{build_ui_ast, layout::build_ui_layout},
};

use super::{
    LayoutNavModel,
    error::FieldCoercionError,
    field::components::ComponentPalette,
    form_state::FormState,
    value_summary::{summarize_inline_value, summarize_inline_value_with_limit},
};

mod composite_list;

pub use composite_list::{CompositeListEditorContext, CompositeListState};

#[derive(Debug, Clone)]
pub struct CompositeState {
    pointer: String,
    mode: CompositeMode,
    variants: Vec<CompositeVariantState>,
}

#[derive(Debug, Clone)]
pub struct CompositeVariantState {
    #[allow(dead_code)]
    id: String,
    title: String,
    #[allow(dead_code)]
    description: Option<String>,
    schema: Value,
    active: bool,
    form: RefCell<Option<FormState>>,
    palette: Arc<ComponentPalette>,
    shape: VariantShape,
}

#[derive(Debug, Clone)]
pub struct CompositeEditorSession {
    pub variant_index: usize,
    pub title: String,
    pub description: Option<String>,
    pub form_state: FormState,
    pub schema: Value,
}
#[derive(Debug, Clone)]
pub struct CompositeVariantSummary {
    pub title: String,
    pub description: Option<String>,
    pub lines: Vec<String>,
}

const WRAPPED_FIELD_NAME: &str = "__value";

#[derive(Debug, Clone, Copy)]
enum VariantShape {
    Object,
    Wrapped,
}
impl CompositeState {
    pub fn new(pointer: &str, field: &CompositeField, palette: Arc<ComponentPalette>) -> Self {
        let mut variants = Vec::with_capacity(field.variants.len());
        for (index, variant) in field.variants.iter().enumerate() {
            variants.push(CompositeVariantState {
                id: variant.id.clone(),
                title: variant.title.clone(),
                description: variant.description.clone(),
                schema: variant.schema.clone(),
                active: matches!(field.mode, CompositeMode::OneOf) && index == 0,
                form: RefCell::new(None),
                palette: Arc::clone(&palette),
                shape: if variant.is_object {
                    VariantShape::Object
                } else {
                    VariantShape::Wrapped
                },
            });
        }

        Self {
            pointer: pointer.to_string(),
            mode: field.mode.clone(),
            variants,
        }
    }

    pub fn summary(&self) -> String {
        match self.mode {
            CompositeMode::OneOf => self
                .variants
                .iter()
                .enumerate()
                .find(|(_, variant)| variant.active)
                .map(|(idx, variant)| format!("Variant: #{} {}", idx + 1, variant.title))
                .unwrap_or_else(|| "Variant: <none>".to_string()),
            CompositeMode::AnyOf => {
                let active = self
                    .variants
                    .iter()
                    .enumerate()
                    .filter(|(_, variant)| variant.active)
                    .map(|(idx, variant)| format!("#{} {}", idx + 1, variant.title))
                    .collect::<Vec<_>>();
                if active.is_empty() {
                    "Variants: []".to_string()
                } else {
                    format!("Variants: {}", active.join(", "))
                }
            }
        }
    }

    pub fn summary_with_preview(&self) -> String {
        let summary = self.summary();
        match self.active_preview() {
            Some(preview) if !preview.is_empty() => format!("{summary} {preview}"),
            _ => summary,
        }
    }

    pub fn summary_with_preview_limit(&self, max_visible: usize) -> String {
        truncate_summary_with_preview(self, max_visible)
    }

    pub fn pointer(&self) -> &str {
        &self.pointer
    }

    pub fn rebind_pointer(&mut self, pointer: &str) {
        self.pointer = pointer.to_string();
    }

    fn pick_variant_index(&self, value: &Value) -> usize {
        if let Value::Object(obj) = value {
            for (idx, variant) in self.variants.iter().enumerate() {
                if variant.matches_value(obj) {
                    return idx;
                }
            }
        }
        0
    }

    pub fn seed_from_value(&mut self, value: &Value) -> Result<(), FieldCoercionError> {
        if self.variants.is_empty() {
            return Ok(());
        }
        let target = self.pick_variant_index(value);
        let pointer = self.pointer.clone();
        for (idx, variant) in self.variants.iter_mut().enumerate() {
            variant.active = idx == target;
            if variant.active {
                let mut form = variant.borrow_form(&pointer)?;
                let mut scratch = None;
                let seed_value = variant.seed_payload(value, &mut scratch);
                form.seed_from_value(seed_value);
            }
        }
        Ok(())
    }

    pub fn is_multi(&self) -> bool {
        matches!(self.mode, CompositeMode::AnyOf)
    }

    pub fn active_summaries(&self) -> Vec<CompositeVariantSummary> {
        let mut summaries = Vec::new();
        for (idx, variant) in self.variants.iter().enumerate() {
            if !variant.active {
                continue;
            }
            match variant.snapshot(self.pointer()) {
                Ok(mut summary) => {
                    summary.title = format!("#{} {}", idx + 1, summary.title);
                    summaries.push(summary);
                }
                Err(err) => summaries.push(CompositeVariantSummary {
                    title: format!("#{} {}", idx + 1, variant.title.clone()),
                    description: variant.description.clone(),
                    lines: vec![format!("Error: {}", err.message)],
                }),
            }
        }
        summaries
    }

    fn active_preview(&self) -> Option<String> {
        let mut previews = Vec::new();
        for variant in self.variants.iter().filter(|variant| variant.active) {
            let Ok(form) = variant.borrow_form(self.pointer()) else {
                continue;
            };
            let Ok(value) = form.try_build_value() else {
                continue;
            };
            let Ok(actual) = variant.unwrap_overlay_value(value, self.pointer()) else {
                continue;
            };
            previews.push(summarize_inline_value(&actual));
        }

        if previews.is_empty() {
            None
        } else {
            Some(previews.join(" + "))
        }
    }

    fn active_preview_with_limit(&self, max_visible: usize) -> Option<String> {
        if max_visible == 0 {
            return None;
        }

        let mut previews = Vec::new();
        for variant in self.variants.iter().filter(|variant| variant.active) {
            let Ok(form) = variant.borrow_form(self.pointer()) else {
                continue;
            };
            let Ok(value) = form.try_build_value() else {
                continue;
            };
            let Ok(actual) = variant.unwrap_overlay_value(value, self.pointer()) else {
                continue;
            };
            previews.push(summarize_inline_value_with_limit(&actual, max_visible));
        }

        if previews.is_empty() {
            None
        } else {
            Some(previews.join(" + "))
        }
    }

    pub fn option_titles(&self) -> Vec<String> {
        self.variants
            .iter()
            .map(|variant| variant.title.clone())
            .collect()
    }

    pub fn variant_descriptions(&self) -> Vec<Option<String>> {
        self.variants
            .iter()
            .map(|variant| variant.description.clone())
            .collect()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.variants.iter().position(|variant| variant.active)
    }

    pub fn active_flags(&self) -> Vec<bool> {
        self.variants.iter().map(|variant| variant.active).collect()
    }

    pub fn active_indices(&self) -> Vec<usize> {
        self.variants
            .iter()
            .enumerate()
            .filter_map(|(idx, variant)| if variant.active { Some(idx) } else { None })
            .collect()
    }

    pub fn apply_single(&mut self, index: usize) -> bool {
        if !matches!(self.mode, CompositeMode::OneOf) {
            return false;
        }
        if self.variants.is_empty() {
            return false;
        }
        let target = index.min(self.variants.len() - 1);
        let mut changed = false;
        for (idx, variant) in self.variants.iter_mut().enumerate() {
            let next_state = idx == target;
            if variant.active != next_state {
                variant.active = next_state;
                changed = true;
            }
        }
        changed
    }

    pub fn rotate_single(&mut self, delta: i32) -> bool {
        if !matches!(self.mode, CompositeMode::OneOf) || self.variants.is_empty() {
            return false;
        }
        let len = self.variants.len() as i32;
        let current = self.selected_index().unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(len);
        if next == current && self.selected_index().is_some() {
            return false;
        }
        self.apply_single(next as usize)
    }

    pub fn ensure_editable_variant(&mut self) {
        self.ensure_editable_variant_with_index(None);
    }

    pub fn ensure_editable_variant_with_index(&mut self, variant_index: Option<usize>) {
        if self.variants.is_empty() {
            return;
        }
        if self.active_indices().is_empty() {
            let target_index = variant_index.unwrap_or(0).min(self.variants.len() - 1);
            match self.mode {
                CompositeMode::OneOf => {
                    let _ = self.apply_single(target_index);
                }
                CompositeMode::AnyOf => {
                    if let Some(variant) = self.variants.get_mut(target_index) {
                        variant.active = true;
                    }
                }
            }
        }
    }

    pub fn take_editor_session(
        &self,
        pointer: &str,
        variant_index: usize,
    ) -> Result<CompositeEditorSession, FieldCoercionError> {
        let variant = self
            .variants
            .get(variant_index)
            .ok_or_else(|| FieldCoercionError {
                pointer: pointer.to_string(),
                message: "invalid variant selection".to_string(),
            })?;
        if !variant.active {
            return Err(FieldCoercionError {
                pointer: pointer.to_string(),
                message: "variant is not active; select it before editing".to_string(),
            });
        }
        let form_state = variant.take_form(pointer)?;
        Ok(CompositeEditorSession {
            variant_index,
            title: variant.title.clone(),
            description: variant.description.clone(),
            form_state,
            schema: variant.overlay_schema(),
        })
    }

    pub fn restore_editor_session(&self, session: CompositeEditorSession) {
        if let Some(variant) = self.variants.get(session.variant_index) {
            variant.store_form(session.form_state);
        }
    }

    pub fn apply_multi(&mut self, flags: &[bool]) -> bool {
        if !matches!(self.mode, CompositeMode::AnyOf) {
            return false;
        }
        if flags.len() != self.variants.len() {
            return false;
        }
        let mut changed = false;
        for (variant, flag) in self.variants.iter_mut().zip(flags.iter()) {
            if variant.active != *flag {
                variant.active = *flag;
                changed = true;
            }
        }
        changed
    }

    pub fn build_value(&self, required: bool) -> Result<Option<Value>, FieldCoercionError> {
        match self.mode {
            CompositeMode::OneOf => {
                if let Some(variant) = self.variants.iter().find(|variant| variant.active) {
                    let form = variant.borrow_form(self.pointer())?;
                    match form.try_build_value() {
                        Ok(value) => {
                            let actual = variant.unwrap_overlay_value(value, self.pointer())?;
                            Ok(Some(actual))
                        }
                        Err(mut err) => {
                            err.pointer = join_pointer(self.pointer(), &err.pointer);
                            Err(err)
                        }
                    }
                } else if required {
                    Err(FieldCoercionError {
                        pointer: self.pointer.clone(),
                        message: "oneOf requires a selected variant".to_string(),
                    })
                } else {
                    Ok(None)
                }
            }
            CompositeMode::AnyOf => {
                // anyOf: 选择第一个激活的变体作为主值
                // 多个激活的变体主要用于验证：告诉系统这个值应该同时符合多个 schema
                let active_variants: Vec<_> = self
                    .variants
                    .iter()
                    .filter(|variant| variant.active)
                    .collect();

                if active_variants.is_empty() {
                    if required {
                        Err(FieldCoercionError {
                            pointer: self.pointer.clone(),
                            message: "anyOf requires at least one active variant".to_string(),
                        })
                    } else {
                        Ok(None)
                    }
                } else {
                    // 返回第一个激活变体的值
                    // 注意：anyOf 不是返回数组，而是一个值可以同时满足多个 schema
                    let primary = active_variants[0];
                    let form = primary.borrow_form(self.pointer())?;
                    match form.try_build_value() {
                        Ok(value) => {
                            let actual = primary.unwrap_overlay_value(value, self.pointer())?;
                            Ok(Some(actual))
                        }
                        Err(mut err) => {
                            err.pointer = join_pointer(self.pointer(), &err.pointer);
                            Err(err)
                        }
                    }
                }
            }
        }
    }
}

fn truncate_summary_with_preview(state: &CompositeState, max_visible: usize) -> String {
    use super::value_summary::truncate_visible_text;
    use unicode_width::UnicodeWidthStr;

    if max_visible == 0 {
        return String::new();
    }

    let summary = state.summary();
    let summary_width = UnicodeWidthStr::width(summary.as_str());
    let separator = " ";
    let separator_width = 1usize;

    if summary_width >= max_visible {
        return truncate_visible_text(&summary, max_visible);
    }

    let preview_budget = max_visible.saturating_sub(summary_width + separator_width);
    if preview_budget == 0 {
        return truncate_visible_text(&summary, max_visible);
    }

    match state
        .active_preview_with_limit(preview_budget)
        .filter(|text| !text.is_empty())
    {
        Some(preview) => format!("{summary}{separator}{preview}"),
        None => summary,
    }
}

fn wrap_non_object_schema(schema: &Value, title: &str, description: Option<&String>) -> Value {
    let mut property = schema.clone();
    if let Value::Object(ref mut map) = property {
        map.entry("title".to_string())
            .or_insert_with(|| Value::String(title.to_string()));
        if let Some(desc) = description
            && !map.contains_key("description")
        {
            map.insert("description".to_string(), Value::String(desc.clone()));
        }
    }
    json!({
        "type": "object",
        "title": title,
        "properties": {
            WRAPPED_FIELD_NAME: property
        },
        "required": [WRAPPED_FIELD_NAME]
    })
}

impl CompositeVariantState {
    fn overlay_schema(&self) -> Value {
        match self.shape {
            VariantShape::Object => self.schema.clone(),
            VariantShape::Wrapped => {
                wrap_non_object_schema(&self.schema, &self.title, self.description.as_ref())
            }
        }
    }

    fn seed_payload<'a>(&self, value: &'a Value, scratch: &'a mut Option<Value>) -> &'a Value {
        if matches!(self.shape, VariantShape::Wrapped) {
            *scratch = Some(json!({ WRAPPED_FIELD_NAME: value }));
            scratch.as_ref().unwrap()
        } else {
            value
        }
    }

    fn unwrap_overlay_value(
        &self,
        value: Value,
        pointer: &str,
    ) -> Result<Value, FieldCoercionError> {
        if !matches!(self.shape, VariantShape::Wrapped) {
            return Ok(value);
        }
        let object = value.as_object().ok_or_else(|| FieldCoercionError {
            pointer: pointer.to_string(),
            message: "overlay payload missing object wrapper".to_string(),
        })?;
        object
            .get(WRAPPED_FIELD_NAME)
            .cloned()
            .ok_or_else(|| FieldCoercionError {
                pointer: pointer.to_string(),
                message: "overlay payload missing wrapped value".to_string(),
            })
    }

    fn ensure_form_ready(&self, pointer: &str) -> Result<(), FieldCoercionError> {
        if self.form.borrow().is_some() {
            return Ok(());
        }
        let schema_value = self.overlay_schema();
        let ui_ast = build_ui_ast(&schema_value).map_err(|err| FieldCoercionError {
            pointer: pointer.to_string(),
            message: format!("failed to parse composite variant '{}': {err}", self.title),
        })?;
        let schema = form_schema_from_ui_ast(&ui_ast);

        let layout = build_ui_layout(&ui_ast);
        let layout_nav = LayoutNavModel::from_uilayout(&layout);

        let mut form_state =
            FormState::from_schema_with_palette(&schema, Arc::clone(&self.palette));
        form_state.set_layout_nav(layout_nav);
        *self.form.borrow_mut() = Some(form_state);
        Ok(())
    }

    fn borrow_form(&self, pointer: &str) -> Result<RefMut<'_, FormState>, FieldCoercionError> {
        self.ensure_form_ready(pointer)?;
        Ok(RefMut::map(
            self.form.borrow_mut(),
            |slot: &mut Option<FormState>| {
                slot.as_mut().expect("variant form should be initialized")
            },
        ))
    }

    fn take_form(&self, pointer: &str) -> Result<FormState, FieldCoercionError> {
        self.ensure_form_ready(pointer)?;
        Ok(self
            .form
            .borrow_mut()
            .take()
            .expect("variant form should be initialized"))
    }

    fn store_form(&self, form_state: FormState) {
        *self.form.borrow_mut() = Some(form_state);
    }

    fn snapshot(&self, pointer: &str) -> Result<CompositeVariantSummary, FieldCoercionError> {
        let form = self.borrow_form(pointer)?;
        let mut lines = Vec::new();
        if form.roots.iter().all(|root| root.sections.is_empty()) {
            lines.push("No fields defined for this variant.".to_string());
        } else {
            for root in &form.roots {
                for section in &root.sections {
                    let label = if root.title.is_empty() || root.title == section.title {
                        format!("Section: {}", section.title)
                    } else {
                        format!("Section: {} › {}", root.title, section.title)
                    };
                    lines.push(label);
                    if section.fields.is_empty() {
                        lines.push("  • <empty>".to_string());
                    } else {
                        for field in &section.fields {
                            lines.push(format!(
                                "  • {} = {}",
                                field.schema.display_label(),
                                field.display_value()
                            ));
                        }
                    }
                }
            }
        }
        Ok(CompositeVariantSummary {
            title: self.title.clone(),
            description: self.description.clone(),
            lines,
        })
    }

    fn matches_value(&self, value: &Map<String, Value>) -> bool {
        let Some(props) = self.schema.get("properties").and_then(Value::as_object) else {
            return true;
        };

        let mut inspected = false;

        for (key, schema) in props {
            if let Some(expected) = schema.get("const") {
                inspected = true;
                if value.get(key) != Some(expected) {
                    return false;
                }
                continue;
            }

            if let Some(options) = schema.get("enum").and_then(Value::as_array) {
                inspected = true;
                let Some(actual) = value.get(key) else {
                    return false;
                };
                if !options.iter().any(|candidate| candidate == actual) {
                    return false;
                }
            }
        }

        // # true
        inspected
    }
}

fn join_pointer(base: &str, child: &str) -> String {
    match (base.is_empty(), child.is_empty()) {
        (true, true) => String::new(),
        (true, false) => child.to_string(),
        (false, true) => base.to_string(),
        (false, false) =>
        {
            #[allow(clippy::if_same_then_else)]
            if child.starts_with('/') {
                format!("{base}{child}")
            } else if base.ends_with('/') {
                format!("{base}{child}")
            } else {
                format!("{base}/{child}")
            }
        }
    }
}
