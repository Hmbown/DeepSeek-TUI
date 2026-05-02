use crate::{
    tui::model::{FieldKind, FieldSchema, FormSchema, FormSection, form_schema_from_ui_ast},
    ui_ast::build_ui_ast,
};
use serde_json::{Value, json};

fn runtime_form_schema(schema: &Value) -> FormSchema {
    let ast = build_ui_ast(schema).expect("ui ast");
    form_schema_from_ui_ast(&ast)
}

fn find_field_by_pointer<'a>(
    sections: &'a [FormSection],
    pointer: &str,
) -> Option<&'a FieldSchema> {
    for section in sections {
        if let Some(field) = section.fields.iter().find(|field| field.pointer == pointer) {
            return Some(field);
        }
        if let Some(found) = find_field_by_pointer(&section.children, pointer) {
            return Some(found);
        }
    }
    None
}

#[test]
fn builds_nested_sections_and_general_root() {
    let schema = json!({
        "type": "object",
        "definitions": {
            "duration": {
                "type": "object",
                "properties": {
                    "value": {"type": "integer"}
                }
            }
        },
        "properties": {
            "metadata": {
                "type": "object",
                "properties": {
                    "serviceName": {"type": "string"}
                }
            },
            "runtime": {
                "type": "object",
                "properties": {
                    "http": {
                        "type": "object",
                        "properties": {
                            "port": {"type": "integer"},
                            "limits": {
                                "type": "object",
                                "properties": {
                                    "timeout": {"$ref": "#/definitions/duration"}
                                }
                            }
                        }
                    }
                }
            },
            "generalFlag": {"type": "string"}
        }
    });

    let form = runtime_form_schema(&schema);
    assert_eq!(form.roots.len(), 3);
    let runtime = form
        .roots
        .iter()
        .find(|root| root.id == "runtime")
        .expect("runtime root");
    assert_eq!(runtime.sections.len(), 1);
    let http = &runtime.sections[0];
    assert_eq!(http.children.len(), 1);
    let http_child = &http.children[0];
    assert_eq!(http_child.fields.len(), 1);
}

#[test]
fn composite_variants_keep_definitions() {
    let schema = json!({
        "type": "object",
        "definitions": {
            "endpoint": {
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                }
            }
        },
        "properties": {
            "notifications": {
                "type": "object",
                "properties": {
                    "channel": {
                        "oneOf": [
                            {
                                "title": "HTTP",
                                "properties": {
                                    "type": {"const": "http"},
                                    "target": {"$ref": "#/definitions/endpoint"}
                                },
                                "required": ["type", "target"]
                            }
                        ]
                    }
                }
            }
        }
    });

    let form = runtime_form_schema(&schema);
    let notifications = form
        .roots
        .iter()
        .find(|root| root.id == "notifications")
        .expect("notifications root");
    let section = notifications.sections.first().expect("section");
    let channel = section
        .fields
        .iter()
        .find(|field| field.name == "channel")
        .expect("channel field");
    match &channel.kind {
        FieldKind::Composite(composite) => {
            assert!(
                composite
                    .variants
                    .first()
                    .and_then(|variant| {
                        variant
                            .schema
                            .get("definitions")
                            .or_else(|| variant.schema.get("$defs"))
                    })
                    .is_some(),
                "variant schema should embed definitions"
            );
        }
        other => panic!("expected composite field, got {:?}", other),
    }
}

#[test]
fn notifications_sections_do_not_duplicate_parent_field() {
    let schema = json!({
        "type": "object",
        "properties": {
            "notifications": {
                "type": "object",
                "properties": {
                    "channels": {"type": "array", "items": {"type": "string"}},
                    "templates": {
                        "type": "object",
                        "additionalProperties": {"type": "string"}
                    }
                },
                "additionalProperties": false
            }
        }
    });
    let form = runtime_form_schema(&schema);
    let notifications = form
        .roots
        .iter()
        .find(|root| root.id == "notifications")
        .expect("notifications root");
    let section = notifications.sections.first().expect("section");
    let names: Vec<_> = section
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect();
    assert_eq!(names, vec!["channels", "templates"]);
}

#[test]
fn additional_properties_true_does_not_add_phantom_field() {
    let schema = json!({
        "type": "object",
        "properties": {
            "room": {
                "type": "object",
                "properties": {
                    "max_size": {"type": "integer"}
                },
                "additionalProperties": true
            }
        }
    });
    let form = runtime_form_schema(&schema);
    let room = form
        .roots
        .iter()
        .find(|root| root.id == "room")
        .expect("room root");
    let section = room.sections.first().expect("room section");
    assert!(
        section.fields.iter().all(|field| field.name != "room"),
        "room section should only contain actual child fields"
    );
    let names: Vec<_> = section
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect();
    assert_eq!(names, vec!["max_size"]);
}

