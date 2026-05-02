use serde::{Deserialize, Serialize};

#[cfg(feature = "web-types")]
use ts_rs::TS;

use super::{UiAst, UiNode, UiNodeKind};

/// A simplified, layout-oriented view of a UiAst tree.
///
/// This type is primarily intended for internal analysis, testing, and
/// compile-time tooling. It is kept intentionally small and does not attempt
/// to model all runtime behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(
    feature = "web-types",
    ts(export, export_to = "web/types/ui-layout.ts")
)]
pub struct UiLayout {
    pub roots: Vec<LayoutRoot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
pub struct LayoutRoot {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub sections: Vec<LayoutSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
pub struct LayoutSection {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub pointer: String,
    pub path: Vec<String>,
    pub field_pointers: Vec<String>,
    pub children: Vec<LayoutSection>,
}

pub(crate) fn build_ui_layout(ast: &UiAst) -> UiLayout {
    let mut object_roots = Vec::new();
    let mut general_fields = Vec::new();

    for node in &ast.roots {
        if is_section_object(node) {
            object_roots.push(node);
        } else {
            general_fields.push(node);
        }
    }

    let mut roots = Vec::new();

    if !general_fields.is_empty() {
        let fields: Vec<String> = general_fields
            .iter()
            .map(|node| node.pointer.clone())
            .collect();

        roots.push(LayoutRoot {
            id: "general".to_string(),
            title: Some("General".to_string()),
            description: None,
            sections: vec![LayoutSection {
                id: "general".to_string(),
                title: "General".to_string(),
                description: None,
                pointer: String::new(),
                path: Vec::new(),
                field_pointers: fields,
                children: Vec::new(),
            }],
        });
    }

    for node in object_roots {
        let section = build_section_from_object(node);
        roots.push(LayoutRoot {
            id: section.id.clone(),
            title: node.title.clone().or_else(|| last_segment(&node.pointer)),
            description: node.description.clone(),
            sections: vec![section],
        });
    }

    if roots.is_empty() {
        roots.push(LayoutRoot {
            id: "general".to_string(),
            title: Some("General".to_string()),
            description: None,
            sections: vec![LayoutSection {
                id: "general".to_string(),
                title: "General".to_string(),
                description: None,
                pointer: String::new(),
                path: Vec::new(),
                field_pointers: Vec::new(),
                children: Vec::new(),
            }],
        });
    }

    UiLayout { roots }
}

fn build_section_from_object(node: &UiNode) -> LayoutSection {
    let path = pointer_segments(&node.pointer);
    let id = if path.is_empty() {
        "root".to_string()
    } else {
        path.join("-")
    };
    let title = node
        .title
        .clone()
        .or_else(|| path.last().cloned())
        .unwrap_or_else(|| "Section".to_string());

    let mut field_pointers = Vec::new();
    let mut children = Vec::new();

    if let UiNodeKind::Object {
        children: inner, ..
    } = &node.kind
    {
        for child in inner {
            if is_section_object(child) {
                children.push(build_section_from_object(child));
            } else {
                field_pointers.push(child.pointer.clone());
            }
        }
    }

    LayoutSection {
        id,
        title,
        description: node.description.clone(),
        pointer: node.pointer.clone(),
        path,
        field_pointers,
        children,
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

fn last_segment(pointer: &str) -> Option<String> {
    let mut segments = pointer_segments(pointer);
    segments.pop()
}

fn is_section_object(node: &UiNode) -> bool {
    matches!(
        &node.kind,
        UiNodeKind::Object { children, .. } if !children.is_empty()
    )
}
