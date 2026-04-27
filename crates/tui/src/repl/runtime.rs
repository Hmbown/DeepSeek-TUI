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
///
/// Conforms to the reference RLM runtime (alexzhang13/rlm) so the same
/// strategies and prompt patterns work here. Helpers exposed:
///
/// - `context` — the user's input (loaded from the persistent state file)
/// - `llm_query(prompt, model=None, max_tokens=None, system=None)`
/// - `llm_query_batched(prompts, model=None)` — concurrent fanout
/// - `rlm_query(prompt, model=None)` — recursive sub-RLM (paper's `sub_RLM`)
/// - `rlm_query_batched(prompts, model=None)` — concurrent recursive sub-RLMs
/// - `SHOW_VARS()` — list user-created REPL variables
/// - `FINAL(value)` / `FINAL_VAR(name)` — terminate the loop
/// - `repl_get(name, default=None)` / `repl_set(name, value)` — explicit store
///
/// Sub-LLM and sub-RLM calls are routed through a localhost HTTP sidecar
/// started by the RLM driver. URLs are injected via env vars
/// (`REPL_LLM_URL`, `REPL_LLM_BATCH_URL`, `REPL_RLM_URL`,
/// `REPL_RLM_BATCH_URL`). When the REPL is used outside an active RLM
/// turn the functions return a clear "unavailable" sentinel.
///
/// Persistent state: every round, all top-level user variables that are
/// JSON-serializable are saved to the state file so the next round can
/// access them as ordinary Python locals (no `repl_get` ceremony needed).
const PYTHON_BOOTSTRAP: &str = r#"
import json as _json
import os as _os
import urllib.request as _urlreq
import urllib.error as _urlerr

# --- Sidecar URLs (set by the RLM driver) ---
_LLM_URL = _os.environ.get('REPL_LLM_URL', '')
_LLM_BATCH_URL = _os.environ.get('REPL_LLM_BATCH_URL', '')
_RLM_URL = _os.environ.get('REPL_RLM_URL', '')
_RLM_BATCH_URL = _os.environ.get('REPL_RLM_BATCH_URL', '')
_STATE_FILE = _os.environ.get('REPL_STATE_FILE', '')

def _post_json(url, body, timeout):
    data = _json.dumps(body).encode('utf-8')
    req = _urlreq.Request(
        url, data=data,
        headers={'Content-Type': 'application/json'},
        method='POST',
    )
    with _urlreq.urlopen(req, timeout=timeout) as resp:
        return _json.loads(resp.read().decode('utf-8'))

def llm_query(prompt, model=None, max_tokens=None, system=None):
    """One-shot sub-LLM call. Returns the completion text as a string.
    Cheap and fast — use for chunk extraction / summarization / Q&A.
    The sub-LLM uses the configured child_model by default."""
    if not _LLM_URL:
        return '[llm_query unavailable: no sidecar URL]'
    body = {'prompt': str(prompt), 'model': model,
            'max_tokens': max_tokens, 'system': system}
    try:
        data = _post_json(_LLM_URL, body, timeout=180)
    except _urlerr.URLError as e:
        return f'[llm_query transport error: {e}]'
    except Exception as e:
        return f'[llm_query error: {e}]'
    if data.get('error'):
        return f'[llm_query: {data["error"]}]'
    return data.get('text', '')

def llm_query_batched(prompts, model=None):
    """Run multiple llm_query calls concurrently. Returns a list of strings
    in the same order as the input prompts. Much faster than sequential
    calls when the sub-prompts are independent."""
    if not isinstance(prompts, (list, tuple)):
        return [f'[llm_query_batched error: prompts must be a list]']
    if not _LLM_BATCH_URL:
        # Fall back to serial llm_query if no batch endpoint is configured.
        return [llm_query(p, model=model) for p in prompts]
    body = {'prompts': [str(p) for p in prompts], 'model': model}
    try:
        data = _post_json(_LLM_BATCH_URL, body, timeout=300)
    except _urlerr.URLError as e:
        return [f'[llm_query_batched transport error: {e}]'] * len(prompts)
    except Exception as e:
        return [f'[llm_query_batched error: {e}]'] * len(prompts)
    results = data.get('results', [])
    if len(results) != len(prompts):
        return [f'[llm_query_batched mismatch: got {len(results)} for {len(prompts)} prompts]'] * len(prompts)
    return [r.get('text', f'[llm_query_batched: {r.get("error","")}]') for r in results]

