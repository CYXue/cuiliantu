use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Supported input formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputFormat {
    Jpeg,
    Png,
    Gif,
    Bmp,
    Tiff,
    WebP,
}

/// Supported output formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Jpeg,
    Png,
    Gif,
    Bmp,
    Tiff,
    WebP,
}

/// JSON-driven extension → format table.
///
/// The single source of truth is `supported_extensions.json` at the crate
/// root, which is `include!`d here (and imported directly by the frontend via
/// Vite) so the two sides can never drift. Add a format by editing that one
/// file; the `format` value must match an `InputFormat` variant name
/// (lower-cased, per the `#[serde(rename_all = "lowercase")]` above).
#[derive(Deserialize)]
struct ExtEntry {
    ext: String,
    #[serde(rename = "format")]
    format: InputFormat,
}

#[derive(Deserialize)]
struct SupportedExtensions {
    input: Vec<ExtEntry>,
    // Consumed only by the frontend; kept in the shared file so the list
    // stays in one place.
    #[allow(dead_code)]
    watermark: Vec<String>,
}

fn extension_map() -> &'static HashMap<String, InputFormat> {
    static MAP: OnceLock<HashMap<String, InputFormat>> = OnceLock::new();
    MAP.get_or_init(|| {
        const RAW: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/supported_extensions.json"
        ));
        let parsed: SupportedExtensions =
            serde_json::from_str(RAW).expect("[formats] failed to parse supported_extensions.json");
        parsed
            .input
            .into_iter()
            .map(|e| (e.ext.to_lowercase(), e.format))
            .collect()
    })
}

impl InputFormat {
    /// Get format from file extension (looked up in the shared JSON table).
    pub fn from_extension(ext: &str) -> Option<Self> {
        extension_map().get(&ext.to_lowercase()).copied()
    }

    /// Check if the extension is supported
    pub fn is_supported(ext: &str) -> bool {
        Self::from_extension(ext).is_some()
    }
}

impl OutputFormat {
    /// Get file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::WebP => "webp",
        }
    }
}

/// Format detected purely from a file's signature bytes.
///
/// Covers more container formats than [`InputFormat`], including HEIC/AVIF/JXL
/// which the `image` crate cannot decode but whose *type* we still want to
/// recognise (e.g. to flag a mismatched extension).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetectedFormat {
    Jpeg,
    Png,
    Gif,
    Bmp,
    Tiff,
    WebP,
    Heic,
    Avif,
    Jxl,
    Unknown,
}

impl DetectedFormat {
    /// Canonical file extension, if the format has one.
    pub fn canonical_extension(&self) -> Option<&'static str> {
        match self {
            Self::Jpeg => Some("jpg"),
            Self::Png => Some("png"),
            Self::Gif => Some("gif"),
            Self::Bmp => Some("bmp"),
            Self::Tiff => Some("tiff"),
            Self::WebP => Some("webp"),
            Self::Heic => Some("heic"),
            Self::Avif => Some("avif"),
            Self::Jxl => Some("jxl"),
            Self::Unknown => None,
        }
    }

    /// MIME type, used for display / downstream logic.
    pub fn mime(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Tiff => "image/tiff",
            Self::WebP => "image/webp",
            Self::Heic => "image/heic",
            Self::Avif => "image/avif",
            Self::Jxl => "image/jxl",
            Self::Unknown => "application/octet-stream",
        }
    }
}

/// Detect the real image format by inspecting the file's magic bytes.
///
/// Works even when the file extension is wrong or missing. Container formats
/// such as HEIC/AVIF/JXL are recognised here but are not necessarily decodable
/// by the `image` crate.
///
/// Each signature is only matched once enough bytes are present, so short but
/// otherwise valid files (e.g. a tiny PNG or a JPEG codestream) are still
/// recognised instead of being rejected as `Unknown`.
pub fn detect_format(bytes: &[u8]) -> DetectedFormat {
    let len = bytes.len();

    // Nothing shorter than 2 bytes can possibly match a known signature.
    if len < 2 {
        return DetectedFormat::Unknown;
    }

    // JPEG XL codestream: FF 0A. A real codestream always carries more bytes,
    // so requiring a third byte trims single-byte false positives at zero
    // cost. The check stays best-effort by nature — the same looseness class
    // as the 2-byte "BM" BMP probe below.
    if len >= 3 && bytes[0] == 0xFF && bytes[1] == 0x0A {
        return DetectedFormat::Jxl;
    }
    // BMP: BM (2 bytes)
    if bytes[0] == 0x42 && bytes[1] == 0x4D {
        return DetectedFormat::Bmp;
    }

    // JPEG: FF D8 FF (3 bytes)
    if len >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return DetectedFormat::Jpeg;
    }

    // TIFF: II*\0 (little-endian) or MM\0* (big-endian) (4 bytes)
    if len >= 4
        && (bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00])
            || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]))
    {
        return DetectedFormat::Tiff;
    }

    // GIF87a / GIF89a (6 bytes)
    if len >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return DetectedFormat::Gif;
    }

    // PNG (8 bytes)
    if len >= 8 && bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return DetectedFormat::Png;
    }

    // The remaining signatures live at byte offsets up to 12.
    if len < 12 {
        return DetectedFormat::Unknown;
    }

    // WebP: RIFF....WEBP
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return DetectedFormat::WebP;
    }
    // ISO-BMFF (HEIF / AVIF): [size][ftyp][major brand]
    if &bytes[4..8] == b"ftyp" {
        match &bytes[8..12] {
            b"avif" | b"avis" => return DetectedFormat::Avif,
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"hevc" | b"hevx" => {
                return DetectedFormat::Heic;
            }
            _ => {}
        }
    }
    // JPEG XL container box `JXL `
    if bytes.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20]) {
        return DetectedFormat::Jxl;
    }

    DetectedFormat::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_format_from_extension() {
        assert_eq!(InputFormat::from_extension("jpg"), Some(InputFormat::Jpeg));
        assert_eq!(InputFormat::from_extension("JPEG"), Some(InputFormat::Jpeg));
        assert_eq!(InputFormat::from_extension("png"), Some(InputFormat::Png));
        assert_eq!(InputFormat::from_extension("webp"), Some(InputFormat::WebP));
        assert_eq!(InputFormat::from_extension("unknown"), None);
    }

    #[test]
    fn test_output_format_extension() {
        assert_eq!(OutputFormat::Jpeg.extension(), "jpg");
        assert_eq!(OutputFormat::WebP.extension(), "webp");
    }

    #[test]
    fn test_detect_format_magic_bytes() {
        // PNG
        assert_eq!(
            detect_format(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]),
            DetectedFormat::Png
        );
        // JPEG
        assert_eq!(
            detect_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            DetectedFormat::Jpeg
        );
        // GIF89a
        assert_eq!(
            detect_format(b"GIF89a\x01\x00\x01\x00"),
            DetectedFormat::Gif
        );
        // WebP
        assert_eq!(
            detect_format(b"RIFF\x1a\x00\x00\x00WEBP\x00"),
            DetectedFormat::WebP
        );
        // HEIC (ftyp heic)
        assert_eq!(
            detect_format(b"\x00\x00\x00\x18ftypheic\x00"),
            DetectedFormat::Heic
        );
        // AVIF (ftyp avif)
        assert_eq!(
            detect_format(b"\x00\x00\x00\x1cftypavif\x00"),
            DetectedFormat::Avif
        );
        // too short
        assert_eq!(detect_format(&[0x89]), DetectedFormat::Unknown);
    }
}
