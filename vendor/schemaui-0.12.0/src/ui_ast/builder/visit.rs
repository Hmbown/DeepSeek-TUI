use serde_json::{Map, Value};

use super::{
    ArrayValidation, ObjectValidation, Result, Schema, SchemaObject, SchemaResolver, UiNode,
    UiNodeKind,
};

pub(super) fn visit_schema_entry(
    resolver: &SchemaResolver<'_>,
    schema: &Schema,
    pointer: String,
    required: bool,
    active_refs: &mut Vec<String>,
) -> Result<UiNode> {
    let recursive_pointer = pointer.clone();
    with_resolved_schema(
        resolver,
        schema,
        active_refs,
        move |resolved| {
            Ok(recursive_boundary_node(
                &resolved,
                recursive_pointer,
                required,
            ))
        },
        move |resolved, active_refs| {
            visit_schema(resolver, &resolved, pointer, required, active_refs)
        },
    )
}

pub(super) fn visit_schema(
    resolver: &SchemaResolver<'_>,
    schema: &SchemaObject,
    pointer: String,
    required: bool,
    active_refs: &mut Vec<String>,
) -> Result<UiNode> {
    if let Some(subschemas) = schema.subschemas.as_ref()
        && let Some(all_of) = subschemas.all_of.as_ref()
        && !all_of.is_empty()
    {
        let merged = super::schema_helpers::merge_all_of(resolver, all_of)?;
        return visit_schema(resolver, &merged, pointer, required, active_refs);
    }

    if let Some(subschemas) = schema.subschemas.as_ref() {
        if let Some(one_of) = subschemas.one_of.as_ref() {
            return super::composite::build_composite_node(
                resolver,
                one_of,
                super::CompositeMode::OneOf,
                schema,
                pointer,
                required,
                active_refs,
            );
        }
        if let Some(any_of) = subschemas.any_of.as_ref() {
            return super::composite::build_composite_node(
                resolver,
                any_of,
                super::CompositeMode::AnyOf,
                schema,
                pointer,
                required,
                active_refs,
            );
        }
    }

    if let Some(template) =
        super::key_value::build_key_value_template(resolver, schema, active_refs)?
    {
        return Ok(UiNode {
            pointer,
            title: super::schema_helpers::schema_title(schema),
            description: super::schema_helpers::schema_description(schema),
            required,
            default_value: super::defaults::schema_default_or_const(schema),
            kind: UiNodeKind::KeyValue {
                template: Box::new(template),
            },
        });
    }

    if super::schema_helpers::is_array_schema(schema) {
        let array = schema.array.as_ref();
        let item_node = match array {
            Some(array) if array.items.is_some() => {
                visit_array_item_kind(resolver, array, active_refs)?
            }
            _ => array_boundary_item_kind(),
        };
        let default_value = super::defaults::schema_default_or_const(schema)
            .or_else(|| Some(Value::Array(Vec::new())));
        return Ok(UiNode {
            pointer,
            title: super::schema_helpers::schema_title(schema),
            description: super::schema_helpers::schema_description(schema),
            required,
            default_value,
            kind: UiNodeKind::Array {
                item: Box::new(item_node),
                min_items: array.and_then(|inner| inner.min_items).map(u64::from),
                max_items: array.and_then(|inner| inner.max_items).map(u64::from),
            },
        });
    }

    if super::schema_helpers::is_object_schema(schema) {
        let fallback_object = ObjectValidation::default();
        let object = schema.object.as_ref().map_or(&fallback_object, Box::as_ref);
        let required_fields = super::schema_helpers::required_list(object);
        let mut children = Vec::new();
        for (name, child_schema) in &object.properties {
            let child_pointer = super::naming::append_pointer(&pointer, name);
            let child = visit_schema_entry(
                resolver,
                child_schema,
                child_pointer,
                required_fields.contains(name),
                active_refs,
            )?;
            children.push(child);
        }
        let default_value =
            super::defaults::schema_default_or_const(schema).or(Some(Value::Object(Map::new())));
        return Ok(UiNode {
            pointer,
            title: super::schema_helpers::schema_title(schema),
            description: super::schema_helpers::schema_description(schema),
            required,
            default_value,
            kind: UiNodeKind::Object {
                children,
                required: required_fields,
            },
        });
    }

    let (scalar, enum_options, enum_values, nullable) = super::defaults::detect_scalar(schema)?;
    let default_value = super::defaults::schema_default_or_const(schema)
        .or_else(|| super::defaults::infer_default_scalar(scalar, enum_values.as_ref()));
    Ok(UiNode {
        pointer,
        title: super::schema_helpers::schema_title(schema),
        description: super::schema_helpers::schema_description(schema),
        required,
        default_value,
        kind: UiNodeKind::Field {
            scalar,
            enum_options,
            enum_values,
            nullable,
        },
    })
}

