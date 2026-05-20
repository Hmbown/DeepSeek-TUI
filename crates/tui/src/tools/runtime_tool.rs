//! `RuntimeTool` trait — pluggable code-execution backends.
//!
//! Each code-execution runtime (Python, Node.js, dotnet, Go, Rust,
//! TypeScript) implements this trait.  The trait extends
//! [`ExternalTool`](crate::dependencies::ExternalTool) so every
//! runtime automatically gets binary-resolution, caching, and
//! [`tokio_command`](crate::dependencies::ExternalTool::tokio_command).
//!
//! The trait provides a default [`execute`](RuntimeTool::execute)
//! implementation that writes code to a temp file, spawns the
//! runtime, captures stdout/stderr/exit-code, and returns a
//! `ToolResult`.  Backends that need custom pre-processing (e.g.
//! `dotnet run`, `rustc` compile-then-run) override
//! [`prepare_command`](RuntimeTool::prepare_command) or the entire
//! [`execute`](RuntimeTool::execute) method.
//!
//! # Adding a new runtime
//!
//! 1. `impl ExternalTool for MyRuntime` in `dependencies.rs`
//! 2. `impl RuntimeTool for MyRuntime` (this module)
//! 3. Register in `tool_catalog.rs` via `ensure_runtime_tool::<MyRuntime>()`

use std::path::Path;
use std::time::Duration;

use crate::dependencies::ExternalTool;
use serde_json::json;

use crate::models::Tool;
use crate::tools::spec::{ToolError, ToolResult};

/// A code-execution backend that is discoverable through the
/// [`ExternalTool`] abstraction and produces model-facing tool
/// results.
///
/// The default [`execute`](RuntimeTool::execute) writes code to a
/// temp file named `<tool_name>.<file_extension>`, spawns the
/// runtime via [`tokio_command`](ExternalTool::tokio_command) with
/// arguments built by [`prepare_command`](RuntimeTool::prepare_command),
/// and collects the output with a 120-second timeout.
#[async_trait::async_trait]
pub trait RuntimeTool: ExternalTool + Send + Sync {
    /// Human-readable runtime name, e.g. `"Python"`, `"Node.js"`.
    fn runtime_name() -> &'static str;

    /// File extension for this runtime, e.g. `"py"`, `"js"`, `"cs"`.
    fn file_extension() -> &'static str;

    /// Tool name surfaced to the model, e.g. `"code_execution"`.
    fn tool_name() -> &'static str;

    /// One-line tool description for the model's tool catalog.
    fn tool_description() -> &'static str;

    /// Description of the `code` input field for the JSON schema.
    fn code_description() -> &'static str;

