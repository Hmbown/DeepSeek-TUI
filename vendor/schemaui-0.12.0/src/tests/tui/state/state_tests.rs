use crate::{
    tui::model::{FieldKind, FieldSchema},
    tui::state::{FieldState, FormState, RootSectionState, SectionState},
};

fn mk_field(name: &str) -> FieldState {
    FieldState::from_schema(FieldSchema {
        name: name.to_string(),
        path: vec![name.to_string()],
        pointer: format!("/{name}"),
        title: name.to_string(),
        description: None,
        kind: FieldKind::String,
        required: false,
        default: None,
        metadata: Default::default(),
    })
}

fn mk_section(id: &str, fields: &[&str]) -> SectionState {
    SectionState {
        id: id.to_string(),
        title: id.to_string(),
        description: None,
        path: vec![id.to_string()],
        depth: 0,
        fields: fields.iter().map(|name| mk_field(name)).collect(),
        scroll_offset: 0,
    }
}

#[test]
fn field_navigation_wraps_across_roots() {
    let root_a = RootSectionState {
        id: "a".into(),
        title: "A".into(),
        description: None,
        sections: vec![mk_section("a/one", &["a1", "a2"])],
    };
    let root_b = RootSectionState {
        id: "b".into(),
        title: "B".into(),
        description: None,
        sections: vec![mk_section("b/one", &["b1"]), mk_section("b/two", &["b2"])],
    };
    let mut state = FormState::from_roots_for_test(vec![root_a, root_b]);
    state.set_field_index(1);
    state.focus_next_field();
    assert_eq!(state.root_index(), 1);
    assert_eq!(state.section_index(), 0);
    assert_eq!(state.field_index(), 0);
    state.focus_next_field();
    assert_eq!(state.section_index(), 1);
    assert_eq!(state.field_index(), 0);
    state.focus_next_field();
    assert_eq!(state.root_index(), 0);
    assert_eq!(state.section_index(), 0);
    assert_eq!(state.field_index(), 0);
}

#[test]
fn field_navigation_wraps_backwards_across_roots() {
    let root_a = RootSectionState {
        id: "a".into(),
        title: "A".into(),
        description: None,
        sections: vec![mk_section("a/one", &["a1"])],
    };
    let root_b = RootSectionState {
        id: "b".into(),
        title: "B".into(),
        description: None,
        sections: vec![mk_section("b/one", &["b1", "b2"])],
    };
    let mut state = FormState::from_roots_for_test(vec![root_a, root_b]);
    state.focus_prev_field();
    assert_eq!(state.root_index(), 1);
    assert_eq!(state.section_index(), 0);
    assert_eq!(state.field_index(), 1);
}

#[test]
fn section_navigation_cycles_ordered_tree() {
    let sections_a = vec![mk_section("auth", &["user"]), mk_section("db", &["url"])];
    let sections_b = vec![mk_section("cache", &["ttl"])];
    let mut state = FormState::from_roots_for_test(vec![
        RootSectionState {
            id: "runtime".into(),
            title: "Runtime".into(),
            description: None,
            sections: sections_a,
        },
        RootSectionState {
            id: "ops".into(),
            title: "Ops".into(),
            description: None,
            sections: sections_b,
        },
    ]);
    state.focus_next_section(1);
    assert_eq!(state.root_index(), 0);
    assert_eq!(state.section_index(), 1);
    state.focus_next_section(1);
    assert_eq!(state.root_index(), 1);
    assert_eq!(state.section_index(), 0);
    state.focus_next_section(1);
    assert_eq!(state.root_index(), 0);
    assert_eq!(state.section_index(), 0);
}

#[test]
fn skips_empty_sections_when_focusing() {
    let empty = mk_section("app", &[]);
    let server = mk_section("server", &["host"]);
    let storage = mk_section("storage", &["path"]);
    let mut state = FormState::from_sections("app", "App", None, vec![empty, server, storage]);
    assert_eq!(
        state.section_index(),
        1,
        "should jump to first populated section"
    );
    state.focus_next_section(1);
    assert_eq!(
        state.section_index(),
        2,
        "Ctrl+Tab should skip empty sections"
    );
    state.focus_next_section(1);
    assert_eq!(
        state.section_index(),
        1,
        "wrap keeps focusable sections only"
    );
}
