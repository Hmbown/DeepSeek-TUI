//! VS Code extension sidecar connector (#466).
//!
//! When `vscode_diagnostics = true` in the LSP config, the engine skips the
//! built-in LSP shim (stdio-based LSP servers) and instead reads diagnostics
//! from a local Unix socket served by a VS Code extension.
//!
//! # Protocol
//!
//! The sidecar listens on a Unix domain socket (default:
//! `/tmp/deepseek-vscode.sock`). We connect, send a single-line JSON
//! request, then read a single-line JSON response.
//!
//! **Request:**
//! ```json
//! {"file": "/absolute/path/to/target.rs"}
//! ```
//!
//! **Response:**
//! ```json
//! {
//!   "file": "relative/path/to/target.rs",
//!   "diagnostics": [
//!     {"line": 12, "column": 8, "severity": 1, "message": "missing semicolon"}
//!   ]
//! }
//! ```
//!
//! `severity` uses LSP codes: 1 = Error, 2 = Warning, 3 = Information,
//! 4 = Hint. The response is optional — if the sidecar has no diagnostics
//! for that file, it returns an empty `diagnostics` array or closes the
//! connection.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::timeout;

use super::diagnostics::{Diagnostic, DiagnosticBlock, Severity};

/// Default path for the VS Code extension sidecar Unix socket.
pub const DEFAULT_VSCODE_SOCKET: &str = "/tmp/deepseek-vscode.sock";

/// Request payload sent to the sidecar.
#[derive(Debug, Serialize)]
struct VscodeRequest {
    /// Absolute path to the file we want diagnostics for.
    file: String,
}

/// One diagnostic entry from the sidecar response.
#[derive(Debug, Deserialize)]
struct VscodeDiagnosticItem {
    /// 1-based line number.
    line: u32,
    /// 1-based column number.
    column: u32,
    /// LSP severity code: 1 = Error, 2 = Warning, 3 = Information, 4 = Hint.
    severity: Option<i64>,
    /// Human-readable diagnostic message.
    message: String,
}

/// Response payload from the sidecar.
#[derive(Debug, Deserialize)]
struct VscodeResponse {
    /// File path (preferably relative to workspace root).
    #[serde(default)]
    file: Option<PathBuf>,
    /// List of diagnostics for that file.
    #[serde(default)]
    diagnostics: Vec<VscodeDiagnosticItem>,
}

