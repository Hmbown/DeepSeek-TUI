use std::fs::OpenOptions;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use serde_json;
use thiserror::Error;

const POLICY_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const POLICY_LOCK_RETRY: Duration = Duration::from_millis(25);

#[derive(Debug, Error)]
pub enum AmendError {
    #[error("prefix rule requires at least one token")]
    EmptyPrefix,
    #[error("policy path has no parent: {path}")]
    MissingParent { path: PathBuf },
    #[error("failed to create policy directory {dir}: {source}")]
    CreatePolicyDir {
        dir: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to format prefix tokens: {source}")]
    SerializePrefix { source: serde_json::Error },
    #[error("failed to open policy file {path}: {source}")]
    OpenPolicyFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write to policy file {path}: {source}")]
    WritePolicyFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to lock policy file {path}: {source}")]
    LockPolicyFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to seek policy file {path}: {source}")]
    SeekPolicyFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to read policy file {path}: {source}")]
    ReadPolicyFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to read metadata for policy file {path}: {source}")]
    PolicyMetadata {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Note this thread uses advisory file locking and performs blocking I/O, so it should be used with
/// [`tokio::task::spawn_blocking`] when called from an async context.
pub fn blocking_append_allow_prefix_rule(
    policy_path: &Path,
    prefix: &[String],
) -> Result<(), AmendError> {
    if prefix.is_empty() {
        return Err(AmendError::EmptyPrefix);
    }

    let tokens = prefix
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| AmendError::SerializePrefix { source })?;
    let pattern = format!("[{}]", tokens.join(", "));
    let rule = format!(r#"prefix_rule(pattern={pattern}, decision="allow")"#);

    let dir = policy_path
        .parent()
        .ok_or_else(|| AmendError::MissingParent {
            path: policy_path.to_path_buf(),
        })?;
    match std::fs::create_dir(dir) {
        Ok(()) => {}
        Err(ref source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(AmendError::CreatePolicyDir {
                dir: dir.to_path_buf(),
                source,
            });
        }
    }
    append_locked_line(policy_path, &rule)
}

fn append_locked_line(policy_path: &Path, line: &str) -> Result<(), AmendError> {
    let _lock = PolicyFileLock::acquire(policy_path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(policy_path)
        .map_err(|source| AmendError::OpenPolicyFile {
            path: policy_path.to_path_buf(),
            source,
        })?;

    let len = file
        .metadata()
        .map_err(|source| AmendError::PolicyMetadata {
            path: policy_path.to_path_buf(),
            source,
        })?
        .len();

    // Ensure file ends in a newline before appending.
    if len > 0 {
        file.seek(SeekFrom::End(-1))
            .map_err(|source| AmendError::SeekPolicyFile {
                path: policy_path.to_path_buf(),
                source,
            })?;
        let mut last = [0; 1];
        file.read_exact(&mut last)
            .map_err(|source| AmendError::ReadPolicyFile {
                path: policy_path.to_path_buf(),
                source,
            })?;

        if last[0] != b'\n' {
            file.write_all(b"\n")
                .map_err(|source| AmendError::WritePolicyFile {
                    path: policy_path.to_path_buf(),
                    source,
                })?;
        }
    }

    file.write_all(format!("{line}\n").as_bytes())
        .map_err(|source| AmendError::WritePolicyFile {
            path: policy_path.to_path_buf(),
            source,
        })?;

    Ok(())
}

struct PolicyFileLock {
    path: PathBuf,
}

impl PolicyFileLock {
    fn acquire(policy_path: &Path) -> Result<Self, AmendError> {
        let lock_path = lock_path_for(policy_path);
        let deadline = Instant::now() + POLICY_LOCK_TIMEOUT;

        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "pid={}", std::process::id());
                    return Ok(Self { path: lock_path });
                }
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Instant::now() >= deadline {
                        return Err(AmendError::LockPolicyFile {
                            path: policy_path.to_path_buf(),
                            source: std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                format!("timed out waiting for {}", lock_path.display()),
                            ),
                        });
                    }
                    thread::sleep(POLICY_LOCK_RETRY);
                }
                Err(source) => {
                    return Err(AmendError::LockPolicyFile {
                        path: policy_path.to_path_buf(),
                        source,
                    });
                }
            }
        }
    }
}

impl Drop for PolicyFileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_path_for(policy_path: &Path) -> PathBuf {
    let Some(file_name) = policy_path.file_name() else {
        return policy_path.with_extension("lock");
    };
    let mut lock_name = file_name.to_os_string();
    lock_name.push(".lock");
    policy_path.with_file_name(lock_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn appends_rule_and_creates_directories() {
        let tmp = tempdir().expect("create temp dir");
        let policy_path = tmp.path().join("rules").join("default.rules");

        blocking_append_allow_prefix_rule(
            &policy_path,
            &[String::from("echo"), String::from("Hello, world!")],
        )
        .expect("append rule");

        let contents = std::fs::read_to_string(&policy_path).expect("default.rules should exist");
        assert_eq!(
            contents,
            r#"prefix_rule(pattern=["echo", "Hello, world!"], decision="allow")
"#
        );
    }

    #[test]
    fn appends_rule_without_duplicate_newline() {
        let tmp = tempdir().expect("create temp dir");
        let policy_path = tmp.path().join("rules").join("default.rules");
        std::fs::create_dir_all(policy_path.parent().unwrap()).expect("create policy dir");
        std::fs::write(
            &policy_path,
            r#"prefix_rule(pattern=["ls"], decision="allow")
"#,
        )
        .expect("write seed rule");

        blocking_append_allow_prefix_rule(
            &policy_path,
            &[String::from("echo"), String::from("Hello, world!")],
        )
        .expect("append rule");

        let contents = std::fs::read_to_string(&policy_path).expect("read policy");
        assert_eq!(
            contents,
            r#"prefix_rule(pattern=["ls"], decision="allow")
prefix_rule(pattern=["echo", "Hello, world!"], decision="allow")
"#
        );
    }

    #[test]
    fn inserts_newline_when_missing_before_append() {
        let tmp = tempdir().expect("create temp dir");
        let policy_path = tmp.path().join("rules").join("default.rules");
        std::fs::create_dir_all(policy_path.parent().unwrap()).expect("create policy dir");
        std::fs::write(
            &policy_path,
            r#"prefix_rule(pattern=["ls"], decision="allow")"#,
        )
        .expect("write seed rule without newline");

        blocking_append_allow_prefix_rule(
            &policy_path,
            &[String::from("echo"), String::from("Hello, world!")],
        )
        .expect("append rule");

        let contents = std::fs::read_to_string(&policy_path).expect("read policy");
        assert_eq!(
            contents,
            r#"prefix_rule(pattern=["ls"], decision="allow")
prefix_rule(pattern=["echo", "Hello, world!"], decision="allow")
"#
        );
    }

    #[test]
    fn removes_policy_lock_file_after_append() {
        let tmp = tempdir().expect("create temp dir");
        let policy_path = tmp.path().join("rules").join("default.rules");

        blocking_append_allow_prefix_rule(&policy_path, &[String::from("echo")])
            .expect("append rule");

        assert!(
            !lock_path_for(&policy_path).exists(),
            "policy lock file should be removed after append"
        );
    }
}
