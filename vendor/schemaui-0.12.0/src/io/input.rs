use std::{borrow::Cow, collections::HashSet};

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Map, Number, Value, json};

use super::DocumentFormat;

const JSON_SCHEMA_DRAFT: &str = "http://json-schema.org/draft-07/schema#";

/// Parse structured data in any supported format into a `serde_json::Value`.
pub fn parse_document_str(contents: &str, format: DocumentFormat) -> Result<Value> {
    match format {
        #[cfg(feature = "json")]
        DocumentFormat::Json => {
            serde_json::from_str::<Value>(contents).with_context(|| "failed to parse JSON document")
        }
        #[cfg(feature = "yaml")]
        DocumentFormat::Yaml => {
            serde_yaml::from_str::<Value>(contents).with_context(|| "failed to parse YAML document")
        }
        #[cfg(feature = "toml")]
        DocumentFormat::Toml => toml::from_str::<toml::Value>(contents)
            .with_context(|| "failed to parse TOML document")
            .and_then(|value| {
                serde_json::to_value(value).context("failed to convert TOML to JSON")
            }),
    }
}

/// Parse an inline document by trying every enabled format.
///
/// The default crate format is attempted first, followed by the remaining
/// enabled formats. This is useful for ergonomic APIs that accept either a
/// `serde_json::Value` or raw inline text without an explicit format hint.
pub fn parse_document_auto(contents: &str) -> Result<Value> {
    let preferred = DocumentFormat::default();
    match parse_document_str(contents, preferred) {
        Ok(value) => Ok(value),
        Err(primary) => {
            for candidate in DocumentFormat::available_formats() {
                if candidate == preferred {
                    continue;
                }
                if let Ok(value) = parse_document_str(contents, candidate) {
                    return Ok(value);
                }
            }
            Err(anyhow::anyhow!(
                "failed to parse inline document: tried {} (first error: {primary})",
                DocumentFormat::format_list()
            ))
        }
    }
}

/// Convert structured data into a JSON Schema with inferred defaults.
pub fn schema_from_data_str(contents: &str, format: DocumentFormat) -> Result<Value> {
    let value = parse_document_str(contents, format)?;
    Ok(schema_from_data_value(&value))
}

/// Convert structured data into a JSON Schema with inferred defaults.
pub fn schema_from_data_value(value: &Value) -> Value {
    let mut schema = infer_schema(value);
    if let Value::Object(ref mut map) = schema {
        map.entry("$schema".to_string())
            .or_insert_with(|| Value::String(JSON_SCHEMA_DRAFT.to_string()));
    }
    schema
}

/// Merge user-provided data into an existing schema as `default` values.
pub fn schema_with_defaults(schema: &Value, defaults: &Value) -> Value {
    let mut enriched = schema.clone();
    DefaultApplier::new().apply(&mut enriched, defaults);
    enriched
}

/// Heuristic used when callers pass a single document and expect SchemaUI to
/// decide whether it is a JSON Schema or already-instantiated config data.
pub fn looks_like_json_schema(value: &Value) -> bool {
    let obj = match value.as_object() {
        Some(map) => map,
        None => return false,
    };

    if obj
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| props.is_empty())
        .unwrap_or(true)
    {
        return false;
    }

    if obj.contains_key("$schema") {
        return true;
    }

    if matches!(obj.get("type"), Some(Value::String(kind)) if kind == "object") {
        return true;
    }

    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        let mut scored = 0usize;

        for value in props.values() {
            if value.get("type").is_some() {
                scored += 1;
            }
            if value.get("properties").is_some() {
                scored += 1;
            }
            if value.get("enum").is_some() {
                scored += 1;
            }
        }

        return scored >= 2;
    }

    false
}

struct DefaultApplier {
    active_refs: HashSet<String>,
}

impl DefaultApplier {
    fn new() -> Self {
        Self {
            active_refs: HashSet::new(),
        }
    }

    fn apply(&mut self, root: &mut Value, defaults: &Value) {
        self.apply_at(root, "", defaults);
    }

