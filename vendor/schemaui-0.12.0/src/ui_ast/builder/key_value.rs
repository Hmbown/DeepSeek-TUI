use serde_json::Value;

use super::{Result, SchemaObject, SchemaResolver, UiKeyValueNode};

pub(super) fn build_key_value_template(
    resolver: &SchemaResolver<'_>,
    schema: &SchemaObject,
    active_refs: &mut Vec<String>,
) -> Result<Option<UiKeyValueNode>> {
    let Some(object) = schema.object.as_ref() else {
        return Ok(None);
    };
    if !object.properties.is_empty() {
        return Ok(None);
    }

    let (value_schema_ref, key_schema_override) =
        if let Some((pattern, pattern_schema)) = object.pattern_properties.iter().next() {
            (
                pattern_schema,
                Some(serde_json::json!({
                    "type": "string",
                    "pattern": pattern,
                    "title": "Key",
                })),
            )
        } else if let Some(additional) = object.additional_properties.as_ref() {
            if matches!(
                &**additional,
                super::Schema::Bool(false) | super::Schema::Bool(true)
            ) {
                return Ok(None);
            }
            (additional.as_ref(), None)
        } else {
            return Ok(None);
        };

    let (value_resolved, value_kind) = super::visit::with_resolved_schema(
        resolver,
        value_schema_ref,
        active_refs,
        |resolved| {
            let kind = super::visit::normalize_embedded_kind(
                resolver,
                &resolved,
                super::visit::recursive_boundary_kind(&resolved),
            )?;
            Ok((resolved, kind))
        },
        |resolved, active_refs| {
            let kind = super::visit::visit_kind(resolver, &resolved, active_refs)?;
            let kind = super::visit::normalize_embedded_kind(resolver, &resolved, kind)?;
            Ok((resolved, kind))
        },
    )?;

    let value_schema = super::schema_helpers::schema_to_value_with_defs(resolver, &value_resolved)?;
    let (value_title, value_description, value_default) =
        super::schema_helpers::schema_titles(&value_resolved, "Value");

    let (key_schema, key_title, key_description, key_default) = if let Some(override_schema) =
        key_schema_override
    {
        (override_schema, "Key".to_string(), None, None)
    } else if let Some(property_names) = object.property_names.as_ref() {
        let key_resolved = resolver.resolve_schema(property_names)?;
        let key_schema = super::schema_helpers::schema_to_value_with_defs(resolver, &key_resolved)?;
        let (title, description, default) =
            super::schema_helpers::schema_titles(&key_resolved, "Key");
        (key_schema, title, description, default)
    } else {
        (
            serde_json::json!({"type": "string", "title": "Key"}),
            "Key".to_string(),
            None,
            None,
        )
    };

    Ok(Some(UiKeyValueNode {
        key_title,
        key_description,
        key_default,
        key_schema: key_schema.clone(),
        value_title,
        value_description,
        value_default,
        value_schema: value_schema.clone(),
        value_kind: Box::new(value_kind),
        entry_schema: key_value_entry_schema(&key_schema, &value_schema),
    }))
}

fn key_value_entry_schema(key_schema: &Value, value_schema: &Value) -> Value {
    serde_json::json!({
        "type": "object",
        "required": ["key", "value"],
        "properties": {
            "key": key_schema,
            "value": value_schema,
        }
    })
}
