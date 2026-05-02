#![allow(dead_code)]

use std::collections::HashMap;

use serde_json::Value;

use super::model::SchemaObject;

#[derive(Debug, Clone)]
pub struct SectionInfo {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
}

pub fn general_section_info() -> SectionInfo {
    SectionInfo {
        id: "general".to_string(),
        title: "General".to_string(),
        description: None,
    }
}

pub fn section_info_for_object(
    schema: &SchemaObject,
    name: &str,
    parent: Option<&SectionInfo>,
) -> SectionInfo {
    if let Some(group) = extension_string(schema, "x-group") {
        let title =
            extension_string(schema, "x-group-title").unwrap_or_else(|| prettify_label(&group));
        let description = extension_string(schema, "x-group-description");
        return SectionInfo {
            id: group,
            title,
            description,
        };
    }

    SectionInfo {
        id: name.to_string(),
        title: schema
            .metadata
            .as_ref()
            .and_then(|m| m.title.clone())
            .unwrap_or_else(|| prettify_label(name)),
        description: schema
            .metadata
            .as_ref()
            .and_then(|m| m.description.clone())
            .or_else(|| parent.and_then(|p| p.description.clone())),
    }
}

pub fn metadata_map(schema: &SchemaObject) -> HashMap<String, Value> {
    schema
        .extensions
        .iter()
        .filter(|(key, _)| key.starts_with("x-"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn root_schema_header(schema: &Value) -> (Option<String>, Option<String>) {
    (
        root_schema_string(schema, "title"),
        root_schema_string(schema, "description"),
    )
}

fn root_schema_string(schema: &Value, key: &str) -> Option<String> {
    schema.as_object()?.get(key)?.as_str().map(str::to_string)
}

pub fn extension_string(schema: &SchemaObject, key: &str) -> Option<String> {
    schema
        .extensions
        .get(key)
        .and_then(|value| value.as_str().map(str::to_string))
}

pub fn prettify_label(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }

    let mut result = String::with_capacity(raw.len());
    let mut capitalize = true;
    for ch in raw.chars() {
        if ch == '_' || ch == '-' {
            result.push(' ');
            capitalize = true;
            continue;
        }

        if capitalize {
            result.push(ch.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(ch);
        }
    }

    result.trim().to_string()
}