    fn apply_at(&mut self, root: &mut Value, pointer: &str, defaults: &Value) {
        let Some(schema) = root.pointer_mut(pointer) else {
            return;
        };
        let Some(schema_obj) = schema.as_object_mut() else {
            return;
        };

        schema_obj.insert("default".to_string(), defaults.clone());

        let mut tasks: Vec<(String, Cow<'_, Value>)> = Vec::new();

        if let Some(default_map) = defaults.as_object() {
            if let Some(properties) = schema_obj.get("properties").and_then(Value::as_object) {
                for key in properties.keys() {
                    if let Some(value) = default_map.get(key) {
                        tasks.push((
                            child_pointer(pointer, &["properties", key]),
                            Cow::Borrowed(value),
                        ));
                    }
                }
            }

            if let Some(patterns) = schema_obj
                .get("patternProperties")
                .and_then(Value::as_object)
            {
                for (pattern, _) in patterns {
                    if let Some(matched) = pattern_defaults(pattern, default_map) {
                        tasks.push((
                            child_pointer(pointer, &["patternProperties", pattern]),
                            Cow::Owned(Value::Object(matched)),
                        ));
                    }
                }
            }

            if let Some(additional_schema) = schema_obj.get("additionalProperties")
                && additional_schema.is_object()
            {
                let excluded: HashSet<&str> = schema_obj
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|props| props.keys().map(|k| k.as_str()).collect())
                    .unwrap_or_default();
                let extras = default_map
                    .iter()
                    .filter(|(key, _)| !excluded.contains(key.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Map<_, _>>();
                if !extras.is_empty() {
                    tasks.push((
                        child_pointer(pointer, &["additionalProperties"]),
                        Cow::Owned(Value::Object(extras)),
                    ));
                }
            }

            if let Some(deps) = schema_obj.get("dependencies").and_then(Value::as_object) {
                for (prop, dep_schema) in deps {
                    if default_map.contains_key(prop) && dep_schema.is_object() {
                        tasks.push((
                            child_pointer(pointer, &["dependencies", prop]),
                            Cow::Borrowed(defaults),
                        ));
                    }
                }
            }

            if let Some(deps) = schema_obj
                .get("dependentSchemas")
                .and_then(Value::as_object)
            {
                for (prop, dep_schema) in deps {
                    if default_map.contains_key(prop) && dep_schema.is_object() {
                        tasks.push((
                            child_pointer(pointer, &["dependentSchemas", prop]),
                            Cow::Borrowed(defaults),
                        ));
                    }
                }
            }
        }

        if let Some(default_array) = defaults.as_array()
            && let Some(items) = schema_obj.get("items")
        {
            match items {
                Value::Array(tuple) => {
                    for (idx, _) in tuple.iter().enumerate() {
                        if let Some(value) = default_array.get(idx) {
                            let idx_str = idx.to_string();
                            tasks.push((
                                child_pointer(pointer, &["items", &idx_str]),
                                Cow::Borrowed(value),
                            ));
                        }
                    }
                }
                Value::Object(_) => {
                    if let Some(first) = default_array.first() {
                        tasks.push((child_pointer(pointer, &["items"]), Cow::Borrowed(first)));
                    }
                }
                _ => {}
            }
        }

        for keyword in ["oneOf", "anyOf", "allOf"] {
            if let Some(Value::Array(branches)) = schema_obj.get(keyword) {
                for idx in 0..branches.len() {
                    let idx_str = idx.to_string();
                    tasks.push((
                        child_pointer(pointer, &[keyword, &idx_str]),
                        Cow::Borrowed(defaults),
                    ));
                }
            }
        }

        let mut ref_targets = Vec::new();
        if let Some(Value::String(reference)) = schema_obj.get("$ref")
            && let Some(target_pointer) = pointer_from_reference(reference)
            && self.active_refs.insert(target_pointer.clone())
        {
            ref_targets.push(target_pointer.clone());
            tasks.push((target_pointer, Cow::Borrowed(defaults)));
        }
        for (child_pointer, value) in tasks {
            self.apply_at(root, &child_pointer, value.as_ref());
        }

        for pointer in ref_targets {
            self.active_refs.remove(&pointer);
        }
    }
}

fn pointer_from_reference(reference: &str) -> Option<String> {
    if let Some(path) = reference.strip_prefix('#') {
        if path.is_empty() {
            Some(String::new())
        } else if path.starts_with('/') {
            Some(path.to_string())
        } else {
            Some(format!("/{}", path))
        }
    } else {
        None
    }
}

fn pattern_defaults(pattern: &str, defaults: &Map<String, Value>) -> Option<Map<String, Value>> {
    let regex = Regex::new(pattern).ok()?;

    let mut matched = Map::new();
    for (key, value) in defaults {
        if regex.is_match(key) {
            matched.insert(key.clone(), value.clone());
        }
    }
    if matched.is_empty() {
        None
    } else {
        Some(matched)
    }
}

#[allow(clippy::if_same_then_else)]
fn child_pointer(base: &str, segments: &[&str]) -> String {
    let mut pointer = base.to_string();
    for segment in segments {
        let escaped = escape_pointer_token(segment);
        if pointer.is_empty() {
            pointer.push('/');
        } else {
            pointer.push('/');
        }
        pointer.push_str(&escaped);
    }
    pointer
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn infer_schema(value: &Value) -> Value {
    match value {
        Value::Null => schema_with_type("null", value),
        Value::Bool(_) => schema_with_type("boolean", value),
        Value::Number(num) => schema_with_type(number_type(num), value),
        Value::String(_) => schema_with_type("string", value),
        Value::Array(items) => array_schema(items),
        Value::Object(map) => object_schema(map),
    }
}

fn schema_with_type(kind: &str, default: &Value) -> Value {
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String(kind.to_string()));
    schema.insert("default".to_string(), default.clone());
    Value::Object(schema)
}

fn object_schema(values: &Map<String, Value>) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (key, value) in values {
        properties.insert(key.clone(), infer_schema(value));
        required.push(Value::String(key.clone()));
    }
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("default".to_string(), Value::Object(values.clone()));
    schema.insert("additionalProperties".to_string(), Value::Bool(true));
    if !properties.is_empty() {
        schema.insert("properties".to_string(), Value::Object(properties));
    }
    if !required.is_empty() {
        schema.insert("required".to_string(), Value::Array(required));
    }
    Value::Object(schema)
}

fn array_schema(items: &[Value]) -> Value {
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("array".to_string()));
    schema.insert("default".to_string(), Value::Array(items.to_vec()));
    if let Some(item_schema) = infer_items_schema(items) {
        schema.insert("items".to_string(), item_schema);
    }
    Value::Object(schema)
}

