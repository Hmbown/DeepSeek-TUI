//! Self-update for the `deepseek` binary.
//!
//! The `update` subcommand fetches the latest release from
//! `github.com/Hmbown/DeepSeek-TUI/releases/latest`, downloads the
//! platform-correct binary, verifies its SHA256 checksum, and atomically
//! replaces the currently running binary.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Run the self-update workflow.
pub fn run_update() -> Result<()> {
    let current_exe =
        std::env::current_exe().context("failed to determine current executable path")?;

    println!("Checking for updates...");
    println!("Current binary: {}", current_exe.display());

    // Detect platform info
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let binary_name = format!("deepseek-{os}-{arch}");

    // Step 1: Fetch latest release metadata
    let release = fetch_latest_release()?;
    let latest_tag = &release.tag_name;
    println!("Latest release: {latest_tag}");

    // Step 2: Find the matching asset
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(&binary_name))
        .with_context(|| {
            format!(
                "no asset found for platform {binary_name} in release {latest_tag}. \
                 Available assets: {}",
                release
                    .assets
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    println!("Downloading {}...", asset.name);

    // Step 3: Download the asset
    let bytes = download_url(&asset.browser_download_url)
        .with_context(|| format!("failed to download {}", asset.name))?;

    // Step 4: Download the SHA256 checksum file if available
    let sha_url = format!("{}.sha256", asset.browser_download_url);
    let expected_hash = match download_url(&sha_url) {
        Ok(sha_bytes) => {
            let sha_text = String::from_utf8_lossy(&sha_bytes);
            // Parse "hash  filename" format
            sha_text.split_whitespace().next().map(|s| s.to_string())
        }
        Err(_) => {
            println!("  (no SHA256 checksum file found; skipping verification)");
            None
        }
    };

    // Step 5: Verify checksum if available
    if let Some(expected) = &expected_hash {
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            bail!("SHA256 mismatch!\n  expected: {expected}\n  actual:   {actual}");
        }
        println!("SHA256 checksum verified.");
    }

    // Step 6: Replace the current binary atomically
    replace_binary(&current_exe, &bytes)?;

    println!(
        "\n✅ Successfully updated to {latest_tag}!\n\
         New binary: {}\n\
         \n\
         Restart the application to use the new version.",
        current_exe.display()
    );

    Ok(())
}

/// GitHub release metadata.
#[derive(serde::Deserialize, Debug)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

/// A single release asset.
#[derive(serde::Deserialize, Debug)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Fetch the latest release metadata from GitHub.
fn fetch_latest_release() -> Result<Release> {
    let url = "https://api.github.com/repos/Hmbown/DeepSeek-TUI/releases/latest";
    let output = Command::new("curl")
        .args([
            "-sSfL",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: deepseek-tui-updater",
            url,
        ])
        .output()
        .context("failed to run curl to fetch release info")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("curl failed: {stderr}");
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let release: Release = serde_json::from_str(&body).with_context(|| {
        format!("failed to parse release JSON from GitHub API. Response: {body}")
    })?;

    Ok(release)
}

/// Download a URL to bytes using curl.
fn download_url(url: &str) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .args(["-sSfL", url])
        .output()
        .with_context(|| format!("failed to download {url}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("curl download failed: {stderr}");
    }

    Ok(output.stdout)
}

/// Compute the SHA256 hex digest of data.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(data);
    format!("{hash:x}")
}

/// Atomically replace the running binary.
///
/// Writes the new binary to a temp file, then renames it over the original.
/// On Unix, we also preserve the original file's permissions.
fn replace_binary(target: &Path, new_bytes: &[u8]) -> Result<()> {
    // Write to a temp file in the same directory (atomic rename requires
    // same filesystem).
    let tmp = target.with_extension("tmp");
    std::fs::write(&tmp, new_bytes)
        .with_context(|| format!("failed to write temp file at {}", tmp.display()))?;

    // Preserve permissions from the original binary (if it exists)
    if target.exists() {
        if let Ok(meta) = std::fs::metadata(target) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        }
    }

    // Atomic rename
    std::fs::rename(&tmp, target)
        .with_context(|| format!("failed to rename temp file to {}", target.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex_known_value() {
        let data = b"hello";
        let hash = sha256_hex(data);
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_hex_empty() {
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_replace_binary_creates_and_replaces() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("deepseek-test");
        // Write initial content
        std::fs::write(&target, b"old binary").unwrap();

        replace_binary(&target, b"new binary content").unwrap();
        let content = std::fs::read_to_string(&target).unwrap();
        assert_eq!(content, "new binary content");
    }

    #[test]
    fn test_replace_binary_creates_new_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("deepseek-new-test");

        replace_binary(&target, b"fresh binary").unwrap();
        let content = std::fs::read_to_string(&target).unwrap();
        assert_eq!(content, "fresh binary");
    }
}