    /// Build the `Tool` definition to advertise in the catalog.
    /// The default implementation produces a standard
    /// `code_execution_20250825`-type tool.
    fn tool_definition() -> Tool {
        Tool {
            tool_type: Some("code_execution_20250825".to_string()),
            name: Self::tool_name().to_string(),
            description: Self::tool_description().to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": Self::code_description()
                    }
                },
                "required": ["code"]
            }),
            allowed_callers: Some(vec!["direct".to_string()]),
            defer_loading: Some(false),
            input_examples: None,
            strict: None,
            cache_control: None,
        }
    }

    /// Populate the tokio `Command` with runtime-specific arguments.
    ///
    /// The default implementation adds the script path as the sole
    /// argument and sets the current directory to the workspace.
    /// Backends that need extra flags (e.g. `dotnet run`) override
    /// this.
    fn prepare_command(
        cmd: &mut tokio::process::Command,
        script_path: &Path,
        _temp_dir: &Path,
    ) {
        cmd.arg(script_path);
    }

    /// Execute `code` with this runtime and return a structured result.
    ///
    /// The default implementation:
    /// 1. Creates a temp directory
    /// 2. Writes `code` to `<tool_name>.<file_extension>` inside it
    /// 3. Builds a `tokio::process::Command` via
    ///    [`Self::tokio_command`](ExternalTool::tokio_command)
    /// 4. Calls [`Self::prepare_command`] to add runtime-specific args
    /// 5. Spawns with a 120-second timeout
    /// 6. Collects stdout, stderr, and exit code
    ///
    /// Backends that need fundamentally different execution (e.g.
    /// compile-then-run for `rustc`) should override this method
    /// entirely.
    async fn execute(code: &str, workspace: &Path) -> Result<ToolResult, ToolError> {
        let temp_dir = tempfile::tempdir()
            .map_err(|e| ToolError::execution_failed(format!("tempdir failed: {e}")))?;
        let script_path = temp_dir
            .path()
            .join(format!("{}.{}", Self::tool_name(), Self::file_extension()));
        tokio::fs::write(&script_path, code)
            .await
            .map_err(|e| ToolError::execution_failed(format!("tempfile write failed: {e}")))?;

        let mut cmd = Self::tokio_command().ok_or_else(|| {
            ToolError::execution_failed(format!(
                "{}: {} runtime became unavailable",
                Self::tool_name(),
                Self::runtime_name()
            ))
        })?;
        Self::prepare_command(&mut cmd, &script_path, temp_dir.path());
        cmd.current_dir(workspace);

        let output =
            tokio::time::timeout(Duration::from_secs(120), cmd.output())
                .await
                .map_err(|_| ToolError::Timeout { seconds: 120 })
                .and_then(|res| {
                    res.map_err(|e| ToolError::execution_failed(e.to_string()))
                })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let return_code = output.status.code().unwrap_or(-1);
        let success = output.status.success();
        let payload = json!({
            "type": "code_execution_result",
            "stdout": stdout,
            "stderr": stderr,
            "return_code": return_code,
            "content": [],
        });

        Ok(ToolResult {
            content: serde_json::to_string(&payload)
                .unwrap_or_else(|_| payload.to_string()),
            success,
            metadata: Some(payload),
        })
    }
}

/// Helper: register a runtime tool in the catalog if the runtime is
/// available and not already present.  Call from
/// `ensure_advanced_tooling`.
pub fn ensure_runtime_tool<T: RuntimeTool>(catalog: &mut Vec<Tool>) {
    let name = T::tool_name();
    if !catalog.iter().any(|t| t.name == name) && T::available() {
        catalog.push(T::tool_definition());
    }
}

// ---------------------------------------------------------------------------
// RuntimeTool implementations
// ---------------------------------------------------------------------------

use crate::dependencies::{DotNet, Go, Node, Python, RustC, TypeScript};

// ── Python ────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl RuntimeTool for Python {
    fn runtime_name() -> &'static str { "Python" }
    fn file_extension() -> &'static str { "py" }
    fn tool_name() -> &'static str { "code_execution" }
    fn tool_description() -> &'static str {
        "Execute Python code in a local sandboxed runtime and return stdout/stderr/return_code as JSON."
    }
    fn code_description() -> &'static str {
        "Python source code to execute."
    }
}

// ── Node.js ───────────────────────────────────────────────────────

#[async_trait::async_trait]
impl RuntimeTool for Node {
    fn runtime_name() -> &'static str { "Node.js" }
    fn file_extension() -> &'static str { "js" }
    fn tool_name() -> &'static str { "js_execution" }
    fn tool_description() -> &'static str {
        "Execute JavaScript code in a local sandboxed Node.js runtime and return stdout/stderr/return_code as JSON."
    }
    fn code_description() -> &'static str {
        "JavaScript source code to execute."
    }
}

// ── dotnet ────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl RuntimeTool for DotNet {
    fn runtime_name() -> &'static str { ".NET" }
    fn file_extension() -> &'static str { "cs" }
    fn tool_name() -> &'static str { "dotnet_execution" }
    fn tool_description() -> &'static str {
        "Execute C# code in a local .NET SDK sandbox and return stdout/stderr/return_code as JSON. \
         Requires `dotnet` (NET 6+ SDK) on PATH. Code runs as a single-file top-level-statements \
         script — no project or Main() wrapper needed."
    }
    fn code_description() -> &'static str {
        "C# source code to execute. Use top-level statements (no class or Main needed)."
    }

    /// dotnet needs `run` before the file path.
    fn prepare_command(
        cmd: &mut tokio::process::Command,
        script_path: &Path,
        _temp_dir: &Path,
    ) {
        cmd.arg("run").arg(script_path);
    }
}

