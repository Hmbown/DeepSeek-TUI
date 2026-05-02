use super::{
    CompositeMode, Result, Schema, SchemaObject, SchemaResolver, UiNode, UiNodeKind, UiVariant,
};

pub(super) fn build_composite_kind(
    resolver: &SchemaResolver<'_>,
    schemas: &[Schema],
    mode: CompositeMode,
    active_refs: &mut Vec<String>,
) -> Result<UiNodeKind> {
    let variants = build_variants(resolver, schemas, active_refs)?;
    Ok(UiNodeKind::Composite {
        mode,
        allow_multiple: false,
        variants,
    })
}

pub(super) fn build_composite_node(
    resolver: &SchemaResolver<'_>,
    schemas: &[Schema],
    mode: CompositeMode,
    schema: &SchemaObject,
    pointer: String,
    required: bool,
    active_refs: &mut Vec<String>,
) -> Result<UiNode> {
    let kind = build_composite_kind(resolver, schemas, mode, active_refs)?;
    let default_value = if let UiNodeKind::Composite {
        variants,
        allow_multiple,
        ..
    } = &kind
    {
        super::defaults::infer_default_for_composite(variants, *allow_multiple)
    } else {
        None
    };

    Ok(UiNode {
        pointer,
        title: super::schema_helpers::schema_title(schema),
        description: super::schema_helpers::schema_description(schema),
        required,
        default_value,
        kind,
    })
}

pub(super) fn build_variant(
    resolver: &SchemaResolver<'_>,
    schema: &Schema,
    index: usize,
    active_refs: &mut Vec<String>,
) -> Result<UiVariant> {
    super::visit::with_resolved_schema(
        resolver,
        schema,
        active_refs,
        |resolved| {
            build_variant_from_resolved_schema(
                resolver,
                index,
                &resolved,
                super::visit::recursive_boundary_kind(&resolved),
            )
        },
        |resolved, active_refs| {
            let node = super::visit::visit_kind(resolver, &resolved, active_refs)?;
            build_variant_from_resolved_schema(resolver, index, &resolved, node)
        },
    )
}

pub(super) fn build_variant_from_resolved_schema(
    resolver: &SchemaResolver<'_>,
    index: usize,
    schema: &SchemaObject,
    node: UiNodeKind,
) -> Result<UiVariant> {
    let schema_value = super::schema_helpers::schema_to_value_with_defs(resolver, schema)?;
    let title = super::schema_helpers::schema_title(schema)
        .or_else(|| Some(super::naming::default_variant_title(index, schema)));
    let description = super::schema_helpers::schema_description(schema);
    let is_object = super::schema_helpers::is_object_schema(schema);

    Ok(UiVariant {
        id: format!("variant_{index}"),
        title,
        description,
        is_object,
        node,
        schema: schema_value,
    })
}

pub(super) fn build_single_variant_composite_kind(
    resolver: &SchemaResolver<'_>,
    schema: &SchemaObject,
    active_refs: &mut Vec<String>,
) -> Result<UiNodeKind> {
    let node = super::visit::visit_kind(resolver, schema, active_refs)?;
    build_single_variant_overlay_kind(resolver, schema, node)
}

pub(super) fn build_single_variant_overlay_kind(
    resolver: &SchemaResolver<'_>,
    schema: &SchemaObject,
    node: UiNodeKind,
) -> Result<UiNodeKind> {
    let variant = build_variant_from_resolved_schema(resolver, 0, schema, node)?;
    Ok(UiNodeKind::Composite {
        mode: CompositeMode::OneOf,
        allow_multiple: false,
        variants: vec![variant],
    })
}

fn build_variants(
    resolver: &SchemaResolver<'_>,
    schemas: &[Schema],
    active_refs: &mut Vec<String>,
) -> Result<Vec<UiVariant>> {
    let mut out = Vec::new();
    for (index, variant) in schemas.iter().enumerate() {
        out.push(build_variant(resolver, variant, index, active_refs)?);
    }
    Ok(out)
}
