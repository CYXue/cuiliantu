//! Before/after preview commands (Squoosh-style compare).
//!
//! Split out of the former `image_commands.rs` monolith.

use base64::Engine as _;
use image::{DynamicImage, ImageFormat};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::State;

use super::ensure_base64_input_bounded;
use super::path_guard::PathWhitelist;
use crate::image::processor::{ImageProcessor, ProcessingOptions, ProcessingOptionsPayload};

/// Before/after preview for the currently selected file (Squoosh-style).
///
/// `after_size` is a REAL size: the processed image is encoded with the same
/// pipeline as the batch output, in memory. No formulas, no estimates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewData {
    /// Downscaled original, PNG, base64 (display only)
    pub before_base64: String,
    /// Downscaled result, PNG, base64 (display only)
    pub after_base64: String,
    /// Real size of the original file
    pub before_size: u64,
    /// Real size of the encoded result
    pub after_size: u64,
    /// Final pixel width after the pipeline
    pub width: u32,
    /// Final pixel height after the pipeline
    pub height: u32,
}

/// Largest side of the preview display images.
const PREVIEW_MAX_SIDE: u32 = 512;

fn preview_base64(img: &DynamicImage) -> String {
    let display = if img.width() > PREVIEW_MAX_SIDE || img.height() > PREVIEW_MAX_SIDE {
        img.resize(
            PREVIEW_MAX_SIDE,
            PREVIEW_MAX_SIDE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img.clone()
    };
    let mut cursor = Cursor::new(Vec::new());
    // Preview is best-effort; an encoding failure yields an empty image
    // rather than failing the whole panel.
    if display.write_to(&mut cursor, ImageFormat::Png).is_ok() {
        base64::engine::general_purpose::STANDARD.encode(cursor.into_inner())
    } else {
        String::new()
    }
}

/// Generate a before/after preview with exact output size estimation.
///
/// Async + `spawn_blocking` so dragging a quality slider re-encodes off the
/// main thread and the UI keeps animating.
#[tauri::command]
pub async fn preview_image(
    path: String,
    options: ProcessingOptionsPayload,
    whitelist: State<'_, PathWhitelist>,
) -> Result<PreviewData, String> {
    let whitelist = whitelist.inner().clone();
    let options: ProcessingOptions = options.into();
    tauri::async_runtime::spawn_blocking(move || {
        whitelist.ensure_allowed(Path::new(&path))?;
        preview_image_impl(&path, &options)
    })
    .await
    .map_err(|e| format!("Preview task failed: {}", e))?
}

/// Single-entry cache of the preview source bytes. Dragging the quality
/// slider re-renders the same file many times a second, and re-reading a
/// large photo from disk on every tick is pure waste. The entry is keyed by
/// path and validated against mtime + size, so an edited file is re-read;
/// only files up to [`PREVIEW_SOURCE_CACHE_MAX_BYTES`] are cached to bound
/// memory.
const PREVIEW_SOURCE_CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;

struct PreviewSourceCache {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
    bytes: Arc<Vec<u8>>,
}

static PREVIEW_SOURCE_CACHE: Mutex<Option<PreviewSourceCache>> = Mutex::new(None);

fn read_preview_source(path: &Path) -> Result<Arc<Vec<u8>>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let modified = meta.modified().ok();
    let len = meta.len();

    {
        let cached = PREVIEW_SOURCE_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(c) = cached.as_ref() {
            if c.path == path && c.len == len && c.modified == modified {
                return Ok(c.bytes.clone());
            }
        }
    }

    let bytes = Arc::new(std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?);
    if len <= PREVIEW_SOURCE_CACHE_MAX_BYTES {
        let mut cached = PREVIEW_SOURCE_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cached = Some(PreviewSourceCache {
            path: path.to_path_buf(),
            modified,
            len,
            bytes: bytes.clone(),
        });
    }
    Ok(bytes)
}

