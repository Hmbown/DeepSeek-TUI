// Tree-sitter multi-language parser adapter.
// Handles parser initialization, AST parsing, symbol extraction,
// import/call extraction for all supported languages.

use std::collections::HashMap;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::types::*;

/// Wraps tree-sitter multi-language parsing.
///
/// Lazily loads language bindings, silently skipping unavailable ones.
/// Pre-compiles queries for each loaded language.
pub struct TreeSitterAdapter {
    pub parsers: HashMap<String, Parser>,
    queries: HashMap<String, HashMap<String, Query>>,
    languages: HashMap<String, Language>,
}

impl TreeSitterAdapter {
    pub fn new() -> Self {
        let mut adapter = Self {
            parsers: HashMap::new(),
            queries: HashMap::new(),
            languages: HashMap::new(),
        };
        adapter.init_parsers();
        adapter
    }

    fn init_parsers(&mut self) {
        self.try_load_language("rust", tree_sitter_rust::LANGUAGE.into());
        self.try_load_language("python", tree_sitter_python::LANGUAGE.into());
        self.try_load_language("javascript", tree_sitter_javascript::LANGUAGE.into());
        self.try_load_language(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        );
        self.try_load_language("go", tree_sitter_go::LANGUAGE.into());
        self.try_load_language("html", tree_sitter_html::LANGUAGE.into());
        self.try_load_language("css", tree_sitter_css::LANGUAGE.into());
        self.try_load_language("json", tree_sitter_json::LANGUAGE.into());
        self.precompile_queries();
    }

    fn try_load_language(&mut self, name: &str, lang: Language) {
        let mut parser = Parser::new();
        match parser.set_language(&lang) {
            Ok(()) => {
                self.parsers.insert(name.to_string(), parser);
                self.languages.insert(name.to_string(), lang);
            }
            Err(_e) => {}
        }
    }

