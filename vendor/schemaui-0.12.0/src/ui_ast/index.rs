use std::collections::{BTreeMap, BTreeSet};

use super::{UiAst, UiNode, UiNodeKind};

/// Collect all JSON Pointer strings that appear in a UiAst tree.
///
/// This helper is pure and makes no assumptions about how the AST will be
/// rendered. It is primarily useful for tests, diagnostics, or building
/// pointer indexes in higher layers.
pub fn collect_pointers(ast: &UiAst) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for node in &ast.roots {
        collect_node_pointers(node, &mut set);
    }
    set
}

fn collect_node_pointers(node: &UiNode, out: &mut BTreeSet<String>) {
    out.insert(node.pointer.clone());
    match &node.kind {
        UiNodeKind::Object { children, .. } => {
            for child in children {
                collect_node_pointers(child, out);
            }
        }
        UiNodeKind::Composite { variants, .. } => {
            for variant in variants {
                collect_kind_pointers(&variant.node, out);
            }
        }
        UiNodeKind::Array { item, .. } => {
            collect_kind_pointers(item, out);
        }
        UiNodeKind::KeyValue { .. } => {}
        UiNodeKind::Field { .. } => {}
    }
}

fn collect_kind_pointers(kind: &UiNodeKind, out: &mut BTreeSet<String>) {
    match kind {
        UiNodeKind::Object { children, .. } => {
            for child in children {
                collect_node_pointers(child, out);
            }
        }
        UiNodeKind::Composite { variants, .. } => {
            for variant in variants {
                collect_kind_pointers(&variant.node, out);
            }
        }
        UiNodeKind::Array { item, .. } => {
            collect_kind_pointers(item, out);
        }
        UiNodeKind::KeyValue { .. } => {}
        UiNodeKind::Field { .. } => {}
    }
}

/// A simple pointer index mapping each JSON Pointer in the UiAst to a
/// pre-order position (0-based) in the traversal.
pub type PointerIndex = BTreeMap<String, usize>;

/// Build a dense pre-order pointer index for the given UiAst. All pointers
/// present in the AST are mapped to a unique integer in the range
/// `0..index.len()`.
pub fn build_pointer_index(ast: &UiAst) -> PointerIndex {
    let mut index = PointerIndex::new();
    let mut counter = 0usize;
    for node in &ast.roots {
        index_node(node, &mut counter, &mut index);
    }
    index
}

fn index_node(node: &UiNode, counter: &mut usize, out: &mut PointerIndex) {
    let id = *counter;
    out.insert(node.pointer.clone(), id);
    *counter += 1;

    match &node.kind {
        UiNodeKind::Object { children, .. } => {
            for child in children {
                index_node(child, counter, out);
            }
        }
        UiNodeKind::Composite { variants, .. } => {
            for variant in variants {
                index_kind(&variant.node, counter, out);
            }
        }
        UiNodeKind::Array { item, .. } => {
            index_kind(item, counter, out);
        }
        UiNodeKind::KeyValue { .. } => {}
        UiNodeKind::Field { .. } => {}
    }
}

fn index_kind(kind: &UiNodeKind, counter: &mut usize, out: &mut PointerIndex) {
    match kind {
        UiNodeKind::Object { children, .. } => {
            for child in children {
                index_node(child, counter, out);
            }
        }
        UiNodeKind::Composite { variants, .. } => {
            for variant in variants {
                index_kind(&variant.node, counter, out);
            }
        }
        UiNodeKind::Array { item, .. } => {
            index_kind(item, counter, out);
        }
        UiNodeKind::KeyValue { .. } => {}
        UiNodeKind::Field { .. } => {}
    }
}
