pub mod clipboard_commands;
pub mod image_commands;
pub mod path_guard;
pub mod update_commands;

/// Upper bound on base64-encoded image payloads accepted over IPC
/// (`save_temp_image` paste input, `preview_image_data` template input).
///
/// Decoded bytes are roughly 3/4 of the encoded text, so this allows ~192 MiB
/// of actual image data — far beyond any real screenshot or template image —
/// while capping the double allocation (IPC string + decoded bytes) that a
/// hostile or runaway frontend could otherwise force. Same discipline as the
/// resize and watermark clamps: every IPC input gets a bound.
pub const MAX_BASE64_IMAGE_INPUT: usize = 256 * 1024 * 1024;

/// Reject a base64 payload that exceeds [`MAX_BASE64_IMAGE_INPUT`].
///
/// Shared by both base64 entry points so the limit and its message stay
/// identical across commands.
pub fn ensure_base64_input_bounded(raw: &str) -> Result<(), String> {
    if raw.len() > MAX_BASE64_IMAGE_INPUT {
        return Err(format!(
            "Image payload too large: {} bytes encoded (limit {})",
            raw.len(),
            MAX_BASE64_IMAGE_INPUT
        ));
    }
    Ok(())
}
