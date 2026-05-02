use serde::{Deserialize, Serialize};

use crate::ui_ast::layout::{LayoutSection, UiLayout};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutNavModel {
    pub roots: Vec<NavRoot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NavRoot {
    pub id: String,
    pub title: String,
    pub sections: Vec<NavSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NavSection {
    pub id: String,
    pub title: String,
    pub first_pointer: Option<String>,
    pub pointers: Vec<String>,
}

impl LayoutNavModel {
    pub fn from_uilayout(layout: &UiLayout) -> Self {
        let mut roots = Vec::new();

        for root in &layout.roots {
            let mut sections = Vec::new();
            for section in &root.sections {
                collect_section(section, &mut sections);
            }
            if sections.is_empty() {
                continue;
            }
            roots.push(NavRoot {
                id: root.id.clone(),
                title: root.title.clone().unwrap_or_else(|| "Root".to_string()),
                sections,
            });
        }

        LayoutNavModel { roots }
    }
}

fn collect_section(section: &LayoutSection, out: &mut Vec<NavSection>) {
    let mut pointers = Vec::new();
    collect_pointers(section, &mut pointers);
    let first_pointer = find_first_pointer(section);

    out.push(NavSection {
        id: section.id.clone(),
        title: section.title.clone(),
        first_pointer,
        pointers,
    });

    for child in &section.children {
        collect_section(child, out);
    }
}

fn collect_pointers(section: &LayoutSection, out: &mut Vec<String>) {
    for fp in &section.field_pointers {
        out.push(fp.clone());
    }
    if !section.pointer.is_empty() {
        out.push(section.pointer.clone());
    }
    for child in &section.children {
        collect_pointers(child, out);
    }
}

fn find_first_pointer(section: &LayoutSection) -> Option<String> {
    if let Some(first) = section.field_pointers.first() {
        return Some(first.clone());
    }

    for child in &section.children {
        if let Some(ptr) = find_first_pointer(child) {
            return Some(ptr);
        }
    }

    if section.pointer.is_empty() {
        None
    } else {
        Some(section.pointer.clone())
    }
}
