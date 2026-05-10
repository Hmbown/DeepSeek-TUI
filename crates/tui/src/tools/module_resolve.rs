//! Read-only tool for resolving frontend import specifiers to workspace files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::module_resolver::{
    ModuleResolver, ResolveImportError, ResolveImportOutcome, ResolveImportRequest, ResolveRule,
};

use super::spec::{
    ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, optional_str, required_str,
};

pub struct ResolveImportTool;

#[async_trait]
impl ToolSpec for ResolveImportTool {
    fn name(&self) -> &'static str {
        "resolve_import"
    }

    fn description(&self) -> &'static str {
        "Resolve a frontend import specifier (for example '@/components/Button') to a concrete workspace file using the importing file's tsconfig/jsconfig aliases."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "specifier": {
                    "type": "string",
                    "description": "Import specifier exactly as written in source, e.g. '@/components/Button'."
                },
                "from": {
                    "type": "string",
                    "description": "Importer file path relative to workspace, e.g. 'apps/web/src/pages/Home.tsx'."
                }
            },
            "required": ["specifier"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let specifier = required_str(&input, "specifier")?.to_string();
        let from = optional_str(&input, "from").map(PathBuf::from);
        let resolver = resolver_for_context(context);
        let request = ResolveImportRequest {
            specifier: specifier.clone(),
            from: from.clone(),
            cwd_hint: context.cwd_hint.clone(),
            active_paths: context.active_paths.clone(),
        };

        match resolver.resolve_import(request) {
            Ok(outcome) => ToolResult::json(&success_json(
                &resolver,
                &specifier,
                from.as_deref(),
                outcome,
            ))
            .map_err(|err| ToolError::execution_failed(format!("serialize result: {err}"))),
            Err(err) => ToolResult::json(&error_json(&resolver, &specifier, from.as_deref(), &err))
                .map_err(|err| ToolError::execution_failed(format!("serialize result: {err}"))),
        }
    }
}

pub(crate) fn resolver_for_context(context: &ToolContext) -> Arc<ModuleResolver> {
    context
        .module_resolver
        .clone()
        .unwrap_or_else(|| Arc::new(ModuleResolver::new(context.workspace.clone())))
}

pub(crate) fn import_error_message(
    resolver: &ModuleResolver,
    specifier: &str,
    from: Option<&Path>,
    err: &ResolveImportError,
) -> String {
    let value = error_json(resolver, specifier, from, err);
    value.to_string()
}

fn success_json(
    resolver: &ModuleResolver,
    specifier: &str,
    from: Option<&Path>,
    outcome: ResolveImportOutcome,
) -> Value {
    json!({
        "specifier": specifier,
        "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
        "resolved_path": rel_string(resolver, &outcome.resolved_path),
        "project_root": rel_string(resolver, &outcome.project_root),
        "config_path": outcome.config_path.as_deref().map(|path| rel_string(resolver, path)),
        "rule": rule_json(outcome.rule),
        "tried": outcome
            .tried
            .iter()
            .map(|path| rel_string(resolver, path))
            .collect::<Vec<_>>(),
    })
}

fn error_json(
    resolver: &ModuleResolver,
    specifier: &str,
    from: Option<&Path>,
    err: &ResolveImportError,
) -> Value {
    match err {
        ResolveImportError::AmbiguousProject { candidates } => json!({
            "error": "ambiguous_project",
            "specifier": specifier,
            "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
            "message": "Multiple frontend project configs can resolve this alias. Pass `from`.",
            "candidates": candidates
                .iter()
                .map(|path| rel_string(resolver, path))
                .collect::<Vec<_>>(),
        }),
        ResolveImportError::MissingContext => json!({
            "error": "missing_context",
            "specifier": specifier,
            "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
            "message": "Pass `from` with the importer file path so the project alias config can be selected.",
        }),
        ResolveImportError::ExternalPackage => json!({
            "error": "external_package",
            "specifier": specifier,
            "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
            "message": "This specifier appears to be an external package, not a workspace file.",
        }),
        ResolveImportError::NoMatchingAlias => json!({
            "error": "no_matching_alias",
            "specifier": specifier,
            "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
            "message": "No local tsconfig/jsconfig alias rule matched this specifier.",
        }),
        ResolveImportError::NotFound { tried } => json!({
            "error": "not_found",
            "specifier": specifier,
            "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
            "message": "Matched a local import rule, but no file was found after extension/index probing.",
            "tried": tried
                .iter()
                .map(|path| rel_string(resolver, path))
                .collect::<Vec<_>>(),
        }),
        ResolveImportError::PathEscape { path } => json!({
            "error": "path_escape",
            "specifier": specifier,
            "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
            "message": "Resolved import path escapes the workspace.",
            "path": rel_string(resolver, path),
        }),
        ResolveImportError::ConfigError { path, message } => json!({
            "error": "config_error",
            "specifier": specifier,
            "from": from.map(|path| path.to_string_lossy().replace('\\', "/")),
            "message": message,
            "config_path": rel_string(resolver, path),
        }),
    }
}

fn rule_json(rule: ResolveRule) -> Value {
    match rule {
        ResolveRule::Relative => json!({"kind": "relative"}),
        ResolveRule::FilePath => json!({"kind": "file_path"}),
        ResolveRule::TsconfigPaths { pattern, target } => {
            json!({"kind": "tsconfig_paths", "pattern": pattern, "target": target})
        }
        ResolveRule::BaseUrl { base_url } => json!({"kind": "base_url", "base_url": base_url}),
    }
}

fn rel_string(resolver: &ModuleResolver, path: &Path) -> String {
    resolver
        .workspace_relative_path(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::spec::ToolContext;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, content).expect("write");
    }

    #[tokio::test]
    async fn resolve_import_tool_returns_resolved_json() {
        let tmp = tempdir().expect("tempdir");
        write(
            &tmp.path().join("apps/web/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        );
        write(
            &tmp.path().join("apps/web/src/components/Button.tsx"),
            "button",
        );
        let ctx = ToolContext::new(tmp.path());

        let result = ResolveImportTool
            .execute(
                json!({
                    "specifier": "@/components/Button",
                    "from": "apps/web/src/pages/Home.tsx"
                }),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.success);
        assert!(result.content.contains("\"resolved_path\""));
        assert!(
            result
                .content
                .contains("apps/web/src/components/Button.tsx")
        );
        assert!(result.content.contains("\"tsconfig_paths\""));
    }

    #[tokio::test]
    async fn resolve_import_tool_returns_ambiguous_json() {
        let tmp = tempdir().expect("tempdir");
        for app in ["web", "admin"] {
            write(
                &tmp.path().join(format!("apps/{app}/tsconfig.json")),
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
            );
            write(
                &tmp.path()
                    .join(format!("apps/{app}/src/components/Button.tsx")),
                "button",
            );
        }
        let ctx = ToolContext::new(tmp.path());

        let result = ResolveImportTool
            .execute(json!({"specifier": "@/components/Button"}), &ctx)
            .await
            .expect("execute");

        assert!(result.success);
        assert!(result.content.contains("\"ambiguous_project\""));
        assert!(result.content.contains("apps/web/tsconfig.json"));
        assert!(result.content.contains("apps/admin/tsconfig.json"));
    }
}
