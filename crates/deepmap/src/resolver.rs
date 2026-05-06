// Import resolver for DeepMap.
//
// Resolves import paths to concrete file paths using relative-path resolution,
// TypeScript/JavaScript tsconfig/jsconfig path aliases, and direct stem lookups.
//
// ## Resolution order
//
// 1. **Relative** (`./` or `../`) — canonicalise, append known extensions, try
//    `/index.{ext}` fallback.
// 2. **Alias** — match against every `paths` entry in discovered tsconfigs.
// 3. **Stem lookup** — bare import that matches a file stem in the graph.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use crate::types::{PathAliasRule, ProjectImportConfig, RepoGraph, Symbol};

/// Resolves import paths within a project using multiple strategies.
pub struct ImportResolver {
    pub project_root: PathBuf,
    pub import_configs: Vec<ProjectImportConfig>,
    /// Stem (file name without extension) → full relative paths.
    pub file_map: HashMap<String, Vec<String>>,
    /// Symbol name → list of symbol IDs (for call-target resolution).
    pub name_index: HashMap<String, Vec<String>>,
}

// Common extensions tried during relative and alias resolution.
const TRY_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".json", ".d.ts"];
const INDEX_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".json"];

impl ImportResolver {
    /// Create a resolver with the given initial configs (no graph indices).
    ///
    /// Used internally by `RepoMapEngine` during bootstrap where the graph
    /// has not yet been populated.
    pub fn new(project_root: &Path, _initial_configs: &[ProjectImportConfig]) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            import_configs: Vec::new(),
            file_map: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    /// Full constructor: build stem / name indices from the graph and
    /// auto-discover tsconfig/jsconfig files on disk.
    ///
    /// This is the constructor callers should use when a dependency graph
    /// is already available.
    pub fn from_graph(project_root: PathBuf, graph: &RepoGraph) -> Self {
        let (file_map, name_index) = Self::build_indices(graph);
        let mut resolver = Self {
            project_root,
            import_configs: Vec::new(),
            file_map,
            name_index,
        };
        let _ = resolver.discover_import_configs();
        resolver
    }

    // -----------------------------------------------------------------------
    // Index construction
    // -----------------------------------------------------------------------

