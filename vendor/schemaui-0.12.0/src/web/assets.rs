use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

use include_dir::{Dir, include_dir};

pub static DIST: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

#[derive(Debug, Clone)]
pub struct AssetResponse {
    pub path: String,
    pub mime: &'static str,
    pub contents: Cow<'static, [u8]>,
}

pub trait WebAssetProvider: Send + Sync + std::fmt::Debug + 'static {
    fn load(&self, path: &str) -> Option<AssetResponse>;
}

#[derive(Debug, Default, Clone)]
pub struct EmbeddedAssets;

impl WebAssetProvider for EmbeddedAssets {
    fn load(&self, path: &str) -> Option<AssetResponse> {
        let normalized = normalize_path(path);
        let file = DIST.get_file(normalized)?;
        Some(AssetResponse {
            path: normalized.to_string(),
            mime: mime_from_path(normalized),
            contents: Cow::Borrowed(file.contents()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct FilesystemAssets {
    root: PathBuf,
}

impl FilesystemAssets {
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }
}

impl WebAssetProvider for FilesystemAssets {
    fn load(&self, path: &str) -> Option<AssetResponse> {
        let normalized = normalize_path(path);
        let joined = self.root.join(normalized);
        let contents = fs::read(joined).ok()?;
        Some(AssetResponse {
            path: normalized.to_string(),
            mime: mime_from_path(normalized),
            contents: Cow::Owned(contents),
        })
    }
}

pub fn embedded_asset(path: &str) -> Option<AssetResponse> {
    EmbeddedAssets.load(path)
}

fn normalize_path(raw: &str) -> &str {
    let trimmed = raw.trim_start_matches('/');
    if trimmed.is_empty() {
        return "index.html";
    }
    trimmed
}

fn mime_from_path(path: &str) -> &'static str {
    match path.rsplit('.').next().map(|ext| ext.to_ascii_lowercase()) {
        Some(ref ext) if ext == "html" => "text/html; charset=utf-8",
        Some(ref ext) if ext == "css" => "text/css; charset=utf-8",
        Some(ref ext) if ext == "js" => "text/javascript; charset=utf-8",
        Some(ref ext) if ext == "json" => "application/json; charset=utf-8",
        Some(ref ext) if ext == "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_index_html() {
        let assets = EmbeddedAssets;
        let asset = assets.load("/").expect("index.html embedded");
        assert_eq!(asset.mime, "text/html; charset=utf-8");
        // Assert that the embedded asset still contains an HTML DOCTYPE. We
        // do a case-insensitive search for `<!doctype html>` so the test is
        // robust across different bundler/minifier behaviors.
        let bytes = asset.contents.as_ref();
        let lower = bytes
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect::<Vec<u8>>();
        let needle = b"<!doctype html>";
        let has_doctype = lower.windows(needle.len()).any(|window| window == needle);
        assert!(
            has_doctype,
            "embedded index.html must contain <!doctype html> (case-insensitive) DOCTYPE",
        );
    }

    #[test]
    fn normalizes_root_path() {
        let assets = EmbeddedAssets;
        let asset = assets.load("").expect("falls back to index");
        assert_eq!(asset.path, "index.html");
    }
}