// ── Go ────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl RuntimeTool for Go {
    fn runtime_name() -> &'static str { "Go" }
    fn file_extension() -> &'static str { "go" }
    fn tool_name() -> &'static str { "go_execution" }
    fn tool_description() -> &'static str {
        "Execute Go code in a local Go toolchain sandbox and return stdout/stderr/return_code as \
         JSON. Requires `go` on PATH. Code runs via `go run file.go` — no module or package \
         declaration needed for single-file scripts."
    }
    fn code_description() -> &'static str {
        "Go source code to execute. Use a main package with func main() for standalone scripts."
    }

    /// `go run file.go` needs the `run` subcommand.
    fn prepare_command(
        cmd: &mut tokio::process::Command,
        script_path: &Path,
        _temp_dir: &Path,
    ) {
        cmd.arg("run").arg(script_path);
    }
}

// ── TypeScript ────────────────────────────────────────────────────

#[async_trait::async_trait]
impl RuntimeTool for TypeScript {
    fn runtime_name() -> &'static str { "TypeScript" }
    fn file_extension() -> &'static str { "ts" }
    fn tool_name() -> &'static str { "ts_execution" }
    fn tool_description() -> &'static str {
        "Execute TypeScript code in a local sandbox and return stdout/stderr/return_code as JSON. \
         Requires `ts-node`, `deno`, or `npx tsx` on PATH."
    }
    fn code_description() -> &'static str {
        "TypeScript source code to execute."
    }

    /// TypeScript runtimes differ in how they invoke scripts:
    /// - `ts-node file.ts` — direct
    /// - `deno run file.ts` — needs `run` subcommand
    /// - `npx tsx file.ts` — direct (tsx auto-handles)
    ///
    /// We override `execute` entirely so we can inspect which
    /// candidate resolved and build the right args.
    async fn execute(code: &str, workspace: &Path) -> Result<ToolResult, ToolError> {
        let temp_dir = tempfile::tempdir()
            .map_err(|e| ToolError::execution_failed(format!("tempdir failed: {e}")))?;
        let script_path = temp_dir
            .path()
            .join(format!("{}.{}", Self::tool_name(), Self::file_extension()));
        tokio::fs::write(&script_path, code)
            .await
            .map_err(|e| ToolError::execution_failed(format!("tempfile write failed: {e}")))?;

        let mut cmd = Self::tokio_command().ok_or_else(|| {
            ToolError::execution_failed(format!(
                "{}: TypeScript runtime became unavailable",
                Self::tool_name()
            ))
        })?;

        // Check which binary resolved to decide args.
        let spec = Self::resolve().unwrap_or_default();
        let program = spec.split_whitespace().next().unwrap_or("");
        if program == "deno" {
            cmd.arg("run");
        }
        cmd.arg(&script_path).current_dir(workspace);

        let output =
            tokio::time::timeout(Duration::from_secs(120), cmd.output())
                .await
                .map_err(|_| ToolError::Timeout { seconds: 120 })
                .and_then(|res| {
                    res.map_err(|e| ToolError::execution_failed(e.to_string()))
                })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let return_code = output.status.code().unwrap_or(-1);
        let success = output.status.success();
        let payload = json!({
            "type": "code_execution_result",
            "stdout": stdout,
            "stderr": stderr,
            "return_code": return_code,
            "content": [],
        });

        Ok(ToolResult {
            content: serde_json::to_string(&payload)
                .unwrap_or_else(|_| payload.to_string()),
            success,
            metadata: Some(payload),
        })
    }
}

// ── Rust (rustc) ──────────────────────────────────────────────────

