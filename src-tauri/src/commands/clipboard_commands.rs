//! Paste support. A pasted screenshot has no file path — it lives only in
//! the clipboard — so the frontend sends its bytes here, the command parks
//! them in the app's temp folder and returns the path. The saved file is
//! registered in the path whitelist so the normal detect / preview / process
//! commands accept it like any file the user picked.

use base64::Engine as _;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::State;

use super::path_guard::PathWhitelist;

/// Directory under the system temp dir where pasted images are parked.
const PASTE_DIR_NAME: &str = "clt-paste";

/// Monotonic suffix so two pastes within the same millisecond never collide.
static PASTE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Save a base64-encoded pasted image into the temp folder and return its
/// path. Also registers the path in the session whitelist.
#[tauri::command]
pub async fn save_temp_image(
    data_base64: String,
    ext: String,
    whitelist: State<'_, PathWhitelist>,
) -> Result<String, String> {
    // `State` borrows from the app and cannot move into `spawn_blocking`;
    // clone the cheap shared handle instead.
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_temp_image_impl(&data_base64, &ext, &whitelist)
    })
    .await
    .map_err(|e| format!("Paste task failed: {}", e))?
}

fn save_temp_image_impl(
    data_base64: &str,
    ext: &str,
    whitelist: &PathWhitelist,
) -> Result<String, String> {
    // Tolerate a `data:image/...;base64,` prefix if the caller included one.
    let raw = data_base64
        .split_once("base64,")
        .map(|(_, b)| b)
        .unwrap_or(data_base64);
    // Bound the payload *before* decoding: the decoded buffer is a second full
    // allocation, and disk space is finite too.
    super::ensure_base64_input_bounded(raw)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("Failed to decode pasted image: {}", e))?;
    if bytes.is_empty() {
        return Err("Pasted image is empty".to_string());
    }

    // Keep only ASCII alphanumerics from the requested extension ("png",
    // "jpeg"); anything odd falls back to "png" — screenshots are bitmaps.
    let safe_ext: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let safe_ext: &str = if safe_ext.is_empty() {
        "png"
    } else {
        &safe_ext
    };

    let dir = std::env::temp_dir().join(PASTE_DIR_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create paste dir: {}", e))?;

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = PASTE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("paste-{millis}-{seq}.{safe_ext}"));

    std::fs::write(&path, &bytes).map_err(|e| format!("Failed to save pasted image: {}", e))?;

    // The temp file is now a legitimate session input.
    whitelist.register(&[path.to_string_lossy().to_string()]);
    Ok(path.to_string_lossy().to_string())
}

/// Remove stale pasted-image files from a previous session.
///
/// `save_temp_image` parks pasted screenshots under the system temp dir; they
/// are recreated on demand and never referenced again, so clearing the folder
/// at startup keeps disk usage bounded without touching any other file.
pub fn cleanup_paste_dir() {
    let dir = std::env::temp_dir().join(PASTE_DIR_NAME);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_base64_to_temp_and_registers_it() {
        let png_bytes = {
            let dir = std::env::temp_dir().join(format!("clt-paste-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let p = dir.join("src.png");
            image::RgbaImage::from_pixel(4, 4, image::Rgba([9, 9, 9, 255]))
                .save(&p)
                .unwrap();
            let bytes = std::fs::read(&p).unwrap();
            let _ = std::fs::remove_dir_all(&dir);
            bytes
        };
        let encoded = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        let wl = PathWhitelist::default();

        let saved = save_temp_image_impl(&encoded, "png", &wl).expect("save must succeed");
        let saved_path = std::path::Path::new(&saved);

        assert!(saved_path.is_file(), "temp file must exist: {saved}");
        assert_eq!(
            std::fs::read(saved_path).unwrap(),
            png_bytes,
            "saved bytes must round-trip exactly"
        );
        assert!(
            wl.is_allowed(saved_path),
            "saved paste must be whitelisted for later commands"
        );
        let _ = std::fs::remove_file(saved_path);
    }

    #[test]
    fn sanitizes_hostile_extension_and_rejects_garbage() {
        let wl = PathWhitelist::default();
        // A path-traversal-ish "extension" must be stripped to alphanumerics.
        let valid = base64::engine::general_purpose::STANDARD.encode(b"ok");
        let saved =
            save_temp_image_impl(&valid, "../../exe", &wl).expect("sanitize must keep it safe");
        assert!(
            !saved.contains(".."),
            "dots must never survive extension sanitizing: {saved}"
        );

        // Garbage base64 must fail cleanly.
        assert!(save_temp_image_impl("!!!not-base64!!!", "png", &wl).is_err());

        let _ = std::fs::remove_file(saved);
    }

    #[test]
    fn oversized_paste_input_is_rejected() {
        // The IPC input bound must actually fire: one byte over the limit is
        // refused before any decode allocation, not silently written to disk.
        let wl = PathWhitelist::default();
        let oversized = "A".repeat(super::super::MAX_BASE64_IMAGE_INPUT + 1);
        let err = save_temp_image_impl(&oversized, "png", &wl)
            .expect_err("over-limit payload must be rejected");
        assert!(err.contains("too large"), "got: {err}");

        // One byte under the limit passes the size gate. Invalid base64 chars
        // are used on purpose so the flow fails at the decoder — proving the
        // size gate was not what rejected it — instead of materializing a
        // ~190 MiB temp file from valid padding bytes.
        let under = "!".repeat(super::super::MAX_BASE64_IMAGE_INPUT);
        let err = save_temp_image_impl(&under, "png", &wl)
            .expect_err("valid-size garbage must reach the decoder");
        assert!(
            err.contains("decode"),
            "failure must come from the decoder, not the size gate: {err}"
        );
    }
}
