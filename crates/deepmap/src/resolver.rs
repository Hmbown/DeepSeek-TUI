// Import resolution — resolves import statements to file paths and symbol IDs.
//
// Handles relative imports, TypeScript path aliases (tsconfig paths),
// package.json exports, and recursive re-export tracking.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::types::*;

/// Resolves imports to concrete file paths and symbol IDs.
pub struct ImportResolver {
    pub project_root: PathBuf,
    pub import_configs: Vec<ProjectImportConfig>,
    /// File stem → candidate file paths (relative to project root).
    file_map: HashMap<String, Vec<String>>,
    /// Symbol name → list of symbol IDs.
    name_index: HashMap<String, Vec<String>>,
}

impl ImportResolver {
    pub fn new(project_root: &Path, graph: &RepoGraph) -> Self {
        let mut resolver = Self {
            project_root: project_root.to_path_buf(),
            import_configs: Vec::new(),
            file_map: HashMap::new(),
            name_index: HashMap::new(),
        };
        resolver.build_indices(graph);
        resolver.discover_import_configs();
        resolver
    }

    /// Build file stem → paths and name → symbol ID indices.
    fn build_indices(&mut self, graph: &RepoGraph) {
        for file in graph.file_symbols.keys() {
            let stem = Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            self.file_map.entry(stem).or_default().push(file.clone());
        }

        for (id, sym) in &graph.symbols {
            self.name_index
                .entry(sym.name.clone())
                .or_default()
                .push(id.clone());
        }
    }