    /// Parse source content with the given language.
    pub fn parse(&mut self, content: &[u8], lang: &str) -> Option<Tree> {
        let parser = self.parsers.get_mut(lang)?;

        const MAX_PARSE_SIZE: usize = 10 * 1024 * 1024;
        if content.len() > MAX_PARSE_SIZE {
            return None;
        }

        // Check for extreme nesting (depth > 1000) to prevent stack overflow.
        if let Ok(text) = std::str::from_utf8(&content[..content.len().min(100_000)]) {
            let mut max_nesting = 0u32;
            let mut current = 0u32;
            for ch in text.chars() {
                match ch {
                    '(' | '{' | '[' | '<' => {
                        current += 1;
                        max_nesting = max_nesting.max(current);
                    }
                    ')' | '}' | ']' | '>' => {
                        current = current.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            if max_nesting > 1000 {
                return None;
            }
        }

        parser.parse(content, None)
    }

    fn precompile_queries(&mut self) {
        for (lang, query_type, src) in crate::queries::queries() {
            if !self.languages.contains_key(lang) {
                continue;
            }
            let language = &self.languages[lang];
            if let Ok(q) = Query::new(language, src) {
                self.queries
                    .entry(lang.to_string())
                    .or_default()
                    .insert(query_type.to_string(), q);
            }
        }
    }

    /// Extract symbols (functions, classes, etc.) from AST.
    /// `source` is the original file content as a UTF-8 string.
    pub fn extract_symbols(
        &self,
        tree: &Tree,
        lang: &str,
        file: &str,
        source: &str,
    ) -> Vec<Symbol> {
        if lang == "html" {
            return self.extract_html_symbols(tree, file, source);
        }
        if lang == "css" {
            return self.extract_css_symbols(tree, file, source);
        }
        if lang == "json" {
            return self.extract_json_symbols(tree, file, source);
        }

        let mut symbols: HashMap<String, Symbol> = HashMap::new();
        let root = tree.root_node();
        let source_bytes = source.as_bytes();

        for query_type in &["function", "class"] {
            let query = match self.queries.get(lang).and_then(|q| q.get(*query_type)) {
                Some(q) => q,
                None => continue,
            };

            let mut cursor = QueryCursor::new();
            let capture_names = query.capture_names().to_vec();
            let mut name_nodes: Vec<Node> = Vec::new();
            let mut def_nodes: Vec<(Node, String)> = Vec::new();

            let mut captures = cursor.captures(query, root, source_bytes);
            while let Some((match_, _)) = captures.next() {
                for capture in match_.captures {
                    let cap_name = capture_names[capture.index as usize];
                    let node = capture.node;
                    if cap_name == "name" || cap_name.starts_with("name") {
                        name_nodes.push(node);
                    } else if cap_name.contains("definition") || cap_name.contains("export") {
                        def_nodes.push((node, cap_name.to_string()));
                    }
                }
            }

            for name_node in &name_nodes {
                let mut matching: Vec<(&Node, &str)> = def_nodes
                    .iter()
                    .filter(|(def_node, _)| within(name_node, def_node))
                    .map(|(def_node, cap_name)| (def_node, cap_name.as_str()))
                    .collect();

                matching.sort_by(|a, b| {
                    let size_a = (
                        a.0.end_position().row - a.0.start_position().row,
                        a.0.end_position().column - a.0.start_position().column,
                    );
                    let size_b = (
                        b.0.end_position().row - b.0.start_position().row,
                        b.0.end_position().column - b.0.start_position().column,
                    );
                    size_a
                        .0
                        .cmp(&size_b.0)
                        .then(size_a.1.cmp(&size_b.1))
                        .then(a.0.start_position().row.cmp(&b.0.start_position().row))
                        .then(
                            a.0.start_position()
                                .column
                                .cmp(&b.0.start_position().column),
                        )
                });

                for (def_node, def_cap) in matching {
                    let kind = def_cap.split('.').last().unwrap_or(def_cap).to_string();
                    let mut vis = if def_cap.contains("export") {
                        "exported"
                    } else {
                        "public"
                    };
                    let name = node_text(name_node, source_bytes);
                    if name.is_empty() {
                        break;
                    }
                    if lang == "python" && name.starts_with('_') && !name.starts_with("__") {
                        vis = "private";
                    }
                    let sym_id =
                        format!("{}::{}::{}", file, name, name_node.start_position().row + 1);
                    symbols.entry(sym_id.clone()).or_insert_with(|| {
                        Symbol::new(
                            sym_id,
                            name.to_string(),
                            kind,
                            file.to_string(),
                            name_node.start_position().row + 1,
                            def_node.end_position().row + 1,
                            name_node.start_position().column,
                            vis.to_string(),
                            String::new(),
                            signature(def_node, source_bytes),
                        )
                    });
                    break;
                }
            }
        }

        // Object-literal method symbols (JS/TS).
        for sym in self.extract_object_literal_methods(tree, lang, file, source_bytes) {
            symbols.entry(sym.id.clone()).or_insert(sym);
        }
        // Anonymous function symbols (JS/TS).
        for sym in self.extract_anonymous_symbols(tree, lang, file, source_bytes) {
            symbols.entry(sym.id.clone()).or_insert(sym);
        }

        let mut result: Vec<Symbol> = symbols.into_values().collect();
        result.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.line.cmp(&b.line))
                .then(a.end_line.cmp(&b.end_line))
                .then(a.col.cmp(&b.col))
                .then(a.name.cmp(&b.name))
                .then(a.kind.cmp(&b.kind))
        });
        result
    }

