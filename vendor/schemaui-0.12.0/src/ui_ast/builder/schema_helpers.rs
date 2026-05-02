use serde_json::Value;

use super::{
    Context, Map, ObjectValidation, Result, Schema, SchemaObject, SchemaResolver, SingleOrVec,
};

pub(super) fn merge_all_of(
    resolver: &SchemaResolver<'_>,
    all_of: &[Schema],
) -> Result<SchemaObject> {
    if all_of.is_empty() {
        anyhow::bail!("allOf must contain at least one schema");
    }

    let mut acc = Value::Object(Map::new());
    for schema in all_of {
        let resolved = resolver.resolve_schema(schema)?;
        let value = schema_to_value(&resolved)?;
        acc = super::naming::deep_merge(acc, value);
    }

    serde_json::from_value::<SchemaObject>(acc).context("failed to deserialize merged allOf schema")
}

pub(super) fn array_item_schema(array: &super::ArrayValidation) -> Result<&Schema> {
    let items = array
        .items
        .as_ref()
        .context("array items must be present")?;
    match items {
        SingleOrVec::Single(schema) => Ok(schema.as_ref()),
        SingleOrVec::Vec(list) => list
            .first()
            .context("tuple arrays must have at least one item"),
    }
}

pub(super) fn schema_allows_null(schema: &SchemaObject) -> bool {
    schema
        .instance_type
        .as_ref()
        .is_some_and(|inner| match inner {
            SingleOrVec::Single(single) => matches!(**single, super::InstanceType::Null),
            SingleOrVec::Vec(list) => list.contains(&super::InstanceType::Null),
        })
}

pub(super) fn instance_type(schema: &SchemaObject) -> Option<super::InstanceType> {
    schema.instance_type.as_ref().and_then(|inner| match inner {
        SingleOrVec::Single(single) => Some(**single),
        SingleOrVec::Vec(list) => list
            .iter()
            .cloned()
            .find(|item| *item != super::InstanceType::Null),
    })
}

pub(super) fn is_object_schema(schema: &SchemaObject) -> bool {
    match instance_type(schema) {
        Some(super::InstanceType::Object) => true,
        None => schema.object.is_some(),
        _ => false,
    }
}

pub(super) fn has_composite_subschemas(schema: &SchemaObject) -> bool {
    schema.subschemas.as_ref().is_some_and(|subs| {
        subs.one_of.as_ref().is_some_and(|list| !list.is_empty())
            || subs.any_of.as_ref().is_some_and(|list| !list.is_empty())
    })
}

pub(super) fn is_array_schema(schema: &SchemaObject) -> bool {
    match instance_type(schema) {
        Some(super::InstanceType::Array) => true,
        _ => schema.array.is_some(),
    }
}

pub(super) fn required_list(object: &ObjectValidation) -> Vec<String> {
    object.required.to_vec()
}

pub(super) fn schema_to_value(schema: &SchemaObject) -> Result<Value> {
    serde_json::to_value(Schema::Object(Box::new(schema.clone())))
        .context("failed to serialize schema")
}

pub(super) fn schema_to_value_with_defs(
    resolver: &SchemaResolver<'_>,
    schema: &SchemaObject,
) -> Result<Value> {
    let mut value = schema_to_value(schema)?;
    if let Value::Object(ref mut map) = value {
        resolver.root_dialect_context().apply_to_overlay(map);
    }
    Ok(value)
}

pub(super) fn schema_title(schema: &SchemaObject) -> Option<String> {
    schema.metadata.as_ref()?.title.clone()
}

pub(super) fn schema_description(schema: &SchemaObject) -> Option<String> {
    schema.metadata.as_ref()?.description.clone()
}

pub(super) fn schema_titles(
    schema: &SchemaObject,
    fallback: &str,
) -> (String, Option<String>, Option<Value>) {
    (
        schema_title(schema).unwrap_or_else(|| fallback.to_string()),
        schema_description(schema),
        super::defaults::schema_default_or_const(schema),
    )
}