#[test]
fn sample_config_schema_keeps_only_user_fields() {
    let schema: Value = serde_json::from_str(include_str!("../../../examples/config-schema.json"))
        .expect("example schema");
    let form = runtime_form_schema(&schema);
    assert_eq!(form.roots.len(), 1, "only general root expected");
    let general = form
        .roots
        .iter()
        .find(|root| root.id == "general")
        .expect("general root");
    let section = general.sections.first().expect("section");
    let names: Vec<_> = section
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect();
    assert!(names.contains(&"username".to_string()));
    assert!(names.contains(&"email".to_string()));
    assert!(names.contains(&"phone".to_string()));
    assert!(names.contains(&"tags".to_string()));
    assert!(names.iter().all(|name| {
        name != "$schema" && name != "title" && name != "type" && name != "required"
    }));
}

#[test]
fn pattern_properties_become_key_value_fields() {
    let schema = json!({
        "type": "object",
        "properties": {
            "labels": {
                "type": "object",
                "patternProperties": {
                    "^[a-z]+$": {"type": "string"}
                },
                "additionalProperties": false
            }
        }
    });
    let form = runtime_form_schema(&schema);
    let field = find_field(&form, |field| field.name == "labels").expect("labels field");
    assert!(matches!(field.kind, FieldKind::KeyValue(_)));
}

#[test]
fn arrays_without_item_schema_fallback_to_json_array() {
    let schema = json!({
        "type": "object",
        "properties": {
            "expose_headers": {
                "type": "array",
                "description": "headers exposed via CORS"
            }
        }
    });
    let form = runtime_form_schema(&schema);
    let field =
        find_field(&form, |field| field.name == "expose_headers").expect("expose_headers field");
    match &field.kind {
        FieldKind::Array(inner) => assert!(matches!(inner.as_ref(), FieldKind::Json)),
        other => panic!("expected array kind, got {:?}", other),
    }
}

#[test]
fn multi_level_refs_are_resolved() {
    let schema = json!({
        "type": "object",
        "definitions": {
            "duration": {
                "type": "object",
                "properties": {
                    "value": {"type": "integer"}
                }
            },
            "timeout": {
                "$ref": "#/definitions/duration"
            }
        },
        "properties": {
            "runtime": {
                "type": "object",
                "properties": {
                    "limits": {
                        "type": "object",
                        "properties": {
                            "requestTimeout": {"$ref": "#/definitions/timeout"}
                        }
                    }
                }
            }
        }
    });
    let form = runtime_form_schema(&schema);
    let field = find_field(&form, |field| {
        field
            .pointer
            .ends_with("/runtime/limits/requestTimeout/value")
    })
    .expect("requestTimeout value field");
    assert_eq!(field.name, "value");
}

#[test]
fn anyof_variant_titles_reflect_shape() {
    let schema: Value = serde_json::from_str(include_str!("../../../examples/complex.schema.json"))
        .expect("complex schema");
    let form = runtime_form_schema(&schema);
    let pointer = "/c/c1/c2/options";
    let field = form
        .roots
        .iter()
        .find_map(|root| find_field_by_pointer(&root.sections, pointer))
        .expect("options field");
    match &field.kind {
        FieldKind::Composite(meta) => {
            let titles: Vec<_> = meta
                .variants
                .iter()
                .map(|variant| variant.title.as_str())
                .collect();
            assert!(
                titles.contains(&"List<string>"),
                "variant titles: {:?}",
                titles
            );
            assert!(
                titles.contains(&"List<integer>"),
                "variant titles: {:?}",
                titles
            );
        }
        other => panic!("expected composite field, got {:?}", other),
    }
}

#[test]
fn allof_properties_merge_into_object_section() {
    let schema = json!({
        "type": "object",
        "properties": {
            "settings": {
                "type": "object",
                "allOf": [
                    {"properties": {"enabled": {"type": "boolean"}}},
                    {"properties": {"threshold": {"type": "integer"}}}
                ]
            }
        }
    });
    let form = runtime_form_schema(&schema);
    let settings_root = form
        .roots
        .iter()
        .find(|root| root.id == "settings")
        .expect("settings root");
    let section = settings_root.sections.first().expect("settings section");
    let names: Vec<_> = section
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"enabled"),
        "merged schema should expose 'enabled'"
    );
    assert!(
        names.contains(&"threshold"),
        "merged schema should expose 'threshold'"
    );
}