    /// Extract imports from AST. Returns (module_name, line_number).
    pub fn extract_imports(&self, tree: &Tree, lang: &str, source: &str) -> Vec<(String, usize)> {
        let query = match self.queries.get(lang).and_then(|q| q.get("import")) {
            Some(q) => q,
            None => return Vec::new(),
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let capture_names = query.capture_names().to_vec();

        if lang == "rust" {
            use std::collections::BTreeMap;
            let mut paths: BTreeMap<usize, String> = BTreeMap::new();
            let mut names: BTreeMap<usize, Vec<String>> = BTreeMap::new();

            let mut captures = cursor.captures(query, tree.root_node(), source_bytes);
            while let Some((match_, _)) = captures.next() {
                for capture in match_.captures {
                    let cap_name = capture_names[capture.index as usize];
                    let text = node_text(&capture.node, source_bytes);
                    let line = capture.node.start_position().row + 1;
                    if cap_name == "path" {
                        paths.insert(line, text.to_string());
                    } else if cap_name == "name" {
                        names.entry(line).or_default().push(text.to_string());
                    }
                }
            }

            let mut lines: Vec<usize> = paths.keys().chain(names.keys()).copied().collect();
            lines.sort();
            lines.dedup();

            let mut results = Vec::new();
            for line in lines {
                if let Some(path) = paths.get(&line) {
                    results.push((path.clone(), line));
                } else if let Some(nms) = names.get(&line) {
                    for name in nms {
                        results.push((name.clone(), line));
                    }
                }
            }
            results
        } else {
            let mut results = Vec::new();
            let mut captures = cursor.captures(query, tree.root_node(), source_bytes);
            while let Some((match_, _)) = captures.next() {
                for capture in match_.captures {
                    let cap_name = capture_names[capture.index as usize];
                    if (lang == "javascript" || lang == "typescript") && cap_name != "source" {
                        continue;
                    }
                    let text = node_text(&capture.node, source_bytes)
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_string();
                    if !text.is_empty() {
                        results.push((text, capture.node.start_position().row + 1));
                    }
                }
            }
            results.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
            results
        }
    }

    /// Extract function calls from AST. Returns (name, line, kind).
    pub fn extract_calls(
        &self,
        tree: &Tree,
        lang: &str,
        source: &str,
    ) -> Vec<(String, usize, String)> {
        let query = match self.queries.get(lang).and_then(|q| q.get("call")) {
            Some(q) => q,
            None => return Vec::new(),
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let capture_names = query.capture_names().to_vec();
        let mut results = Vec::new();

        let mut captures = cursor.captures(query, tree.root_node(), source_bytes);
        while let Some((match_, _)) = captures.next() {
            for capture in match_.captures {
                let cap_name = capture_names[capture.index as usize];
                if cap_name.contains("reference") || cap_name.contains("call") || cap_name == "name"
                {
                    let name = node_text(&capture.node, source_bytes);
                    if !name.is_empty() {
                        let kind = call_reference_kind(&capture.node);
                        results.push((
                            name.to_string(),
                            capture.node.start_position().row + 1,
                            kind,
                        ));
                    }
                }
            }
        }
        results.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        results
    }

    /// Extract JS/TS import bindings.
    pub fn extract_js_ts_import_bindings(
        &self,
        tree: &Tree,
        lang: &str,
        source: &str,
    ) -> Vec<JsImportBinding> {
        if lang != "javascript" && lang != "typescript" {
            return Vec::new();
        }
        // TODO: implement full JS/TS import binding extraction from AST
        let _ = (tree, source);
        Vec::new()
    }

    /// Extract JS/TS export bindings.
    pub fn extract_js_ts_export_bindings(
        &self,
        tree: &Tree,
        lang: &str,
        source: &str,
    ) -> Vec<JsExportBinding> {
        if lang != "javascript" && lang != "typescript" {
            return Vec::new();
        }
        // TODO: implement full JS/TS export binding extraction from AST
        let _ = (tree, source);
        Vec::new()
    }

    // ── HTML / CSS / JSON symbol extraction ──

    fn extract_html_symbols(&self, tree: &Tree, file: &str, source: &str) -> Vec<Symbol> {
        let mut symbols: HashMap<String, Symbol> = HashMap::new();
        let mut seen: HashMap<(String, usize), usize> = HashMap::new();
        let root = tree.root_node();
        for node in walk_tree(&root) {
            if node.kind() != "element" {
                continue;
            }
            let tag_name = first_child_of_type(&node, "start_tag")
                .and_then(|st| {
                    let mut cursor = st.walk();
                    st.children(&mut cursor).find(|c| c.kind() == "tag_name")
                })
                .map(|tn| node_text(&tn, source.as_bytes()).to_string());

            if let Some(tag_name) = tag_name {
                let line = node.start_position().row + 1;
                let mut visible = format!("<{}>", tag_name);
                let key = (visible.clone(), line);
                let count = seen.get(&key).copied().unwrap_or(0) + 1;
                seen.insert(key.clone(), count);
                if count > 1 {
                    visible = format!("{}#{}", visible, count);
                }
                let sym_id = format!("{}::{}::{}", file, visible, line);
                symbols.entry(sym_id.clone()).or_insert_with(|| {
                    Symbol::new(
                        sym_id,
                        visible.clone(),
                        "element".into(),
                        file.to_string(),
                        line,
                        node.end_position().row + 1,
                        node.start_position().column,
                        "public".into(),
                        String::new(),
                        visible,
                    )
                });
            }
        }
        let mut result: Vec<Symbol> = symbols.into_values().collect();
        result.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.name.cmp(&b.name))
        });
        result
    }

    fn extract_css_symbols(&self, tree: &Tree, file: &str, source: &str) -> Vec<Symbol> {
        let mut symbols: HashMap<String, Symbol> = HashMap::new();
        let mut seen: HashMap<(String, usize), usize> = HashMap::new();
        let selector_types = [
            "class_selector",
            "id_selector",
            "tag_name",
            "nesting_selector",
        ];
        let root = tree.root_node();
        for node in walk_tree(&root) {
            if !selector_types.contains(&node.kind()) {
                continue;
            }
            let raw = node_text(&node, source.as_bytes()).trim().to_string();
            if raw.is_empty() {
                continue;
            }
            let line = node.start_position().row + 1;
            let kind = if raw.starts_with('.') {
                "class_selector"
            } else if raw.starts_with('#') {
                "id_selector"
            } else {
                "selector"
            };
            let key = (raw.clone(), line);
            let count = seen.get(&key).copied().unwrap_or(0) + 1;
            seen.insert(key, count);
            let visible = if count == 1 {
                raw.clone()
            } else {
                format!("{}#{}", raw, count)
            };
            let sym_id = format!("{}::{}::{}", file, visible, line);
            symbols.entry(sym_id.clone()).or_insert_with(|| {
                Symbol::new(
                    sym_id,
                    visible.clone(),
                    kind.to_string(),
                    file.to_string(),
                    line,
                    node.end_position().row + 1,
                    node.start_position().column,
                    "public".into(),
                    String::new(),
                    raw.clone(),
                )
            });
        }
        let mut result: Vec<Symbol> = symbols.into_values().collect();
        result.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.name.cmp(&b.name))
        });
        result
    }

    fn extract_json_symbols(&self, tree: &Tree, file: &str, source: &str) -> Vec<Symbol> {
        let mut symbols: HashMap<String, Symbol> = HashMap::new();
        let mut seen: HashMap<(String, usize), usize> = HashMap::new();
        let root = tree.root_node();
        for node in walk_tree(&root) {
            if node.kind() != "pair" {
                continue;
            }
            let key_name = node
                .child_by_field_name("key")
                .map(|k| {
                    node_text(&k, source.as_bytes())
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_string()
                })
                .filter(|s| !s.is_empty());

            if let Some(key_name) = key_name {
                let line = node.start_position().row + 1;
                let key = (key_name.clone(), line);
                let count = seen.get(&key).copied().unwrap_or(0) + 1;
                seen.insert(key, count);
                let visible = if count == 1 {
                    key_name.clone()
                } else {
                    format!("{}#{}", key_name, count)
                };
                let sym_id = format!("{}::{}::{}", file, visible, line);
                symbols.entry(sym_id.clone()).or_insert_with(|| {
                    Symbol::new(
                        sym_id,
                        visible,
                        "json_key".into(),
                        file.to_string(),
                        line,
                        node.end_position().row + 1,
                        node.start_position().column,
                        "public".into(),
                        String::new(),
                        format!("\"{}\"", key_name),
                    )
                });
            }
        }
        let mut result: Vec<Symbol> = symbols.into_values().collect();
        result.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.name.cmp(&b.name))
        });
        result
    }

    // ── JS/TS specific extraction ──

    fn extract_object_literal_methods(
        &self,
        tree: &Tree,
        lang: &str,
        file: &str,
        source: &[u8],
    ) -> Vec<Symbol> {
        if lang != "javascript" && lang != "typescript" {
            return Vec::new();
        }
        let mut symbols: HashMap<String, Symbol> = HashMap::new();
        for node in walk_tree(&tree.root_node()) {
            if node.kind() != "pair" {
                continue;
            }
            let value_node = node.child_by_field_name("value");
            if value_node.map_or(true, |v| {
                v.kind() != "arrow_function" && v.kind() != "function_expression"
            }) {
                continue;
            }
            let key_node = node.child_by_field_name("key").or_else(|| {
                let mut cursor = node.walk();
                node.children(&mut cursor)
                    .find(|c| matches!(c.kind(), "property_identifier" | "identifier" | "string"))
            });
            let name = key_node.and_then(|k| {
                if k.kind() == "string" {
                    Some(
                        node_text(&k, source)
                            .trim_matches(|c| c == '"' || c == '\'')
                            .to_string(),
                    )
                } else {
                    Some(node_text(&k, source).to_string())
                }
            });
            if let (Some(name), Some(value_node)) = (name, value_node) {
                if name.is_empty() {
                    continue;
                }
                let kn = key_node.unwrap();
                let line = kn.start_position().row + 1;
                let sym_id = format!("{}::{}::{}", file, name, line);
                symbols.entry(sym_id.clone()).or_insert_with(|| {
                    Symbol::new(
                        sym_id,
                        name.clone(),
                        "method".into(),
                        file.to_string(),
                        line,
                        value_node.end_position().row + 1,
                        kn.start_position().column,
                        "public".into(),
                        String::new(),
                        signature(&value_node, source),
                    )
                });
            }
        }
        let mut result: Vec<Symbol> = symbols.into_values().collect();
        result.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.name.cmp(&b.name))
        });
        result
    }

    fn extract_anonymous_symbols(
        &self,
        tree: &Tree,
        lang: &str,
        file: &str,
        source: &[u8],
    ) -> Vec<Symbol> {
        if lang != "javascript" && lang != "typescript" {
            return Vec::new();
        }
        let query = match self
            .queries
            .get(lang)
            .and_then(|q| q.get("anonymous_function"))
        {
            Some(q) => q,
            None => return Vec::new(),
        };
        let root = tree.root_node();
        let mut cursor = QueryCursor::new();
        let mut results: HashMap<String, Symbol> = HashMap::new();
        let mut captures = cursor.captures(query, root, source);
        while let Some((match_, _)) = captures.next() {
            for capture in match_.captures {
                let node = capture.node;
                if node.end_position().row <= node.start_position().row {
                    continue;
                }
                if has_named_owner(&node) {
                    continue;
                }
                let line = node.start_position().row + 1;
                let name = format!("<anonymous@{}>", line);
                let sym_id = format!("{}::{}::{}", file, name, line);
                results.entry(sym_id.clone()).or_insert_with(|| {
                    Symbol::new(
                        sym_id,
                        name.clone(),
                        "anonymous_function".into(),
                        file.to_string(),
                        line,
                        node.end_position().row + 1,
                        node.start_position().column,
                        "private".into(),
                        String::new(),
                        signature(&node, source),
                    )
                });
            }
        }
        results.into_values().collect()
    }
}