fn infer_items_schema(items: &[Value]) -> Option<Value> {
    let (first, rest) = items.split_first()?;

    let first_schema = infer_schema(first);
    let first_signature = strip_defaults(&first_schema);
    let mut variants = vec![(first_schema, first_signature)];

    for item in rest {
        let schema = infer_schema(item);
        let signature = strip_defaults(&schema);
        if !variants.iter().any(|(_, existing)| *existing == signature) {
            variants.push((schema, signature));
        }
    }

    if variants.len() == 1 {
        variants.pop().map(|(schema, _)| schema)
    } else {
        let schemas: Vec<Value> = variants.into_iter().map(|(schema, _)| schema).collect();
        Some(json!({ "anyOf": schemas }))
    }
}

fn strip_defaults(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sanitized = Map::new();
            for (key, subvalue) in map {
                if key == "default" {
                    continue;
                }
                sanitized.insert(key.clone(), strip_defaults(subvalue));
            }
            Value::Object(sanitized)
        }
        Value::Array(items) => Value::Array(items.iter().map(strip_defaults).collect()),
        other => other.clone(),
    }
}

fn number_type(number: &Number) -> &'static str {
    if number.is_i64() || number.is_u64() {
        "integer"
    } else {
        "number"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_schema_for_object_defaults() {
        let value = json!({"host": "localhost", "port": 8080});
        let schema = schema_from_data_value(&value);
        assert_eq!(schema["properties"]["host"]["default"], json!("localhost"));
        assert_eq!(schema["properties"]["port"]["type"], json!("integer"));
        assert_eq!(schema["default"], value);
    }

    #[test]
    fn builds_schema_for_array_defaults() {
        let value = json!(["a", "b"]);
        let schema = schema_from_data_value(&value);
        assert_eq!(schema["type"], json!("array"));
        assert_eq!(schema["default"], value);
        assert_eq!(schema["items"]["type"], json!("string"));
    }

    #[cfg(feature = "json")]
    #[test]
    fn parse_json_documents() {
        let raw = "{\"enabled\":true}";
        let parsed = parse_document_str(raw, DocumentFormat::Json).unwrap();
        assert_eq!(parsed["enabled"], Value::Bool(true));
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn parse_yaml_documents() {
        let raw = "enabled: true\nname: dev";
        let parsed = parse_document_str(raw, DocumentFormat::Yaml).unwrap();
        assert_eq!(parsed["enabled"], Value::Bool(true));
        assert_eq!(parsed["name"], json!("dev"));
    }

    #[cfg(feature = "toml")]
    #[test]
    fn parse_toml_documents() {
        let raw = "enabled = true\nname = \"dev\"";
        let parsed = parse_document_str(raw, DocumentFormat::Toml).unwrap();
        assert_eq!(parsed["enabled"], Value::Bool(true));
        assert_eq!(parsed["name"], json!("dev"));
    }

    #[test]
    fn merges_defaults_into_existing_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "host": {"type": "string"},
                "port": {"type": "integer"}
            }
        });
        let defaults = json!({"host": "localhost", "port": 8080});
        let enriched = schema_with_defaults(&schema, &defaults);
        assert_eq!(
            enriched["properties"]["host"]["default"],
            json!("localhost")
        );
        assert_eq!(enriched["properties"]["port"]["default"], json!(8080));
        assert_eq!(enriched["default"], defaults);
    }

    #[test]
    fn merges_array_defaults() {
        let schema = json!({
            "type": "array",
            "items": {"type": "object", "properties": {"tag": {"type": "string"}}}
        });
        let defaults = json!([{ "tag": "api" }]);
        let enriched = schema_with_defaults(&schema, &defaults);
        assert_eq!(enriched["default"], defaults);
        assert_eq!(
            enriched["items"]["properties"]["tag"]["default"],
            json!("api")
        );
    }

    #[test]
    fn propagates_defaults_through_refs() {
        let schema = json!({
            "definitions": {
                "endpoint": {
                    "type": "object",
                    "properties": {
                        "host": {"type": "string"},
                        "port": {"type": "integer"}
                    }
                }
            },
            "type": "object",
            "properties": {
                "service": {"$ref": "#/definitions/endpoint"}
            }
        });
        let defaults = json!({
            "service": {"host": "localhost", "port": 8080}
        });
        let enriched = schema_with_defaults(&schema, &defaults);
        assert_eq!(
            enriched["properties"]["service"]["default"]["host"],
            json!("localhost")
        );
        assert_eq!(
            enriched["definitions"]["endpoint"]["properties"]["port"]["default"],
            json!(8080)
        );
    }

    #[test]
    fn applies_defaults_for_pattern_properties() {
        let schema = json!({
            "type": "object",
            "patternProperties": {
                "^env_": {"type": "string"}
            }
        });
        let defaults = json!({"env_api": "v1", "other": "noop"});
        let enriched = schema_with_defaults(&schema, &defaults);
        assert_eq!(enriched["default"], defaults);
        assert_eq!(
            enriched["patternProperties"]["^env_"]["default"]["env_api"],
            json!("v1")
        );
        assert!(
            enriched["patternProperties"]["^env_"]["default"]
                .get("other")
                .is_none()
        );
    }
}
