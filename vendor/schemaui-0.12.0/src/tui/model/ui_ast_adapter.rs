use std::collections::HashMap;

use crate::ui_ast::{CompositeMode, ScalarKind, UiAst, UiNode, UiNodeKind, UiVariant};

use super::form_schema::{
    CompositeField, CompositeMode as DomainCompositeMode, CompositeVariant, FieldKind, FieldSchema,
    FormSchema, FormSection, KeyValueField, RootSection,
};

/// Build a legacy `FormSchema` tree from the canonical [`UiAst`].
pub fn form_schema_from_ui_ast(ast: &UiAst) -> FormSchema {
    let mut roots = Vec::new();
    let mut general_fields = Vec::new();

    for node in &ast.roots {
        if is_section_object(node) {
            let section = build_section_from_object(node);
            roots.push(RootSection {
                id: section.id.clone(),
                title: node
                    .title
                    .clone()
                    .unwrap_or_else(|| prettify_label(&section.id)),
                description: node.description.clone(),
                sections: vec![section],
            });
        } else {
            general_fields.push(field_schema_from_node(node));
        }
    }

    if !general_fields.is_empty() {
        roots.insert(
            0,
            RootSection {
                id: "general".into(),
                title: "General".into(),
                description: None,
                sections: vec![FormSection {
                    id: "general".into(),
                    title: "General".into(),
                    description: None,
                    path: Vec::new(),
                    fields: general_fields,
                    children: Vec::new(),
                }],
            },
        );
    }

    if roots.is_empty() {
        roots.push(RootSection {
            id: "general".into(),
            title: "General".into(),
            description: None,
            sections: vec![FormSection {
                id: "general".into(),
                title: "General".into(),
                description: None,
                path: Vec::new(),
                fields: Vec::new(),
                children: Vec::new(),
            }],
        });
    }

    FormSchema {
        title: None,
        description: None,
        roots,
    }
}

fn build_section_from_object(node: &UiNode) -> FormSection {
    let pointer_path = pointer_segments(&node.pointer);
    let id = section_id(&pointer_path);
    let title = node
        .title
        .clone()
        .or_else(|| pointer_path.last().cloned())
        .unwrap_or_else(|| "Section".into());
    let description = node.description.clone();
    let mut fields = Vec::new();
    let mut children = Vec::new();

    if let UiNodeKind::Object {
        children: inner, ..
    } = &node.kind
    {
        for child in inner {
            if is_section_object(child) {
                children.push(build_section_from_object(child));
            } else {
                fields.push(field_schema_from_node(child));
            }
        }
    }

    FormSection {
        id,
        title,
        description,
        path: pointer_path,
        fields,
        children,
    }
}

fn field_schema_from_node(node: &UiNode) -> FieldSchema {
    let path = pointer_segments(&node.pointer);
    let name = path
        .last()
        .cloned()
        .unwrap_or_else(|| node.pointer.trim_start_matches('/').to_string());
    let title = node.title.clone().unwrap_or_else(|| prettify_label(&name));

    FieldSchema {
        name,
        path,
        pointer: node.pointer.clone(),
        title,
        description: node.description.clone(),
        kind: field_kind_from_node_kind(&node.kind),
        required: node.required,
        default: node.default_value.clone(),
        metadata: HashMap::new(),
    }
}

fn field_kind_from_node_kind(kind: &UiNodeKind) -> FieldKind {
    match kind {
        UiNodeKind::Field {
            scalar,
            enum_options,
            enum_values,
            nullable,
        } => {
            let kind = match enum_options {
                Some(options) if !options.is_empty() => FieldKind::Enum {
                    labels: options.clone(),
                    values: enum_values.clone().unwrap_or_else(|| {
                        options
                            .iter()
                            .cloned()
                            .map(serde_json::Value::String)
                            .collect()
                    }),
                },
                _ => match scalar {
                    ScalarKind::String => FieldKind::String,
                    ScalarKind::Integer => FieldKind::Integer,
                    ScalarKind::Number => FieldKind::Number,
                    ScalarKind::Boolean => FieldKind::Boolean,
                },
            };
            if *nullable && !matches!(kind, FieldKind::Enum { .. }) {
                FieldKind::Nullable(Box::new(kind))
            } else {
                kind
            }
        }
        UiNodeKind::Array { item, .. } => {
            FieldKind::Array(Box::new(field_kind_from_node_kind(item)))
        }
        UiNodeKind::KeyValue { template } => FieldKind::KeyValue(Box::new(KeyValueField {
            key_title: template.key_title.clone(),
            key_description: template.key_description.clone(),
            key_default: template.key_default.clone(),
            key_schema: template.key_schema.clone(),
            value_title: template.value_title.clone(),
            value_description: template.value_description.clone(),
            value_default: template.value_default.clone(),
            value_schema: template.value_schema.clone(),
            value_kind: Box::new(field_kind_from_node_kind(template.value_kind.as_ref())),
            entry_schema: template.entry_schema.clone(),
        })),
        UiNodeKind::Composite { mode, variants, .. } => {
            FieldKind::Composite(Box::new(CompositeField {
                mode: match mode {
                    CompositeMode::OneOf => DomainCompositeMode::OneOf,
                    CompositeMode::AnyOf => DomainCompositeMode::AnyOf,
                },
                variants: variants.iter().map(to_composite_variant).collect(),
            }))
        }
        UiNodeKind::Object { .. } => FieldKind::Json,
    }
}

fn to_composite_variant(variant: &UiVariant) -> CompositeVariant {
    CompositeVariant {
        id: variant.id.clone(),
        title: variant.title.clone().unwrap_or_else(|| variant.id.clone()),
        description: variant.description.clone(),
        schema: variant.schema.clone(),
        is_object: variant.is_object,
    }
}

fn pointer_segments(pointer: &str) -> Vec<String> {
    if pointer.is_empty() || pointer == "/" {
        return Vec::new();
    }
    pointer
        .split('/')
        .skip(1)
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .collect()
}

fn section_id(path: &[String]) -> String {
    if path.is_empty() {
        "root".into()
    } else {
        path.join("-")
    }
}

fn prettify_label(input: &str) -> String {
    let mut chars = input.chars().peekable();
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        if ch == '_' || ch == '-' {
            out.push(' ');
            continue;
        }
        if out.is_empty() {
            out.push(ch.to_ascii_uppercase());
        } else if ch.is_uppercase()
            && chars
                .peek()
                .map(|next| next.is_lowercase())
                .unwrap_or(false)
        {
            out.push(' ');
            out.push(ch);
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        input.to_string()
    } else {
        out
    }
}

fn is_section_object(node: &UiNode) -> bool {
    matches!(
        &node.kind,
        UiNodeKind::Object { children, .. } if !children.is_empty()
    )
}