// ── Helpers ──

fn node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn within(child: &Node, parent: &Node) -> bool {
    child.start_position() >= parent.start_position()
        && child.end_position() <= parent.end_position()
}

fn call_reference_kind(node: &Node) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "call_expression" {
            if let Some(func_node) = parent.child_by_field_name("function") {
                let ft = func_node.kind();
                if ft == "member_expression"
                    || ft == "field_expression"
                    || ft == "selector_expression"
                {
                    return "member".into();
                }
            }
            return "direct".into();
        }
        current = parent.parent();
    }
    "direct".into()
}

fn signature(node: &Node, source: &[u8]) -> String {
    let text = node_text(node, source);
    let first_line = text.lines().next().unwrap_or("");
    first_line.trim().to_string()
}

fn first_child_of_type<'a>(node: &'a Node, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

fn walk_tree<'a>(root: &Node<'a>) -> Vec<Node<'a>> {
    let mut nodes = vec![*root];
    let mut result = Vec::new();
    while let Some(current) = nodes.pop() {
        result.push(current);
        let mut cursor = current.walk();
        let mut children: Vec<Node<'a>> = current.children(&mut cursor).collect();
        children.reverse();
        nodes.extend(children);
    }
    result
}

fn has_named_owner(node: &Node) -> bool {
    let mut current = node.parent();
    let mut depth = 0;
    while let Some(parent) = current {
        if depth >= 4 {
            return false;
        }
        if parent.kind() == "function_declaration" || parent.kind() == "method_definition" {
            return true;
        }
        if parent.kind() == "pair" {
            let value_node = parent.child_by_field_name("value");
            let key_node = parent.child_by_field_name("key");
            if value_node.map_or(false, |v| &v == node) && key_node.is_some() {
                return true;
            }
        }
        if parent.kind() == "variable_declarator" {
            let mut cursor = parent.walk();
            if parent
                .children(&mut cursor)
                .any(|c| c.kind() == "identifier")
            {
                return true;
            }
        }
        current = parent.parent();
        depth += 1;
    }
    false
}
