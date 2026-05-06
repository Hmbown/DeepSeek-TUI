// Tree-sitter based symbol extraction engine.
//
// Provides `TreeSitterAdapter` that wraps tree-sitter parsers for 15 languages
// and extracts symbols, imports, calls, and bindings from source code.
//
// ## tree-sitter 0.25 API
//
// - `Language` constants use the `LANGUAGE.into()` pattern.
// - Query captures iterate via `StreamingIterator`.
// - Node cursors use `Node::walk()` + `Node::children(&mut cursor)`.

use std::collections::HashMap;
use std::path::Path;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::queries::queries;
use crate::types::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum source content size (10 MiB) -- larger files are skipped.
const MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum AST nesting depth -- trees deeper than this are skipped.
const MAX_NESTING_DEPTH: u32 = 1000;

/// Query type constants used as keys in the queries map.
const QTYPE_FUNCTION: &str = "function";
const QTYPE_CLASS: &str = "class";
const QTYPE_IMPORT: &str = "import";
const QTYPE_CALL: &str = "call";

// ---------------------------------------------------------------------------
// Module-level helper functions
// ---------------------------------------------------------------------------

/// Safely extract UTF-8 text from a tree-sitter node.
pub fn node_text<'a>(node: Node<'a>, source_bytes: &'a [u8]) -> &'a str {
    node.utf8_text(source_bytes).unwrap_or("")
}

/// Check whether child is within the byte span of parent.
pub fn within(child: Node, parent: Node) -> bool {
    child.start_byte() >= parent.start_byte() && child.end_byte() <= parent.end_byte()
}

/// Determine the call kind from a call-expression's function child.
///
/// Returns `"direct"` for simple identifiers, `"member"` for
/// member/field/navigation expressions, `"scoped"` for scoped identifiers.
pub fn call_reference_kind(node: Node) -> String {
    if let Some(parent) = node.parent() {
        // Check for tree-sitter node kind in any of the call-like node types.
        let call_kinds = [
            "call_expression",
            "call",
            "invocation_expression",
            "method_invocation",
            "function_call_expression",
            "member_call_expression",
            "call_expression",
        ];
        if call_kinds.contains(&parent.kind()) {
            if let Some(func) = parent.child_by_field_name("function") {
                let k = func.kind();
                if matches!(
                    k,
                    "member_expression"
                        | "field_expression"
                        | "navigation_expression"
                        | "selector_expression"
                        | "member_access_expression"
                        | "attribute"
                        | "directly_identified_expression"
                ) {
                    return "member".to_string();
                }
                if k == "scoped_identifier" {
                    return "scoped".to_string();
                }
            }
            return "direct".to_string();
        }
    }
    "unknown".to_string()
}

/// Walk the entire tree and collect all nodes in pre-order.
pub fn walk_tree(root: Node) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut cursor = root.walk();
    let mut visited_children = false;
    loop {
        nodes.push(cursor.node());
        if !visited_children && cursor.goto_first_child() {
            visited_children = false;
        } else if cursor.goto_next_sibling() {
            visited_children = false;
        } else if cursor.goto_parent() {
            visited_children = true;
        } else {
            break;
        }
    }
    nodes
}

/// Find the first direct child with the given node kind.
pub fn first_child_of_type<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return Some(child);
        }
    }
    None
}

/// Return the first line of a node's text (the "signature").
pub fn signature(node: Node, source_bytes: &[u8]) -> String {
    let text = node_text(node, source_bytes);
    text.lines().next().unwrap_or("").trim().to_string()
}

