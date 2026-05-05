//! Lightweight frontend import resolver for monorepo workspaces.
//!
//! This intentionally handles a small, safe subset of TypeScript resolution:
//! local relative imports, `tsconfig.json` / `jsconfig.json` `baseUrl` and
//! `paths`, JSONC config files, and local `extends`. It never executes project
//! JavaScript/TypeScript configuration.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use serde::Serialize;

const PROBE_EXTENSIONS: [&str; 9] = [
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "json",
];
const INDEX_FILES: [&str; 7] = [
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "index.vue",
    "index.svelte",
    "index.json",
];
const MAX_CONFIG_SCAN_DEPTH: usize = 8;

#[derive(Debug, Clone)]
pub struct ModuleResolver {
    workspace: PathBuf,
    cache: Arc<RwLock<ResolverCache>>,
}

#[derive(Debug, Clone)]
pub struct ResolveImportRequest {
    pub specifier: String,
    pub from: Option<PathBuf>,
    pub cwd_hint: Option<PathBuf>,
    pub active_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveImportOutcome {
    #[serde(skip_serializing)]
    pub resolved_path: PathBuf,
    #[serde(skip_serializing)]
    pub project_root: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<PathBuf>,
    pub rule: ResolveRule,
    #[serde(skip_serializing)]
    pub tried: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolveRule {
    Relative,
    FilePath,
    TsconfigPaths { pattern: String, target: String },
    BaseUrl { base_url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveImportError {
    ExternalPackage,
    MissingContext,
    AmbiguousProject { candidates: Vec<PathBuf> },
    NoMatchingAlias,
    NotFound { tried: Vec<PathBuf> },
    PathEscape { path: PathBuf },
    ConfigError { path: PathBuf, message: String },
}

impl std::fmt::Display for ResolveImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExternalPackage => write!(
                f,
                "specifier appears to be an external package, not a workspace file"
            ),
            Self::MissingContext => write!(
                f,
                "missing importer context; pass `from` with the file that contains the import"
            ),
            Self::AmbiguousProject { candidates } => write!(
                f,
                "multiple frontend project configs can resolve this alias: {}",
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::NoMatchingAlias => write!(f, "no matching local alias rule was found"),
            Self::NotFound { tried } => write!(
                f,
                "matched local import rules but no file was found; tried {} path(s)",
                tried.len()
            ),
            Self::PathEscape { path } => {
                write!(f, "resolved path escapes workspace: {}", path.display())
            }
            Self::ConfigError { path, message } => {
                write!(f, "failed to read {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ResolveImportError {}

#[derive(Debug, Default)]
struct ResolverCache {
    raw_configs: HashMap<PathBuf, CachedRawConfig>,
}

#[derive(Debug, Clone)]
struct CachedRawConfig {
    fingerprint: FileFingerprint,
    raw: Result<RawConfig, ResolveImportError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug, Clone, Default)]
struct RawConfig {
    extends: Option<String>,
    base_url: Option<String>,
    paths: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
struct MergedConfig {
    config_path: PathBuf,
    config_dir: PathBuf,
    base_url: Option<PathBuf>,
    paths: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
struct PatternMatch<'a> {
    pattern: &'a str,
    targets: &'a [String],
    capture: String,
    prefix_len: usize,
}

impl ModuleResolver {
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let workspace = workspace
            .canonicalize()
            .unwrap_or_else(|_| normalize_path(&workspace));
        Self {
            workspace,
            cache: Arc::new(RwLock::new(ResolverCache::default())),
        }
    }

    #[must_use]
    pub fn workspace_relative_path(&self, path: &Path) -> PathBuf {
        let normalized = normalize_path(path);
        normalized
            .strip_prefix(&self.workspace)
            .map(Path::to_path_buf)
            .unwrap_or(normalized)
    }

    pub fn resolve_import(
        &self,
        request: ResolveImportRequest,
    ) -> Result<ResolveImportOutcome, ResolveImportError> {
        let specifier = request.specifier.trim();
        if specifier.is_empty() {
            return Err(ResolveImportError::NoMatchingAlias);
        }

        if is_relative_specifier(specifier) {
            return self.resolve_relative(specifier, request.from.as_deref());
        }

        if Path::new(specifier).is_absolute() {
            return self.resolve_absolute_file_path(specifier);
        }

        if let Some(from) = request.from.as_deref() {
            let importer = self.workspace_path(from)?;
            let config_path = self
                .nearest_config_for_path(&importer)
                .ok_or(ResolveImportError::MissingContext)?;
            let config = self.load_merged_config(&config_path)?;
            return self.resolve_with_config(specifier, &config);
        }

        let hint_configs = self.configs_from_hints(&request);
        let hint_outcomes = self.resolve_across_configs(specifier, hint_configs)?;
        if let Some(outcome) = hint_outcomes {
            return Ok(outcome);
        }

        let scanned = self.scan_project_configs();
        match self.resolve_across_configs(specifier, scanned)? {
            Some(outcome) => Ok(outcome),
            None if is_probably_external_package(specifier) => {
                Err(ResolveImportError::ExternalPackage)
            }
            None => Err(ResolveImportError::NoMatchingAlias),
        }
    }

    fn resolve_relative(
        &self,
        specifier: &str,
        from: Option<&Path>,
    ) -> Result<ResolveImportOutcome, ResolveImportError> {
        let from = from.ok_or(ResolveImportError::MissingContext)?;
        let importer = self.workspace_path(from)?;
        let base_dir = if importer.exists() && importer.is_dir() {
            importer.clone()
        } else {
            importer
                .parent()
                .map(Path::to_path_buf)
                .ok_or(ResolveImportError::MissingContext)?
        };
        let candidate = normalize_path(&base_dir.join(specifier));
        let mut tried = Vec::new();
        let resolved = self.probe_candidate(&candidate, &mut tried)?;
        let config_path = self.nearest_config_for_path(&importer);
        let project_root = config_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.workspace.clone());

        Ok(ResolveImportOutcome {
            resolved_path: resolved,
            project_root,
            config_path,
            rule: ResolveRule::Relative,
            tried,
        })
    }

    fn resolve_absolute_file_path(
        &self,
        specifier: &str,
    ) -> Result<ResolveImportOutcome, ResolveImportError> {
        let candidate = normalize_path(Path::new(specifier));
        let mut tried = Vec::new();
        let resolved = self.probe_candidate(&candidate, &mut tried)?;
        Ok(ResolveImportOutcome {
            resolved_path: resolved,
            project_root: self.workspace.clone(),
            config_path: None,
            rule: ResolveRule::FilePath,
            tried,
        })
    }

    fn resolve_with_config(
        &self,
        specifier: &str,
        config: &MergedConfig,
    ) -> Result<ResolveImportOutcome, ResolveImportError> {
        let mut tried = Vec::new();
        let mut matches = Vec::new();
        for (pattern, targets) in &config.paths {
            if let Some((capture, prefix_len)) = match_paths_pattern(pattern, specifier) {
                matches.push(PatternMatch {
                    pattern,
                    targets,
                    capture,
                    prefix_len,
                });
            }
        }
        matches.sort_by(|a, b| {
            b.prefix_len
                .cmp(&a.prefix_len)
                .then_with(|| b.pattern.len().cmp(&a.pattern.len()))
        });

        for matched in matches {
            for target in matched.targets {
                let replaced = replace_capture(target, &matched.capture);
                let base = config.base_url.as_ref().unwrap_or(&config.config_dir);
                let candidate = normalize_path(&base.join(&replaced));
                match self.probe_candidate(&candidate, &mut tried) {
                    Ok(resolved_path) => {
                        return Ok(ResolveImportOutcome {
                            resolved_path,
                            project_root: config.config_dir.clone(),
                            config_path: Some(config.config_path.clone()),
                            rule: ResolveRule::TsconfigPaths {
                                pattern: matched.pattern.to_string(),
                                target: target.clone(),
                            },
                            tried,
                        });
                    }
                    Err(ResolveImportError::NotFound { .. }) => {}
                    Err(err) => return Err(err),
                }
            }
        }

        if !tried.is_empty() {
            return Err(ResolveImportError::NotFound { tried });
        }

        if let Some(base_url) = config.base_url.as_ref()
            && is_base_url_candidate(specifier)
        {
            let candidate = normalize_path(&base_url.join(specifier));
            match self.probe_candidate(&candidate, &mut tried) {
                Ok(resolved_path) => {
                    return Ok(ResolveImportOutcome {
                        resolved_path,
                        project_root: config.config_dir.clone(),
                        config_path: Some(config.config_path.clone()),
                        rule: ResolveRule::BaseUrl {
                            base_url: self
                                .workspace_relative_path(base_url)
                                .to_string_lossy()
                                .replace('\\', "/"),
                        },
                        tried,
                    });
                }
                Err(ResolveImportError::NotFound { .. }) => {}
                Err(err) => return Err(err),
            }
        }

        if !tried.is_empty() {
            Err(ResolveImportError::NotFound { tried })
        } else if is_probably_external_package(specifier) {
            Err(ResolveImportError::ExternalPackage)
        } else {
            Err(ResolveImportError::NoMatchingAlias)
        }
    }

    fn resolve_across_configs(
        &self,
        specifier: &str,
        configs: Vec<PathBuf>,
    ) -> Result<Option<ResolveImportOutcome>, ResolveImportError> {
        let mut outcomes = Vec::new();
        let mut not_found_tried = Vec::new();

        for config_path in configs {
            let config = match self.load_merged_config(&config_path) {
                Ok(config) => config,
                Err(ResolveImportError::ConfigError { .. }) => continue,
                Err(err) => return Err(err),
            };
            match self.resolve_with_config(specifier, &config) {
                Ok(outcome) => outcomes.push(outcome),
                Err(ResolveImportError::NotFound { tried }) => not_found_tried.extend(tried),
                Err(ResolveImportError::ExternalPackage | ResolveImportError::NoMatchingAlias) => {}
                Err(err) => return Err(err),
            }
        }

        outcomes.sort_by(|a, b| a.config_path.cmp(&b.config_path));
        outcomes.dedup_by(|a, b| a.config_path == b.config_path);
        match outcomes.len() {
            0 if !not_found_tried.is_empty() => Err(ResolveImportError::NotFound {
                tried: not_found_tried,
            }),
            0 => Ok(None),
            1 => Ok(outcomes.into_iter().next()),
            _ => Err(ResolveImportError::AmbiguousProject {
                candidates: outcomes
                    .into_iter()
                    .filter_map(|outcome| outcome.config_path)
                    .collect(),
            }),
        }
    }

    fn configs_from_hints(&self, request: &ResolveImportRequest) -> Vec<PathBuf> {
        let mut configs = Vec::new();
        for active in &request.active_paths {
            if let Ok(path) = self.workspace_path(active)
                && let Some(config) = self.nearest_config_for_path(&path)
                && !configs.contains(&config)
            {
                configs.push(config);
            }
        }
        if let Some(cwd) = request.cwd_hint.as_deref() {
            if let Ok(path) = self.workspace_path(cwd)
                && let Some(config) = self.nearest_config_for_path(&path)
                && !configs.contains(&config)
            {
                configs.push(config);
            }
        }
        configs
    }

    fn nearest_config_for_path(&self, path: &Path) -> Option<PathBuf> {
        let normalized = normalize_path(path);
        let mut dir = if normalized.exists() && normalized.is_dir() {
            normalized
        } else {
            normalized.parent()?.to_path_buf()
        };

        loop {
            let tsconfig = dir.join("tsconfig.json");
            if tsconfig.is_file() {
                return Some(normalize_path(&tsconfig));
            }
            let jsconfig = dir.join("jsconfig.json");
            if jsconfig.is_file() {
                return Some(normalize_path(&jsconfig));
            }
            if dir == self.workspace {
                return None;
            }
            dir = dir.parent()?.to_path_buf();
            if !dir.starts_with(&self.workspace) {
                return None;
            }
        }
    }

    fn scan_project_configs(&self) -> Vec<PathBuf> {
        let mut by_dir: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
        self.scan_project_configs_inner(&self.workspace, 0, &mut by_dir);
        by_dir.into_values().collect()
    }

    fn scan_project_configs_inner(
        &self,
        dir: &Path,
        depth: usize,
        by_dir: &mut BTreeMap<PathBuf, PathBuf>,
    ) {
        if depth > MAX_CONFIG_SCAN_DEPTH {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        let mut child_dirs = Vec::new();
        let mut found_tsconfig = None;
        let mut found_jsconfig = None;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if should_skip_scan_dir(&name) {
                    continue;
                }
                child_dirs.push(path);
                continue;
            }
            match name.as_ref() {
                "tsconfig.json" => found_tsconfig = Some(path),
                "jsconfig.json" => found_jsconfig = Some(path),
                _ => {}
            }
        }

        if let Some(config) = found_tsconfig.or(found_jsconfig) {
            by_dir.insert(normalize_path(dir), normalize_path(&config));
        }

        for child in child_dirs {
            self.scan_project_configs_inner(&child, depth + 1, by_dir);
        }
    }

    fn load_merged_config(&self, config_path: &Path) -> Result<MergedConfig, ResolveImportError> {
        let config_path = normalize_path(config_path);
        self.ensure_inside_workspace(&config_path)?;
        self.load_merged_config_inner(&config_path, &mut Vec::new())
    }

    fn load_merged_config_inner(
        &self,
        config_path: &Path,
        stack: &mut Vec<PathBuf>,
    ) -> Result<MergedConfig, ResolveImportError> {
        let config_path = normalize_path(config_path);
        if stack.contains(&config_path) {
            return Err(ResolveImportError::ConfigError {
                path: config_path,
                message: "cyclic extends chain".to_string(),
            });
        }

        let config_dir = config_path.parent().map(Path::to_path_buf).ok_or_else(|| {
            ResolveImportError::ConfigError {
                path: config_path.clone(),
                message: "config has no parent directory".to_string(),
            }
        })?;
        let raw = self.read_raw_config(&config_path)?;

        stack.push(config_path.clone());
        let mut merged = raw
            .extends
            .as_deref()
            .and_then(|extends| resolve_local_extends(&config_dir, extends))
            .filter(|path| path.is_file())
            .map(|parent| self.load_merged_config_inner(&parent, stack))
            .transpose()?
            .unwrap_or_else(|| MergedConfig {
                config_path: config_path.clone(),
                config_dir: config_dir.clone(),
                base_url: None,
                paths: BTreeMap::new(),
            });
        let _ = stack.pop();

        merged.config_path = config_path.clone();
        merged.config_dir = config_dir.clone();
        if let Some(base_url) = raw.base_url {
            merged.base_url = Some(normalize_path(&config_dir.join(base_url)));
        }
        for (pattern, targets) in raw.paths {
            merged.paths.insert(pattern, targets);
        }
        Ok(merged)
    }

    fn read_raw_config(&self, config_path: &Path) -> Result<RawConfig, ResolveImportError> {
        let config_path = normalize_path(config_path);
        let fingerprint =
            file_fingerprint(&config_path).map_err(|message| ResolveImportError::ConfigError {
                path: config_path.clone(),
                message,
            })?;

        if let Ok(cache) = self.cache.read()
            && let Some(cached) = cache.raw_configs.get(&config_path)
            && cached.fingerprint == fingerprint
        {
            return cached.raw.clone();
        }

        let raw = parse_raw_config(&config_path);
        if let Ok(mut cache) = self.cache.write() {
            cache.raw_configs.insert(
                config_path,
                CachedRawConfig {
                    fingerprint,
                    raw: raw.clone(),
                },
            );
        }
        raw
    }

    fn probe_candidate(
        &self,
        candidate: &Path,
        tried: &mut Vec<PathBuf>,
    ) -> Result<PathBuf, ResolveImportError> {
        let candidate = normalize_path(candidate);
        self.ensure_inside_workspace(&candidate)?;

        tried.push(candidate.clone());
        if candidate.is_file() {
            return Ok(candidate.canonicalize().unwrap_or(candidate));
        }

        if candidate.extension().is_none() {
            for ext in PROBE_EXTENSIONS {
                let mut with_ext = candidate.clone();
                with_ext.set_extension(ext);
                self.ensure_inside_workspace(&with_ext)?;
                tried.push(with_ext.clone());
                if with_ext.is_file() {
                    return Ok(with_ext.canonicalize().unwrap_or(with_ext));
                }
            }
        }

        if candidate.is_dir() {
            for index in INDEX_FILES {
                let index_path = candidate.join(index);
                self.ensure_inside_workspace(&index_path)?;
                tried.push(index_path.clone());
                if index_path.is_file() {
                    return Ok(index_path.canonicalize().unwrap_or(index_path));
                }
            }
        }

        Err(ResolveImportError::NotFound {
            tried: tried.clone(),
        })
    }

    fn workspace_path(&self, path: &Path) -> Result<PathBuf, ResolveImportError> {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        let normalized = normalize_path(&candidate);
        self.ensure_inside_workspace(&normalized)?;
        Ok(normalized)
    }

    fn ensure_inside_workspace(&self, path: &Path) -> Result<(), ResolveImportError> {
        let normalized = normalize_path(path);
        if normalized.starts_with(&self.workspace) {
            Ok(())
        } else {
            Err(ResolveImportError::PathEscape { path: normalized })
        }
    }
}

fn parse_raw_config(config_path: &Path) -> Result<RawConfig, ResolveImportError> {
    let content =
        fs::read_to_string(config_path).map_err(|err| ResolveImportError::ConfigError {
            path: config_path.to_path_buf(),
            message: err.to_string(),
        })?;
    let cleaned = remove_trailing_commas(&strip_json_comments(&content));
    let value: serde_json::Value =
        serde_json::from_str(&cleaned).map_err(|err| ResolveImportError::ConfigError {
            path: config_path.to_path_buf(),
            message: err.to_string(),
        })?;

    let extends = value
        .get("extends")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let compiler_options = value
        .get("compilerOptions")
        .and_then(serde_json::Value::as_object);
    let base_url = compiler_options
        .and_then(|options| options.get("baseUrl"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let mut paths = BTreeMap::new();
    if let Some(paths_value) = compiler_options
        .and_then(|options| options.get("paths"))
        .and_then(serde_json::Value::as_object)
    {
        for (pattern, targets) in paths_value {
            let targets = match targets {
                serde_json::Value::Array(items) => items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                serde_json::Value::String(target) => vec![target.clone()],
                _ => Vec::new(),
            };
            if !targets.is_empty() {
                paths.insert(pattern.clone(), targets);
            }
        }
    }

    Ok(RawConfig {
        extends,
        base_url,
        paths,
    })
}

fn file_fingerprint(path: &Path) -> Result<FileFingerprint, String> {
    let metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    Ok(FileFingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn resolve_local_extends(config_dir: &Path, extends: &str) -> Option<PathBuf> {
    let raw = Path::new(extends);
    if !(raw.is_absolute() || extends.starts_with('.')) {
        return None;
    }

    let base = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        config_dir.join(raw)
    };
    let mut candidates = vec![base.clone()];
    if !extends.ends_with(".json") {
        candidates.push(append_json_suffix(&base));
        candidates.push(base.join("tsconfig.json"));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .map(|path| normalize_path(&path))
}

fn append_json_suffix(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".json");
    PathBuf::from(value)
}

fn match_paths_pattern(pattern: &str, specifier: &str) -> Option<(String, usize)> {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return (pattern == specifier).then(|| (String::new(), pattern.len()));
    };
    if !specifier.starts_with(prefix) || !specifier.ends_with(suffix) {
        return None;
    }
    let capture_start = prefix.len();
    let capture_end = specifier.len().checked_sub(suffix.len())?;
    if capture_start > capture_end {
        return None;
    }
    Some((
        specifier[capture_start..capture_end].to_string(),
        prefix.len(),
    ))
}

fn replace_capture(target: &str, capture: &str) -> String {
    if target.contains('*') {
        target.replace('*', capture)
    } else {
        target.to_string()
    }
}

fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

fn is_base_url_candidate(specifier: &str) -> bool {
    !specifier.starts_with('@') && specifier.contains('/')
}

fn is_probably_external_package(specifier: &str) -> bool {
    if specifier.starts_with("@/") || specifier.starts_with("~/") || specifier.starts_with("#/") {
        return false;
    }
    !is_relative_specifier(specifier) && !Path::new(specifier).is_absolute()
}

fn should_skip_scan_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "coverage"
            | ".next"
            | ".turbo"
            | ".cache"
            | "vendor"
    )
}

fn strip_json_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '/' if chars.peek() == Some(&'/') => {
                let _ = chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                let _ = chars.next();
                let mut prev = '\0';
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && next == '/' {
                        break;
                    }
                    prev = next;
                }
            }
            _ => out.push(ch),
        }
    }

