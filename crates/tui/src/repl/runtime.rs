//! Python sandbox runtime for the REPL.
//!
//! Each code-execution round spawns a fresh `python3` process with all
//! state loaded from / saved to a JSON file. This is simpler and more
//! robust than trying to manage a long-lived subprocess with async
//! stdout re-attachment.
//!
//! State persistence across rounds:
//!   - `_repl_vars` dict is serialized to a JSON file after each round
//!   - The next round reads it back before executing new code
//!   - This matches the paper's "persistent variable store" design

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::process::Command;

use super::sandbox::parse_final;

/// Python REPL runtime — executes code blocks in isolated processes
/// with persistent variable state via a JSON state file.
#[derive(Debug, Clone)]
pub struct PythonRuntime {
    /// Path to the state file for variable persistence.
    state_path: PathBuf,
    /// Max bytes of stdout to return per round.
    stdout_limit: usize,
    /// Total rounds executed.
    round_count: u64,
    /// When the runtime was created.
    started: Instant,
    /// Extra env vars passed to every spawned `python3` invocation. The RLM
    /// loop uses this to inject `REPL_LLM_URL` / `REPL_RLM_URL` so that
    /// `llm_query()` / `sub_rlm()` inside Python can reach the local sidecar.
    extra_env: Vec<(String, String)>,
}

/// Result of executing one code block.
#[derive(Debug, Clone)]
pub struct ReplRound {
    /// Truncated stdout (for LLM feedback — paper's "metadata only").
    pub stdout: String,
    /// Full stdout (for debugging).
    pub full_stdout: String,
    /// Stderr from this round.
    pub stderr: String,
    /// Whether the code raised an unhandled Python exception.
    pub has_error: bool,
    /// If a FINAL(answer) or FINAL_VAR(var) was detected.
    pub final_value: Option<String>,
    /// Wall-clock duration.
    pub elapsed: Duration,
}

const DEFAULT_STDOUT_LIMIT: usize = 8_192;
const ROUND_TIMEOUT: Duration = Duration::from_secs(120);

/// Python bootstrap — loaded at the top of every execution round.
/// Provides `llm_query()`, `sub_rlm()`, `FINAL()`, `FINAL_VAR()`,
/// `repl_get/set`, and loads/saves the persistent variable state.
///
/// `llm_query()` and `sub_rlm()` work by POSTing to a localhost HTTP
/// sidecar started by the RLM driver in Rust. The URLs are passed via
/// the `REPL_LLM_URL` and `REPL_RLM_URL` environment variables. When
/// the REPL is used outside an RLM turn (or the sidecar isn't running)
/// the functions return a clear "unavailable" sentinel rather than
/// silently lying about a fake response.
const PYTHON_BOOTSTRAP: &str = r#"
import sys, json, os, urllib.request, urllib.error

# --- Persistent variable store ---
_repl_vars = {}
_STATE_FILE = os.environ.get('REPL_STATE_FILE', '')
if _STATE_FILE and os.path.exists(_STATE_FILE):
    try:
        with open(_STATE_FILE, 'r') as f:
            _repl_vars = json.load(f)
    except:
        pass

# --- HTTP sidecar URLs (set by the RLM driver) ---
_LLM_URL = os.environ.get('REPL_LLM_URL', '')
_RLM_URL = os.environ.get('REPL_RLM_URL', '')

def _post_json(url, body, timeout):
    data = json.dumps(body).encode('utf-8')
    req = urllib.request.Request(
        url, data=data,
        headers={'Content-Type': 'application/json'},
        method='POST',
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode('utf-8'))

def llm_query(prompt, model=None, max_tokens=None, system=None):
    """One-shot sub-LLM call. Returns the completion text as a string.

    Routed through the local Rust sidecar; uses the configured child_model
    by default, or whatever `model` you pass.
    """
    if not _LLM_URL:
        return '[llm_query unavailable: no sidecar URL — RLM not active]'
    body = {'prompt': str(prompt), 'model': model,
            'max_tokens': max_tokens, 'system': system}
    try:
        data = _post_json(_LLM_URL, body, timeout=180)
    except urllib.error.URLError as e:
        return f'[llm_query transport error: {e}]'
    except Exception as e:
        return f'[llm_query error: {e}]'
    if data.get('error'):
        return f'[llm_query: {data["error"]}]'
    return data.get('text', '')