/// Query the VS Code extension sidecar for diagnostics on `file`.
///
/// Connects to the Unix socket at `socket_path`, sends a JSON request, and
/// waits up to `timeout_ms` for a response. Returns `None` when the sidecar
/// is unreachable, times out, or returns an empty diagnostics list.
pub async fn query_sidecar(
    file: &Path,
    workspace: &Path,
    socket_path: &str,
    timeout_ms: u64,
) -> Option<DiagnosticBlock> {
    let absolute = if file.is_absolute() {
        file.to_path_buf()
    } else {
        workspace.join(file)
    };

    let wait = Duration::from_millis(timeout_ms);

    // Connect to the sidecar socket.
    let stream = match timeout(wait, UnixStream::connect(socket_path)).await {
        Ok(Ok(s)) => s,
        Ok(Err(err)) => {
            tracing::debug!(
                socket = %socket_path,
                error = %err,
                "vscode: failed to connect to sidecar socket"
            );
            return None;
        }
        Err(_) => {
            tracing::debug!(
                socket = %socket_path,
                "vscode: connection to sidecar timed out"
            );
            return None;
        }
    };

    // Split into reader/writer halves.
    let (mut reader, mut writer) = stream.into_split();

    // Send the JSON request followed by a newline.
    let request = VscodeRequest {
        file: absolute.to_string_lossy().to_string(),
    };
    let body = match serde_json::to_string(&request) {
        Ok(b) => b,
        Err(err) => {
            tracing::debug!(?err, "vscode: failed to serialize request");
            return None;
        }
    };

    let framed = format!("{body}\n");
    if let Err(err) = writer.write_all(framed.as_bytes()).await {
        tracing::debug!(?err, "vscode: failed to write request");
        return None;
    }
    // Drop the writer so the reader half can see EOF when the sidecar
    // finishes writing.
    drop(writer);

    // Read the response (one JSON line).
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1024];
    let read_result = timeout(wait, async {
        loop {
            let n = reader.read(&mut tmp).await?;
            if n == 0 {
                break; // EOF
            }
            buf.extend_from_slice(&tmp[..n]);
            // Check if we have a complete newline-terminated line.
            if buf.contains(&b'\n') {
                break;
            }
        }
        Ok::<_, std::io::Error>(())
    })
    .await;

    match read_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            tracing::debug!(?err, "vscode: read error from sidecar");
            return None;
        }
        Err(_) => {
            tracing::debug!("vscode: read from sidecar timed out");
            return None;
        }
    }

    // Parse the first line as JSON.
    let line = match buf.iter().position(|&b| b == b'\n') {
        Some(pos) => &buf[..pos],
        None => {
            tracing::debug!("vscode: no newline found in sidecar response");
            return None;
        }
    };

    let response: VscodeResponse = match serde_json::from_slice(line) {
        Ok(r) => r,
        Err(err) => {
            tracing::debug!(?err, raw = %String::from_utf8_lossy(line), "vscode: failed to parse response");
            return None;
        }
    };

    if response.diagnostics.is_empty() {
        return None;
    }

    // Convert sidecar items to the internal Diagnostic type.
    let items: Vec<Diagnostic> = response
        .diagnostics
        .into_iter()
        .map(|item| Diagnostic {
            line: item.line,
            column: item.column,
            severity: Severity::from_lsp(item.severity).unwrap_or(Severity::Error),
            message: item.message,
        })
        .collect();

    // Determine the file path for the block. Prefer the response's `file`
    // field (which should be relative), otherwise fall back to the
    // workspace-relative path.
    let block_file = response
        .file
        .unwrap_or_else(|| relative_to_workspace(workspace, &absolute));

    Some(DiagnosticBlock {
        file: block_file,
        items,
    })
}

/// Compute a workspace-relative path, falling back to filename only.
fn relative_to_workspace(workspace: &Path, path: &Path) -> PathBuf {
    if let Ok(rel) = path.strip_prefix(workspace) {
        return rel.to_path_buf();
    }
    PathBuf::from(
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("unknown")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn relative_path_inside_workspace() {
        let ws = PathBuf::from("/home/user/project");
        let p = PathBuf::from("/home/user/project/src/main.rs");
        assert_eq!(
            relative_to_workspace(&ws, &p),
            PathBuf::from("src/main.rs")
        );
    }

    #[test]
    fn relative_path_outside_workspace() {
        let ws = PathBuf::from("/home/user/project");
        let p = PathBuf::from("/tmp/foo.rs");
        assert_eq!(relative_to_workspace(&ws, &p), PathBuf::from("foo.rs"));
    }

    #[test]
    fn deserialize_sidecar_response() {
        let json = r#"{
            "file": "src/main.rs",
            "diagnostics": [
                {"line": 12, "column": 8, "severity": 1, "message": "missing semicolon"},
                {"line": 13, "column": 1, "severity": 2, "message": "unused variable"}
            ]
        }"#;
        let resp: VscodeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.file, Some(PathBuf::from("src/main.rs")));
        assert_eq!(resp.diagnostics.len(), 2);
        assert_eq!(resp.diagnostics[0].line, 12);
        assert_eq!(resp.diagnostics[0].severity, Some(1));
    }

    #[test]
    fn deserialize_sidecar_empty_diagnostics() {
        let json = r#"{"file": "src/main.rs", "diagnostics": []}"#;
        let resp: VscodeResponse = serde_json::from_str(json).unwrap();
        assert!(resp.diagnostics.is_empty());
    }

    #[test]
    fn deserialize_sidecar_minimal_response() {
        let json = r#"{"diagnostics": [{"line": 1, "column": 1, "message": "err"}]}"#;
        let resp: VscodeResponse = serde_json::from_str(json).unwrap();
        assert!(resp.file.is_none());
        assert_eq!(resp.diagnostics.len(), 1);
        // When severity is absent, default to Error.
        let diag = &resp.diagnostics[0];
        let severity = Severity::from_lsp(diag.severity).unwrap_or(Severity::Error);
        assert_eq!(severity, Severity::Error);
    }
}