/// Shared preview pipeline: decode → apply → encode → [`PreviewData`].
///
/// `preview_image` (file path) and `preview_image_data` (base64 template)
/// differ only in where the source bytes come from; everything downstream is
/// identical, so it lives here.
fn render_preview(bytes: &[u8], options: &ProcessingOptions) -> Result<PreviewData, String> {
    let before_size = bytes.len() as u64;

    // Orientation-aware decode: the preview must match what the batch run
    // will write (both bake EXIF orientation in before the pipeline).
    let img = ImageProcessor::decode_bytes_with_orientation(bytes)
        .map_err(|e| format!("Failed to decode image: {}", e))?;

    let watermark = ImageProcessor::prepare_watermark(options);
    // Shrink the BEFORE image to its 512px thumbnail *before* the pipeline:
    // `preview_base64` only needs a small copy, so doing it first means the
    // full-resolution image can be moved into `apply_pipeline` instead of
    // cloned (a 4000×3000 RGBA clone is ~48 MiB of pure waste per preview).
    let before_base64 = preview_base64(&img);
    let processed = ImageProcessor::apply_pipeline(img, options, watermark.as_ref());
    let (width, height) = (processed.width(), processed.height());

    let after_bytes =
        ImageProcessor::encode_to_memory(&processed, options).map_err(|e| e.to_string())?;
    let after_size = after_bytes.len() as u64;

    Ok(PreviewData {
        before_base64,
        after_base64: preview_base64(&processed),
        before_size,
        after_size,
        width,
        height,
    })
}

fn preview_image_impl(path: &str, options: &ProcessingOptions) -> Result<PreviewData, String> {
    let bytes = read_preview_source(Path::new(path))?;
    render_preview(&bytes, options)
}

/// Same as `preview_image`, but the source is a base64-encoded image instead of
/// a file path. Used to preview effects on a built-in template image when the
/// user has not selected any file yet.
#[tauri::command]
pub async fn preview_image_data(
    input_base64: String,
    options: ProcessingOptionsPayload,
) -> Result<PreviewData, String> {
    // Base64 input has no filesystem path, hence no whitelist check.
    let options: ProcessingOptions = options.into();
    tauri::async_runtime::spawn_blocking(move || preview_image_data_impl(&input_base64, &options))
        .await
        .map_err(|e| format!("Preview task failed: {}", e))?
}

fn preview_image_data_impl(
    input_base64: &str,
    options: &ProcessingOptions,
) -> Result<PreviewData, String> {
    // Tolerate a `data:image/...;base64,` prefix if the caller included one.
    let raw = input_base64
        .split_once("base64,")
        .map(|(_, b)| b)
        .unwrap_or(input_base64);
    // Same IPC input bound as the paste path: cap the decode-time allocation.
    ensure_base64_input_bounded(raw)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("Failed to decode base64: {}", e))?;
    render_preview(&bytes, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique scratch directory for one test.
    fn scratch_dir(name: &str) -> PathBuf {
        static TEST_DIR_COUNTER: std::sync::atomic::AtomicU32 =
            std::sync::atomic::AtomicU32::new(0);
        let id = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("clt-{}-{}-{}", name, std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
        dir
    }

    /// Write a small valid PNG and return its path.
    fn write_png(dir: &Path, name: &str) -> String {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(8, 8, image::Rgba([12, 34, 56, 255]))
            .save(&path)
            .expect("failed to write test png");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn preview_source_cache_invalidates_on_change() {
        let dir = scratch_dir("preview-cache");
        let p = write_png(&dir, "a.png");

        let first = read_preview_source(Path::new(&p)).unwrap();
        // Same path + unchanged mtime/size -> the cached bytes are reused.
        assert!(Arc::ptr_eq(
            &first,
            &read_preview_source(Path::new(&p)).unwrap()
        ));

        // Rewrite with different content and a bumped mtime -> re-read.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&p, b"changed content").unwrap();
        let second = read_preview_source(Path::new(&p)).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(second.as_slice(), &b"changed content"[..]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_image_data_rejects_oversized_base64() {
        // One byte over the IPC input bound must fail before any decode
        // allocation happens — the reverse of the compliant-size path.
        let options = ProcessingOptions::default();
        let oversized = "A".repeat(super::super::MAX_BASE64_IMAGE_INPUT + 1);
        let err = preview_image_data_impl(&oversized, &options)
            .expect_err("over-limit payload must be rejected");
        assert!(err.contains("too large"), "got: {err}");
    }
}