def sub_rlm(prompt):
    """Recursive sub-RLM call (paper's `sub_RLM`). Runs a full RLM turn on
    `prompt` at depth-1 and returns its final answer string. Bounded by the
    parent turn's recursion budget.
    """
    if not _RLM_URL:
        return '[sub_rlm unavailable: no sidecar URL — RLM not active]'
    try:
        data = _post_json(_RLM_URL, {'prompt': str(prompt)}, timeout=600)
    except urllib.error.URLError as e:
        return f'[sub_rlm transport error: {e}]'
    except Exception as e:
        return f'[sub_rlm error: {e}]'
    if data.get('error'):
        return f'[sub_rlm: {data["error"]}]'
    return data.get('text', '')

# --- FINAL / FINAL_VAR ---
def FINAL(value):
    """Signal the REPL to stop with this final answer."""
    print(f'__REPL_FINAL__::{json.dumps(str(value))}', flush=True)

def FINAL_VAR(name):
    """Signal the REPL to stop, returning the named variable."""
    val = _repl_vars.get(str(name), f'<variable {name!r} not found>')
    print(f'__REPL_FINAL__::{json.dumps(str(val))}', flush=True)

# --- State helpers ---
def repl_get(name, default=None):
    return _repl_vars.get(str(name), default)

def repl_set(name, value):
    _repl_vars[str(name)] = value

# --- Save state after execution ---
def _save_state():
    if _STATE_FILE:
        try:
            with open(_STATE_FILE, 'w') as f:
                json.dump(_repl_vars, f)
        except:
            pass
"#;

/// Code suffix — appended after user code to save state.
const PYTHON_SUFFIX: &str = r#"
# --- Save state after execution ---
_save_state()
"#;

impl PythonRuntime {
    /// Create a new Python REPL runtime.
    pub async fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join("deepseek_repl");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create REPL temp dir: {e}"))?;

        let state_path = dir.join(format!("state_{}.json", std::process::id()));

        Ok(Self {
            state_path,
            stdout_limit: DEFAULT_STDOUT_LIMIT,
            round_count: 0,
            started: Instant::now(),
            extra_env: Vec::new(),
        })
    }

    /// Create with a specific state path (for testing / RLM integration).
    pub fn with_state_path(path: PathBuf) -> Self {
        Self {
            state_path: path,
            stdout_limit: DEFAULT_STDOUT_LIMIT,
            round_count: 0,
            started: Instant::now(),
            extra_env: Vec::new(),
        }
    }

    /// Set an env var that will be passed to every subsequent `python3`
    /// invocation. Used by the RLM driver to inject sidecar URLs.
    pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        self.extra_env.retain(|(k, _)| k != &key);
        self.extra_env.push((key, value));
    }

    /// Execute a block of Python code.
    ///
    /// Spawns a `python3 -u` process with the bootstrap, the user code,
    /// and the suffix, then collects stdout/stderr.
    pub async fn execute(&mut self, code: &str) -> Result<ReplRound, String> {
        let round_start = Instant::now();
        self.round_count += 1;

        // Build the full script: bootstrap + user code + suffix.
        let full_script = format!(
            "{}\n\n# --- User code (round {}) ---\ntry:\n{}\nexcept Exception as _repl_err:\n    print(f'__REPL_ERROR__::{{_repl_err}}', flush=True)\n\n{}",
            PYTHON_BOOTSTRAP,
            self.round_count,
            indent_code(code, 4),
            PYTHON_SUFFIX,
        );

        let output = tokio::time::timeout(ROUND_TIMEOUT, async {
            let mut cmd = Command::new("python3");
            cmd.arg("-u") // unbuffered
                .arg("-c")
                .arg(&full_script)
                .env(
                    "REPL_STATE_FILE",
                    self.state_path.to_string_lossy().as_ref(),
                );
            for (k, v) in &self.extra_env {
                cmd.env(k, v);
            }
            cmd.output()
                .await
                .map_err(|e| format!("Failed to execute python3: {e}"))
        })
        .await
        .map_err(|_| {
            format!(
                "Python REPL round timed out after {}s",
                ROUND_TIMEOUT.as_secs()
            )
        })??;

        let full_stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let has_error = !output.status.success() || full_stdout.contains("__REPL_ERROR__::");

        // Parse FINAL markers and clean up protocol lines.
        let (display_stdout, final_value) = parse_final(&full_stdout);
        let display_stdout = clean_repl_output(&display_stdout);
        let display_stdout = truncate_stdout(&display_stdout, self.stdout_limit);

        Ok(ReplRound {
            stdout: display_stdout,
            full_stdout,
            stderr,
            has_error,
            final_value,
            elapsed: round_start.elapsed(),
        })
    }

    /// Total rounds executed.
    pub fn round_count(&self) -> u64 {
        self.round_count
    }

    /// Wall-clock uptime.
    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Clean protocol lines (__REPL_LLM_QUERY__, etc.) from stdout.
