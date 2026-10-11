//! Image type detection: magic-byte signature + best-effort dimensions.
//!
//! Split out of the former `image_commands.rs` monolith.

use image::ImageReader;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;
use tauri::State;

use super::path_guard::PathWhitelist;
use crate::image::formats::{detect_format, DetectedFormat};

/// Information about a file's real image type, detected from its signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeInfo {
    /// Absolute path of the file
    pub path: String,
    /// Lowercase file extension without the leading dot (empty if none)
    pub extension: String,
    /// Format detected from magic bytes
    pub detected_format: DetectedFormat,
    /// MIME type of the detected format
    pub detected_mime: String,
    /// Whether the extension matches the detected format
    pub matches_extension: bool,
    /// Pixel width if the image could be decoded (None for unsupported containers)
    pub width: Option<u32>,
    /// Pixel height if the image could be decoded
    pub height: Option<u32>,
    /// File size in bytes
    pub size_bytes: u64,
}

/// At most this many bytes are read for signature detection. Every signature
/// checked by `detect_format` lives in the first bytes of the file; reading a
/// whole multi-hundred-MB image just to sniff a header is pure waste.
const HEADER_SNIFF_BYTES: u64 = 64 * 1024;

/// Detect the real type of one image file from its signature bytes.
///
/// The file does not need to be decodable: container formats (HEIC/AVIF/JXL)
/// whose pixels the `image` crate cannot read are still recognised, and a
/// lying extension is flagged. This is the shared implementation behind the
/// batch command (see [`detect_file_types`]); there is deliberately no
/// single-file IPC command — one round-trip per drop is all the frontend
/// needs.
pub(super) fn detect_file_type_impl(path_str: String) -> Result<FileTypeInfo, String> {
    let path = Path::new(&path_str);

    let metadata = std::fs::metadata(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let size_bytes = metadata.len();

    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // Only the first bytes are needed for signature detection.
    let mut header = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("Failed to read file: {}", e))?
        .take(HEADER_SNIFF_BYTES)
        .read_to_end(&mut header)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    let detected = detect_format(&header);

    let matches_extension = match detected.canonical_extension() {
        Some(ext) => extension == ext,
        None => false,
    };

    // Best-effort dimensions; unsupported containers (HEIC/AVIF) yield None.
    // `into_dimensions` decodes the header only, never the pixels, and the
    // header buffer we already read for signature detection doubles as the
    // source — every format's dimension fields live in its first bytes, so
    // the file is never opened a second time. Note that the reported
    // dimensions are the on-disk orientation (EXIF, if any, is NOT applied
    // here).
    let (width, height) = match ImageReader::new(Cursor::new(header.as_slice()))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
    {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    };

    Ok(FileTypeInfo {
        path: path_str,
        extension,
        detected_format: detected,
        detected_mime: detected.mime().to_string(),
        matches_extension,
        width,
        height,
        size_bytes,
    })
}

/// One entry of the batch `detect_file_types` command: either the detected
/// info or an error message. Best-effort by design — one unreadable or
/// disallowed file must not fail the whole drop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeDetectResult {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<FileTypeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Batch type detection: one IPC round-trip for a whole drop instead of one
/// per file. Files are probed in parallel; every entry reports its own
/// success or failure.
#[tauri::command]
pub async fn detect_file_types(
    paths: Vec<String>,
    whitelist: State<'_, PathWhitelist>,
) -> Result<Vec<FileTypeDetectResult>, String> {
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || detect_file_types_impl(paths, &whitelist))
        .await
        .map_err(|e| format!("Detect task failed: {}", e))
}

pub(super) fn detect_file_types_impl(
    paths: Vec<String>,
    whitelist: &PathWhitelist,
) -> Vec<FileTypeDetectResult> {
    paths
        .into_par_iter()
        .map(|path| match whitelist.ensure_allowed(Path::new(&path)) {
            Ok(()) => match detect_file_type_impl(path.clone()) {
                Ok(info) => FileTypeDetectResult {
                    path,
                    info: Some(info),
                    error: None,
                },
                Err(e) => FileTypeDetectResult {
                    path,
                    info: None,
                    error: Some(e),
                },
            },
            Err(e) => FileTypeDetectResult {
                path,
                info: None,
                error: Some(e),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique scratch directory for one test.
    fn scratch_dir(name: &str) -> std::path::PathBuf {
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
    fn detect_file_types_batch_reports_info_and_per_file_errors() {
        let dir = scratch_dir("detect-batch");
        let png = write_png(&dir, "a.png");
        let garbage = dir.join("junk.bin");
        std::fs::write(&garbage, b"definitely not an image").unwrap();

        let wl = PathWhitelist::default();
        wl.register(&[dir.to_string_lossy().to_string()]);

        // NOT whitelisted: must come back as a per-file error, not fail the
        // whole batch.
        let outside = std::env::temp_dir().join("clt-detect-outside.png");

        let results = detect_file_types_impl(
            vec![
                png,
                garbage.to_string_lossy().to_string(),
                outside.to_string_lossy().to_string(),
            ],
            &wl,
        );

        assert_eq!(results.len(), 3);
        // Real PNG: full info, no error.
        let info = results[0].info.as_ref().expect("png must detect");
        assert!(info.width.is_some() && info.height.is_some());
        assert!(results[0].error.is_none());
        // Garbage bytes: detection still succeeds per file (type unknown).
        assert!(results[1].info.is_some());
        assert!(results[1].error.is_none());
        // Outside the whitelist: per-file error, no info.
        assert!(results[2].info.is_none());
        assert!(results[2].error.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
