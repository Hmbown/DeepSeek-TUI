use std::collections::BTreeMap;

use serde_json::Value;

use super::{UiAst, UiNode, UiNodeKind};

/// A map from UiAst JSON Pointers to their backend-provided default values.
///
/// This reflects the defaults encoded in the UiAst itself; higher-level
/// frontends may apply additional defaulting behavior on top.
pub type DefaultIndex = BTreeMap<String, Value>;

pub(crate) fn collect_defaults(ast: &UiAst) -> DefaultIndex {
    let mut out = DefaultIndex::new();
    for node in &ast.roots {
        collect_node_defaults(node, &mut out);
    }
    out
}

fn collect_node_defaults(node: &UiNode, out: &mut DefaultIndex) {
    if let Some(value) = node.default_value.clone() {
        out.insert(node.pointer.clone(), value);
    }

    match &node.kind {
        UiNodeKind::Object { children, .. } => {
            for child in children {
                collect_node_defaults(child, out);
            }
        }
        UiNodeKind::Array { .. } => {}
        UiNodeKind::KeyValue { .. } => {}
        UiNodeKind::Composite { .. } => {}
        UiNodeKind::Field { .. } => {}
    }
}