fn clean_repl_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            !line.starts_with("__REPL_LLM_QUERY__::")
                && !line.starts_with("__REPL_FINAL__::")
                && !line.starts_with("__REPL_ERROR__::")
                && !line.starts_with("__REPL_DONE__")
                && !line.starts_with("__REPL_READY__")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn indent_code(code: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    code.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_stdout(stdout: &str, limit: usize) -> String {
    if stdout.len() <= limit {
        return stdout.to_string();
    }
    let take = limit.saturating_sub(80);
    let mut out: String = stdout.chars().take(take).collect();
    let omitted = stdout.len().saturating_sub(take);
    out.push_str(&format!(
        "\n\n[... REPL output truncated: {omitted} bytes omitted ...]\n"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn repl_executes_simple_code() {
        let mut rt = PythonRuntime::new().await.expect("create runtime");
        let round = rt
            .execute("print('hello from repl')")
            .await
            .expect("execute");
        assert!(round.stdout.contains("hello from repl"));
        assert!(!round.has_error);
        assert!(round.final_value.is_none());
    }

    #[tokio::test]
    async fn repl_handles_final() {
        let mut rt = PythonRuntime::new().await.expect("create runtime");
        let round = rt
            .execute("FINAL('the answer is 42')")
            .await
            .expect("execute");
        assert_eq!(round.final_value.as_deref(), Some("the answer is 42"));
    }

    #[tokio::test]
    async fn repl_persists_variables_across_rounds() {
        let dir = std::env::temp_dir().join("deepseek_repl_test");
        std::fs::create_dir_all(&dir).ok();
        let state_path = dir.join(format!("test_state_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&state_path);

        let mut rt = PythonRuntime::with_state_path(state_path.clone());

        // Round 1: set a variable.
        rt.execute("repl_set('count', 41)").await.expect("round 1");
        // Round 2: read it back and increment.
        let round = rt
            .execute(
                "val = repl_get('count', 0); repl_set('count', val + 1); print(f'count={val+1}')",
            )
            .await
            .expect("round 2");
        assert!(round.stdout.contains("count=42"));

        // Round 3: verify via FINAL_VAR.
        let round = rt.execute("FINAL_VAR('count')").await.expect("round 3");
        assert_eq!(round.final_value.as_deref(), Some("42"));

        let _ = std::fs::remove_file(&state_path);
    }

    #[test]
    fn clean_output_removes_protocol_lines() {
        let raw = "hello\n__REPL_FINAL__::\"done\"\nworld\n__REPL_LLM_QUERY__::{}";
        let cleaned = clean_repl_output(raw);
        assert!(cleaned.contains("hello"));
        assert!(cleaned.contains("world"));
        assert!(!cleaned.contains("__REPL_FINAL__"));
        assert!(!cleaned.contains("__REPL_LLM_QUERY__"));
    }

    #[test]
    fn indent_preserves_empty_lines() {
        let code = "print(1)\n\nprint(2)";
        let result = indent_code(code, 4);
        assert_eq!(result, "    print(1)\n\n    print(2)");
    }
}
