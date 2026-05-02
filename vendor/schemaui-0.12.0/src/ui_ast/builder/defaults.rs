use serde_json::{Map, Value};

use super::{Result, ScalarKind, SchemaObject, UiNodeKind, UiVariant};

pub(super) type DetectedScalar = (ScalarKind, Option<Vec<String>>, Option<Vec<Value>>, bool);

pub(super) fn schema_default_or_const(schema: &SchemaObject) -> Option<Value> {
    schema_default(schema).or_else(|| schema_const_value(schema).cloned())
}

pub(super) fn infer_default_scalar(scalar: ScalarKind, opts: Option<&Vec<Value>>) -> Option<Value> {
    if let Some(options) = opts
        && let Some(first) = options.first()
    {
        return Some(first.clone());
    }

    let value = match scalar {
        ScalarKind::String => Value::String(String::new()),
        ScalarKind::Integer | ScalarKind::Number => Value::Number(0.into()),
        ScalarKind::Boolean => Value::Bool(false),
    };
    Some(value)
}

pub(super) fn infer_default_for_composite(
    variants: &[UiVariant],
    allow_multiple: bool,
) -> Option<Value> {
    if allow_multiple {
        return Some(Value::Array(Vec::new()));
    }

    variants.first().and_then(generate_variant_default)
}

pub(super) fn detect_scalar(schema: &SchemaObject) -> Result<DetectedScalar> {
    if let Some(enum_values) = schema.enum_values.as_ref()
        && !enum_values.is_empty()
    {
        let labels = enum_values.iter().map(enum_label).collect::<Vec<_>>();
        let nullable = enum_values.iter().any(Value::is_null);
        return Ok((
            infer_enum_scalar(enum_values),
            Some(labels),
            Some(enum_values.clone()),
            nullable,
        ));
    }

    if let Some(const_value) = schema_const_value(schema) {
        let labels = vec![enum_label(const_value)];
        let values = vec![const_value.clone()];
        let nullable = values.iter().any(Value::is_null);
        return Ok((
            infer_enum_scalar(&values),
            Some(labels),
            Some(values),
            nullable,
        ));
    }

    let instance = super::schema_helpers::instance_type(schema);
    if matches!(instance, Some(super::InstanceType::Null)) {
        return Ok((
            ScalarKind::String,
            Some(vec!["null".to_string()]),
            Some(vec![Value::Null]),
            true,
        ));
    }

    let scalar = match instance {
        Some(super::InstanceType::String) | None => ScalarKind::String,
        Some(super::InstanceType::Integer) => ScalarKind::Integer,
        Some(super::InstanceType::Number) => ScalarKind::Number,
        Some(super::InstanceType::Boolean) => ScalarKind::Boolean,
        Some(super::InstanceType::Null) => {
            unreachable!("null instance is handled as a fixed null enum")
        }
        Some(super::InstanceType::Array | super::InstanceType::Object) => {
            anyhow::bail!("composite types should be handled earlier")
        }
    };

    Ok((
        scalar,
        None,
        None,
        super::schema_helpers::schema_allows_null(schema),
    ))
}

fn schema_default(schema: &SchemaObject) -> Option<Value> {
    schema
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.default.clone())
}

fn schema_const_value(schema: &SchemaObject) -> Option<&Value> {
    schema
        .const_value
        .as_ref()
        .or_else(|| schema.extensions.get("const"))
}

fn generate_variant_default(variant: &UiVariant) -> Option<Value> {
    if variant.is_object
        && let UiNodeKind::Object { children, required } = &variant.node
    {
        let mut object = Map::new();

        if let Value::Object(schema_object) = &variant.schema
            && let Some(Value::Object(properties)) = schema_object.get("properties")
        {
            for (key, property_schema) in properties {
                if let Value::Object(property_object) = property_schema
                    && let Some(const_value) = property_object.get("const")
                {
                    object.insert(key.clone(), const_value.clone());
                }
            }
        }

        for child in children {
            let field_name = child.pointer.split('/').next_back().unwrap_or("");
            if !field_name.is_empty()
                && required.contains(&field_name.to_string())
                && !object.contains_key(field_name)
            {
                if let Some(default) = &child.default_value {
                    object.insert(field_name.to_string(), default.clone());
                } else if let Some(default) = default_for_kind(&child.kind) {
                    object.insert(field_name.to_string(), default);
                }
            }
        }

        return Some(Value::Object(object));
    }

    if let UiNodeKind::Array { item, .. } = &variant.node
        && let Some(item_default) = default_for_kind(item)
    {
        return Some(Value::Array(vec![item_default]));
    }

    default_for_kind(&variant.node)
}

fn default_for_kind(kind: &UiNodeKind) -> Option<Value> {
    match kind {
        UiNodeKind::Field {
            scalar,
            enum_values,
            ..
        } => infer_default_scalar(*scalar, enum_values.as_ref()),
        UiNodeKind::Array { .. } => Some(Value::Array(Vec::new())),
        UiNodeKind::KeyValue { .. } | UiNodeKind::Object { .. } => Some(Value::Object(Map::new())),
        UiNodeKind::Composite {
            variants,
            allow_multiple,
            ..
        } => infer_default_for_composite(variants, *allow_multiple),
    }
}

fn enum_label(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Array(items) => items.iter().map(enum_label).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn infer_enum_scalar(values: &[Value]) -> ScalarKind {
    let mut inferred = None;
    for value in values {
        let current = match value {
            Value::Number(number) if number.is_i64() || number.is_u64() => ScalarKind::Integer,
            Value::Number(_) => ScalarKind::Number,
            Value::Bool(_) => ScalarKind::Boolean,
            Value::String(_) => ScalarKind::String,
            _ => return ScalarKind::String,
        };
        match inferred {
            Some(existing) if existing != current => return ScalarKind::String,
            Some(_) => {}
            None => inferred = Some(current),
        }
    }
    inferred.unwrap_or(ScalarKind::String)
}