    /// Build the stem→paths and name→symbol-IDs indices from the graph.
    ///
    /// * `file_map`: `HashMap<stem, Vec<relative_path>>`
    /// * `name_index`: `HashMap<symbol_name, Vec<symbol_id>>`
    fn build_indices(
        graph: &RepoGraph,
    ) -> (
        HashMap<String, Vec<String>>,
        HashMap<String, Vec<String>>,
    ) {
        let mut file_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut name_index: HashMap<String, Vec<String>> = HashMap::new();

        // Build file_map from file_symbols keys.
        for file_path in graph.file_symbols.keys() {
            let stem = Path::new(file_path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if !stem.is_empty() {
                file_map
                    .entry(stem)
                    .or_default()
                    .push(file_path.clone());
            }
        }

        // Build name_index from symbols.
        for (id, sym) in &graph.symbols {
            name_index
                .entry(sym.name.clone())
                .or_default()
                .push(id.clone());
        }

        (file_map, name_index)
    }

    // -----------------------------------------------------------------------
    // tsconfig discovery & parsing
    // -----------------------------------------------------------------------

    /// Walk the project root (one level deep) for `tsconfig.json` or
    /// `jsconfig.json` files and parse them into `ProjectImportConfig`s.
    pub fn discover_import_configs(&mut self) -> Result<(), String> {
        let config_names = &["tsconfig.json", "jsconfig.json"];

        // Check project root.
        for name in config_names {
            let path = self.project_root.join(name);
            if path.exists() {
                match Self::parse_tsconfig(&path) {
                    Ok(cfg) => self.import_configs.push(cfg),
                    Err(e) => log::warn!("Failed to parse {}: {}", path.display(), e),
                }
            }
        }

        // Check one level of subdirectories.
        if let Ok(entries) = fs::read_dir(&self.project_root) {
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_dir() {
                    continue;
                }
                for name in config_names {
                    let path = entry.path().join(name);
                    if path.exists() {
                        match Self::parse_tsconfig(&path) {
                            Ok(cfg) => self.import_configs.push(cfg),
                            Err(e) => {
                                log::warn!(
                                    "Failed to parse {}: {}",
                                    path.display(),
                                    e
                                )
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Parse a tsconfig.json / jsconfig.json file and extract
    /// `compilerOptions.paths` and `compilerOptions.baseUrl`.
    pub fn parse_tsconfig(path: &Path) -> Result<ProjectImportConfig, String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

        let cleaned = Self::strip_jsonc(&raw);
        let json: serde_json::Value =
            serde_json::from_str(&cleaned).map_err(|e| format!("JSON parse error: {}", e))?;

        let config_dir = path.parent().map(|p| p.to_string_lossy().to_string());

        let base_url = json
            .get("compilerOptions")
            .and_then(|co| co.get("baseUrl"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let alias_rules = json
            .get("compilerOptions")
            .and_then(|co| co.get("paths"))
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(pattern, targets)| PathAliasRule {
                        alias_pattern: pattern.clone(),
                        target_patterns: targets
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ProjectImportConfig {
            config_path: Some(path.to_string_lossy().to_string()),
            config_dir,
            base_url,
            alias_rules,
        })
    }

    /// Remove `//` and `/* */` comments (but not inside strings) and trailing
    /// commas from a JSONC string so it can be parsed by `serde_json`.
    pub fn strip_jsonc(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            let c = chars[i];

            // String literal — copy verbatim until closing quote.
            if c == '"' {
                out.push(c);
                i += 1;
                while i < len {
                    let sc = chars[i];
                    out.push(sc);
                    if sc == '\\' {
                        // Skip escaped char.
                        if i + 1 < len {
                            i += 1;
                            out.push(chars[i]);
                        }
                    } else if sc == '"' {
                        break;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }

            // Line comment.
            if c == '/' && i + 1 < len && chars[i + 1] == '/' {
                // Trim trailing whitespace before the comment.
                let trimmed = out.trim_end().len();
                out.truncate(trimmed);
                i += 2;
                while i < len && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }

            // Block comment.
            if c == '/' && i + 1 < len && chars[i + 1] == '*' {
                i += 2;
                while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2; // skip */
                continue;
            }

            // Trailing comma before } or ].
            if c == ',' {
                let next_non_ws = chars[i + 1..].iter().copied().find(|&x| !x.is_whitespace());
                if matches!(next_non_ws, Some('}' | ']')) {
                    // Skip the comma — just don't emit it.
                    i += 1;
                    continue;
                }
            }

            out.push(c);
            i += 1;
        }

        out
    }

    // -----------------------------------------------------------------------
    // Import resolution
    // -----------------------------------------------------------------------

    /// Resolve an import string from a source file to zero or more concrete
    /// file paths (relative to the project root).
    ///
    /// Returns an empty `Vec` when nothing could be resolved.
    pub fn resolve_import_targets(
        &self,
        source_file: &str,
        import_path: &str,
    ) -> Vec<String> {
        let mut results = Vec::new();

        // ---------------------------------------------------------------
        // 1. Relative imports
        // ---------------------------------------------------------------
        if import_path.starts_with("./") || import_path.starts_with("../") {
            let source_dir = Path::new(source_file)
                .parent()
                .unwrap_or(Path::new(""));
            let resolved = source_dir.join(import_path);
            let normal = normalise_path(&resolved.to_string_lossy());

            // Try known extensions.
            for ext in TRY_EXTENSIONS {
                let candidate = format!("{}{}", normal, ext);
                if self.project_root.join(&candidate).exists() {
                    results.push(candidate);
                }
            }

            // Try /index.{ext} fallback.
            for ext in INDEX_EXTENSIONS {
                let candidate = format!("{}/index{}", normal, ext);
                if self.project_root.join(&candidate).exists() {
                    results.push(candidate);
                }
            }

            return results;
        }

        // ---------------------------------------------------------------
        // 2. Alias resolution (tsconfig paths)
        // ---------------------------------------------------------------
        for cfg in &self.import_configs {
            let base_url = cfg.base_url.as_deref().unwrap_or(".");
            let base_path = self.project_root.join(base_url);

            for rule in &cfg.alias_rules {
                let pattern = &rule.alias_pattern;

                let resolved = if let Some(wc_pos) = pattern.find('*') {
                    let prefix = &pattern[..wc_pos];
                    let suffix = &pattern[wc_pos + 1..];
                    if import_path.starts_with(prefix) && import_path.ends_with(suffix) {
                        let rest = &import_path[prefix.len()
                            ..import_path.len() - suffix.len()];
                        rule.target_patterns.first().map(|tp| {
                            let resolved_target = tp.replace('*', rest);
                            base_path.join(&resolved_target)
                        })
                    } else {
                        None
                    }
                } else if import_path == pattern {
                    rule.target_patterns.first().map(|tp| base_path.join(tp))
                } else {
                    None
                };

                if let Some(full_path) = resolved {
                    let full_str = full_path.to_string_lossy().to_string();
                    for ext in TRY_EXTENSIONS {
                        let candidate = format!("{}{}", full_str, ext);
                        if Path::new(&candidate).exists() {
                            results.push(candidate);
                        }
                    }
                    // Also try the raw path (might already have extension).
                    if Path::new(&full_str).exists() {
                        results.push(full_str);
                    }
                }

                if !results.is_empty() {
                    return results;
                }
            }
        }

        // ---------------------------------------------------------------
        // 3. Direct stem lookup
        // ---------------------------------------------------------------
        let stem = Path::new(import_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        if let Some(paths) = self.file_map.get(&stem) {
            for p in paths {
                if !results.contains(p) {
                    results.push(p.clone());
                }
            }
        }

        results
    }

    /// Convenience wrapper used internally by the engine's edge builder.
    /// Returns the single best-matching file path, or `None`.
    pub fn resolve_import(&self, source_file: &str, import_path: &str) -> Option<String> {
        self.resolve_import_targets(source_file, import_path)
            .into_iter()
            .next()
    }

    // -----------------------------------------------------------------------
    // Call-target resolution
    // -----------------------------------------------------------------------

    /// Given a file and a line number, find the symbol whose line range
    /// contains `call_line`.  Returns the symbol name if found.
    pub fn resolve_calling_symbol_with_graph(
        &self,
        file: &str,
        call_line: usize,
        graph: &RepoGraph,
    ) -> Option<String> {
        if let Some(sym_ids) = graph.file_symbols.get(file) {
            // Symbols are typically ordered by line ascending.
            // Return the innermost (most specific) match.
            let mut best: Option<&Symbol> = None;
            for sid in sym_ids {
                if let Some(sym) = graph.symbols.get(sid) {
                    if call_line >= sym.line && call_line <= sym.end_line {
                        best = match best {
                            None => Some(sym),
                            Some(prev) => {
                                // Prefer the narrowest range (likely the child).
                                let cur_span = sym.end_line - sym.line;
                                let prev_span = prev.end_line - prev.line;
                                Some(if cur_span < prev_span { sym } else { prev })
                            }
                        };
                    }
                }
            }
            best.map(|s| s.name.clone())
        } else {
            None
        }
    }

    /// Resolve a call expression to a symbol name.
    ///
    /// Checks whether `call_name` appears in the `name_index`.  For method
    /// calls like `obj.method` it also tries the part after the last `.`.
    ///
    /// The `call_kind` and `local_names` parameters are accepted for future
    /// disambiguation (e.g. filtering by symbol kind) but are not yet used.
    #[allow(unused_variables)]
    pub fn resolve_call_target(
        &self,
        file: &str,
        call_name: &str,
        call_line: usize,
        call_kind: &str,
        local_names: &[String],
    ) -> Option<String> {
        // Direct lookup.
        if self.name_index.contains_key(call_name) {
            return Some(call_name.to_string());
        }

        // Method call: try the part after the dot.
        if let Some(dot) = call_name.rfind('.') {
            let base = &call_name[dot + 1..];
            if self.name_index.contains_key(base) {
                return Some(base.to_string());
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Normalise a path string: resolve `..` and `.` segments without accessing
/// the filesystem.
fn normalise_path(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "." | "" => continue,
            ".." => {
                if stack.last().map_or(true, |&s| s == "..") {
                    stack.push("..");
                } else {
                    stack.pop();
                }
            }
            _ => stack.push(segment),
        }
    }
    stack.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_dot() {
        assert_eq!(normalise_path("a/b/./c"), "a/b/c");
    }

    #[test]
    fn normalise_double_dot() {
        assert_eq!(normalise_path("a/b/../c"), "a/c");
    }

    #[test]
    fn normalise_multiple() {
        assert_eq!(normalise_path("a/b/c/../../d"), "a/d");
    }

    #[test]
    fn strip_simple_line_comment() {
        let input = r#"{"a": 1 // comment
}"#;
        let out = ImportResolver::strip_jsonc(input);
        assert_eq!(out, "{\"a\": 1\n}");
    }

    #[test]
    fn strip_block_comment() {
        let input = r#"{"a": /* block */ 1}"#;
        let out = ImportResolver::strip_jsonc(input);
        assert_eq!(out, "{\"a\":  1}");
    }

    #[test]
    fn strip_trailing_comma() {
        let input = r#"{"a": 1,}"#;
        let out = ImportResolver::strip_jsonc(input);
        assert_eq!(out, "{\"a\": 1}");
    }

    #[test]
    fn strip_string_keeps_comment_like_content() {
        let input = r#"{"a": "// not a comment"}"#;
        let out = ImportResolver::strip_jsonc(input);
        assert_eq!(out, input);
    }
}
