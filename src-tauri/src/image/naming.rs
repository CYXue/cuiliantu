//! Output file naming: template expansion, conflict resolution and the
//! Windows reserved-device guard.
//!
//! Split out of the former `processor.rs` monolith.

use std::path::{Path, PathBuf};

use super::processor::{ConflictPolicy, ProcessingResult};

/// `true` if the name's device part collides with a Windows reserved device
/// name. Windows refuses to create regular files named CON, PRN, AUX, NUL,
/// COM1-9 or LPT1-9 even when they carry an extension.
pub(super) fn is_windows_reserved_device_name(name: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let device = name.split('.').next().unwrap_or(name);
    RESERVED.contains(&device.to_ascii_uppercase().as_str())
}

impl super::processor::ImageProcessor {
    /// Expand the filename template in a single pass. `{width}`/`{height}`
    /// refer to the final (post-resize) dimensions.
    ///
    /// Unlike the previous chained `String::replace`, a value that itself
    /// contains a placeholder token (e.g. a file named `{width}.png`) is
    /// inserted verbatim instead of being expanded a second time.
    pub(super) fn output_filename(
        input: &Path,
        width: u32,
        height: u32,
        ext: &str,
        pattern: &str,
    ) -> String {
        let stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".to_string());
        let width_s = width.to_string();
        let height_s = height.to_string();

        let mut name = String::with_capacity(pattern.len() + 16);
        let mut rest = pattern;
        while let Some(open) = rest.find('{') {
            name.push_str(&rest[..open]);
            let after = &rest[open..];
            match after.find('}') {
                // A well-formed token: substitute the known ones, keep any
                // unknown token verbatim (same as `replace` did).
                Some(close) => {
                    let token = &after[..=close];
                    match token {
                        "{name}" => name.push_str(&stem),
                        "{width}" => name.push_str(&width_s),
                        "{height}" => name.push_str(&height_s),
                        "{format}" => name.push_str(ext),
                        _ => name.push_str(token),
                    }
                    rest = &after[close + 1..];
                }
                // No closing brace: the rest is literal text.
                None => {
                    name.push_str(after);
                    rest = "";
                }
            }
        }
        name.push_str(rest);

        // Keep it a single safe filename component. Collapse any `..` segment
        // first so a stem like `..` (or `a/../b`) cannot escape the output
        // directory when joined below, then neutralise path separators.
        let name: String = name
            .replace("..", "_")
            .chars()
            .map(|c| if c == '/' || c == '\\' { '_' } else { c })
            .collect();
        let name = name.trim();
        let name = if name.is_empty() { &stem } else { name };

        // Windows reserves device names even when they carry an extension
        // (`NUL.png` IS the NUL device — the write silently disappears).
        // Append an underscore so output is always a writable regular file;
        // other platforms never hit this branch.
        let name = if is_windows_reserved_device_name(name) {
            format!("{name}_")
        } else {
            name.to_string()
        };

        format!("{}.{}", name, ext)
    }

    pub(crate) fn resolve_output_path(
        dir: &Path,
        filename: &str,
        policy: ConflictPolicy,
    ) -> PathBuf {
        let candidate = dir.join(filename);
        if policy != ConflictPolicy::AutoRename || !candidate.exists() {
            return candidate;
        }

        let stem = Path::new(filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".to_string());
        let ext = Path::new(filename)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        for i in 1..=9999u32 {
            let candidate = dir.join(format!("{}_{}.{}", stem, i, ext));
            if !candidate.exists() {
                return candidate;
            }
        }
        candidate
    }

    pub(super) fn skipped_result(input: &Path, original_size: u64) -> ProcessingResult {
        ProcessingResult {
            original_path: input.to_string_lossy().to_string(),
            output_path: String::new(),
            original_size,
            output_size: original_size,
            reduction_percent: 0.0,
            success: true,
            error: None,
            skipped: true,
            within_target: true,
        }
    }
}
