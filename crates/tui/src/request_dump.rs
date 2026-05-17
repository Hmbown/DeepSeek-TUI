//! Dev-only request dumper for diagnosing DeepSeek prefix-cache misses.
//!
//! Activated by `DEEPSEEK_DUMP_REQUESTS=1`. When enabled, every `messages`
//! request body sent to `/chat/completions` is written to
//! `~/.deepseek/req-dumps/<UTC-stamp>-pid<N>/<seq>-<kind>.json` (override the
//! base dir with `DEEPSEEK_DUMP_DIR`). When the env var is unset, both
//! [`session_dir`] and [`dump_chat_request`] short-circuit on the first check,
//! so production paths pay zero extra work.
//!
//! Pair with `deepseek cache-diff` to walk consecutive dumps and surface the
//! first byte that drifts between turns — that's almost always the prefix
//! killer that's tanking the hit rate.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use serde_json::Value;

static SESSION_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// Returns the per-process dump directory, creating it on first call.
///
/// `None` when `DEEPSEEK_DUMP_REQUESTS` isn't set to `1`, when the home
/// directory can't be resolved, or when the directory can't be created.
/// Errors are swallowed — this is a debug aid, not a request-blocking path.
fn session_dir() -> Option<&'static PathBuf> {
    SESSION_DIR
        .get_or_init(|| {
            if std::env::var("DEEPSEEK_DUMP_REQUESTS").ok().as_deref() != Some("1") {
                return None;
            }
            let base = std::env::var("DEEPSEEK_DUMP_DIR")
                .ok()
                .map(PathBuf::from)
                .or_else(|| dirs::home_dir().map(|h| h.join(".deepseek").join("req-dumps")))?;
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%SZ");
            let pid = std::process::id();
            let dir = base.join(format!("{stamp}-pid{pid}"));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!(target: "deepseek::request_dump", "could not create dump dir {}: {e}", dir.display());
                return None;
            }
            tracing::info!(target: "deepseek::request_dump", "dumping chat requests to {}", dir.display());
            Some(dir)
        })
        .as_ref()
}

/// Write the request body to disk if `DEEPSEEK_DUMP_REQUESTS=1`. No-op
/// otherwise. `kind` is a short tag (e.g. `"stream"`, `"oneshot"`) used in
/// the dump filename to distinguish the two `/chat/completions` code paths.
pub(crate) fn dump_chat_request(body: &Value, kind: &str) {
    let Some(dir) = session_dir() else { return };
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = dir.join(format!("{seq:04}-{kind}.json"));
    match serde_json::to_vec_pretty(body) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, &bytes) {
                tracing::warn!(target: "deepseek::request_dump", "write failed {}: {e}", path.display());
            }
        }
        Err(e) => {
            tracing::warn!(target: "deepseek::request_dump", "serialize failed for {}: {e}", path.display());
        }
    }
}

// ── Diff command (`deepseek cache-diff`) ─────────────────────────────────