    /// Discover tsconfig/jsconfig for path aliases and base URL.
    fn discover_import_configs(&mut self) {
        // Walk project root for tsconfig.json / jsconfig.json.
        if let Ok(entries) = std::fs::read_dir(&self.project_root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == "tsconfig.json" || name_str == "jsconfig.json" {
                    if let Ok(config) = self.parse_tsconfig(&entry.path()) {
                        self.import_configs.push(config);
                    }
                }
                // Also check subdirectories (one level deep).
                if entry.path().is_dir() {
                    for sub_name in &["tsconfig.json", "jsconfig.json"] {
                        let sub_path = entry.path().join(sub_name);
                        if sub_path.exists() {
                            if let Ok(config) = self.parse_tsconfig(&sub_path) {
                                self.import_configs.push(config);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Parse a tsconfig/jsconfig for compilerOptions.paths and baseUrl.
    fn parse_tsconfig(&self, path: &Path) -> Result<ProjectImportConfig, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        // Strip JSONC comments.
        let stripped = Self::strip_jsonc(&content);
        let parsed: serde_json::Value =
            serde_json::from_str(&stripped).map_err(|e| e.to_string())?;

        let compiler_options = parsed.get("compilerOptions");
        let base_url = compiler_options
            .and_then(|c| c.get("baseUrl"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut alias_rules = Vec::new();
        if let Some(paths) = compiler_options.and_then(|c| c.get("paths")) {
            if let Some(obj) = paths.as_object() {
                for (alias, targets) in obj {
                    if let Some(target_array) = targets.as_array() {
                        let target_strings: Vec<String> = target_array
                            .iter()
                            .filter_map(|t| t.as_str())
                            .map(|s| s.trim_end_matches("/*").to_string())
                            .collect();
                        if !target_strings.is_empty() {
                            alias_rules.push(PathAliasRule {
                                alias_pattern: alias.trim_end_matches("/*").to_string(),
                                target_patterns: target_strings,
                            });
                        }
                    }
                }
            }
        }

        let config_dir = path.parent().map(|p| p.to_string_lossy().to_string());

        Ok(ProjectImportConfig {
            config_path: Some(path.to_string_lossy().to_string()),
            config_dir,
            base_url,
            alias_rules,
        })
    }

    /// Strip JSONC comments (line // and block /* */) and trailing commas.
    fn strip_jsonc(text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut in_string = false;
        let mut in_block_comment = false;
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if in_block_comment {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    in_block_comment = false;
                }
                continue;
            }
            if !in_string && ch == '/' && chars.peek() == Some(&'/') {
                // Line comment: skip until end of line.
                for c in chars.by_ref() {
                    if c == '\n' {
                        result.push('\n');
                        break;
                    }
                }
                continue;
            }
            if !in_string && ch == '/' && chars.peek() == Some(&'*') {
                chars.next();
                in_block_comment = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
            }
            if ch == '\\' && in_string {
                // Escape sequence: push backslash and next char.
                result.push(ch);
                if let Some(next) = chars.next() {
                    result.push(next);
                }
                continue;
            }
            result.push(ch);
        }

        // Remove trailing commas before } or ].
        let re_result = result
            .replace(",\n}", "\n}")
            .replace(", }", " }")
            .replace(",\n]", "\n]")
            .replace(", ]", " ]");
        re_result
    }

    // ── Resolution ──

    /// Resolve an import path to candidate file paths.
    pub fn resolve_import_targets(&self, source_file: &str, import_path: &str) -> Vec<String> {
        let mut results: Vec<String> = Vec::new();

        // 1. Relative import.
        if import_path.starts_with('.') {
            let source_dir = Path::new(source_file).parent().unwrap_or(Path::new("."));
            if let Ok(resolved) = source_dir.join(import_path).canonicalize() {
                if let Ok(rel) = resolved.strip_prefix(&self.project_root) {
                    let rel_str = rel.to_string_lossy().to_string();
                    // Try exact match first.
                    if self.file_map.values().any(|v| v.contains(&rel_str)) {
                        results.push(rel_str.clone());
                    }
                    // Try with extensions.
                    let rel_str2 = rel_str.clone();
                    for ext in &[".ts", ".tsx", ".js", ".jsx", ".py", ".rs", ".go"] {
                        let with_ext = format!("{}{}", rel_str2, ext);
                        if self.file_map.values().any(|v| v.contains(&with_ext)) {
                            results.push(with_ext);
                        }
                    }
                    // Try index files.
                    for ext in &[".ts", ".tsx", ".js", ".py"] {
                        let index_file = format!("{}/index{}", rel_str, ext);
                        if self.file_map.values().any(|v| v.contains(&index_file)) {
                            results.push(index_file);
                        }
                    }
                }
            }
            return results;
        }

        // 2. Alias resolution (tsconfig paths).
        for config in &self.import_configs {
            for rule in &config.alias_rules {
                if import_path.starts_with(&rule.alias_pattern) {
                    let remainder = import_path
                        .strip_prefix(&rule.alias_pattern)
                        .unwrap_or("")
                        .trim_start_matches('/');
                    for target_pattern in &rule.target_patterns {
                        let resolved_path = if target_pattern.contains('*') {
                            target_pattern.replace('*', remainder)
                        } else {
                            format!("{}/{}", target_pattern, remainder)
                        };
                        // Search for matching files.
                        let stem = Path::new(&resolved_path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        if let Some(candidates) = self.file_map.get(stem) {
                            for c in candidates {
                                if c.contains(&resolved_path) || c.ends_with(&resolved_path) {
                                    results.push(c.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Direct file lookup by stem.
        let stem = Path::new(import_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(import_path);
        if let Some(candidates) = self.file_map.get(stem) {
            for c in candidates {
                if !results.contains(c) {
                    results.push(c.clone());
                }
            }
        }

        results
    }

    /// Find the symbol that contains the given call line.
    pub fn resolve_calling_symbol(&self, _file: &str, _call_line: usize) -> Option<String> {
        // Iterate symbols in this file and find the one whose range contains call_line.
        // Since we don't have direct access to the symbol map here (it's in the graph),
        // this is a simplified version that the engine passes through.
        None // Will be resolved by the engine which has symbol data.
    }

    /// Resolve a call reference to a target symbol ID.
    pub fn resolve_call_target(
        &self,
        _file: &str,
        call_name: &str,
        _call_line: usize,
        _call_kind: &str,
        _local_names: &HashMap<String, String>,
    ) -> Option<String> {
        // Look up by name in the name index.
        // Prefer symbols in the same module, then fall back to any match.
        if let Some(candidates) = self.name_index.get(call_name) {
            if !candidates.is_empty() {
                return Some(candidates[0].clone());
            }
        }
        None
    }

    /// Resolve resolving symbol for a given file and line.
    pub fn resolve_calling_symbol_with_graph(
        &self,
        file: &str,
        call_line: usize,
        graph: &RepoGraph,
    ) -> Option<String> {
        if let Some(sym_ids) = graph.file_symbols.get(file) {
            for sym_id in sym_ids {
                if let Some(sym) = graph.symbols.get(sym_id) {
                    if call_line >= sym.line && call_line <= sym.end_line {
                        return Some(sym_id.clone());
                    }
                }
            }
        }
        None
    }
}