def rlm_query(prompt, model=None):
    """Spawn a recursive RLM sub-call (paper's `sub_RLM`). The child gets
    its own REPL and can iterate, query further sub-LLMs, etc. Use when a
    sub-task itself requires multi-step reasoning. Bounded by the parent's
    recursion budget; falls back to llm_query when at depth=0."""
    if not _RLM_URL:
        return '[rlm_query unavailable: no sidecar URL]'
    try:
        data = _post_json(_RLM_URL, {'prompt': str(prompt), 'model': model}, timeout=600)
    except _urlerr.URLError as e:
        return f'[rlm_query transport error: {e}]'
    except Exception as e:
        return f'[rlm_query error: {e}]'
    if data.get('error'):
        return f'[rlm_query: {data["error"]}]'
    return data.get('text', '')

def rlm_query_batched(prompts, model=None):
    """Spawn multiple recursive RLM sub-calls in parallel. Each prompt
    gets its own child RLM. Returns a list in input order."""
    if not isinstance(prompts, (list, tuple)):
        return [f'[rlm_query_batched error: prompts must be a list]']
    if not _RLM_BATCH_URL:
        return [rlm_query(p, model=model) for p in prompts]
    body = {'prompts': [str(p) for p in prompts], 'model': model}
    try:
        data = _post_json(_RLM_BATCH_URL, body, timeout=900)
    except _urlerr.URLError as e:
        return [f'[rlm_query_batched transport error: {e}]'] * len(prompts)
    except Exception as e:
        return [f'[rlm_query_batched error: {e}]'] * len(prompts)
    results = data.get('results', [])
    if len(results) != len(prompts):
        return [f'[rlm_query_batched mismatch: got {len(results)} for {len(prompts)} prompts]'] * len(prompts)
    return [r.get('text', f'[rlm_query_batched: {r.get("error","")}]') for r in results]

def FINAL(value):
    """Signal the RLM loop to stop with this final answer."""
    print(f'__REPL_FINAL__::{_json.dumps(str(value))}', flush=True)

def FINAL_VAR(name):
    """Signal the RLM loop to stop, returning a named variable as the answer."""
    name_str = str(name).strip().strip("'\"")
    if name_str in globals():
        val = globals()[name_str]
        print(f'__REPL_FINAL__::{_json.dumps(str(val))}', flush=True)
    else:
        print(f"FINAL_VAR error: variable '{name_str}' not found. "
              f"Use SHOW_VARS() to list available variables.", flush=True)

def SHOW_VARS():
    """Return a dict of {name: type-name} for all user variables in the REPL."""
    out = {}
    for k, v in list(globals().items()):
        if k.startswith('_') or k in _BOOTSTRAP_NAMES:
            continue
        out[k] = type(v).__name__
    return out

def repl_get(name, default=None):
    return globals().get(str(name), default)

def repl_set(name, value):
    globals()[str(name)] = value

# Names defined by the bootstrap that should NOT be persisted as user vars.
_BOOTSTRAP_NAMES = {
    'llm_query', 'llm_query_batched', 'rlm_query', 'rlm_query_batched',
    'SHOW_VARS', 'FINAL', 'FINAL_VAR', 'repl_get', 'repl_set',
}

# Restore user variables from the previous round's state file. Any
# JSON-serializable value persists as a regular Python local.
def _load_state():
    if not _STATE_FILE or not _os.path.exists(_STATE_FILE):
        return
    try:
        with open(_STATE_FILE, 'r') as f:
            data = _json.load(f)
        if isinstance(data, dict):
            for k, v in data.items():
                if not k.startswith('_'):
                    globals()[k] = v
    except Exception:
        pass

# Save user variables (everything that's JSON-serializable and not a
# bootstrap helper) to the state file for the next round.
def _save_state():
    if not _STATE_FILE:
        return
    out = {}
    for k, v in list(globals().items()):
        if k.startswith('_') or k in _BOOTSTRAP_NAMES:
            continue
        try:
            _json.dumps(v)
        except (TypeError, ValueError):
            continue
        out[k] = v
    try:
        with open(_STATE_FILE, 'w') as f:
            _json.dump(out, f)
    except Exception:
        pass

_load_state()
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