#[test]
fn referenced_field_keeps_instance_metadata() {
    let schema = json!({
        "definitions": {
            "duration_ms": {
                "title": "Definition Title",
                "description": "Definition description",
                "type": "integer"
            }
        },
        "type": "object",
        "properties": {
            "timeout": {
                "$ref": "#/definitions/duration_ms",
                "title": "Request Timeout",
                "description": "Per-request timeout",
                "default": 5
            }
        }
    });

    let form = runtime_form_schema(&schema);
    let field = find_field(&form, |field| field.pointer == "/timeout").expect("timeout field");
    assert_eq!(field.title, "Request Timeout");
    assert_eq!(field.description.as_deref(), Some("Per-request timeout"));
    assert_eq!(field.default, Some(json!(5)));
    assert!(matches!(field.kind, FieldKind::Integer));
}

#[test]
fn recursive_refs_collapse_to_json_array_field_without_overflow() {
    let schema = json!({
        "type": "object",
        "properties": {
            "tree": {"$ref": "#/definitions/treeNode"}
        },
        "definitions": {
            "treeNode": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "children": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/treeNode"}
                    }
                },
                "required": ["name"]
            }
        }
    });

    let form = runtime_form_schema(&schema);
    let root = form
        .roots
        .iter()
        .find(|root| root.id == "tree")
        .expect("tree root section");
    let section = root.sections.first().expect("tree section");

    let name_field = find_field(&form, |field| field.pointer == "/tree/name").expect("name field");
    assert!(matches!(name_field.kind, FieldKind::String));

    let children_field = section
        .fields
        .iter()
        .find(|field| field.pointer == "/tree/children")
        .expect("children field");
    match &children_field.kind {
        FieldKind::Array(inner) => {
            assert!(
                matches!(inner.as_ref(), FieldKind::Composite(_)),
                "recursive descendants should stay editable via a lazy composite boundary, got {:?}",
                inner
            );
        }
        other => panic!(
            "expected array field for recursive children, got {:?}",
            other
        ),
    }
}

#[test]
fn form_schema_preserves_declared_root_and_field_order() {
    let schema = json!({
        "type": "object",
        "properties": {
            "zeta": {
                "type": "object",
                "title": "Zeta",
                "properties": {
                    "second": {"type": "string"},
                    "first": {"type": "string"},
                    "network": {
                        "type": "object",
                        "title": "Network",
                        "properties": {
                            "port": {"type": "integer"}
                        }
                    },
                    "auth": {
                        "type": "object",
                        "title": "Auth",
                        "properties": {
                            "user": {"type": "string"}
                        }
                    }
                }
            },
            "alpha": {
                "type": "object",
                "title": "Alpha",
                "properties": {
                    "enabled": {"type": "boolean"}
                }
            }
        }
    });

    let form = runtime_form_schema(&schema);
    let root_ids: Vec<_> = form.roots.iter().map(|root| root.id.as_str()).collect();
    assert_eq!(
        root_ids,
        vec!["zeta", "alpha"],
        "form roots should follow schema declaration order for top-level object sections",
    );

    let zeta = form
        .roots
        .iter()
        .find(|root| root.id == "zeta")
        .expect("zeta root");
    let zeta_section = zeta.sections.first().expect("zeta section");
    let field_names: Vec<_> = zeta_section
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        field_names,
        vec!["second", "first"],
        "fields inside a section should preserve schema declaration order",
    );

    let child_titles: Vec<_> = zeta_section
        .children
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert_eq!(
        child_titles,
        vec!["Network", "Auth"],
        "nested sections should preserve schema declaration order",
    );
}

fn find_field(form: &FormSchema, predicate: impl Fn(&FieldSchema) -> bool) -> Option<&FieldSchema> {
    for root in &form.roots {
        if let Some(field) = find_in_sections(&root.sections, &predicate) {
            return Some(field);
        }
    }
    None
}

fn find_in_sections<'a>(
    sections: &'a [FormSection],
    predicate: &impl Fn(&FieldSchema) -> bool,
) -> Option<&'a FieldSchema> {
    for section in sections {
        for field in &section.fields {
            if predicate(field) {
                return Some(field);
            }
        }
        if let Some(found) = find_in_sections(&section.children, predicate) {
            return Some(found);
        }
    }
    None
}