pub(super) fn visit_kind(
    resolver: &SchemaResolver<'_>,
    schema: &SchemaObject,
    active_refs: &mut Vec<String>,
) -> Result<UiNodeKind> {
    if let Some(subschemas) = schema.subschemas.as_ref()
        && let Some(all_of) = subschemas.all_of.as_ref()
        && !all_of.is_empty()
    {
        let merged = super::schema_helpers::merge_all_of(resolver, all_of)?;
        return visit_kind(resolver, &merged, active_refs);
    }

    if let Some(subschemas) = schema.subschemas.as_ref() {
        if let Some(one_of) = subschemas.one_of.as_ref() {
            return super::composite::build_composite_kind(
                resolver,
                one_of,
                super::CompositeMode::OneOf,
                active_refs,
            );
        }
        if let Some(any_of) = subschemas.any_of.as_ref() {
            return super::composite::build_composite_kind(
                resolver,
                any_of,
                super::CompositeMode::AnyOf,
                active_refs,
            );
        }
    }

    if let Some(template) =
        super::key_value::build_key_value_template(resolver, schema, active_refs)?
    {
        return Ok(UiNodeKind::KeyValue {
            template: Box::new(template),
        });
    }

    if super::schema_helpers::is_array_schema(schema) {
        let array = schema.array.as_ref();
        let item_node = match array {
            Some(array) if array.items.is_some() => {
                visit_array_item_kind(resolver, array, active_refs)?
            }
            _ => array_boundary_item_kind(),
        };
        return Ok(UiNodeKind::Array {
            item: Box::new(item_node),
            min_items: array.and_then(|inner| inner.min_items).map(u64::from),
            max_items: array.and_then(|inner| inner.max_items).map(u64::from),
        });
    }

    if super::schema_helpers::is_object_schema(schema) {
        let fallback_object = ObjectValidation::default();
        let object = schema.object.as_ref().map_or(&fallback_object, Box::as_ref);
        let required_fields = super::schema_helpers::required_list(object);
        let mut children = Vec::new();
        for (name, child_schema) in &object.properties {
            let pointer = super::naming::append_pointer("", name);
            let node = visit_schema_entry(
                resolver,
                child_schema,
                pointer,
                required_fields.contains(name),
                active_refs,
            )?;
            children.push(node);
        }
        return Ok(UiNodeKind::Object {
            children,
            required: required_fields,
        });
    }

    let (scalar, enum_options, enum_values, nullable) = super::defaults::detect_scalar(schema)?;
    Ok(UiNodeKind::Field {
        scalar,
        enum_options,
        enum_values,
        nullable,
    })
}

