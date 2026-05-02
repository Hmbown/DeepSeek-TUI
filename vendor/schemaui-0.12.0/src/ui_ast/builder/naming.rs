use serde_json::Value;

use super::{InstanceType, Schema, SchemaObject, SingleOrVec};

pub(super) fn default_variant_title(index: usize, schema: &SchemaObject) -> String {
    if let Some(reference) = schema.reference.as_ref()
        && let Some(name) = reference.split('/').next_back()
    {
        return humanize_identifier(name);
    }

    if let Some(object) = schema.object.as_ref() {
        if let Some(type_prop) = object.properties.get("type")
            && let Some(const_value) = get_const_value(type_prop)
            && let Some(text) = const_value.as_str()
        {
            return text.to_string();
        }

        if let Some(kind_prop) = object.properties.get("kind")
            && let Some(const_value) = get_const_value(kind_prop)
            && let Some(text) = const_value.as_str()
        {
            return humanize_identifier(text);
        }

        for key in ["id", "name", "key"] {
            if object.properties.contains_key(key) {
                let base_type = super::schema_helpers::instance_type(schema)
                    .map(|kind| format!("{kind:?}").to_lowercase())
                    .unwrap_or_else(|| "variant".to_string());
                return format!("{base_type} with {key}");
            }
        }
    }

    if let Some(array) = schema.array.as_ref()
        && let Some(items) = &array.items
    {
        match items {
            SingleOrVec::Single(item_schema) => {
                if let Schema::Object(item_object) = item_schema.as_ref() {
                    let item_object = item_object.as_ref();
                    if let Some(item_ref) = item_object.reference.as_ref()
                        && let Some(name) = item_ref.split('/').next_back()
                    {
                        return format!("{} array", humanize_identifier(name));
                    }

                    if let Some(item_instance) = super::schema_helpers::instance_type(item_object) {
                        let kind = match item_instance {
                            InstanceType::String => Some("string"),
                            InstanceType::Integer => Some("integer"),
                            InstanceType::Number => Some("number"),
                            InstanceType::Boolean => Some("boolean"),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            return format!("List<{kind}>");
                        }
                    }
                }
            }
            SingleOrVec::Vec(_) => return "Tuple array".to_string(),
        }
    }

    if let Some(instance) = super::schema_helpers::instance_type(schema) {
        return match instance {
            InstanceType::String => "Text".to_string(),
            InstanceType::Integer => "Integer".to_string(),
            InstanceType::Number => "Number".to_string(),
            InstanceType::Boolean => "Boolean".to_string(),
            InstanceType::Array => "List".to_string(),
            InstanceType::Object => "Object".to_string(),
            InstanceType::Null => "Null".to_string(),
        };
    }

    format!("Option {}", index + 1)
}

pub(super) fn append_pointer(base: &str, segment: &str) -> String {
    let encoded = segment.replace('~', "~0").replace('/', "~1");
    if base.is_empty() || base == "/" {
        format!("/{encoded}")
    } else if base.ends_with('/') {
        format!("{base}{encoded}")
    } else {
        format!("{base}/{encoded}")
    }
}

pub(super) fn deep_merge(base: Value, addition: Value) -> Value {
    match (base, addition) {
        (Value::Object(mut left), Value::Object(right)) => {
            for (key, value) in right {
                let merged = if let Some(existing) = left.remove(&key) {
                    deep_merge(existing, value)
                } else {
                    value
                };
                left.insert(key, merged);
            }
            Value::Object(left)
        }
        (Value::Array(mut left), Value::Array(mut right)) => {
            left.append(&mut right);
            dedup_array(&mut left);
            Value::Array(left)
        }
        (_, new_value) => new_value,
    }
}

fn humanize_identifier(text: &str) -> String {
    let mut result = String::new();
    let mut prev_upper = false;

    for (index, ch) in text.chars().enumerate() {
        if ch.is_uppercase() {
            if index > 0 && !prev_upper {
                result.push(' ');
            }
            result.push(ch);
            prev_upper = true;
        } else {
            if index == 0 {
                result.push(ch.to_ascii_uppercase());
            } else {
                result.push(ch);
            }
            prev_upper = false;
        }
    }

    result
}

fn get_const_value(schema: &Schema) -> Option<&Value> {
    if let Schema::Object(object) = schema {
        let object = object.as_ref();
        if let Some(const_value) = object.const_value.as_ref() {
            return Some(const_value);
        }
        if let Some(const_value) = object.extensions.get("const") {
            return Some(const_value);
        }
    }
    None
}

fn dedup_array(values: &mut Vec<Value>) {
    let mut index = 0;
    while index < values.len() {
        let is_duplicate = values[..index]
            .iter()
            .any(|existing| existing == &values[index]);
        if is_duplicate {
            values.remove(index);
        } else {
            index += 1;
        }
    }
}
