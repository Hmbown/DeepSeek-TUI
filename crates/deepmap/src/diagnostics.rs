// Language-aware diagnostics runner.
// Auto-detects project type and runs appropriate tools:
// Rust → cargo check, Python → ruff/mypy, TypeScript → tsc/eslint, Go → go vet/build.
// Parses structured output and optionally correlates issues with symbols.

use std::path::Path;
use std::process::Command;

/// A single diagnostic issue.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticIssue {
    pub tool: String,
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub severity: String, // "error" | "warning" | "info"
    pub code: String,
    pub message: String,
    pub symbol: Option<String>,
    pub symbol_confidence: String, // "exact" | "line" | "none"
}

/// Result from running a single diagnostic tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticResult {
    pub tool: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub skipped: bool,
    pub errors: usize,
    pub warnings: usize,
    pub truncated: bool,
    pub raw_excerpt: Option<String>,
}

/// The main diagnostics runner.
pub struct DiagnosticRunner {
    pub project_root: String,
    pub max_items: usize,
}

impl DiagnosticRunner {
    pub fn new(project_root: &Path, max_items: usize) -> Self {
        Self {
            project_root: project_root.to_string_lossy().to_string(),
            max_items,
        }
    }

    /// Detect project languages by checking for config files.
    pub fn detect_languages(&self) -> Vec<String> {
        let root = Path::new(&self.project_root);
        let mut langs = Vec::new();

        if root.join("Cargo.toml").exists() {
            langs.push("rust".to_string());
        }
        if root.join("pyproject.toml").exists()
            || root.join("setup.py").exists()
            || root.join("requirements.txt").exists()
        {
            langs.push("python".to_string());
        }
        if root.join("tsconfig.json").exists() || root.join("package.json").exists() {
            langs.push("typescript".to_string());
        }
        if root.join("go.mod").exists() {
            langs.push("go".to_string());
        }

        langs
    }

    /// Run all detected diagnostics.
    pub fn run_all(&self, types: &[String]) -> Vec<DiagnosticResult> {
        let mut results = Vec::new();

        for lang in types {
            match lang.as_str() {
                "rust" => results.push(self.run_cargo_check()),
                "python" => {
                    results.push(self.run_ruff());
                    results.push(self.run_mypy());
                }
                "typescript" => {
                    results.push(self.run_tsc());
                }
                "go" => {
                    results.push(self.run_go_vet());
                    results.push(self.run_go_build());
                }
                _ => {}
            }
        }

        results
    }

    // ── Rust: cargo check ──

