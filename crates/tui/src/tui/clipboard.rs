//! Clipboard handling for paste support in TUI
//!
//! Supports text and image paste operations. Images on the clipboard are
//! encoded as PNG and persisted under `~/.deepseek/clipboard-images/` so the
//! model can reach them via the existing `@`-mention / file tools (DeepSeek
//! V4 does not currently accept inline image input on its Chat Completions
//! endpoint, so we materialize the bytes to disk instead of base64-embedding
//! them in the request).

#[cfg(target_os = "macos")]
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use arboard::{Clipboard, ImageData};
use image::{ImageBuffer, Rgba};

// === Types ===

/// Metadata captured for a pasted clipboard image. Used by the composer to
/// render a status hint like `Pasted 1024x768 image (235KB) → <path>`.
#[derive(Clone)]
pub struct PastedImage {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub byte_len: usize,
}

impl PastedImage {
    /// Short human-readable summary, e.g. `1024x768 PNG`.
    pub fn short_label(&self) -> String {
        format!("{}x{} PNG", self.width, self.height)
    }

    /// Approximate file size suffix, e.g. `235KB`.
    pub fn size_label(&self) -> String {
        let kb = (self.byte_len as f64 / 1024.0).round() as u64;
        format!("{kb}KB")
    }
}

/// Clipboard payloads supported by the TUI.
pub enum ClipboardContent {
    Text(String),
    Image(PastedImage),
}

/// Clipboard reader/writer helper.
pub struct ClipboardHandler {
    clipboard: Option<Clipboard>,
}

impl ClipboardHandler {
    /// Create a new clipboard handler, falling back to a no-op when unavailable.
    pub fn new() -> Self {
        let clipboard = Clipboard::new().ok();
        Self { clipboard }
    }

    /// Read the clipboard and return the parsed content.
    ///
    /// `workspace` is used as a fallback location when `~/.deepseek/` cannot
    /// be resolved (e.g. running with a stripped HOME in CI sandboxes).
    pub fn read(&mut self, workspace: &Path) -> Option<ClipboardContent> {
        let clipboard = self.clipboard.as_mut()?;
        if let Ok(text) = clipboard.get_text() {
            return Some(ClipboardContent::Text(text));
        }

        if let Ok(image) = clipboard.get_image()
            && let Ok(pasted) = save_image_as_png(workspace, &image)
        {
            return Some(ClipboardContent::Image(pasted));
        }

        None
    }

    /// Write text to the clipboard (no-op if unavailable).
    pub fn write_text(&mut self, text: &str) -> Result<()> {
        if let Some(clipboard) = self.clipboard.as_mut()
            && clipboard.set_text(text.to_string()).is_ok()
        {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let mut child = Command::new("pbcopy")
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| anyhow::anyhow!("Failed to run pbcopy: {e}"))?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(text.as_bytes())
                    .map_err(|e| anyhow::anyhow!("Failed to write to pbcopy: {e}"))?;
            }
            let status = child
                .wait()
                .map_err(|e| anyhow::anyhow!("Failed to wait for pbcopy: {e}"))?;
            if status.success() {
                return Ok(());
            }
            Err(anyhow::anyhow!("pbcopy failed"))
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(anyhow::anyhow!("Clipboard unavailable"))
        }
    }
}

/// Resolve the directory pasted images should land in. Prefers
/// `~/.deepseek/clipboard-images/` so the path is stable across worktrees and
/// matches the location described in user-facing docs; falls back to
/// `<workspace>/clipboard-images/` if the home dir is unavailable.
fn clipboard_images_dir(workspace: &Path) -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".deepseek").join("clipboard-images");
    }
    workspace.join("clipboard-images")
}

/// Encode an RGBA `ImageData` from arboard as PNG and persist it. Returns
/// the resulting path along with metadata used to render the paste hint.
fn save_image_as_png(workspace: &Path, image: &ImageData) -> Result<PastedImage> {
    save_image_as_png_in(&clipboard_images_dir(workspace), image)
}

/// Lower-level variant that writes into an explicit directory. Exposed so the
/// unit tests don't have to scribble inside the user's real home directory.
fn save_image_as_png_in(dir: &Path, image: &ImageData) -> Result<PastedImage> {
    std::fs::create_dir_all(dir).context("create clipboard-images dir")?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = dir.join(format!("clipboard-{timestamp}.png"));

    let width = u32::try_from(image.width).context("clipboard image width too large")?;
    let height = u32::try_from(image.height).context("clipboard image height too large")?;

    // arboard hands us RGBA8 row-major. Copy into an ImageBuffer so we can
    // run it through the `image` crate's PNG encoder. We pad / truncate any
    // mismatched trailing bytes — defensive only, arboard already validates
    // the buffer length on every supported backend.
    let expected = (width as usize) * (height as usize) * 4;
    let mut rgba = image.bytes.as_ref().to_vec();
    if rgba.len() < expected {
        rgba.resize(expected, 0);
    } else if rgba.len() > expected {
        rgba.truncate(expected);
    }

    let buffer: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba)
        .context("clipboard image dimensions did not match buffer length")?;
    buffer
        .save_with_format(&path, image::ImageFormat::Png)
        .context("write clipboard PNG")?;

    let byte_len = std::fs::metadata(&path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    Ok(PastedImage {
        path,
        width,
        height,
        byte_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn solid_rgba(width: u16, height: u16, rgba: [u8; 4]) -> ImageData<'static> {
        let mut bytes = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for _ in 0..(width as usize * height as usize) {
            bytes.extend_from_slice(&rgba);
        }
        ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Owned(bytes),
        }
    }

    #[test]
    fn save_image_as_png_writes_valid_png() {
        let dir = tempfile::tempdir().unwrap();
        let img = solid_rgba(8, 4, [255, 0, 0, 255]);
        let pasted = save_image_as_png_in(dir.path(), &img).expect("encode png");

        assert_eq!(pasted.width, 8);
        assert_eq!(pasted.height, 4);
        assert!(pasted.byte_len > 0);
        assert_eq!(
            pasted.path.extension().and_then(|s| s.to_str()),
            Some("png")
        );

        // The first eight bytes of any PNG file are the magic signature; if
        // we ever regress to PPM or another format this will catch it.
        let header = std::fs::read(&pasted.path).unwrap();
        assert_eq!(&header[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn pasted_image_labels_format_correctly() {
        let p = PastedImage {
            path: PathBuf::from("/tmp/x.png"),
            width: 1024,
            height: 768,
            byte_len: 235 * 1024,
        };
        assert_eq!(p.short_label(), "1024x768 PNG");
        assert_eq!(p.size_label(), "235KB");
    }
}