    out
}

fn remove_trailing_commas(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && matches!(chars[j], '}' | ']') {
                i += 1;
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    out
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut prefix: Option<std::ffi::OsString> = None;
    let mut is_root = false;
    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix_component) => {
                prefix = Some(prefix_component.as_os_str().to_owned());
            }
            Component::RootDir => {
                is_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() && !is_root {
                    stack.push(Component::ParentDir.as_os_str().to_owned());
                }
            }
            Component::Normal(part) => {
                stack.push(part.to_owned());
            }
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if is_root {
        normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    }
    for part in stack {
        normalized.push(part);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, content).expect("write");
    }

    fn request(specifier: &str, from: Option<&str>) -> ResolveImportRequest {
        ResolveImportRequest {
            specifier: specifier.to_string(),
            from: from.map(PathBuf::from),
            cwd_hint: None,
            active_paths: Vec::new(),
        }
    }

    #[test]
    fn resolves_single_project_alias() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        );
        write(&tmp.path().join("apps/web/src/lib/api.ts"), "export {};");

        let resolver = ModuleResolver::new(tmp.path());
        let outcome = resolver
            .resolve_import(request("@/lib/api", Some("apps/web/src/pages/Home.tsx")))
            .expect("resolve");

        assert_eq!(
            resolver.workspace_relative_path(&outcome.resolved_path),
            PathBuf::from("apps/web/src/lib/api.ts")
        );
        assert!(matches!(
            outcome.rule,
            ResolveRule::TsconfigPaths { ref pattern, .. } if pattern == "@/*"
        ));
    }

    #[test]
    fn resolves_same_alias_from_different_projects() {
        let tmp = tempdir().expect("tempdir");
        for app in ["web", "admin"] {
            write(
                &tmp.path().join(format!("apps/{app}/tsconfig.json")),
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
            );
            write(
                &tmp.path()
                    .join(format!("apps/{app}/src/components/Button.tsx")),
                app,
            );
        }

        let resolver = ModuleResolver::new(tmp.path());
        let web = resolver
            .resolve_import(request(
                "@/components/Button",
                Some("apps/web/src/pages/Home.tsx"),
            ))
            .expect("resolve web");
        let admin = resolver
            .resolve_import(request(
                "@/components/Button",
                Some("apps/admin/src/pages/Home.tsx"),
            ))
            .expect("resolve admin");

        assert_eq!(
            resolver.workspace_relative_path(&web.resolved_path),
            PathBuf::from("apps/web/src/components/Button.tsx")
        );
        assert_eq!(
            resolver.workspace_relative_path(&admin.resolved_path),
            PathBuf::from("apps/admin/src/components/Button.tsx")
        );
    }

    #[test]
    fn returns_ambiguous_without_importer_context() {
        let tmp = tempdir().expect("tempdir");
        for app in ["web", "admin"] {
            write(
                &tmp.path().join(format!("apps/{app}/tsconfig.json")),
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
            );
            write(
                &tmp.path()
                    .join(format!("apps/{app}/src/components/Button.tsx")),
                app,
            );
        }

        let resolver = ModuleResolver::new(tmp.path());
        let err = resolver
            .resolve_import(request("@/components/Button", None))
            .unwrap_err();

        assert!(matches!(
            err,
            ResolveImportError::AmbiguousProject { ref candidates } if candidates.len() == 2
        ));
    }

    #[test]
    fn extends_merges_parent_paths_and_child_overrides() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("tsconfig.base.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@shared/*":["packages/shared/src/*"],"@/*":["root/*"]}}}"#,
        );
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"{"extends":"../../tsconfig.base","compilerOptions":{"baseUrl":"../..","paths":{"@/*":["apps/web/src/*"]}}}"#,
        );
        write(&tmp.path().join("apps/web/src/lib/api.ts"), "web");
        write(&tmp.path().join("packages/shared/src/theme.ts"), "shared");

        let resolver = ModuleResolver::new(tmp.path());
        let child = resolver
            .resolve_import(request("@/lib/api", Some("apps/web/src/pages/Home.tsx")))
            .expect("child alias");
        let parent = resolver
            .resolve_import(request(
                "@shared/theme",
                Some("apps/web/src/pages/Home.tsx"),
            ))
            .expect("parent alias");

        assert_eq!(
            resolver.workspace_relative_path(&child.resolved_path),
            PathBuf::from("apps/web/src/lib/api.ts")
        );
        assert_eq!(
            resolver.workspace_relative_path(&parent.resolved_path),
            PathBuf::from("packages/shared/src/theme.ts")
        );
    }

    #[test]
    fn longest_prefix_wins() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"],"@ui/*":["src/ui/*"]}}}"#,
        );
        write(&tmp.path().join("apps/web/src/ui/Button.tsx"), "ui");

        let resolver = ModuleResolver::new(tmp.path());
        let outcome = resolver
            .resolve_import(request("@ui/Button", Some("apps/web/src/pages/Home.tsx")))
            .expect("resolve");

        assert_eq!(
            resolver.workspace_relative_path(&outcome.resolved_path),
            PathBuf::from("apps/web/src/ui/Button.tsx")
        );
        assert!(matches!(
            outcome.rule,
            ResolveRule::TsconfigPaths { ref pattern, .. } if pattern == "@ui/*"
        ));
    }

    #[test]
    fn probes_extensions_and_index_files() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        );
        write(
            &tmp.path().join("apps/web/src/components/Button.tsx"),
            "tsx",
        );
        write(&tmp.path().join("apps/web/src/routes/index.ts"), "index");

        let resolver = ModuleResolver::new(tmp.path());
        let component = resolver
            .resolve_import(request(
                "@/components/Button",
                Some("apps/web/src/pages/Home.tsx"),
            ))
            .expect("component");
        let index = resolver
            .resolve_import(request("@/routes", Some("apps/web/src/pages/Home.tsx")))
            .expect("index");

        assert_eq!(
            resolver.workspace_relative_path(&component.resolved_path),
            PathBuf::from("apps/web/src/components/Button.tsx")
        );
        assert_eq!(
            resolver.workspace_relative_path(&index.resolved_path),
            PathBuf::from("apps/web/src/routes/index.ts")
        );
    }

    #[test]
    fn scoped_package_is_external_without_matching_alias() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        );

        let resolver = ModuleResolver::new(tmp.path());
        let err = resolver
            .resolve_import(request("@scope/pkg", Some("apps/web/src/pages/Home.tsx")))
            .unwrap_err();

        assert_eq!(err, ResolveImportError::ExternalPackage);
    }

    #[test]
    fn rejects_alias_targets_that_escape_workspace() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["../../../../outside/*"]}}}"#,
        );

        let resolver = ModuleResolver::new(tmp.path());
        let err = resolver
            .resolve_import(request("@/secret", Some("apps/web/src/pages/Home.tsx")))
            .unwrap_err();

        assert!(matches!(err, ResolveImportError::PathEscape { .. }));
    }

    #[test]
    fn parses_jsonc_comments_and_trailing_commas() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"
            {
              // comment
              "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                  "@/*": ["src/*",],
                },
              },
            }
            "#,
        );
        write(&tmp.path().join("apps/web/src/lib/api.ts"), "api");

        let resolver = ModuleResolver::new(tmp.path());
        let outcome = resolver
            .resolve_import(request("@/lib/api", Some("apps/web/src/pages/Home.tsx")))
            .expect("resolve jsonc");

        assert_eq!(
            resolver.workspace_relative_path(&outcome.resolved_path),
            PathBuf::from("apps/web/src/lib/api.ts")
        );
    }
}