/// Walk consecutive `*.json` dumps in `dir` and report the first byte that
/// drifts between them — the spot where DeepSeek's prefix cache stops
/// hitting. If `dir` is `None`, picks the most recent subdirectory under
/// `~/.deepseek/req-dumps/`.
pub(crate) fn run_cache_diff(dir: Option<&Path>) -> Result<()> {
    let dir = match dir {
        Some(p) => p.to_path_buf(),
        None => latest_dump_dir()?,
    };
    println!("dump dir: {}\n", dir.display());

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("could not read {}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    paths.sort();

    if paths.len() < 2 {
        println!(
            "only {} dump(s) found — need ≥ 2 to diff. Run a real \
             session with `DEEPSEEK_DUMP_REQUESTS=1` first.",
            paths.len()
        );
        return Ok(());
    }

    for pair in paths.windows(2) {
        let a = read_body(&pair[0])?;
        let b = read_body(&pair[1])?;
        let a_name = pair[0].file_name().unwrap_or_default().to_string_lossy();
        let b_name = pair[1].file_name().unwrap_or_default().to_string_lossy();
        println!("─── {a_name} → {b_name} ───");
        report_pair(&a, &b);
        println!();
    }
    Ok(())
}

fn latest_dump_dir() -> Result<PathBuf> {
    let base = dirs::home_dir()
        .context("could not resolve $HOME")?
        .join(".deepseek")
        .join("req-dumps");
    let mut subs: Vec<PathBuf> = std::fs::read_dir(&base)
        .with_context(|| format!("no dump dir at {}", base.display()))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subs.sort();
    subs.pop().with_context(|| {
        format!(
            "no session subdirectories under {} — run with \
             DEEPSEEK_DUMP_REQUESTS=1 first",
            base.display()
        )
    })
}

fn read_body(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

/// Compare two request bodies and print a one-screen verdict on whether the
/// second is a strict cache-friendly extension of the first.
fn report_pair(a: &Value, b: &Value) {
    // 1. model — different model means a different cache namespace.
    let am = a.get("model").and_then(Value::as_str).unwrap_or("?");
    let bm = b.get("model").and_then(Value::as_str).unwrap_or("?");
    if am != bm {
        println!("  model changed: {am} → {bm}  (full cache miss expected)");
        return;
    }

    // 2. tools — any byte change means cache misses everywhere after the
    //    tools block (which sits before messages in the prompt template).
    let at = canonical(a.get("tools"));
    let bt = canonical(b.get("tools"));
    if at != bt {
        let pos = first_byte_diff(&at, &bt);
        println!(
            "  ❌ tools[] differs ({} → {} bytes) — invalidates entire prefix",
            at.len(),
            bt.len()
        );
        print_byte_context("    ", &at, &bt, pos);
        return;
    }
    if !at.is_empty() {
        println!("  ✓ tools[] byte-identical ({} bytes)", at.len());
    }

    // 3. messages — find longest common prefix.
    let empty: Vec<Value> = Vec::new();
    let am_arr = a.get("messages").and_then(Value::as_array).unwrap_or(&empty);
    let bm_arr = b.get("messages").and_then(Value::as_array).unwrap_or(&empty);

    let mut common = 0;
    let n = am_arr.len().min(bm_arr.len());
    while common < n {
        let ai = canonical(Some(&am_arr[common]));
        let bi = canonical(Some(&bm_arr[common]));
        if ai != bi {
            break;
        }
        common += 1;
    }

    if common == am_arr.len() && bm_arr.len() >= am_arr.len() {
        let added = bm_arr.len() - am_arr.len();
        println!(
            "  ✅ strict prefix extension: {} prior message(s) byte-identical, +{} new",
            common, added
        );
        return;
    }

    println!(
        "  ❌ messages[{}] diverges (cache miss from this point on)",
        common
    );
    if common < am_arr.len() && common < bm_arr.len() {
        let role = bm_arr[common]
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let ai = canonical(Some(&am_arr[common]));
        let bi = canonical(Some(&bm_arr[common]));
        let pos = first_byte_diff(&ai, &bi);
        println!("     role: {role}, first diff at byte {pos} of message:");
        print_byte_context("       ", &ai, &bi, pos);
    } else {
        println!("     (one side is shorter — turn count regressed?)");
    }
}

/// Serialize a JSON value with `serde_json`'s default ordering. The crate
/// is built with `preserve_order` so insertion order is stable for any
/// given construction code path — that's what lets us byte-compare here.
fn canonical(v: Option<&Value>) -> String {
    match v {
        Some(v) => serde_json::to_string(v).unwrap_or_default(),
        None => String::new(),
    }
}

fn first_byte_diff(a: &str, b: &str) -> usize {
    a.bytes()
        .zip(b.bytes())
        .position(|(x, y)| x != y)
        .unwrap_or_else(|| a.len().min(b.len()))
}

fn print_byte_context(indent: &str, a: &str, b: &str, pos: usize) {
    let lo = pos.saturating_sub(60);
    let hi_a = (pos + 60).min(a.len());
    let hi_b = (pos + 60).min(b.len());
    let ctx_a = a.get(lo..hi_a).unwrap_or("");
    let ctx_b = b.get(lo..hi_b).unwrap_or("");
    println!("{indent}A: …{}…", ctx_a.replace('\n', "\\n"));
    println!("{indent}B: …{}…", ctx_b.replace('\n', "\\n"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dump_is_noop_when_env_unset() {
        // SAFETY: unit-test scope, no concurrent env access here.
        unsafe {
            std::env::remove_var("DEEPSEEK_DUMP_REQUESTS");
        }
        dump_chat_request(&json!({"messages": []}), "stream");
    }

    #[test]
    fn first_byte_diff_finds_first_mismatch() {
        assert_eq!(first_byte_diff("abc", "abd"), 2);
        assert_eq!(first_byte_diff("abc", "abc"), 3);
        assert_eq!(first_byte_diff("ab", "abcd"), 2);
    }

    #[test]
    fn canonical_is_byte_stable_for_same_input() {
        let v = json!({"role": "user", "content": "hi"});
        assert_eq!(canonical(Some(&v)), canonical(Some(&v)));
    }
}