pub(super) fn visit_array_item_kind(
    resolver: &SchemaResolver<'_>,
    array: &ArrayValidation,
    active_refs: &mut Vec<String>,
) -> Result<UiNodeKind> {
    let item_schema = super::schema_helpers::array_item_schema(array)?;
    with_resolved_schema(
        resolver,
        item_schema,
        active_refs,
        |resolved| normalize_embedded_kind(resolver, &resolved, recursive_boundary_kind(&resolved)),
        |resolved, active_refs| {
            if super::schema_helpers::is_object_schema(&resolved)
                && !super::schema_helpers::has_composite_subschemas(&resolved)
            {
                super::composite::build_single_variant_composite_kind(
                    resolver,
                    &resolved,
                    active_refs,
                )
            } else {
                let kind = visit_kind(resolver, &resolved, active_refs)?;
                normalize_embedded_kind(resolver, &resolved, kind)
            }
        },
    )
}

pub(super) fn normalize_embedded_kind(
    resolver: &SchemaResolver<'_>,
    schema: &SchemaObject,
    kind: UiNodeKind,
) -> Result<UiNodeKind> {
    match kind {
        kind @ UiNodeKind::Array { .. } | kind @ UiNodeKind::Object { .. } => {
            super::composite::build_single_variant_overlay_kind(resolver, schema, kind)
        }
        other => Ok(other),
    }
}

pub(super) fn recursive_boundary_kind(schema: &SchemaObject) -> UiNodeKind {
    if super::schema_helpers::is_array_schema(schema) {
        let array = schema.array.as_ref();
        return UiNodeKind::Array {
            item: Box::new(array_boundary_item_kind()),
            min_items: array.and_then(|inner| inner.min_items).map(u64::from),
            max_items: array.and_then(|inner| inner.max_items).map(u64::from),
        };
    }

    if let Ok((scalar, enum_options, enum_values, nullable)) =
        super::defaults::detect_scalar(schema)
    {
        return UiNodeKind::Field {
            scalar,
            enum_options,
            enum_values,
            nullable,
        };
    }

    UiNodeKind::Object {
        children: Vec::new(),
        required: Vec::new(),
    }
}

pub(super) fn with_resolved_schema<T, F, R>(
    resolver: &SchemaResolver<'_>,
    schema: &Schema,
    active_refs: &mut Vec<String>,
    on_recursive: R,
    on_resolved: F,
) -> Result<T>
where
    F: FnOnce(SchemaObject, &mut Vec<String>) -> Result<T>,
    R: FnOnce(SchemaObject) -> Result<T>,
{
    let resolved = resolver.resolve_schema(schema)?;
    if let Some(reference) = super::schema_reference(schema) {
        if active_refs.iter().any(|active| active == reference) {
            return on_recursive(resolved);
        }
        active_refs.push(reference.to_string());
        let result = on_resolved(resolved, active_refs);
        active_refs.pop();
        result
    } else {
        on_resolved(resolved, active_refs)
    }
}

fn array_boundary_item_kind() -> UiNodeKind {
    UiNodeKind::Object {
        children: Vec::new(),
        required: Vec::new(),
    }
}

fn recursive_boundary_node(schema: &SchemaObject, pointer: String, required: bool) -> UiNode {
    let kind = recursive_boundary_kind(schema);
    let default_value = match &kind {
        UiNodeKind::Field {
            scalar,
            enum_values,
            ..
        } => super::defaults::schema_default_or_const(schema)
            .or_else(|| super::defaults::infer_default_scalar(*scalar, enum_values.as_ref())),
        UiNodeKind::Array { .. } => super::defaults::schema_default_or_const(schema)
            .or_else(|| Some(Value::Array(Vec::new()))),
        UiNodeKind::KeyValue { .. } | UiNodeKind::Object { .. } => {
            super::defaults::schema_default_or_const(schema)
                .or_else(|| Some(Value::Object(Map::new())))
        }
        UiNodeKind::Composite {
            variants,
            allow_multiple,
            ..
        } => super::defaults::schema_default_or_const(schema)
            .or_else(|| super::defaults::infer_default_for_composite(variants, *allow_multiple)),
    };

    UiNode {
        pointer,
        title: super::schema_helpers::schema_title(schema),
        description: super::schema_helpers::schema_description(schema),
        required,
        default_value,
        kind,
    }
}