    fn run_cargo_check(&self) -> DiagnosticResult {
        let start = std::time::Instant::now();
        let mut result = DiagnosticResult {
            tool: "cargo check".into(),
            command: "cargo check --message-format json".into(),
            exit_code: None,
            duration_ms: 0,
            skipped: false,
            errors: 0,
            warnings: 0,
            truncated: false,
            raw_excerpt: None,
        };

        let output = match Command::new("cargo")
            .args(["check", "--message-format", "json"])
            .current_dir(&self.project_root)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                result.skipped = true;
                result.raw_excerpt = Some(format!("cargo not found: {}", e));
                result.duration_ms = start.elapsed().as_millis() as u64;
                return result;
            }
        };

        result.exit_code = Some(output.status.code().unwrap_or(-1));
        result.duration_ms = start.elapsed().as_millis() as u64;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(reason) = msg["reason"].as_str() {
                        if reason == "compiler-message" {
                            let level = msg["message"]["level"].as_str().unwrap_or("unknown");
                            match level {
                                "error" => result.errors += 1,
                                "warning" => result.warnings += 1,
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Capture a short excerpt of the raw output for the last error.
            if let Some(last) = stdout.lines().filter(|l| l.contains("error")).last() {
                let excerpt: String = last.chars().take(200).collect();
                result.raw_excerpt = Some(excerpt);
            }
        }

        result
    }

    // ── Python: ruff ──

    fn run_ruff(&self) -> DiagnosticResult {
        let start = std::time::Instant::now();
        let mut result = DiagnosticResult {
            tool: "ruff".into(),
            command: "ruff check --output-format json".into(),
            exit_code: None,
            duration_ms: 0,
            skipped: false,
            errors: 0,
            warnings: 0,
            truncated: false,
            raw_excerpt: None,
        };

        let output = match Command::new("ruff")
            .args(["check", "--output-format", "json"])
            .current_dir(&self.project_root)
            .output()
        {
            Ok(o) => o,
            Err(_) => {
                result.skipped = true;
                result.duration_ms = start.elapsed().as_millis() as u64;
                return result;
            }
        };

        result.exit_code = Some(output.status.code().unwrap_or(-1));
        result.duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Ok(issues) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
            for issue in &issues {
                if let Some(fix) = issue.get("fix") {
                    if fix.is_object() {
                        result.warnings += 1; // ruff auto-fixable = warning
                    }
                } else {
                    result.errors += 1;
                }
            }
            if !issues.is_empty() && result.errors + result.warnings > self.max_items {
                result.truncated = true;
            }
        }

        result
    }

    // ── Python: mypy ──

    fn run_mypy(&self) -> DiagnosticResult {
        let start = std::time::Instant::now();
        let mut result = DiagnosticResult {
            tool: "mypy".into(),
            command: "mypy .".into(),
            exit_code: None,
            duration_ms: 0,
            skipped: false,
            errors: 0,
            warnings: 0,
            truncated: false,
            raw_excerpt: None,
        };

        let output = match Command::new("mypy")
            .arg(".")
            .current_dir(&self.project_root)
            .output()
        {
            Ok(o) => o,
            Err(_) => {
                result.skipped = true;
                result.duration_ms = start.elapsed().as_millis() as u64;
                return result;
            }
        };

        result.exit_code = Some(output.status.code().unwrap_or(-1));
        result.duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains(": error:") {
                result.errors += 1;
            } else if line.contains(": warning:") {
                result.warnings += 1;
            }
        }

        result
    }

    // ── TypeScript: tsc ──

    fn run_tsc(&self) -> DiagnosticResult {
        let start = std::time::Instant::now();
        let mut result = DiagnosticResult {
            tool: "tsc".into(),
            command: "npx tsc --noEmit".into(),
            exit_code: None,
            duration_ms: 0,
            skipped: false,
            errors: 0,
            warnings: 0,
            truncated: false,
            raw_excerpt: None,
        };

        let output = match Command::new("npx")
            .args(["tsc", "--noEmit"])
            .current_dir(&self.project_root)
            .output()
        {
            Ok(o) => o,
            Err(_) => {
                result.skipped = true;
                result.duration_ms = start.elapsed().as_millis() as u64;
                return result;
            }
        };

        result.exit_code = Some(output.status.code().unwrap_or(-1));
        result.duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains(": error TS") {
                result.errors += 1;
            }
        }

        result
    }

    // ── Go: go vet ──

    fn run_go_vet(&self) -> DiagnosticResult {
        let start = std::time::Instant::now();
        let mut result = DiagnosticResult {
            tool: "go vet".into(),
            command: "go vet ./...".into(),
            exit_code: None,
            duration_ms: 0,
            skipped: false,
            errors: 0,
            warnings: 0,
            truncated: false,
            raw_excerpt: None,
        };

        let output = match Command::new("go")
            .args(["vet", "./..."])
            .current_dir(&self.project_root)
            .output()
        {
            Ok(o) => o,
            Err(_) => {
                result.skipped = true;
                result.duration_ms = start.elapsed().as_millis() as u64;
                return result;
            }
        };

        result.exit_code = Some(output.status.code().unwrap_or(-1));
        result.duration_ms = start.elapsed().as_millis() as u64;

        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines() {
            if !line.trim().is_empty() {
                result.errors += 1;
            }
        }

        result
    }

    // ── Go: go build ──

    fn run_go_build(&self) -> DiagnosticResult {
        let start = std::time::Instant::now();
        let mut result = DiagnosticResult {
            tool: "go build".into(),
            command: "go build ./...".into(),
            exit_code: None,
            duration_ms: 0,
            skipped: false,
            errors: 0,
            warnings: 0,
            truncated: false,
            raw_excerpt: None,
        };

        let output = match Command::new("go")
            .args(["build", "./..."])
            .current_dir(&self.project_root)
            .output()
        {
            Ok(o) => o,
            Err(_) => {
                result.skipped = true;
                result.duration_ms = start.elapsed().as_millis() as u64;
                return result;
            }
        };

        result.exit_code = Some(output.status.code().unwrap_or(-1));
        result.duration_ms = start.elapsed().as_millis() as u64;

        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines() {
            if !line.trim().is_empty() {
                result.errors += 1;
            }
        }

        result
    }
}