/// Check whether a tree's nesting depth exceeds `max_depth`.
///
/// Uses iterative DFS with cursor traversal to avoid stack overflow on
/// deeply nested trees.
fn has_excessive_nesting(root: Node, max_depth: u32) -> bool {
    let mut cursor = root.walk();
    let mut depth: u32 = 1;
    let mut visited_children = false;

    loop {
        if depth > max_depth {
            return true;
        }
        if !visited_children && cursor.goto_first_child() {
            depth += 1;
            visited_children = false;
        } else if cursor.goto_next_sibling() {
            visited_children = false;
        } else if cursor.goto_parent() {
            depth = depth.saturating_sub(1);
            visited_children = true;
        } else {
            break;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Raw capture data (extracted from query matches)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct RawCapture {
    name: String,
    kind: String,
    start_byte: usize,
    end_byte: usize,
    start_row: usize,
    end_row: usize,
    start_col: usize,
}

// ---------------------------------------------------------------------------
// TreeSitterAdapter
// ---------------------------------------------------------------------------

/// Wraps tree-sitter parsers, compiled queries, and per-language `Language`
/// handles for multi-language code analysis.
pub struct TreeSitterAdapter {
    /// Language name -> Parser instance.
    parsers: HashMap<String, Parser>,
    /// Language name -> (query type -> compiled Query).
    queries: HashMap<String, HashMap<String, Query>>,
    /// Language name -> raw Language handle (for Query::new).
    languages: HashMap<String, Language>,

    // -- Per-parse result buffers --------------------------------------------
    current_symbols: Vec<Symbol>,
    current_imports: Vec<String>,
    current_calls: Vec<(String, usize, String)>,
    current_import_bindings: Vec<JsImportBinding>,
    current_exports: Vec<JsExportBinding>,
    /// File path used in the most recent `parse` call.
    current_file: String,
}

impl TreeSitterAdapter {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new adapter, loading all available language parsers and
    /// compiling all query patterns.
    ///
    /// Languages whose tree-sitter crate is not linked are silently skipped.
    pub fn new() -> Self {
        let mut adapter = Self {
            parsers: HashMap::new(),
            queries: HashMap::new(),
            languages: HashMap::new(),
            current_symbols: Vec::new(),
            current_imports: Vec::new(),
            current_calls: Vec::new(),
            current_import_bindings: Vec::new(),
            current_exports: Vec::new(),
            current_file: String::new(),
        };
        adapter.init_parsers();
        adapter.init_queries();
        adapter
    }

    /// Load all 15 language parsers. Missing crates are silently skipped.
    fn init_parsers(&mut self) {
        // Helper: try to register one language.
        macro_rules! try_load {
            ($name:expr, $lang:expr) => {
                if !self.languages.contains_key($name) {
                    let lang: Language = $lang;
                    let mut parser = Parser::new();
                    if parser.set_language(&lang).is_ok() {
                        self.parsers.insert($name.to_string(), parser);
                        self.languages.insert($name.to_string(), lang);
                    }
                }
            };
        }

        // -- 8 existing languages -------------------------------------------
        try_load!("python", tree_sitter_python::LANGUAGE.into());
        try_load!("javascript", tree_sitter_javascript::LANGUAGE.into());
        try_load!(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        );
        try_load!("go", tree_sitter_go::LANGUAGE.into());
        try_load!("rust", tree_sitter_rust::LANGUAGE.into());
        try_load!("html", tree_sitter_html::LANGUAGE.into());
        try_load!("css", tree_sitter_css::LANGUAGE.into());
        try_load!("json", tree_sitter_json::LANGUAGE.into());
        // 7 additional languages (java, kotlin, swift, cpp, csharp, php, ruby)
        // have tree-sitter grammars but not yet published as Rust crates.
        // Queries for these languages are defined in queries.rs and will be
        // compiled automatically when the corresponding parser crates are added.
    }

    /// Compile all query patterns from `queries()` into `Query` objects.
    fn init_queries(&mut self) {
        for (lang, qtype, pattern) in queries() {
            if let Some(language) = self.languages.get(lang) {
                if let Ok(query) = Query::new(language, pattern) {
                    self.queries
                        .entry(lang.to_string())
                        .or_default()
                        .insert(qtype.to_string(), query);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Public API (called by RepoMapEngine)
    // -----------------------------------------------------------------------

    /// Parse a source file and extract all symbols, imports, calls, and
    /// JS/TS-specific bindings.
    ///
    /// Results are stored internally and accessed via `symbols()`, `imports()`,
    /// `calls()`, `import_bindings()`, and `exports()`.
    pub fn parse(&mut self, path: &Path, content: &str, lang: &str) -> Result<(), String> {
        self.clear_results();
        self.current_file = path.to_string_lossy().to_string();

        // Size guard
        if content.len() > MAX_CONTENT_BYTES {
            return Ok(());
        }

        // Get or create parser
        let parser = self
            .parsers
            .get_mut(lang)
            .ok_or_else(|| format!("No parser registered for language: {}", lang))?;

        // Parse
        let source_bytes = content.as_bytes();
        let tree = parser
            .parse(source_bytes, None)
            .ok_or_else(|| "Tree-sitter returned None from parse".to_string())?;

        // Nesting depth guard
        if has_excessive_nesting(tree.root_node(), MAX_NESTING_DEPTH) {
            return Ok(());
        }

        let root = tree.root_node();

        // Extract symbols
        {
            let symbols = self.extract_symbols(root, source_bytes, lang);
            self.current_symbols = symbols;
        }

        // Extract imports
        {
            let imports = self.extract_imports(root, source_bytes, lang);
            self.current_imports = imports;
        }

        // Extract calls
        {
            let calls = self.extract_calls(root, source_bytes, lang);
            self.current_calls = calls;
        }

        // JS/TS-specific bindings
        if lang == "javascript" || lang == "typescript" {
            let bindings = self.extract_js_ts_import_bindings(root, source_bytes);
            self.current_import_bindings = bindings;

            let exports = self.extract_js_ts_export_bindings(root, source_bytes);
            self.current_exports = exports;

            // Additional JS/TS symbol extraction passes.
            let extra = self.extract_object_literal_methods(root, source_bytes);
            self.current_symbols.extend(extra);

            let extra2 = self.extract_anonymous_symbols(root, source_bytes);
            self.current_symbols.extend(extra2);

            let extra3 = self.extract_exported_function_expressions(root, source_bytes);
            self.current_symbols.extend(extra3);
        }

        Ok(())
    }

    /// Return symbols extracted during the last `parse` call.
    pub fn symbols(&self) -> Vec<Symbol> {
        self.current_symbols.clone()
    }

    /// Return import paths extracted during the last `parse` call.
    pub fn imports(&self) -> Vec<String> {
        self.current_imports.clone()
    }

    /// Return calls (name, line, kind) extracted during the last `parse` call.
    pub fn calls(&self) -> Vec<(String, usize, String)> {
        self.current_calls.clone()
    }

    /// Return JS/TS import bindings extracted during the last `parse` call.
    pub fn import_bindings(&self) -> Vec<JsImportBinding> {
        self.current_import_bindings.clone()
    }

    /// Return JS/TS export bindings extracted during the last `parse` call.
    pub fn exports(&self) -> Vec<JsExportBinding> {
        self.current_exports.clone()
    }

    // -----------------------------------------------------------------------
    // Internal: run queries and collect raw captures
    // -----------------------------------------------------------------------

    /// Run a named query type for a language and return raw captures.
    fn run_query(
        &self,
        lang: &str,
        qtype: &str,
        root: Node,
        source_bytes: &[u8],
    ) -> Vec<RawCapture> {
        let mut results = Vec::new();

        let lang_queries = match self.queries.get(lang) {
            Some(q) => q,
            None => return results,
        };
        let query = match lang_queries.get(qtype) {
            Some(q) => q,
            None => return results,
        };

        let name_idx = query.capture_index_for_name("name");
        let def_idx = query.capture_index_for_name("def");

        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(query, root, source_bytes);

        while let Some(item) = captures.next() {
            let (match_, _capture_index) = item;
            let mut name_node: Option<Node> = None;
            let mut def_node: Option<Node> = None;

            for capture in match_.captures {
                if Some(capture.index) == name_idx {
                    name_node = Some(capture.node);
                }
                if Some(capture.index) == def_idx {
                    def_node = Some(capture.node);
                }
            }

            if let (Some(name), Some(def)) = (name_node, def_node) {
                let pos = def.start_position();
                let end_pos = def.end_position();
                results.push(RawCapture {
                    name: node_text(name, source_bytes).to_string(),
                    kind: qtype.to_string(),
                    start_byte: def.start_byte(),
                    end_byte: def.end_byte(),
                    start_row: pos.row,
                    end_row: end_pos.row,
                    start_col: pos.column,
                });
            }
        }

        results
    }

    // -----------------------------------------------------------------------
    // Internal: extract_symbols
    // -----------------------------------------------------------------------

    /// Extract symbols from the AST using function + class queries.
    ///
    /// For HTML / CSS / JSON, uses specialised plain-symbol extraction.
    /// For JS/TS, only collects named declarations here; anonymous and
    /// object-literal methods are added in separate passes.
    fn extract_symbols(&self, root: Node, source_bytes: &[u8], lang: &str) -> Vec<Symbol> {
        // HTML / CSS / JSON use simplified extractors.
        match lang {
            "html" => return self.extract_html_tags(root, source_bytes),
            "css" => return self.extract_css_selectors(root, source_bytes),
            "json" => return self.extract_json_pairs(root, source_bytes),
            _ => {}
        }

        let file = &self.current_file;

        // Run function + class queries.
        let mut captures = Vec::new();
        captures.extend(self.run_query(lang, QTYPE_FUNCTION, root, source_bytes));
        captures.extend(self.run_query(lang, QTYPE_CLASS, root, source_bytes));

        // Sort by span size ascending, then by start byte (inner first).
        captures.sort_by(|a, b| {
            let a_size = a.end_byte - a.start_byte;
            let b_size = b.end_byte - b.start_byte;
            a_size.cmp(&b_size).then(a.start_byte.cmp(&b.start_byte))
        });

        // Build symbols.
        let mut symbols: Vec<Symbol> = Vec::new();

        for cap in &captures {
            let visibility = if cap.kind == "class" {
                "pub"
            } else {
                "private"
            };

            let id = format!("{}:{}:{}", file, cap.name, cap.start_row + 1);

            symbols.push(Symbol::new(
                id,
                cap.name.clone(),
                cap.kind.clone(),
                file.clone(),
                cap.start_row + 1, // 1-indexed
                cap.end_row + 1,
                cap.start_col + 1,
                visibility.to_string(),
                String::new(), // docstring (not extracted yet)
                String::new(), // signature (set below)
            ));
        }

        symbols
    }

    /// Extract HTML elements as symbols.
    fn extract_html_tags(&self, root: Node, source_bytes: &[u8]) -> Vec<Symbol> {
        let file = &self.current_file;
        let captures = self.run_query("html", QTYPE_FUNCTION, root, source_bytes);
        let mut symbols = Vec::new();
        for cap in &captures {
            let id = format!("{}:{}:{}", file, cap.name, cap.start_row + 1);
            symbols.push(Symbol::new(
                id,
                cap.name.clone(),
                "element".to_string(),
                file.clone(),
                cap.start_row + 1,
                cap.end_row + 1,
                cap.start_col + 1,
                String::new(),
                String::new(),
                String::new(),
            ));
        }
        symbols
    }

    /// Extract CSS selectors as symbols.
    fn extract_css_selectors(&self, root: Node, source_bytes: &[u8]) -> Vec<Symbol> {
        let file = &self.current_file;
        let captures = self.run_query("css", QTYPE_FUNCTION, root, source_bytes);
        let mut symbols = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for cap in &captures {
            if !seen.insert(cap.name.clone()) {
                continue;
            }
            let id = format!("{}:{}:{}", file, cap.name, cap.start_row + 1);
            symbols.push(Symbol::new(
                id,
                cap.name.clone(),
                "selector".to_string(),
                file.clone(),
                cap.start_row + 1,
                cap.end_row + 1,
                cap.start_col + 1,
                String::new(),
                String::new(),
                String::new(),
            ));
        }
        symbols
    }

    /// Extract JSON key-value pairs as symbols.
    fn extract_json_pairs(&self, root: Node, source_bytes: &[u8]) -> Vec<Symbol> {
        let file = &self.current_file;
        let captures = self.run_query("json", QTYPE_FUNCTION, root, source_bytes);
        let mut symbols = Vec::new();
        for cap in &captures {
            let id = format!("{}:{}:{}", file, cap.name, cap.start_row + 1);
            symbols.push(Symbol::new(
                id,
                cap.name.trim_matches('"').to_string(),
                "json_key".to_string(),
                file.clone(),
                cap.start_row + 1,
                cap.end_row + 1,
                cap.start_col + 1,
                String::new(),
                String::new(),
                String::new(),
            ));
        }
        symbols
    }

    // -----------------------------------------------------------------------
    // Internal: extract_imports
    // -----------------------------------------------------------------------

    /// Extract import paths from the AST.
    fn extract_imports(&self, root: Node, source_bytes: &[u8], lang: &str) -> Vec<String> {
        let captures = self.run_query(lang, QTYPE_IMPORT, root, source_bytes);

        match lang {
            // Rust: prefer the scoped path, convert :: to /
            "rust" => {
                let mut imports = Vec::new();
                for cap in &captures {
                    imports.push(cap.name.clone().replace("::", "/"));
                }
                imports
            }
            // JS/TS: strip quotes from module path
            "javascript" | "typescript" => {
                let mut imports = Vec::new();
                for cap in &captures {
                    let cleaned = cap.name.trim_matches('\'').trim_matches('"').to_string();
                    if !cleaned.is_empty() {
                        imports.push(cleaned);
                    }
                }
                imports
            }
            // Python: keep dotted name as-is
            "python" => {
                let mut imports = Vec::new();
                for cap in &captures {
                    imports.push(cap.name.clone());
                }
                imports
            }
            // C++: strip angle brackets / quotes from include paths
            "cpp" => {
                let mut imports = Vec::new();
                for cap in &captures {
                    let cleaned = cap
                        .name
                        .trim_matches('<')
                        .trim_matches('>')
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    imports.push(cleaned);
                }
                imports
            }
            // PHP: strip surrounding quotes
            "php" => {
                let mut imports = Vec::new();
                for cap in &captures {
                    let cleaned = cap.name.trim_matches('\'').trim_matches('"').to_string();
                    imports.push(cleaned);
                }
                imports
            }
            // Ruby: walk the tree to find require/include calls
            "ruby" => {
                let mut imports = Vec::new();
                for node in walk_tree(root) {
                    if node.kind() == "call" {
                        if let Some(method) = node.child_by_field_name("method") {
                            let method_name = node_text(method, source_bytes);
                            if matches!(
                                method_name,
                                "require"
                                    | "require_relative"
                                    | "include"
                                    | "extend"
                                    | "load"
                                    | "autoload"
                            ) {
                                if let Some(args) = node.child_by_field_name("arguments") {
                                    let mut arg_cursor = args.walk();
                                    for arg in args.children(&mut arg_cursor) {
                                        let arg_kind = arg.kind();
                                        if arg_kind == "string" || arg_kind == "simple_symbol" {
                                            let text = node_text(arg, source_bytes)
                                                .trim_matches('\'')
                                                .trim_matches('"')
                                                .trim_matches(':')
                                                .to_string();
                                            imports.push(text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                imports
            }
            // Default: use raw capture text
            _ => {
                let mut imports = Vec::new();
                for cap in &captures {
                    imports.push(cap.name.clone());
                }
                imports
            }
        }
    }

    // -----------------------------------------------------------------------
    // Internal: extract_calls
    // -----------------------------------------------------------------------

    /// Extract call expressions from the AST.
    fn extract_calls(&self, root: Node, source_bytes: &[u8], lang: &str) -> Vec<(String, usize, String)> {
        let mut calls: Vec<(String, usize, String)> = Vec::new();

        // Run the call query and use `call_reference_kind` for
        // accurate kind detection via parent node inspection.
        if let Some(lang_queries) = self.queries.get(lang) {
            if let Some(query) = lang_queries.get(QTYPE_CALL) {
                let def_idx = query.capture_index_for_name("def");
                let name_idx = query.capture_index_for_name("name");

                let mut cursor = QueryCursor::new();
                let mut captures2 = cursor.captures(query, root, source_bytes);

                while let Some(item) = captures2.next() {
                    let (match_, _idx) = item;
                    let mut def_node: Option<Node> = None;
                    let mut name_node: Option<Node> = None;

                    for cap in match_.captures {
                        if Some(cap.index) == def_idx {
                            def_node = Some(cap.node);
                        }
                        if Some(cap.index) == name_idx {
                            name_node = Some(cap.node);
                        }
                    }

                    if let (Some(def), Some(name)) = (def_node, name_node) {
                        let name_text = node_text(name, source_bytes).to_string();
                        let kind = call_reference_kind(def);
                        let line = def.start_position().row + 1;
                        calls.push((name_text, line, kind));
                    }
                }
            }
        }

        // Deduplicate by (name, line).
        let mut seen = std::collections::HashSet::new();
        calls.retain(|(name, line, _kind)| seen.insert((name.clone(), *line)));

        calls
    }

    // -----------------------------------------------------------------------
    // Internal: JS/TS import bindings
    // -----------------------------------------------------------------------

    /// Extract import bindings for JavaScript / TypeScript files.
    ///
    /// Handles both ES module `import` statements and CommonJS `require()`.
    fn extract_js_ts_import_bindings(&self, root: Node, source_bytes: &[u8]) -> Vec<JsImportBinding> {
        let mut bindings = Vec::new();

        for node in walk_tree(root) {
            match node.kind() {
                // ---- ES import statement -----------------------------------
                "import_statement" => {
                    let line = node.start_position().row + 1;

                    let source = node
                        .child_by_field_name("source")
                        .map(|n| {
                            node_text(n, source_bytes)
                                .trim_matches('\'')
                                .trim_matches('"')
                                .to_string()
                        })
                        .unwrap_or_default();

                    if source.is_empty() {
                        continue;
                    }

                    if let Some(clause) = node.child_by_field_name("import_clause") {
                        // 1. Default import: `import foo from ...`
                        if let Some(default) = clause.child_by_field_name("name") {
                            let local = node_text(default, source_bytes).to_string();
                            bindings.push(JsImportBinding {
                                local_name: local.clone(),
                                imported_name: local,
                                module: source.clone(),
                                line,
                                kind: "default".to_string(),
                            });
                        }

                        // 2. Named imports: `import { a, b } from ...`
                        if let Some(named) = clause.child_by_field_name("named_imports") {
                            let mut spec_cursor = named.walk();
                            for spec in named.children(&mut spec_cursor) {
                                if spec.kind() == "import_specifier" {
                                    let imported = spec
                                        .child_by_field_name("name")
                                        .map(|n| node_text(n, source_bytes).to_string())
                                        .unwrap_or_default();
                                    let local = spec
                                        .child_by_field_name("alias")
                                        .map(|n| node_text(n, source_bytes).to_string())
                                        .unwrap_or_else(|| imported.clone());
                                    if !imported.is_empty() {
                                        bindings.push(JsImportBinding {
                                            local_name: local,
                                            imported_name: imported,
                                            module: source.clone(),
                                            line,
                                            kind: "named".to_string(),
                                        });
                                    }
                                }
                            }
                        }

                        // 3. Namespace import: `import * as ns from ...`
                        if let Some(ns) = clause.child_by_field_name("namespace_import") {
                            if let Some(name) = ns.child_by_field_name("name") {
                                let ns_name = node_text(name, source_bytes).to_string();
                                bindings.push(JsImportBinding {
                                    local_name: ns_name.clone(),
                                    imported_name: "*".to_string(),
                                    module: source.clone(),
                                    line,
                                    kind: "namespace".to_string(),
                                });
                            }
                        }
                    }

                    // 4. Side-effect-only: `import 'module'`
                    if bindings.iter().all(|b| b.line != line || b.module != source) {
                        // Only add if no other bindings for this source on same line
                        // (but import_statement is one line so it's fine)
                    }
                }

                // ---- CommonJS require() ------------------------------------
                "call_expression" => {
                    let func = node.child_by_field_name("function");
                    if func.map_or(true, |f| node_text(f, source_bytes) != "require") {
                        continue;
                    }

                    let line = node.start_position().row + 1;
                    let args = node.child_by_field_name("arguments");
                    let source = args
                        .and_then(|a| {
                            let mut cur = a.walk();
                            a.children(&mut cur)
                                .find(|c| c.kind() == "string")
                        })
                        .map(|s| {
                            node_text(s, source_bytes)
                                .trim_matches('\'')
                                .trim_matches('"')
                                .to_string()
                        })
                        .unwrap_or_default();

                    if source.is_empty() {
                        continue;
                    }

                    let local_name = find_require_lhs(node, source_bytes);
                    bindings.push(JsImportBinding {
                        local_name,
                        imported_name: "default".to_string(),
                        module: source,
                        line,
                        kind: "require".to_string(),
                    });
                }

                _ => {}
            }
        }

        bindings
    }

    // -----------------------------------------------------------------------
    // Internal: JS/TS export bindings
    // -----------------------------------------------------------------------

    /// Extract export bindings for JavaScript / TypeScript files.
    ///
    /// Handles ES `export` statements and CommonJS `module.exports`.
    fn extract_js_ts_export_bindings(&self, root: Node, source_bytes: &[u8]) -> Vec<JsExportBinding> {
        let mut exports = Vec::new();

        for node in walk_tree(root) {
            match node.kind() {
                // ---- ES export statement -----------------------------------
                "export_statement" => {
                    let line = node.start_position().row + 1;

                    // 1. export declaration: `export function foo() {}`
                    if let Some(decl) = node.child_by_field_name("declaration") {
                        if let Some(name) = decl.child_by_field_name("name") {
                            let name_text = node_text(name, source_bytes).to_string();
                            exports.push(JsExportBinding {
                                exported_name: name_text.clone(),
                                source_name: Some(name_text),
                                module: None,
                                line,
                                kind: "named".to_string(),
                            });
                        }
                    }

                    // 2. export default
                    if let Some(default) = node.child_by_field_name("default") {
                        if let Some(name) = default.child_by_field_name("name") {
                            let name_text = node_text(name, source_bytes).to_string();
                            exports.push(JsExportBinding {
                                exported_name: "default".to_string(),
                                source_name: Some(name_text),
                                module: None,
                                line,
                                kind: "default".to_string(),
                            });
                        } else {
                            exports.push(JsExportBinding {
                                exported_name: "default".to_string(),
                                source_name: None,
                                module: None,
                                line,
                                kind: "default".to_string(),
                            });
                        }
                    }

                    // 3. Named exports: `export { name1, name2 }`
                    if let Some(export_clause) = node.child_by_field_name("export_clause") {
                        let mut spec_cursor = export_clause.walk();
                        for spec in export_clause.children(&mut spec_cursor) {
                            if spec.kind() == "export_specifier" {
                                let local = spec
                                    .child_by_field_name("name")
                                    .map(|n| node_text(n, source_bytes).to_string());
                                let exported = spec
                                    .child_by_field_name("alias")
                                    .map(|n| node_text(n, source_bytes).to_string())
                                    .or_else(|| local.clone());
                                if let (Some(local), Some(exported)) = (local, exported) {
                                    exports.push(JsExportBinding {
                                        exported_name: exported,
                                        source_name: Some(local),
                                        module: None,
                                        line,
                                        kind: "named".to_string(),
                                    });
                                }
                            }
                        }
                    }

                    // 4. Re-export with source: `export * from 'module'`
                    if let Some(source) = node.child_by_field_name("source") {
                        let module = node_text(source, source_bytes)
                            .trim_matches('\'')
                            .trim_matches('"')
                            .to_string();

                        if node.child_by_field_name("export_clause").is_none()
                            && node.child_by_field_name("declaration").is_none()
                        {
                            // `export * from 'module'`
                            exports.push(JsExportBinding {
                                exported_name: "*".to_string(),
                                source_name: None,
                                module: Some(module),
                                line,
                                kind: "re-export-wildcard".to_string(),
                            });
                        }
                    }
                }

                // ---- CommonJS module.exports -------------------------------
                "assignment_expression" => {
                    let left = node.child_by_field_name("left");
                    let right = node.child_by_field_name("right");
                    if let (Some(l), Some(r)) = (left, right) {
                        let left_text = node_text(l, source_bytes);
                        let line = node.start_position().row + 1;

                        if left_text == "module.exports" {
                            // `module.exports = { ... }`
                            if r.kind() == "object" || r.kind() == "object_pattern" {
                                exports.push(JsExportBinding {
                                    exported_name: "default".to_string(),
                                    source_name: None,
                                    module: None,
                                    line,
                                    kind: "commonjs-object".to_string(),
                                });
                                let mut obj_cursor = r.walk();
                                for pair in r.children(&mut obj_cursor) {
                                    if pair.kind() == "pair" {
                                        if let Some(key) = pair.child_by_field_name("key") {
                                            let key_text = node_text(key, source_bytes).to_string();
                                            exports.push(JsExportBinding {
                                                exported_name: key_text.clone(),
                                                source_name: Some(key_text),
                                                module: None,
                                                line,
                                                kind: "commonjs-property".to_string(),
                                            });
                                        }
                                    }
                                }
                            } else {
                                // `module.exports = value`
                                let name = r
                                    .child_by_field_name("name")
                                    .map(|n| node_text(n, source_bytes).to_string());
                                exports.push(JsExportBinding {
                                    exported_name: "default".to_string(),
                                    source_name: name,
                                    module: None,
                                    line,
                                    kind: "commonjs".to_string(),
                                });
                            }
                        }

                        // `exports.foo = ...`
                        if left_text.starts_with("exports.") {
                            let name = left_text.strip_prefix("exports.").unwrap_or("").to_string();
                            if !name.is_empty() {
                                exports.push(JsExportBinding {
                                    exported_name: name.clone(),
                                    source_name: Some(name),
                                    module: None,
                                    line,
                                    kind: "commonjs-property".to_string(),
                                });
                            }
                        }
                    }
                }

                _ => {}
            }
        }

        exports
    }

    // -----------------------------------------------------------------------
    // Internal: JS/TS additional symbol extraction passes
    // -----------------------------------------------------------------------

    /// Extract methods from object literals (e.g. `{ foo() {} }`).
    fn extract_object_literal_methods(&self, root: Node, source_bytes: &[u8]) -> Vec<Symbol> {
        let file = &self.current_file;
        let mut symbols = Vec::new();

        for node in walk_tree(root) {
            if node.kind() == "pair" {
                if let Some(key) = node.child_by_field_name("key") {
                    if let Some(value) = node.child_by_field_name("value") {
                        let is_function = value.kind() == "function"
                            || value.kind() == "arrow_function"
                            || value.kind() == "method_definition";
                        if is_function {
                            let name = node_text(key, source_bytes).to_string();
                            let pos = key.start_position();
                            let end = value.end_position();
                            let id = format!("{}:{}:{}", file, name, pos.row + 1);
                            symbols.push(Symbol::new(
                                id,
                                name,
                                "method".to_string(),
                                file.clone(),
                                pos.row + 1,
                                end.row + 1,
                                pos.column + 1,
                                String::new(),
                                String::new(),
                                String::new(),
                            ));
                        }
                    }
                }
            }
        }

        symbols
    }

    /// Extract anonymous functions assigned to variables or exported.
    ///
    /// Handles:
    /// - `const foo = function() {}`
    /// - `const foo = () => {}`
    fn extract_anonymous_symbols(&self, root: Node, source_bytes: &[u8]) -> Vec<Symbol> {
        let file = &self.current_file;
        let mut symbols = Vec::new();

        for node in walk_tree(root) {
            if node.kind() == "variable_declarator" {
                if let Some(name) = node.child_by_field_name("name") {
                    if let Some(value) = node.child_by_field_name("value") {
                        let is_anonymous = value.kind() == "arrow_function"
                            || value.kind() == "function"
                            || (value.kind() == "function_declaration"
                                && value.child_by_field_name("name").is_none());
                        if is_anonymous {
                            let sym_name = node_text(name, source_bytes).to_string();
                            let pos = name.start_position();
                            let end = value.end_position();
                            let id = format!("{}:{}:{}", file, sym_name, pos.row + 1);
                            symbols.push(Symbol::new(
                                id,
                                sym_name,
                                "function".to_string(),
                                file.clone(),
                                pos.row + 1,
                                end.row + 1,
                                pos.column + 1,
                                String::new(),
                                String::new(),
                                String::new(),
                            ));
                        }
                    }
                }
            }
        }

        symbols
    }

    /// Extract function expressions that are directly exported.
    ///
    /// Handles:
    /// - `export default function() {}`
    /// - `export default () => {}`
    fn extract_exported_function_expressions(&self, root: Node, source_bytes: &[u8]) -> Vec<Symbol> {
        let file = &self.current_file;
        let mut symbols = Vec::new();

        for node in walk_tree(root) {
            if node.kind() == "export_statement" {
                if let Some(default) = node.child_by_field_name("default") {
                    let is_anonymous_fn = matches!(default.kind(), "function" | "arrow_function")
                        && default.child_by_field_name("name").is_none();

                    if is_anonymous_fn {
                        let pos = default.start_position();
                        let end = default.end_position();
                        let name = "default_export".to_string();
                        let id = format!("{}:{}:{}", file, name, pos.row + 1);
                        symbols.push(Symbol::new(
                            id,
                            name,
                            "function".to_string(),
                            file.clone(),
                            pos.row + 1,
                            end.row + 1,
                            pos.column + 1,
                            "export".to_string(),
                            String::new(),
                            String::new(),
                        ));
                    }
                }

                // `export { foo as default }`
                if let Some(clause) = node.child_by_field_name("export_clause") {
                    let mut cur = clause.walk();
                    for spec in clause.children(&mut cur) {
                        if spec.kind() == "export_specifier" {
                            if let Some(alias) = spec.child_by_field_name("alias") {
                                if node_text(alias, source_bytes) == "default" {
                                    if let Some(name) = spec.child_by_field_name("name") {
                                        let sym_name = node_text(name, source_bytes).to_string();
                                        let pos = name.start_position();
                                        let id = format!("{}:{}:{}", file, sym_name, pos.row + 1);
                                        symbols.push(Symbol::new(
                                            id,
                                            sym_name,
                                            "function".to_string(),
                                            file.clone(),
                                            pos.row + 1,
                                            pos.row + 1,
                                            pos.column + 1,
                                            "export".to_string(),
                                            String::new(),
                                            String::new(),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        symbols
    }

    // -----------------------------------------------------------------------
    // Internal: state management
    // -----------------------------------------------------------------------

    /// Clear all per-parse result buffers.
    fn clear_results(&mut self) {
        self.current_symbols.clear();
        self.current_imports.clear();
        self.current_calls.clear();
        self.current_import_bindings.clear();
        self.current_exports.clear();
        self.current_file.clear();
    }
}

impl Default for TreeSitterAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helper for extracting the LHS variable name from `const x = require(...)`
// ---------------------------------------------------------------------------

/// Given a `require()` call_expression node, find the variable it is assigned
/// to by walking up to the parent variable_declarator.
fn find_require_lhs(node: Node, source_bytes: &[u8]) -> String {
    if let Some(parent) = node.parent() {
        if parent.kind() == "variable_declarator" {
            if let Some(name) = parent.child_by_field_name("name") {
                return node_text(name, source_bytes).to_string();
            }
        }
    }
    String::new()
}