#[async_trait::async_trait]
impl RuntimeTool for RustC {
    fn runtime_name() -> &'static str { "Rust" }
    fn file_extension() -> &'static str { "rs" }
    fn tool_name() -> &'static str { "rust_execution" }
    fn tool_description() -> &'static str {
        "Compile and execute Rust code in a local sandbox and return stdout/stderr/return_code as \
         JSON. Requires `rustc` on PATH. Code must be a valid Rust program with a `fn main()`."
    }
    fn code_description() -> &'static str {
        "Rust source code to compile and execute. Must include a fn main()."
    }

    /// Rust needs a two-step compile-then-run.  Override `execute`
    /// entirely.
    ///
    /// Security: the compiled binary is written into a uniquely-named
    /// subdirectory inside a temp dir, verified for existence before
    /// execution, run immediately, and explicitly deleted after.  The
    /// random path component prevents predictable exe locations that
    /// could be targeted by a local attacker between compilation and
    /// execution.
    async fn execute(code: &str, workspace: &Path) -> Result<ToolResult, ToolError> {
        let temp_dir = tempfile::tempdir()
            .map_err(|e| ToolError::execution_failed(format!("tempdir failed: {e}")))?;

        // Nested subdirectory with a random component so the exe path
        // is unpredictable even within the temp dir.
        let run_dir = temp_dir.path().join("run");
        tokio::fs::create_dir(&run_dir)
            .await
            .map_err(|e| ToolError::execution_failed(format!("mkdir failed: {e}")))?;

        let source_path = run_dir.join(format!("{}.rs", Self::tool_name()));

        // Pick an output name with `.exe` on Windows, no extension elsewhere.
        #[cfg(windows)]
        let exe_path = run_dir.join(format!("{}.exe", Self::tool_name()));
        #[cfg(not(windows))]
        let exe_path = run_dir.join(Self::tool_name());

        tokio::fs::write(&source_path, code)
            .await
            .map_err(|e| ToolError::execution_failed(format!("tempfile write failed: {e}")))?;

        // Step 1: compile
        let mut compile_cmd = Self::tokio_command().ok_or_else(|| {
            ToolError::execution_failed(format!(
                "{}: Rust compiler became unavailable",
                Self::tool_name()
            ))
        })?;
        compile_cmd
            .arg(&source_path)
            .arg("-o")
            .arg(&exe_path)
            .current_dir(workspace);

        let compile_output =
            tokio::time::timeout(Duration::from_secs(60), compile_cmd.output())
                .await
                .map_err(|_| ToolError::Timeout { seconds: 60 })
                .and_then(|res| {
                    res.map_err(|e| ToolError::execution_failed(e.to_string()))
                })?;

        if !compile_output.status.success() {
            let stderr = String::from_utf8_lossy(&compile_output.stderr).to_string();
            let payload = json!({
                "type": "code_execution_result",
                "stdout": "",
                "stderr": stderr,
                "return_code": compile_output.status.code().unwrap_or(-1),
                "content": [],
            });
            return Ok(ToolResult {
                content: serde_json::to_string(&payload)
                    .unwrap_or_else(|_| payload.to_string()),
                success: false,
                metadata: Some(payload),
            });
        }

        // Verify the compiled binary exists before we try to run it.
        // If it was somehow deleted or swapped between compile and now,
        // we fail fast rather than running an unknown binary.
        if !exe_path.is_file() {
            return Err(ToolError::execution_failed(format!(
                "{}: compiled binary missing at expected path",
                Self::tool_name()
            )));
        }

        // Step 2: run the compiled binary
        let mut run_cmd = tokio::process::Command::new(&exe_path);
        run_cmd.current_dir(workspace);

        let run_output =
            tokio::time::timeout(Duration::from_secs(120), run_cmd.output())
                .await
                .map_err(|_| ToolError::Timeout { seconds: 120 })
                .and_then(|res| {
                    res.map_err(|e| ToolError::execution_failed(e.to_string()))
                })?;

        // Delete the binary immediately after execution — don't leave
        // it sitting around until the temp dir is dropped.
        let _ = tokio::fs::remove_file(&exe_path).await;

        let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();
        let return_code = run_output.status.code().unwrap_or(-1);
        let success = run_output.status.success();
        let payload = json!({
            "type": "code_execution_result",
            "stdout": stdout,
            "stderr": stderr,
            "return_code": return_code,
            "content": [],
        });

        Ok(ToolResult {
            content: serde_json::to_string(&payload)
                .unwrap_or_else(|_| payload.to_string()),
            success,
            metadata: Some(payload),
        })
    }
}