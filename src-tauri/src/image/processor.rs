use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont};
use color_quant::NeuQuant;
use image::codecs::gif::{GifDecoder, GifEncoder};
use image::metadata::Orientation;
use image::{
    AnimationDecoder, DynamicImage, Frame, GenericImageView, ImageDecoder, ImageFormat,
    ImageReader, RgbaImage,
};
use imageproc::drawing::{draw_text_mut, text_size};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};
use thiserror::Error;

// EXIF / ICC metadata re-embedding (JPEG -> JPEG). `kamadak-exif` is pure Rust
// (no native codec), so it keeps the project's zero-system-library build.
use exif::experimental::Writer;
use exif::{Reader as ExifReader, Tag, Value};

// Re-export so the head-less CLI (and other consumers) can reach the output
// format enum through the same `image::processor` path as the other options.
pub use super::formats::OutputFormat;

/// Embedded watermark font: 得意黑 Smiley Sans (SIL OFL 1.1).
///
/// Chosen because it is a compact single-weight CJK font (~2.6 MB) covering
/// simplified Chinese, kana, Latin, Cyrillic and Greek — enough for all
/// interface languages. The full license text ships next to the font.
const WATERMARK_FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/SmileySans-Oblique.ttf");

/// Image processing errors
#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("Failed to read image: {0}")]
    ReadError(String),
    #[error("Failed to write image: {0}")]
    WriteError(String),
}

/// Compression type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressionType {
    Lossy,
    Lossless,
}

/// How the width/height box is interpreted (Converseen-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResizeMode {
    /// Width/height form a pixel box.
    #[default]
    Pixels,
    /// Scale by a percentage of the original; aspect ratio is kept.
    Percent,
}

/// How the image fits into the pixel box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    /// Exact box; aspect ratio may change.
    #[default]
    Stretch,
    /// Largest size that fits inside the box; aspect ratio kept.
    Contain,
    /// Smallest size that covers the box, center-cropped; aspect ratio kept.
    Cover,
}

/// Where the watermark sits on the base image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WatermarkPosition {
    TopLeft,
    TopRight,
    #[default]
    BottomRight,
    BottomLeft,
    Center,
}

/// Watermark configuration, tagged by `kind` (`"image"` / `"text"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatermarkConfig {
    /// Picture watermark (XnConvert-style overlay).
    Image {
        /// Path to the watermark image (PNG with alpha recommended).
        path: String,
        /// Opacity 1-100 (100 = fully opaque).
        opacity: u8,
        /// Watermark width as a percentage of the base image width.
        scale_percent: u32,
        position: WatermarkPosition,
        /// Distance from the edges, as a percentage of the shorter base side.
        margin_percent: u32,
    },
    /// Text watermark rendered with the embedded Smiley Sans font.
    Text {
        /// Watermark text; `\n` (or Enter in the UI) starts a new line.
        text: String,
        /// Font size in pixels (px per EM).
        font_size: u32,
        /// Text color as hex, `"#RRGGBB"` or `"RRGGBB"` (invalid -> white).
        color: String,
        /// Opacity 1-100 (100 = fully opaque).
        opacity: u8,
        position: WatermarkPosition,
        /// Distance from the edges, as a percentage of the shorter base side.
        margin_percent: u32,
    },
}

impl WatermarkConfig {
    fn opacity(&self) -> u8 {
        match self {
            Self::Image { opacity, .. } | Self::Text { opacity, .. } => *opacity,
        }
    }

    fn position(&self) -> WatermarkPosition {
        match self {
            Self::Image { position, .. } | Self::Text { position, .. } => *position,
        }
    }

    fn margin_percent(&self) -> u32 {
        match self {
            Self::Image { margin_percent, .. } | Self::Text { margin_percent, .. } => {
                *margin_percent
            }
        }
    }
}

/// A watermark prepared once per batch: the image is decoded once, the text
/// stamp is rasterized once. Both end up as an RGBA overlay.
pub enum PreparedWatermark {
    Image(DynamicImage),
    Text(RgbaImage),
}

/// Parse `"#RRGGBB"` / `"RRGGBB"`; anything invalid falls back to white.
fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let s = hex.trim().trim_start_matches('#');
    if s.len() == 6 {
        if let Ok(v) = u32::from_str_radix(s, 16) {
            return (
                ((v >> 16) & 255) as u8,
                ((v >> 8) & 255) as u8,
                (v & 255) as u8,
            );
        }
    }
    (255, 255, 255)
}

/// Crop fully transparent borders so the stamp fits its ink exactly — that
/// keeps position/margin math independent of the font's internal metrics.
fn trim_transparent(img: &RgbaImage) -> Option<RgbaImage> {
    let (mut min_x, mut min_y) = (img.width(), img.height());
    let (mut max_x, mut max_y) = (0u32, 0u32);
    let mut found = false;

    for (x, y, px) in img.enumerate_pixels() {
        if px.0[3] > 0 {
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !found {
        return None;
    }
    Some(
        image::imageops::crop_imm(img, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
            .to_image(),
    )
}

/// Rasterize text into a tight RGBA stamp (alpha = coverage).
///
/// Multi-line: `\n` starts a new line (`\r\n` is normalized). Each line is
/// placed on the font's natural line grid (`height()` + `line_gap()`); blank
/// lines keep their slot so user-intended spacing survives, and the trailing
/// padding is removed by the transparent-border trim.
fn render_text_stamp(text: &str, font_size: u32, color: &str) -> Option<RgbaImage> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let font = FontRef::try_from_slice(WATERMARK_FONT_BYTES).ok()?;
    // Safety net for the IPC payload: keep the rasterized size in the same
    // 8..=400 band the UI enforces (see FONT_SIZE_MIN/MAX in WatermarkSection).
    let scale = PxScale::from(font_size.clamp(8, 400) as f32);
    let scaled = font.as_scaled(scale);
    let (r, g, b) = parse_hex_color(color);

    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return None;
    }

    let line_height = (scaled.height() + scaled.line_gap().max(0.0))
        .ceil()
        .max(1.0) as u32;
    let pad = 4u32;

    // Canvas must fit the widest line; every line gets one row of the grid.
    let mut max_w = 0u32;
    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        let (w, _) = text_size(scale, &font, line);
        max_w = max_w.max(w);
    }
    if max_w == 0 {
        return None;
    }

    let mut canvas = RgbaImage::new(
        max_w + pad * 2,
        (line_height * lines.len() as u32) + pad * 2,
    );
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        draw_text_mut(
            &mut canvas,
            image::Rgba([r, g, b, 255]),
            pad as i32,
            (pad + line_height * i as u32) as i32,
            scale,
            &font,
            line,
        );
    }

    trim_transparent(&canvas)
}

/// What happens when an output file already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Overwrite the existing file.
    #[default]
    Overwrite,
    /// Append _1, _2, ... until a free name is found.
    AutoRename,
    /// Keep the existing file; mark this input as skipped.
    Skip,
}

/// Output / writing options: format, quality, naming, conflicts, size guards.
///
/// Grouped out of the old single 40-field `ProcessingOptions` so that adding
/// or changing one concern touches a small, well-scoped struct instead of a
/// flat monster, and the domain model reads top-to-bottom by intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputOptions {
    /// Output format
    pub format: OutputFormat,
    /// Quality (0-100, for lossy compression)
    pub quality: u8,
    /// Keep metadata (EXIF, etc.)
    pub keep_metadata: bool,
    /// Compression type
    pub compression: CompressionType,
    /// Output filename template: {name} {width} {height} {format}.
    #[serde(default = "default_filename_pattern")]
    pub filename_pattern: String,
    /// Behaviour when the output file already exists.
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
    /// Only write the output when it is smaller than the original.
    #[serde(default)]
    pub skip_if_larger: bool,
    /// Preserve GIF animation frame-by-frame when converting GIF -> GIF.
    #[serde(default = "default_true")]
    pub preserve_animation: bool,
    /// Target output size in bytes; when set AND the output format is JPEG,
    /// the encoder bisects quality so the file fits the budget (keeping the
    /// highest quality possible). `None` uses the fixed `quality` value.
    #[serde(default)]
    pub target_size_bytes: Option<u64>,
    /// Reduce to N colors (2-256) for PNG/GIF output; None = off.
    #[serde(default)]
    pub quantize_colors: Option<u32>,
    /// Mirror the input directory tree under the output directory instead of
    /// flattening every file into a single folder. Batch-only; ignored when
    /// there is no shared parent directory among the inputs.
    #[serde(default)]
    pub mirror_subdirs: bool,
}

/// Resize box and fit strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeOptions {
    /// Resize width (None = keep original)
    pub width: Option<u32>,
    /// Resize height (None = keep original)
    pub height: Option<u32>,
    /// How width/height are interpreted.
    #[serde(default)]
    pub resize_mode: ResizeMode,
    /// Percentage scale (1-1000); used when `resize_mode` is Percent.
    #[serde(default = "default_resize_percent")]
    pub resize_percent: f64,
    /// Fit strategy inside the pixel box.
    #[serde(default)]
    pub fit_mode: FitMode,
}

/// Geometry transforms applied after resize (90°-step rotate + flips + fine
/// rotation at full resolution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformOptions {
    /// Clockwise rotation in degrees (0/90/180/270).
    #[serde(default)]
    pub rotate: u32,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    /// Fine rotation in degrees (`-180..=180`), applied at full resolution
    /// with canvas auto-expansion and transparent corners. `0` = off. Exact
    /// 90/180/270 values take the lossless fast path.
    #[serde(default)]
    pub rotate_degrees: f32,
}

/// Crop configuration: interactive drag-rect or ratio center-crop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropOptions {
    /// Interactive drag-crop rectangle `[fx, fy, fw, fh]`, normalized
    /// `0.0..=1.0` relative to the (orientation-corrected) image. Drawn on one
    /// image in the preview panel, mapped proportionally onto every image in
    /// the batch. `None` = off. Applied before `crop_ratio`.
    #[serde(default)]
    pub crop_rect: Option<[f64; 4]>,
    /// Center-crop the (orientation-corrected) image to a target aspect ratio
    /// before any resizing, e.g. `(1, 1)` for a square or `(16, 9)` for
    /// widescreen. `None` = off. Batch-friendly ratio crop: one ratio applied
    /// to many images of different sizes.
    #[serde(default)]
    pub crop_ratio: Option<(u32, u32)>,
}

/// Tonal / stylistic adjustments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjustOptions {
    /// Additive brightness, `-100..=100` (maps onto `imageops::brighten`'s
    /// -255..=255 channel offset). `0` = identity.
    #[serde(default)]
    pub brightness: i32,
    /// Contrast, `-100..=100`; `0` = identity (`imageops::contrast` semantics:
    /// `percent = ((100 + c) / 100)²`).
    #[serde(default)]
    pub contrast: i32,
    /// Hue rotation in degrees, `-180..=180`; `0` = identity.
    #[serde(default)]
    pub hue: i32,
    /// USM sharpen amount `0..=100`; `0` = off. Maps onto
    /// `unsharpen(0.5 + amount/100·2.5, 2)`.
    #[serde(default)]
    pub sharpen: u8,
    /// Gaussian blur sigma `0..=50`; `0` = off.
    #[serde(default)]
    pub blur: f32,
    #[serde(default)]
    pub grayscale: bool,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub sepia: bool,
    /// Percentile (1%..99%) contrast stretch; safe on already full-range
    /// images (identity).
    #[serde(default)]
    pub auto_contrast: bool,
}

/// Image processing options, grouped by concern.
///
/// The wire format from the frontend stays flat (see [`ProcessingOptionsPayload`]
/// and its `From` impl) — this is the canonical, grouped domain model the
/// processor, CLI and tests build directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingOptions {
    pub output: OutputOptions,
    pub resize: ResizeOptions,
    pub transform: TransformOptions,
    pub crop: CropOptions,
    pub adjust: AdjustOptions,
    /// Optional watermark (picture or text).
    #[serde(default)]
    pub watermark: Option<WatermarkConfig>,
}

/// Flat IPC payload the frontend sends.
///
/// Kept separate from the canonical [`ProcessingOptions`] so the wire format is
/// decoupled from the (grouped) domain model — an anti-corruption layer at the
/// IPC boundary. Every field is `#[serde(default)]`-tolerant so a partial or
/// older payload still deserializes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingOptionsPayload {
    // The first six fields predate the `#[serde(default)]` discipline applied
    // to everything after them, so a partial payload (an older frontend, or a
    // template saved before a newer option existed) used to fail to parse
    // outright instead of degrading.
    //
    // `serde(default)` alone is not enough here: it yields `0`/`false`/the
    // first enum variant, which disagrees with the factory defaults below
    // (`quality` is 80, `format` is WebP). Each function therefore points at
    // the one authoritative source — `ProcessingOptions::default()` — so the
    // wire default can never drift from the UI's default.
    #[serde(default = "default_format")]
    pub format: OutputFormat,
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub keep_metadata: bool,
    #[serde(default = "default_compression")]
    pub compression: CompressionType,
    #[serde(default)]
    pub resize_mode: ResizeMode,
    #[serde(default = "default_resize_percent")]
    pub resize_percent: f64,
    #[serde(default)]
    pub fit_mode: FitMode,
    #[serde(default)]
    pub rotate: u32,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    #[serde(default)]
    pub watermark: Option<WatermarkConfig>,
    #[serde(default)]
    pub quantize_colors: Option<u32>,
    #[serde(default = "default_filename_pattern")]
    pub filename_pattern: String,
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
    #[serde(default)]
    pub skip_if_larger: bool,
    #[serde(default = "default_true")]
    pub preserve_animation: bool,
    #[serde(default)]
    pub mirror_subdirs: bool,
    #[serde(default)]
    pub target_size_bytes: Option<u64>,
    #[serde(default)]
    pub crop_rect: Option<[f64; 4]>,
    #[serde(default)]
    pub crop_ratio: Option<(u32, u32)>,
    #[serde(default)]
    pub rotate_degrees: f32,
    #[serde(default)]
    pub brightness: i32,
    #[serde(default)]
    pub contrast: i32,
    #[serde(default)]
    pub hue: i32,
    #[serde(default)]
    pub sharpen: u8,
    #[serde(default)]
    pub blur: f32,
    #[serde(default)]
    pub grayscale: bool,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub sepia: bool,
    #[serde(default)]
    pub auto_contrast: bool,
}

impl From<ProcessingOptionsPayload> for ProcessingOptions {
    fn from(p: ProcessingOptionsPayload) -> Self {
        ProcessingOptions {
            output: OutputOptions {
                format: p.format,
                quality: p.quality,
                keep_metadata: p.keep_metadata,
                compression: p.compression,
                filename_pattern: p.filename_pattern,
                conflict_policy: p.conflict_policy,
                skip_if_larger: p.skip_if_larger,
                preserve_animation: p.preserve_animation,
                target_size_bytes: p.target_size_bytes,
                quantize_colors: p.quantize_colors,
                mirror_subdirs: p.mirror_subdirs,
            },
            resize: ResizeOptions {
                // Clamp at the IPC boundary (same discipline as the watermark
                // fields): a hostile or buggy payload must not carry a resize
                // target that later turns into a giant allocation. The resizer
                // itself re-applies the envelope (see `apply_resize`) so
                // non-IPC callers are covered too.
                width: p.width.map(|w| w.min(ImageProcessor::MAX_RESIZE_DIMENSION)),
                height: p
                    .height
                    .map(|h| h.min(ImageProcessor::MAX_RESIZE_DIMENSION)),
                resize_mode: p.resize_mode,
                resize_percent: p.resize_percent.clamp(
                    ImageProcessor::RESIZE_PERCENT_MIN,
                    ImageProcessor::RESIZE_PERCENT_MAX,
                ),
                fit_mode: p.fit_mode,
            },
            transform: TransformOptions {
                rotate: p.rotate,
                flip_horizontal: p.flip_horizontal,
                flip_vertical: p.flip_vertical,
                rotate_degrees: p.rotate_degrees,
            },
            crop: CropOptions {
                crop_rect: p.crop_rect,
                crop_ratio: p.crop_ratio,
            },
            adjust: AdjustOptions {
                brightness: p.brightness,
                contrast: p.contrast,
                hue: p.hue,
                sharpen: p.sharpen,
                blur: p.blur,
                grayscale: p.grayscale,
                invert: p.invert,
                sepia: p.sepia,
                auto_contrast: p.auto_contrast,
            },
            watermark: p.watermark,
        }
    }
}

fn default_resize_percent() -> f64 {
    100.0
}

fn default_filename_pattern() -> String {
    "{name}".to_string()
}

fn default_true() -> bool {
    true
}

/// Wire defaults that must match [`ProcessingOptions::default`] exactly.
fn default_format() -> OutputFormat {
    ProcessingOptions::default().output.format
}

fn default_quality() -> u8 {
    ProcessingOptions::default().output.quality
}

fn default_compression() -> CompressionType {
    ProcessingOptions::default().output.compression
}

impl Default for ProcessingOptions {
    fn default() -> Self {
        Self {
            output: OutputOptions {
                format: OutputFormat::WebP,
                quality: 80,
                keep_metadata: false,
                compression: CompressionType::Lossy,
                filename_pattern: default_filename_pattern(),
                conflict_policy: ConflictPolicy::Overwrite,
                skip_if_larger: false,
                preserve_animation: true,
                target_size_bytes: None,
                quantize_colors: None,
                mirror_subdirs: false,
            },
            resize: ResizeOptions {
                width: None,
                height: None,
                resize_mode: ResizeMode::Pixels,
                resize_percent: 100.0,
                fit_mode: FitMode::Stretch,
            },
            transform: TransformOptions {
                rotate: 0,
                flip_horizontal: false,
                flip_vertical: false,
                rotate_degrees: 0.0,
            },
            crop: CropOptions {
                crop_rect: None,
                crop_ratio: None,
            },
            adjust: AdjustOptions {
                brightness: 0,
                contrast: 0,
                hue: 0,
                sharpen: 0,
                blur: 0.0,
                grayscale: false,
                invert: false,
                sepia: false,
                auto_contrast: false,
            },
            watermark: None,
        }
    }
}

impl Default for OutputOptions {
    fn default() -> Self {
        ProcessingOptions::default().output
    }
}

impl Default for ResizeOptions {
    fn default() -> Self {
        ProcessingOptions::default().resize
    }
}

impl Default for TransformOptions {
    fn default() -> Self {
        ProcessingOptions::default().transform
    }
}

impl Default for CropOptions {
    fn default() -> Self {
        ProcessingOptions::default().crop
    }
}

impl Default for AdjustOptions {
    fn default() -> Self {
        ProcessingOptions::default().adjust
    }
}

/// Result of processing a single image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingResult {
    /// Original file path
    pub original_path: String,
    /// Output file path (empty when skipped)
    pub output_path: String,
    /// Original file size in bytes
    pub original_size: u64,
    /// Output file size in bytes
    pub output_size: u64,
    /// Reduction percentage
    pub reduction_percent: f64,
    /// Whether processing was successful
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// True when nothing was written (not smaller / name conflict / cancelled)
    #[serde(default)]
    pub skipped: bool,
    /// True when the output fit the requested target size, or no target was
    /// set. `false` means even the minimum search quality exceeded the budget
    /// (only possible in JPEG target-size mode).
    #[serde(default)]
    pub within_target: bool,
}

/// Image processor
pub struct ImageProcessor;

impl ImageProcessor {
    /// Process a single image into `output_dir` (loads the watermark itself).
    pub fn process_image<P: AsRef<Path>, Q: AsRef<Path>>(
        input_path: P,
        output_dir: Q,
        options: &ProcessingOptions,
    ) -> Result<ProcessingResult, ProcessError> {
        let watermark = Self::prepare_watermark(options);
        Self::process_image_with(input_path, output_dir, options, watermark.as_ref())
    }

    /// Same as [`process_image`], but with a prepared watermark so that
    /// batch processing decodes the image / rasterizes the text only once.
    pub fn process_image_with<P: AsRef<Path>, Q: AsRef<Path>>(
        input_path: P,
        output_dir: Q,
        options: &ProcessingOptions,
        watermark: Option<&PreparedWatermark>,
    ) -> Result<ProcessingResult, ProcessError> {
        let input_path = input_path.as_ref();
        let output_dir = output_dir.as_ref();

        let original_size = std::fs::metadata(input_path)
            .map_err(|e| ProcessError::ReadError(e.to_string()))?
            .len();

        // Full pipeline, encoded in memory: the encoded size is the real
        // output size, so "only save when smaller" is exact, not estimated.
        let (mut encoded, width, height) = Self::render(input_path, options, watermark)?;

        // Re-attach source EXIF/ICC when the user asks and the output is JPEG.
        // Best-effort: any failure keeps the re-encoded bytes untouched.
        if options.output.keep_metadata && options.output.format == OutputFormat::Jpeg {
            encoded = reattach_jpeg_metadata(input_path, &encoded).unwrap_or(encoded);
        }

        if options.output.skip_if_larger && encoded.len() as u64 >= original_size {
            return Ok(Self::skipped_result(input_path, original_size));
        }

        let ext = options.output.format.extension();
        let filename = Self::output_filename(
            input_path,
            width,
            height,
            ext,
            &options.output.filename_pattern,
        );
        let output_path =
            Self::resolve_output_path(output_dir, &filename, options.output.conflict_policy);

        if options.output.conflict_policy == ConflictPolicy::Skip && output_path.exists() {
            return Ok(Self::skipped_result(input_path, original_size));
        }

        std::fs::write(&output_path, &encoded)
            .map_err(|e| ProcessError::WriteError(e.to_string()))?;

        let output_size = std::fs::metadata(&output_path)
            .map_err(|e| ProcessError::WriteError(e.to_string()))?
            .len();

        let reduction_percent = if original_size > 0 {
            ((original_size as f64 - output_size as f64) / original_size as f64) * 100.0
        } else {
            0.0
        };

        // Target-size mode only applies to JPEG; for every other format the
        // option is ignored, so "within target" is trivially satisfied.
        let within_target = match (options.output.target_size_bytes, options.output.format) {
            (Some(limit), OutputFormat::Jpeg) if limit > 0 => encoded.len() as u64 <= limit,
            _ => true,
        };

        Ok(ProcessingResult {
            original_path: input_path.to_string_lossy().to_string(),
            output_path: output_path.to_string_lossy().to_string(),
            original_size,
            output_size,
            reduction_percent,
            success: true,
            error: None,
            skipped: false,
            within_target,
        })
    }

    /// Decode, then run the whole transformation pipeline.
    fn render(
        input_path: &Path,
        options: &ProcessingOptions,
        watermark: Option<&PreparedWatermark>,
    ) -> Result<(Vec<u8>, u32, u32), ProcessError> {
        // Decompression-bomb guard: probe the header before any decoder
        // allocates pixel buffers. Covers the animated GIF and the static
        // path alike; headers that cannot be probed fail later with the
        // decoder's own (more specific) error.
        if let Some((w, h)) = Self::probe_dimensions(input_path) {
            Self::check_pixel_cap(w as u64, h as u64).map_err(ProcessError::ReadError)?;
        }

        // Animated path: GIF -> GIF keeps every frame (Imagine/Caesium lose
        // animation; we do not).
        if options.output.preserve_animation && options.output.format == OutputFormat::Gif {
            if let Ok(frames) = Self::decode_gif_frames(input_path) {
                if frames.len() > 1 {
                    let mut out_frames = Vec::with_capacity(frames.len());
                    for f in frames {
                        let img = DynamicImage::ImageRgba8(f.buffer().clone());
                        let img = Self::apply_pipeline(img, options, watermark);
                        out_frames.push(Frame::from_parts(
                            img.to_rgba8(),
                            f.left(),
                            f.top(),
                            f.delay(),
                        ));
                    }
                    let (w, h) = (
                        out_frames[0].buffer().width(),
                        out_frames[0].buffer().height(),
                    );
                    let bytes = Self::encode_frames_to_memory(out_frames)?;
                    return Ok((bytes, w, h));
                }
            }
        }

        // Static path. `with_guessed_format` sniffs magic bytes, so a lying
        // extension no longer breaks decoding. We go through `into_decoder`
        // (instead of `decode`) so the EXIF orientation can be read from the
        // header first: the `image` crate encoders drop all metadata, so a
        // phone photo would otherwise come out sideways with no tag left to
        // correct it. Formats without orientation (PNG/GIF/WebP) simply
        // return `NoTransforms`.
        let reader = ImageReader::open(input_path)
            .map_err(|e| ProcessError::ReadError(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| ProcessError::ReadError(e.to_string()))?;
        let img = Self::decode_reader_oriented(reader)?;

        let img = Self::apply_pipeline(img, options, watermark);
        let (width, height) = (img.width(), img.height());
        let bytes = Self::encode_to_memory(&img, options)?;
        Ok((bytes, width, height))
    }

    /// Decompressed-pixel ceiling: images whose header claims more pixels are
    /// refused before any decoder allocates. 256 MP ≈ 1 GiB of RGBA8 — far
    /// above any real photograph, far below out-of-memory territory.
    const MAX_DECODED_PIXELS: u64 = 256 * 1024 * 1024;

    /// Upper bound for a single resize target dimension. Beyond this the
    /// output is no longer a photograph; a typo'd `999999999` in the UI (or a
    /// hostile IPC payload) must not turn into a gigantic pixel allocation.
    const MAX_RESIZE_DIMENSION: u32 = 65_535;

    /// Upper bound for total output pixels, same envelope as the decode cap.
    /// The resizer allocates the target buffer before sampling, so an output
    /// bomb needs its own guard — the decompression-bomb ceiling only covers
    /// the *decode* side.
    const MAX_RESIZE_PIXELS: u64 = Self::MAX_DECODED_PIXELS;

    /// Resize-percent envelope, enforced at the IPC boundary (`From` impl for
    /// the payload) and again in [`Self::apply_resize`].
    const RESIZE_PERCENT_MIN: f64 = 1.0;
    const RESIZE_PERCENT_MAX: f64 = 1000.0;

    /// Clamp a resize target to [`Self::MAX_RESIZE_DIMENSION`] per side and
    /// [`Self::MAX_RESIZE_PIXELS`] in total (scaled down proportionally when
    /// the box would exceed the pixel budget). Zero dimensions are raised to
    /// 1 so the encoder never sees an empty image.
    fn clamp_resize_target(mut w: u32, mut h: u32) -> (u32, u32) {
        w = w.clamp(1, Self::MAX_RESIZE_DIMENSION);
        h = h.clamp(1, Self::MAX_RESIZE_DIMENSION);
        let pixels = u64::from(w) * u64::from(h);
        if pixels > Self::MAX_RESIZE_PIXELS {
            let scale = ((Self::MAX_RESIZE_PIXELS as f64) / (pixels as f64)).sqrt();
            w = ((w as f64 * scale).floor() as u32).max(1);
            h = ((h as f64 * scale).floor() as u32).max(1);
        }
        (w, h)
    }

    /// Header-only dimension probe (`None` when the file cannot be sniffed).
    ///
    /// Used by the batch memory clamp and the decompression-bomb guard; it
    /// never decodes pixels.
    pub(crate) fn probe_dimensions(path: &Path) -> Option<(u32, u32)> {
        ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    }

    /// Refuse decompression bombs before the decoder allocates pixel buffers.
    fn check_pixel_cap(w: u64, h: u64) -> Result<(), String> {
        let pixels = w.saturating_mul(h);
        if pixels > Self::MAX_DECODED_PIXELS {
            return Err(format!(
                "image too large: {}x{} ({} MP) exceeds the {} MP decode limit",
                w,
                h,
                pixels / 1_000_000,
                Self::MAX_DECODED_PIXELS / 1_000_000
            ));
        }
        Ok(())
    }

    /// Decode an `ImageReader` and bake the EXIF orientation into the pixels.
    fn decode_reader_oriented<R: std::io::BufRead + std::io::Seek>(
        reader: ImageReader<R>,
    ) -> Result<DynamicImage, ProcessError> {
        let mut decoder = reader
            .into_decoder()
            .map_err(|e| ProcessError::ReadError(e.to_string()))?;
        let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
        let mut img = DynamicImage::from_decoder(decoder)
            .map_err(|e| ProcessError::ReadError(e.to_string()))?;
        img.apply_orientation(orientation);
        Ok(img)
    }

    /// Decode an in-memory image and bake in its EXIF orientation (if any).
    ///
    /// Used by the preview commands so that what the user sees matches what
    /// the batch pipeline will write: both apply orientation before the
    /// transformation pipeline runs.
    pub(crate) fn decode_bytes_with_orientation(bytes: &[u8]) -> Result<DynamicImage, String> {
        // Decompression-bomb guard: the header is parsed twice (probe +
        // decode), which is far cheaper than decoding a bomb's pixels.
        let (w, h) = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| format!("Failed to read image header: {}", e))?
            .into_dimensions()
            .map_err(|e| format!("Failed to read image header: {}", e))?;
        Self::check_pixel_cap(w as u64, h as u64)?;

        let reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| format!("Failed to read image header: {}", e))?;
        Self::decode_reader_oriented(reader).map_err(|e| e.to_string())
    }

    /// Fixed pipeline: resize -> rotate -> flip -> watermark -> quantize.
    pub(crate) fn apply_pipeline(
        img: DynamicImage,
        options: &ProcessingOptions,
        watermark: Option<&PreparedWatermark>,
    ) -> DynamicImage {
        // Geometry first (at full resolution, before any resampling): the
        // interactive drag-crop, then the ratio crop, then fine rotation with
        // canvas expansion. The 90°-step rotate and flips run after resize,
        // as before.
        let img = Self::apply_drag_crop(img, options);
        let img = Self::apply_edit_crop(img, options);
        let img = Self::apply_fine_rotation(img, options);
        let img = Self::apply_resize(img, options);
        let img = Self::apply_rotation(img, options);
        let img = Self::apply_flip(img, options);
        // Color/style edits before the watermark so stamp pixels are never
        // re-tinted by the user's adjustments.
        let img = Self::apply_color_edit(img, options);
        let img = Self::apply_watermark(img, watermark, options);
        Self::apply_quantize(img, options)
    }

    /// Interactive drag-crop (see [`super::edit::crop_fraction`]).
    fn apply_drag_crop(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        match options.crop.crop_rect {
            Some(rect) if rect.len() == 4 && rect.iter().all(|v| v.is_finite()) => {
                super::edit::crop_fraction(&img, rect)
            }
            _ => img,
        }
    }

    /// Ratio center-crop (see [`super::edit::center_crop_to_ratio`]).
    fn apply_edit_crop(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        match options.crop.crop_ratio {
            Some((rw, rh)) if rw > 0 && rh > 0 => super::edit::center_crop_to_ratio(&img, rw, rh),
            _ => img,
        }
    }

    /// Fine rotation with canvas expansion (see
    /// [`super::edit::rotate_with_expansion`]).
    fn apply_fine_rotation(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        let deg = options.transform.rotate_degrees;
        if !(deg.is_finite() && deg.abs() > f32::EPSILON) {
            return img;
        }
        let deg = deg.clamp(-180.0, 180.0);
        super::edit::rotate_with_expansion(&img, deg as f64)
    }

    /// Tonal and stylistic edits, applied in a fixed order (brightness →
    /// contrast → hue → auto-contrast → grayscale → sepia → invert → sharpen
    /// → blur). A single no-op check keeps untouched batches allocation-free.
    fn apply_color_edit(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        let o = options;
        let none_active = o.adjust.brightness == 0
            && o.adjust.contrast == 0
            && o.adjust.hue == 0
            && o.adjust.sharpen == 0
            && !(o.adjust.blur.is_finite() && o.adjust.blur > 0.0)
            && !o.adjust.grayscale
            && !o.adjust.invert
            && !o.adjust.sepia
            && !o.adjust.auto_contrast;
        if none_active {
            return img;
        }

        // Work in RGBA8 so every op below — and the watermark overlay
        // downstream — sees one consistent pixel type (grayscale()/blur() on a
        // Luma8 source would otherwise leave a mixed-type pipeline). The
        // tonal helpers in `edit` are RGB-only and keep alpha bit-exact.
        let mut img = DynamicImage::ImageRgba8(img.to_rgba8());

        if o.adjust.brightness != 0 {
            let add = (o.adjust.brightness.clamp(-100, 100) as f32 * 2.55).round() as i32;
            img = super::edit::brighten_rgb(&img, add);
        }
        if o.adjust.contrast != 0 {
            img = super::edit::contrast_rgb(&img, o.adjust.contrast.clamp(-100, 100) as f32);
        }
        if o.adjust.hue != 0 {
            img = super::edit::hue_rotate_rgb(&img, o.adjust.hue.clamp(-180, 180) as f32);
        }
        if o.adjust.auto_contrast {
            img = super::edit::auto_contrast_stretch(&img);
        }
        if o.adjust.grayscale {
            // grayscale() yields Luma8; convert back so the downstream
            // watermark overlay keeps its RGBA color compositing.
            img = DynamicImage::ImageRgba8(img.grayscale().to_rgba8());
        }
        if o.adjust.sepia {
            img = super::edit::apply_sepia(&img);
        }
        if o.adjust.invert {
            img = super::edit::invert_rgb(&img);
        }
        if o.adjust.sharpen > 0 {
            let amount = o.adjust.sharpen.clamp(0, 100) as f32;
            let sigma = 0.5 + amount / 100.0 * 2.5;
            img = img.unsharpen(sigma, 2);
        }
        if o.adjust.blur.is_finite() && o.adjust.blur > 0.0 {
            img = img.blur(o.adjust.blur.clamp(0.0, 50.0));
        }
        img
    }

    fn apply_resize(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        let filter = image::imageops::FilterType::Lanczos3;

        match options.resize.resize_mode {
            ResizeMode::Percent => {
                if (options.resize.resize_percent - 100.0).abs() < f64::EPSILON {
                    return img;
                }
                // Envelope the factor: a hostile or buggy payload could
                // otherwise ask for a billion-percent upscale.
                let factor = options
                    .resize
                    .resize_percent
                    .clamp(Self::RESIZE_PERCENT_MIN, Self::RESIZE_PERCENT_MAX)
                    / 100.0;
                let w = ((img.width() as f64 * factor).round() as u32).max(1);
                let h = ((img.height() as f64 * factor).round() as u32).max(1);
                let (w, h) = Self::clamp_resize_target(w, h);
                img.resize_exact(w, h, filter)
            }
            ResizeMode::Pixels => match (options.resize.width, options.resize.height) {
                (None, None) => img,
                (Some(w), Some(h)) => {
                    // Guard against a zero dimension (the percent branch already
                    // uses `.max(1)`); `resize_exact(0, 0, ..)` is undefined for
                    // the image crate and must never reach the encoder. The
                    // clamp also bounds hostile/oversized targets (see
                    // `clamp_resize_target`).
                    let (w, h) = Self::clamp_resize_target(w, h);
                    match options.resize.fit_mode {
                        FitMode::Stretch => img.resize_exact(w, h, filter),
                        FitMode::Contain => img.resize(w, h, filter),
                        FitMode::Cover => img.resize_to_fill(w, h, filter),
                    }
                }
                (Some(w), None) => {
                    if w == img.width() {
                        return img;
                    }
                    let ratio = w as f64 / img.width() as f64;
                    let h = ((img.height() as f64 * ratio).round() as u32).max(1);
                    let (w, h) = Self::clamp_resize_target(w, h);
                    img.resize_exact(w, h, filter)
                }
                (None, Some(h)) => {
                    if h == img.height() {
                        return img;
                    }
                    let ratio = h as f64 / img.height() as f64;
                    let w = ((img.width() as f64 * ratio).round() as u32).max(1);
                    let (w, h) = Self::clamp_resize_target(w, h);
                    img.resize_exact(w, h, filter)
                }
            },
        }
    }

    fn apply_rotation(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        match options.transform.rotate {
            90 => img.rotate90(),
            180 => img.rotate180(),
            270 => img.rotate270(),
            _ => img,
        }
    }

    fn apply_flip(mut img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        if options.transform.flip_horizontal {
            img = img.fliph();
        }
        if options.transform.flip_vertical {
            img = img.flipv();
        }
        img
    }

    fn apply_watermark(
        img: DynamicImage,
        prepared: Option<&PreparedWatermark>,
        options: &ProcessingOptions,
    ) -> DynamicImage {
        let Some(cfg) = &options.watermark else {
            return img;
        };
        let Some(wm) = prepared else {
            return img;
        };
        // Clamp to the documented 0..=100 range. The frontend sliders bound
        // this, but `ProcessingOptions` is deserialized straight from the IPC
        // payload, so a malicious or buggy caller could pass e.g. 255 and
        // produce an out-of-range alpha factor.
        let opacity = cfg.opacity().clamp(0, 100);
        if opacity == 0 {
            return img;
        }

        // Build the overlay: image watermarks scale relative to the base
        // width; text stamps are used at their rasterized size.
        let mut overlay: RgbaImage = match (wm, cfg) {
            (PreparedWatermark::Image(w), WatermarkConfig::Image { scale_percent, .. }) => {
                // Keep scale in a sane 1..=100% band so the watermark can never
                // be sized to zero or blown up without bound.
                let scale = (*scale_percent).clamp(1, 100);
                let target_w =
                    (((img.width() as f64) * (scale as f64 / 100.0)).round() as u32).max(1);
                let scaled = if target_w != w.width() {
                    let ratio = target_w as f64 / w.width() as f64;
                    let target_h = ((w.height() as f64 * ratio).round() as u32).max(1);
                    w.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3)
                } else {
                    w.clone()
                };
                scaled.to_rgba8()
            }
            (PreparedWatermark::Text(stamp), WatermarkConfig::Text { .. }) => stamp.clone(),
            // Kind mismatch (cannot happen with prepare_watermark): skip.
            _ => return img,
        };

        // Pre-multiply the configured opacity into the alpha channel, then a
        // plain alpha overlay does the rest.
        let factor = opacity as f32 / 100.0;
        for px in overlay.pixels_mut() {
            px.0[3] = ((px.0[3] as f32) * factor).round() as u8;
        }

        let margin = ((img.width().min(img.height()) as f64)
            * (cfg.margin_percent().clamp(0, 100) as f64 / 100.0))
            .round() as i64;
        let (x, y) = Self::watermark_coords(
            cfg.position(),
            img.dimensions(),
            overlay.dimensions(),
            margin,
        );

        let mut out = img;
        image::imageops::overlay(&mut out, &overlay, x, y);
        out
    }

    fn watermark_coords(
        position: WatermarkPosition,
        (bw, bh): (u32, u32),
        (ww, wh): (u32, u32),
        margin: i64,
    ) -> (i64, i64) {
        let (bw, bh) = (bw as i64, bh as i64);
        let (ww, wh) = (ww as i64, wh as i64);

        let x = match position {
            WatermarkPosition::TopLeft | WatermarkPosition::BottomLeft => margin,
            WatermarkPosition::TopRight | WatermarkPosition::BottomRight => bw - ww - margin,
            WatermarkPosition::Center => (bw - ww) / 2,
        };
        let y = match position {
            WatermarkPosition::TopLeft | WatermarkPosition::TopRight => margin,
            WatermarkPosition::BottomLeft | WatermarkPosition::BottomRight => bh - wh - margin,
            WatermarkPosition::Center => (bh - wh) / 2,
        };
        (x.max(0), y.max(0))
    }

    /// NeuQuant color reduction for PNG/GIF output (Imagine-style).
    fn apply_quantize(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        let Some(n) = options.output.quantize_colors else {
            return img;
        };
        if !matches!(options.output.format, OutputFormat::Png | OutputFormat::Gif) {
            return img;
        }

        let n = (n as usize).clamp(2, 256);
        let mut rgba = img.to_rgba8();
        {
            let flat: &[u8] = rgba.as_raw();
            let nq = NeuQuant::new(10, n, flat);
            for px in rgba.pixels_mut() {
                nq.map_pixel(&mut px.0);
            }
        }
        DynamicImage::ImageRgba8(rgba)
    }

    /// Encode a baseline RGB JPEG at a fixed quality.
    fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, ProcessError> {
        let mut cursor = Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
        img.to_rgb8()
            .write_with_encoder(encoder)
            .map_err(|e| ProcessError::WriteError(e.to_string()))?;
        Ok(cursor.into_inner())
    }

    /// Bisect JPEG quality so the encoded size fits `limit`, keeping quality as
    /// high as possible. Returns `(bytes, quality_used, within_limit)`.
    fn encode_jpeg_to_target(
        img: &DynamicImage,
        limit: u64,
    ) -> Result<(Vec<u8>, u8, bool), ProcessError> {
        // Bounds for the search: below MIN the artifacts are too visible for
        // ID photos; above MAX we waste bytes with no perceptual gain.
        const MIN_SEARCH_QUALITY: u8 = 30;
        const MAX_SEARCH_QUALITY: u8 = 95;

        let limit = limit as usize;
        let (mut lo, mut hi) = (MIN_SEARCH_QUALITY, MAX_SEARCH_QUALITY);
        let mut best: Option<(Vec<u8>, u8)> = None;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let encoded = Self::encode_jpeg(img, mid)?;
            if encoded.len() <= limit {
                best = Some((encoded, mid));
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        match best {
            Some((bytes, q)) => Ok((bytes, q, true)),
            None => {
                // Even the floor quality is too big: ship the best-effort file
                // and flag it so the caller can warn the user.
                let bytes = Self::encode_jpeg(img, MIN_SEARCH_QUALITY)?;
                Ok((bytes, MIN_SEARCH_QUALITY, false))
            }
        }
    }

    /// Encode into memory. Used both for writing files and for the preview's
    /// exact size estimation.
    pub(crate) fn encode_to_memory(
        img: &DynamicImage,
        options: &ProcessingOptions,
    ) -> Result<Vec<u8>, ProcessError> {
        match options.output.format {
            OutputFormat::Jpeg => {
                // Target-size mode supersedes the fixed quality slider.
                if let Some(limit) = options.output.target_size_bytes {
                    if limit > 0 {
                        let (bytes, _q, _within) = Self::encode_jpeg_to_target(img, limit)?;
                        return Ok(bytes);
                    }
                }
                let mut cursor = Cursor::new(Vec::new());
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut cursor,
                    options.output.quality,
                );
                img.to_rgb8()
                    .write_with_encoder(encoder)
                    .map_err(|e| ProcessError::WriteError(e.to_string()))?;
                Ok(cursor.into_inner())
            }
            OutputFormat::Png => Self::encode_native(img, ImageFormat::Png),
            OutputFormat::Gif => Self::encode_native(img, ImageFormat::Gif),
            OutputFormat::Bmp => Self::encode_native(img, ImageFormat::Bmp),
            OutputFormat::Tiff => Self::encode_native(img, ImageFormat::Tiff),
            OutputFormat::WebP => {
                let rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();
                let encoded = if options.output.compression == CompressionType::Lossless {
                    webp::Encoder::from_rgba(&rgba, width, height).encode_lossless()
                } else {
                    webp::Encoder::from_rgba(&rgba, width, height)
                        .encode(options.output.quality as f32)
                };
                Ok(encoded.to_vec())
            }
        }
    }

    fn encode_native(img: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, ProcessError> {
        let mut cursor = Cursor::new(Vec::new());
        img.write_to(&mut cursor, format)
            .map_err(|e| ProcessError::WriteError(e.to_string()))?;
        Ok(cursor.into_inner())
    }

    fn encode_frames_to_memory(frames: Vec<Frame>) -> Result<Vec<u8>, ProcessError> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut encoder = GifEncoder::new(&mut cursor);
            encoder
                .encode_frames(frames)
                .map_err(|e| ProcessError::WriteError(e.to_string()))?;
        }
        Ok(cursor.into_inner())
    }

    fn decode_gif_frames(path: &Path) -> Result<Vec<Frame>, ProcessError> {
        let file = std::fs::File::open(path).map_err(|e| ProcessError::ReadError(e.to_string()))?;
        let decoder = GifDecoder::new(BufReader::new(file))
            .map_err(|e| ProcessError::ReadError(e.to_string()))?;
        decoder
            .into_frames()
            .collect::<image::ImageResult<Vec<Frame>>>()
            .map_err(|e| ProcessError::ReadError(e.to_string()))
    }

    /// Prepare the watermark once per call/batch: decode the picture or
    /// rasterize the text into a stamp. Missing files and empty text degrade
    /// gracefully to "no watermark" instead of failing the whole batch.
    pub(crate) fn prepare_watermark(options: &ProcessingOptions) -> Option<PreparedWatermark> {
        match options.watermark.as_ref()? {
            WatermarkConfig::Image { path, .. } => {
                let path = path.trim();
                if path.is_empty() {
                    return None;
                }
                let bytes = std::fs::read(path).ok()?;
                let img = image::load_from_memory(&bytes).ok()?;
                Some(PreparedWatermark::Image(img))
            }
            WatermarkConfig::Text {
                text,
                font_size,
                color,
                ..
            } => render_text_stamp(text, *font_size, color).map(PreparedWatermark::Text),
        }
    }

    /// Expand the filename template. `{width}`/`{height}` refer to the final
    /// (post-resize) dimensions.
    fn output_filename(input: &Path, width: u32, height: u32, ext: &str, pattern: &str) -> String {
        let stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".to_string());

        let name = pattern
            .replace("{name}", &stem)
            .replace("{width}", &width.to_string())
            .replace("{height}", &height.to_string())
            .replace("{format}", ext);

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

    fn skipped_result(input: &Path, original_size: u64) -> ProcessingResult {
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

/// Re-embed source JPEG metadata (EXIF + ICC) into a freshly encoded JPEG.
///
/// The `image` crate encoders drop every marker, so a re-encode otherwise
/// strips the camera/date/color profile that a photographer may rely on. We
/// re-attach them on a best-effort basis: any failure (no EXIF, undecodable
/// ICC, parse error) yields `None` and the caller keeps the encoded bytes, so
/// metadata preservation can never produce a broken file.
///
/// EXIF orientation is forced to 1 because [`ImageProcessor::render`] already
/// bakes the *real* orientation into the pixels; leaving the original tag would
/// make a viewer rotate the already-rotated image a second time.
fn reattach_jpeg_metadata(source: &Path, out: &[u8]) -> Option<Vec<u8>> {
    if out.len() < 2 || out[0] != 0xFF || out[1] != 0xD8 {
        return None; // not a JPEG
    }
    let source_bytes = read_metadata_prefix(source)?;

    // EXIF (orientation normalized to 1) — only when we can both read and
    // rewrite it, so we never ship a tag that double-rotates the image.
    let exif_tiff = read_exif_tiff_with_normalized_orientation(&source_bytes);
    // ICC profile: re-embed the raw APP2 payload verbatim (keeps the "ICC"
    // identifier and chunk sequence the decoders expect).
    let icc_payload = find_jpeg_app2_icc(&source_bytes);

    let exif_app1 = exif_tiff.map(|tiff| build_app1_exif(&tiff));
    let icc_app2 = icc_payload.map(|p| build_app2_icc(&p));

    if exif_app1.is_none() && icc_app2.is_none() {
        return None;
    }

    let mut result = Vec::with_capacity(out.len() + 4096);
    result.extend_from_slice(&out[0..2]); // SOI
    if let Some(seg) = exif_app1 {
        result.extend_from_slice(&seg);
    }
    if let Some(seg) = icc_app2 {
        result.extend_from_slice(&seg);
    }
    result.extend_from_slice(&out[2..]); // remainder of the encoded stream
    Some(result)
}

/// At most this many bytes are read from the source when re-attaching JPEG
/// metadata. EXIF (APP1) and ICC (APP2) segments live before the first scan
/// (SOS) marker — near the start of the file — so re-attachment never needs
/// the whole (possibly tens-of-MB) source photo the decoder just streamed.
const METADATA_PREFIX_BYTES: u64 = 8 * 1024 * 1024;

/// Read at most [`METADATA_PREFIX_BYTES`] bytes from the start of `path`.
/// A segment that (absurdly) extends past the cap fails to parse downstream
/// and is then simply omitted — re-attachment stays best-effort either way.
fn read_metadata_prefix(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let cap = file.metadata().ok()?.len().min(METADATA_PREFIX_BYTES);
    let mut buf = Vec::with_capacity(cap as usize);
    file.take(cap).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Read EXIF from a JPEG and return the TIFF blob with Orientation forced to 1.
///
/// Returns `None` when there is no EXIF at all, or when the blob cannot be
/// parsed/re-serialized (so the caller simply omits the EXIF segment rather
/// than risking a malformed one).
fn read_exif_tiff_with_normalized_orientation(jpeg: &[u8]) -> Option<Vec<u8>> {
    let reader = ExifReader::new();
    let exif = reader.read_from_container(&mut Cursor::new(jpeg)).ok()?;

    // Rebuild owned fields, forcing Orientation to 1 (normal). The pipeline has
    // already baked the real rotation into the pixels, so keeping the original
    // tag would double-rotate the image in an EXIF-aware viewer. `Field` is a
    // public struct literal (tag / ifd_num / value), so we clone every value
    // and swap only the Orientation one.
    let fields: Vec<exif::Field> = exif
        .fields()
        .map(|f| {
            if f.tag == Tag::Orientation {
                exif::Field {
                    tag: Tag::Orientation,
                    ifd_num: f.ifd_num,
                    value: Value::Short(vec![1u16]),
                }
            } else {
                exif::Field {
                    tag: f.tag,
                    ifd_num: f.ifd_num,
                    value: f.value.clone(),
                }
            }
        })
        .collect();
    if fields.is_empty() {
        return None;
    }

    let mut writer = Writer::new();
    for f in &fields {
        writer.push_field(f);
    }
    let mut cursor = Cursor::new(Vec::new());
    writer.write(&mut cursor, exif.little_endian()).ok()?;
    Some(cursor.into_inner())
}

/// Locate the ICC color-profile APP2 payload inside a JPEG. The returned slice
/// is the segment data (after the 2-byte length), still carrying the leading
/// `"ICC"` identifier and chunk sequence byte the decoders expect.
fn find_jpeg_app2_icc(jpeg: &[u8]) -> Option<Vec<u8>> {
    if !(jpeg.len() >= 2 && jpeg[0] == 0xFF && jpeg[1] == 0xD8) {
        return None;
    }
    let mut i = 2usize;
    while i + 4 < jpeg.len() {
        if jpeg[i] != 0xFF {
            break;
        }
        let marker = jpeg[i + 1];
        i += 2;
        // Standalone markers carry no length payload.
        if marker == 0xD9 || marker == 0xDA || (0xD0..=0xD7).contains(&marker) {
            if marker == 0xDA {
                break; // SOS: compressed image data follows, stop scanning.
            }
            continue;
        }
        if i + 2 > jpeg.len() {
            break;
        }
        let len = u16::from_be_bytes([jpeg[i], jpeg[i + 1]]) as usize;
        if len < 2 || i + len > jpeg.len() {
            break;
        }
        let data = &jpeg[i + 2..i + len];
        if marker == 0xE2 && data.len() >= 4 && &data[0..4] == b"ICC" {
            return Some(data.to_vec());
        }
        i += len;
    }
    None
}

/// Wrap an EXIF TIFF blob in an APP1 segment: `FF E1 <len> "Exif\0\0" <tiff>`.
fn build_app1_exif(tiff: &[u8]) -> Vec<u8> {
    let mut seg = Vec::with_capacity(8 + tiff.len());
    seg.push(0xFF);
    seg.push(0xE1);
    let len = (2 + 6 + tiff.len()) as u16; // length field counts itself + payload
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(tiff);
    seg
}

/// Wrap an ICC profile payload in an APP2 segment: `FF E2 <len> <payload>`.
fn build_app2_icc(payload: &[u8]) -> Vec<u8> {
    let mut seg = Vec::with_capacity(4 + payload.len());
    seg.push(0xFF);
    seg.push(0xE2);
    let len = (2 + payload.len()) as u16;
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(payload);
    seg
}

/// `true` if the name's device part collides with a Windows reserved device
/// name. Windows refuses to create regular files named CON, PRN, AUX, NUL,
/// COM1-9 or LPT1-9 even when they carry an extension.
fn is_windows_reserved_device_name(name: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let device = name.split('.').next().unwrap_or(name);
    RESERVED.contains(&device.to_ascii_uppercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Delay, ImageEncoder, Rgba, RgbaImage};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clt-proc-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn pipeline_applies_crop_ratio_and_sepia() {
        // 100x50 white source, ratio-cropped to 1:1 → 50x50; sepia turns
        // white (255,255,255) into (255, 255, 0.937·255=239).
        let img =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(100, 50, Rgba([255, 255, 255, 255])));
        let opts = ProcessingOptions {
            crop: CropOptions {
                crop_ratio: Some((1, 1)),
                ..Default::default()
            },
            adjust: AdjustOptions {
                sepia: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let out = ImageProcessor::apply_pipeline(img, &opts, None);
        assert_eq!((out.width(), out.height()), (50, 50));
        assert_eq!(out.get_pixel(25, 25).0, [255, 255, 239, 255]);
    }

    #[test]
    fn pipeline_brightness_shifts_black_and_identity_when_off() {
        // brightness 50 → +127.5 → round 128 per channel on black.
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255])));
        let opts = ProcessingOptions {
            adjust: AdjustOptions {
                brightness: 50,
                ..Default::default()
            },
            ..Default::default()
        };
        let out = ImageProcessor::apply_pipeline(img, &opts, None);
        assert_eq!(out.get_pixel(0, 0).0, [128, 128, 128, 255]);

        // All edit fields at their defaults → byte-identical passthrough.
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([7, 42, 200, 255])));
        let out = ImageProcessor::apply_pipeline(img.clone(), &ProcessingOptions::default(), None);
        assert_eq!(out.dimensions(), img.dimensions());
        assert_eq!(out.get_pixel(3, 3).0, [7, 42, 200, 255]);
    }

    #[test]
    fn windows_reserved_output_names_get_an_suffix() {
        // `NUL.png` is the NUL device on Windows; the guard must rename it.
        let reserved = ImageProcessor::output_filename(Path::new("NUL.png"), 8, 8, "png", "{name}");
        assert_eq!(reserved, "NUL_.png");

        // Case-insensitive.
        let lowercase =
            ImageProcessor::output_filename(Path::new("con.jpg"), 8, 8, "png", "{name}");
        assert_eq!(lowercase, "con_.png");

        // Normal names and names that merely CONTAIN a reserved word pass through.
        let normal = ImageProcessor::output_filename(Path::new("cat.png"), 8, 8, "png", "{name}");
        assert_eq!(normal, "cat.png");
        let containing =
            ImageProcessor::output_filename(Path::new("console.png"), 8, 8, "png", "{name}");
        assert_eq!(containing, "console.png");
    }

    fn solid_img(w: u32, h: u32, color: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, Rgba(color)))
    }

    fn write_png(img: &DynamicImage, path: &Path) {
        img.save_with_format(path, ImageFormat::Png)
            .expect("save png");
    }

    #[test]
    fn decode_bytes_with_orientation_passes_pixels_through() {
        // Glue test for the EXIF-orientation decode used by the preview
        // commands: an image without orientation metadata must come out
        // unchanged. (JPEG/TIFF/WebP carry real EXIF; hand-crafting one
        // byte-for-byte would be brittle, and the transform table itself
        // lives in the `image` crate. The full `render()` path — which also
        // goes through the oriented decoder — is covered by every process
        // test below.)
        let dir = temp_dir("decode-oriented");
        let path = dir.join("a.png");
        write_png(&solid_img(10, 6, [1, 2, 3, 255]), &path);
        let bytes = std::fs::read(&path).expect("read back png");

        let decoded = ImageProcessor::decode_bytes_with_orientation(&bytes)
            .expect("in-memory decode must succeed");
        assert_eq!((decoded.width(), decoded.height()), (10, 6));

        // Garbage bytes must fail with an error, not panic.
        assert!(ImageProcessor::decode_bytes_with_orientation(b"not an image").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_process_error_messages() {
        assert_eq!(
            ProcessError::ReadError("a.png".into()).to_string(),
            "Failed to read image: a.png"
        );
        assert_eq!(
            ProcessError::WriteError("b.png".into()).to_string(),
            "Failed to write image: b.png"
        );
    }

    #[test]
    fn pixel_cap_rejects_bomb_headers() {
        assert!(ImageProcessor::check_pixel_cap(100, 100).is_ok());
        // 64 MP: real-world large, still allowed.
        assert!(ImageProcessor::check_pixel_cap(8_000, 8_000).is_ok());
        // 400 MP: beyond the 256 MP ceiling.
        let err = ImageProcessor::check_pixel_cap(20_000, 20_000).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn decode_bytes_rejects_decompression_bombs() {
        // A BMP header claiming 40000x40000 (1.6 GP): the guard must refuse
        // it before the decoder allocates ~6 GiB of pixels.
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&0u32.to_le_bytes()); // file size (not used by the probe)
        bmp.extend_from_slice(&0u16.to_le_bytes()); // reserved
        bmp.extend_from_slice(&0u16.to_le_bytes());
        bmp.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
        bmp.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER size
        bmp.extend_from_slice(&40_000u32.to_le_bytes()); // width
        bmp.extend_from_slice(&40_000u32.to_le_bytes()); // height
        bmp.extend_from_slice(&1u16.to_le_bytes()); // planes
        bmp.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
        bmp.extend_from_slice(&0u32.to_le_bytes()); // compression = BI_RGB
        bmp.extend_from_slice(&0u32.to_le_bytes()); // image size
        bmp.extend_from_slice(&0u32.to_le_bytes()); // x pels per meter
        bmp.extend_from_slice(&0u32.to_le_bytes()); // y pels per meter
        bmp.extend_from_slice(&0u32.to_le_bytes()); // colors used
        bmp.extend_from_slice(&0u32.to_le_bytes()); // important colors

        let err = ImageProcessor::decode_bytes_with_orientation(&bmp)
            .expect_err("a 1.6 GP header must be refused by the decode guard");
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn percent_resize_keeps_aspect_ratio() {
        let opts = ProcessingOptions {
            resize: ResizeOptions {
                resize_mode: ResizeMode::Percent,
                resize_percent: 50.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let out = ImageProcessor::apply_resize(solid_img(100, 50, [1, 2, 3, 255]), &opts);
        assert_eq!((out.width(), out.height()), (50, 25));

        let opts = ProcessingOptions {
            resize: ResizeOptions {
                resize_mode: ResizeMode::Percent,
                resize_percent: 200.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let out = ImageProcessor::apply_resize(solid_img(100, 50, [1, 2, 3, 255]), &opts);
        assert_eq!((out.width(), out.height()), (200, 100));
    }

    #[test]
    fn fit_modes_differ_for_square_box() {
        let base = || solid_img(100, 50, [9, 9, 9, 255]);
        let stretch = ProcessingOptions {
            resize: ResizeOptions {
                width: Some(50),
                height: Some(50),
                fit_mode: FitMode::Stretch,
                ..Default::default()
            },
            ..Default::default()
        };
        let contain = ProcessingOptions {
            resize: ResizeOptions {
                fit_mode: FitMode::Contain,
                ..stretch.resize.clone()
            },
            ..stretch.clone()
        };
        let cover = ProcessingOptions {
            resize: ResizeOptions {
                fit_mode: FitMode::Cover,
                ..stretch.resize.clone()
            },
            ..stretch.clone()
        };

        assert_eq!(
            ImageProcessor::apply_resize(base(), &stretch).dimensions(),
            (50, 50)
        );
        let c = ImageProcessor::apply_resize(base(), &contain);
        assert_eq!(c.dimensions(), (50, 25));
        assert_eq!(
            ImageProcessor::apply_resize(base(), &cover).dimensions(),
            (50, 50)
        );
    }

    #[test]
    fn rotate90_is_clockwise() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        let opts = ProcessingOptions {
            transform: TransformOptions {
                rotate: 90,
                ..Default::default()
            },
            ..Default::default()
        };
        let out = ImageProcessor::apply_rotation(DynamicImage::ImageRgba8(img), &opts);
        assert_eq!(out.dimensions(), (1, 2));
        assert_eq!(out.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(out.get_pixel(0, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn flip_swaps_pixels() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        let opts = ProcessingOptions {
            transform: TransformOptions {
                flip_horizontal: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let out = ImageProcessor::apply_flip(DynamicImage::ImageRgba8(img), &opts);
        assert_eq!(out.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(out.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn quantize_reduces_distinct_colors() {
        // A smooth gradient has many colors; 4-color quantization must shrink it.
        let mut img = RgbaImage::new(32, 32);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgba([(x * 8) as u8, (y * 8) as u8, 128, 255]);
        }
        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::Png,
                quantize_colors: Some(4),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = ImageProcessor::apply_quantize(DynamicImage::ImageRgba8(img), &opts);
        let mut colors = std::collections::HashSet::new();
        for px in out.to_rgba8().pixels() {
            colors.insert(px.0);
        }
        assert!(
            colors.len() <= 4,
            "expected <= 4 colors, got {}",
            colors.len()
        );
    }

    #[test]
    fn quantize_ignored_for_non_png_gif_output() {
        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::WebP,
                quantize_colors: Some(4),
                ..Default::default()
            },
            ..Default::default()
        };
        let img = solid_img(8, 8, [1, 2, 3, 255]);
        let out = ImageProcessor::apply_quantize(img, &opts);
        assert_eq!(out.get_pixel(0, 0).0, [1, 2, 3, 255]);
    }

    #[test]
    fn watermark_lands_in_chosen_corner() {
        let wm = PreparedWatermark::Image(solid_img(10, 10, [255, 0, 0, 255]));
        let opts = ProcessingOptions {
            watermark: Some(WatermarkConfig::Image {
                path: "ignored".into(),
                opacity: 100,
                scale_percent: 10, // 100px base -> 10px watermark
                position: WatermarkPosition::BottomRight,
                margin_percent: 0,
            }),
            ..Default::default()
        };
        let out = ImageProcessor::apply_watermark(
            solid_img(100, 100, [255, 255, 255, 255]),
            Some(&wm),
            &opts,
        );
        assert_eq!(out.get_pixel(95, 95).0, [255, 0, 0, 255]);
        // Top-left corner stays untouched.
        assert_eq!(out.get_pixel(5, 5).0, [255, 255, 255, 255]);
    }

    #[test]
    fn watermark_opacity_blends() {
        let wm = PreparedWatermark::Image(solid_img(10, 10, [255, 0, 0, 255]));
        let opts = ProcessingOptions {
            watermark: Some(WatermarkConfig::Image {
                path: "ignored".into(),
                opacity: 50,
                scale_percent: 10,
                position: WatermarkPosition::TopLeft,
                margin_percent: 0,
            }),
            ..Default::default()
        };
        let out = ImageProcessor::apply_watermark(
            solid_img(100, 100, [255, 255, 255, 255]),
            Some(&wm),
            &opts,
        );
        let px = out.get_pixel(5, 5).0;
        // ~50% red over white -> (255, ~127, ~127); keep the exact blend
        // rounding out of the assertion.
        assert_eq!(px[0], 255);
        assert!((120..=135).contains(&px[1]), "g = {}", px[1]);
        assert!((120..=135).contains(&px[2]), "b = {}", px[2]);
    }

    #[test]
    fn text_watermark_renders_and_lands_bottom_right() {
        // Embedded Smiley Sans must rasterize CJK + Latin.
        let stamp = render_text_stamp("测试水印 Test", 48, "#FF0000")
            .expect("text stamp must render for non-empty text");
        assert!(stamp.width() > 0 && stamp.height() > 0);
        assert!(
            stamp.pixels().any(|px| px.0[3] > 0),
            "stamp must contain ink"
        );

        let wm = PreparedWatermark::Text(stamp);
        let opts = ProcessingOptions {
            watermark: Some(WatermarkConfig::Text {
                text: "测试水印 Test".into(),
                font_size: 48,
                color: "#FF0000".into(),
                opacity: 100,
                position: WatermarkPosition::BottomRight,
                margin_percent: 0,
            }),
            ..Default::default()
        };
        let base = solid_img(400, 200, [255, 255, 255, 255]);
        let out = ImageProcessor::apply_watermark(base, Some(&wm), &opts);
        // Bottom-right corner must carry red ink; top-left stays white.
        assert!(
            out.to_rgba8()
                .pixels()
                .any(|px| px.0[0] == 255 && px.0[1] < 128 && px.0[3] == 255),
            "expected red ink somewhere on the image"
        );
        assert_eq!(out.get_pixel(2, 2).0, [255, 255, 255, 255]);
    }

    #[test]
    fn empty_text_yields_no_watermark() {
        let opts = ProcessingOptions {
            watermark: Some(WatermarkConfig::Text {
                text: "   ".into(),
                font_size: 48,
                color: "#FFFFFF".into(),
                opacity: 100,
                position: WatermarkPosition::Center,
                margin_percent: 3,
            }),
            ..Default::default()
        };
        assert!(ImageProcessor::prepare_watermark(&opts).is_none());
    }

    #[test]
    fn invalid_color_falls_back_to_white() {
        assert_eq!(parse_hex_color("nothex"), (255, 255, 255));
        assert_eq!(parse_hex_color("#00ff00"), (0, 255, 0));
        assert_eq!(parse_hex_color("ff8000"), (255, 128, 0));
    }

    #[test]
    fn multiline_stamp_stacks_lines() {
        let single = render_text_stamp("测试", 48, "#FFFFFF").expect("single line");
        let double = render_text_stamp("测试\n测试", 48, "#FFFFFF").expect("two lines");
        assert!(
            double.height() > single.height(),
            "two lines must be taller than one ({} vs {})",
            double.height(),
            single.height()
        );

        // The widest line defines the trimmed width.
        let wide = render_text_stamp("测试测试", 48, "#FFFFFF").unwrap();
        let stacked = render_text_stamp("测试测试\n测试", 48, "#FFFFFF").unwrap();
        assert_eq!(stacked.width(), wide.width());
    }

    #[test]
    fn multiline_normalizes_crlf_and_keeps_blank_slots() {
        let crlf = render_text_stamp("A\r\nB", 48, "#FFFFFF").unwrap();
        let lf = render_text_stamp("A\nB", 48, "#FFFFFF").unwrap();
        assert_eq!(crlf.dimensions(), lf.dimensions());

        // A blank middle line adds vertical space between the ink lines.
        let tight = render_text_stamp("A\nB", 48, "#FFFFFF").unwrap();
        let spaced = render_text_stamp("A\n\nB", 48, "#FFFFFF").unwrap();
        assert!(spaced.height() > tight.height());

        // Whitespace-only text is still "no watermark".
        assert!(render_text_stamp(" \n  ", 48, "#FFFFFF").is_none());
    }

    #[test]
    fn skip_if_larger_marks_skipped_and_writes_nothing() {
        let dir = temp_dir("skip-larger");
        let input = dir.join("in.png");
        // A tiny solid PNG (~70 bytes); BMP output is far larger.
        write_png(&solid_img(8, 8, [10, 20, 30, 255]), &input);
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::Bmp,
                skip_if_larger: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts).expect("process");
        assert!(
            result.skipped,
            "bmp of a tiny png must be skipped as larger"
        );
        assert!(result.output_path.is_empty());
        let entries: Vec<_> = std::fs::read_dir(&out_dir).unwrap().collect();
        assert!(entries.is_empty(), "nothing must be written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filename_pattern_expands_tokens() {
        let dir = temp_dir("pattern");
        let input = dir.join("photo.png");
        write_png(&solid_img(32, 16, [1, 2, 3, 255]), &input);
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        // The pattern is a stem template; the extension is appended by the
        // processor itself.
        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::WebP,
                filename_pattern: "{name}_{width}x{height}".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts).expect("process");
        let name = Path::new(&result.output_path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(name, "photo_32x16.webp");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_rename_avoids_overwrite() {
        let dir = temp_dir("autorename");
        let input = dir.join("a.png");
        write_png(&solid_img(8, 8, [1, 2, 3, 255]), &input);
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("a.webp"), b"existing").unwrap();

        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::WebP,
                conflict_policy: ConflictPolicy::AutoRename,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts).expect("process");
        assert!(result.output_path.ends_with("a_1.webp"));
        assert!(!result.skipped);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_policy_keeps_existing_file() {
        let dir = temp_dir("skippolicy");
        let input = dir.join("a.png");
        write_png(&solid_img(8, 8, [1, 2, 3, 255]), &input);
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("a.webp"), b"existing").unwrap();

        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::WebP,
                conflict_policy: ConflictPolicy::Skip,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts).expect("process");
        assert!(result.skipped);
        assert_eq!(
            std::fs::read(out_dir.join("a.webp")).unwrap(),
            b"existing".to_vec()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gif_to_gif_preserves_animation() {
        let dir = temp_dir("anim");
        let input = dir.join("anim.gif");

        // Build a 2-frame animated GIF.
        let file = std::fs::File::create(&input).unwrap();
        let mut encoder = GifEncoder::new(file);
        let mut f1 = RgbaImage::new(8, 8);
        f1.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let mut f2 = RgbaImage::new(8, 8);
        f2.put_pixel(0, 0, Rgba([0, 0, 255, 255]));
        encoder
            .encode_frames(vec![
                Frame::from_parts(f1, 0, 0, Delay::from_numer_denom_ms(100, 1000)),
                Frame::from_parts(f2, 0, 0, Delay::from_numer_denom_ms(100, 1000)),
            ])
            .unwrap();
        drop(encoder);

        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::Gif,
                preserve_animation: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts).expect("process");
        assert!(!result.skipped);

        let out_file = std::fs::File::open(&result.output_path).unwrap();
        let decoder = GifDecoder::new(BufReader::new(out_file)).unwrap();
        let frames = decoder
            .into_frames()
            .collect::<image::ImageResult<Vec<_>>>()
            .unwrap();
        assert_eq!(frames.len(), 2, "animation frames must survive");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lying_extension_still_decodes() {
        let dir = temp_dir("liar");
        let input = dir.join("actually_png.jpg"); // PNG bytes, wrong extension
        write_png(&solid_img(8, 8, [7, 7, 7, 255]), &input);
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::WebP,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts)
            .expect("magic-byte sniffing must decode a mislabeled file");
        assert!(!result.skipped);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Target-size mode bisects JPEG quality so the output fits the budget
    /// while keeping quality as high as possible. This is the upload-prep
    /// requirement folded into the normal conversion pipeline.
    #[test]
    fn target_size_bytes_bisects_jpeg_under_limit() {
        let dir = temp_dir("target");
        let input = dir.join("gradient.png");
        // High-entropy gradient so the JPEG size is non-trivial and stable.
        let mut img = RgbaImage::new(800, 1000);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
        DynamicImage::ImageRgba8(img)
            .save_with_format(&input, ImageFormat::Png)
            .unwrap();
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::Jpeg,
                target_size_bytes: Some(50 * 1024),
                ..Default::default()
            },
            resize: ResizeOptions {
                width: Some(196),
                height: Some(250),
                fit_mode: FitMode::Stretch,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts)
            .expect("process with target size");
        assert!(!result.skipped);
        assert!(
            result.output_size <= 50 * 1024,
            "size = {}",
            result.output_size
        );
        assert!(result.within_target, "must report within target");

        // Output is a real baseline JPEG and the right dimensions.
        let bytes = std::fs::read(&result.output_path).unwrap();
        assert_eq!(&bytes[0..3], &[0xFF, 0xD8, 0xFF]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Non-JPEG formats ignore target size, so the file is written normally
    /// and `within_target` stays true (the constraint simply does not apply).
    #[test]
    fn target_size_ignored_for_non_jpeg() {
        let dir = temp_dir("target-png");
        let input = dir.join("a.png");
        write_png(&solid_img(64, 64, [1, 2, 3, 255]), &input);
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let opts = ProcessingOptions {
            output: OutputOptions {
                format: OutputFormat::Png,
                target_size_bytes: Some(1),
                ..Default::default()
            }, // impossible for PNG, must be ignored
            ..Default::default()
        };
        let result = ImageProcessor::process_image(&input, &out_dir, &opts)
            .expect("process png with target size");
        assert!(!result.skipped);
        assert!(result.within_target, "non-jpeg target is not applicable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `keep_metadata` re-attaches the source EXIF with orientation normalized
    /// to 1 (the pipeline already bakes the real rotation into the pixels), and
    /// the re-embedded file must still decode.
    #[test]
    fn keep_metadata_reattaches_exif_with_normalized_orientation() {
        // 1x1 baseline JPEG with no metadata, used both as the "source with
        // injected EXIF" and the freshly encoded output we re-embed into.
        let base = {
            let mut c = Cursor::new(Vec::new());
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut c, 90)
                .write_image(&[200, 100, 50], 1, 1, image::ExtendedColorType::Rgb8)
                .unwrap();
            c.into_inner()
        };
        assert_eq!(&base[0..2], &[0xFF, 0xD8]);

        // Build an EXIF blob that claims orientation = 6 (rotate 90 CW) and
        // inject it as an APP1 segment into the source JPEG.
        let exif_tiff = {
            let field = exif::Field {
                tag: Tag::Orientation,
                ifd_num: exif::In::PRIMARY,
                value: Value::Short(vec![6]),
            };
            let mut writer = Writer::new();
            writer.push_field(&field);
            let mut cur = Cursor::new(Vec::new());
            writer.write(&mut cur, false).unwrap();
            cur.into_inner()
        };
        let app1 = build_app1_exif(&exif_tiff);
        let mut source = Vec::with_capacity(base.len() + app1.len());
        source.extend_from_slice(&base[0..2]);
        source.extend_from_slice(&app1);
        source.extend_from_slice(&base[2..]);

        let dir = temp_dir("exif-meta");
        let source_path = dir.join("with_exif.jpg");
        std::fs::write(&source_path, &source).unwrap();

        // Re-embed metadata from `source` into the plain `base` output.
        let out = reattach_jpeg_metadata(&source_path, &base).expect("must reattach");
        assert_eq!(&out[0..2], &[0xFF, 0xD8]);
        assert_eq!(&out[2..4], &[0xFF, 0xE1], "APP1 must be inserted after SOI");

        // The re-embedded EXIF must now read orientation = 1 (normalized).
        let exif = ExifReader::new()
            .read_from_container(&mut Cursor::new(out.as_slice()))
            .unwrap();
        let orientation = exif
            .fields()
            .find(|f| f.tag == Tag::Orientation)
            .and_then(|f| match &f.value {
                Value::Short(v) => v.first().copied().map(|x| x as u64),
                _ => None,
            })
            .unwrap_or(0);
        assert_eq!(orientation, 1, "orientation must be normalized to 1");

        // And the file must still decode.
        assert!(image::ImageReader::new(Cursor::new(out.clone()))
            .with_guessed_format()
            .unwrap()
            .decode()
            .is_ok());

        // A source with no EXIF/ICC reattaches nothing.
        let plain_path = dir.join("plain.jpg");
        std::fs::write(&plain_path, &base).unwrap();
        assert!(reattach_jpeg_metadata(&plain_path, &base).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- contract guards ---------------------------------------------------
    //
    // `ProcessingOptions` (grouped) + `ProcessingOptionsPayload` (flat) +
    // the `From` conversion + `src/types/index.ts` (frontend) must all name
    // the same option. That is four places to touch per field, and nothing in
    // the type system links them — a field added to the domain model and
    // forgotten in the payload silently deserializes to `None`/`0` at the IPC
    // boundary and the UI control quietly stops working.
    //
    // These tests lock the field *names* down so a rename has to be deliberate.

    /// Option names the frontend sends and the pipeline consumes. Keep in sync
    /// with `ProcessingOptions` in `src/types/index.ts`.
    const FRONTEND_OPTION_FIELDS: &[&str] = &[
        "format",
        "quality",
        "width",
        "height",
        "keep_metadata",
        "compression",
        "resize_mode",
        "resize_percent",
        "fit_mode",
        "rotate",
        "flip_horizontal",
        "flip_vertical",
        "watermark",
        "quantize_colors",
        "filename_pattern",
        "conflict_policy",
        "skip_if_larger",
        "preserve_animation",
        "target_size_bytes",
        "mirror_subdirs",
        "crop_rect",
        "crop_ratio",
        "rotate_degrees",
        "brightness",
        "contrast",
        "hue",
        "sharpen",
        "blur",
        "grayscale",
        "invert",
        "sepia",
        "auto_contrast",
    ];

    #[test]
    fn payload_accepts_exactly_the_frontend_field_set() {
        // Serialize the *flat* payload — that is the shape the frontend
        // actually sends. `ProcessingOptions` (the grouped domain model) is the
        // wrong thing to serialize here: its fields are `output`/`resize`/…,
        // so comparing it against the flat list would pass for entirely the
        // wrong reason.
        //
        // `ProcessingOptionsPayload` has no `Default` and the `From` conversion
        // is one-way (payload -> model), so derive a payload by round-tripping
        // the default model through serde instead.
        let payload: ProcessingOptionsPayload = serde_json::from_value(
            serde_json::to_value(ProcessingOptions::default()).expect("default options serialize"),
        )
        .expect("default options round-trip into a payload");
        let json = serde_json::to_value(&payload)
            .expect("payload serializes")
            .as_object()
            .expect("payload serializes to an object")
            .clone();
        let mut actual: Vec<&str> = json.keys().map(|k| k.as_str()).collect();
        actual.sort_unstable();

        let mut expected: Vec<&str> = FRONTEND_OPTION_FIELDS.to_vec();
        expected.sort_unstable();

        assert_eq!(
            actual, expected,
            "the IPC payload's fields and the documented frontend field set \
             have drifted; a new option needs the group struct, the Payload \
             field, the From conversion and src/types/index.ts all updated"
        );
    }

    #[test]
    fn payload_round_trips_every_documented_field() {
        // A payload with every field explicitly set must survive the
        // conversion into the grouped model unchanged. If a field is missing
        // from `From`, it silently resets to the group default here.
        let json = serde_json::json!({
            "format": "png",
            "quality": 55,
            "width": 800,
            "height": 600,
            "keep_metadata": true,
            "compression": "lossless",
            "resize_mode": "percent",
            "resize_percent": 42.5,
            "fit_mode": "cover",
            "rotate": 90,
            "flip_horizontal": true,
            "flip_vertical": true,
            "watermark": null,
            "quantize_colors": 128,
            "filename_pattern": "{name}_{width}",
            "conflict_policy": "skip",
            "skip_if_larger": true,
            "preserve_animation": false,
            "target_size_bytes": 4096,
            "crop_rect": [0.1, 0.2, 0.5, 0.6],
            "crop_ratio": [16, 9],
            "rotate_degrees": 15.5,
            "brightness": 10,
            "contrast": -10,
            "hue": 45,
            "sharpen": 30,
            "blur": 2.5,
            "grayscale": true,
            "invert": true,
            "sepia": true,
            "auto_contrast": true
        });
        let payload: ProcessingOptionsPayload =
            serde_json::from_value(json).expect("payload deserializes");
        let o: ProcessingOptions = payload.into();

        assert_eq!(o.output.format, OutputFormat::Png);
        assert_eq!(o.output.quality, 55);
        assert!(o.output.keep_metadata);
        assert_eq!(o.output.compression, CompressionType::Lossless);
        assert_eq!(o.output.filename_pattern, "{name}_{width}");
        assert_eq!(o.output.conflict_policy, ConflictPolicy::Skip);
        assert!(o.output.skip_if_larger);
        assert!(!o.output.preserve_animation);
        assert_eq!(o.output.target_size_bytes, Some(4096));
        assert_eq!(o.output.quantize_colors, Some(128));
        assert!(!o.output.mirror_subdirs);

        assert_eq!(o.resize.width, Some(800));
        assert_eq!(o.resize.height, Some(600));
        assert_eq!(o.resize.resize_mode, ResizeMode::Percent);
        assert_eq!(o.resize.resize_percent, 42.5);
        assert_eq!(o.resize.fit_mode, FitMode::Cover);

        assert_eq!(o.transform.rotate, 90);
        assert!(o.transform.flip_horizontal);
        assert!(o.transform.flip_vertical);
        assert_eq!(o.transform.rotate_degrees, 15.5);

        assert_eq!(o.crop.crop_rect, Some([0.1, 0.2, 0.5, 0.6]));
        assert_eq!(o.crop.crop_ratio, Some((16, 9)));

        assert_eq!(o.adjust.brightness, 10);
        assert_eq!(o.adjust.contrast, -10);
        assert_eq!(o.adjust.hue, 45);
        assert_eq!(o.adjust.sharpen, 30);
        assert_eq!(o.adjust.blur, 2.5);
        assert!(o.adjust.grayscale);
        assert!(o.adjust.invert);
        assert!(o.adjust.sepia);
        assert!(o.adjust.auto_contrast);
    }

    #[test]
    fn partial_payload_keeps_defaults_for_absent_fields() {
        // `#[serde(default)]` is what lets an older frontend (or a saved
        // template predating a newer option) still deserialize. Without it the
        // whole call would fail instead of degrading.
        let payload: ProcessingOptionsPayload =
            serde_json::from_value(serde_json::json!({ "format": "jpeg", "quality": 40 }))
                .expect("a partial payload must still parse");
        let o: ProcessingOptions = payload.into();
        let d = ProcessingOptions::default();

        assert_eq!(o.output.quality, 40);
        assert_eq!(o.output.filename_pattern, d.output.filename_pattern);
        assert_eq!(o.resize.resize_percent, d.resize.resize_percent);
        assert_eq!(o.adjust.blur, d.adjust.blur);
        assert_eq!(o.crop.crop_rect, None);
        assert!(!o.output.mirror_subdirs);
        // `preserve_animation` uses `default_true`, the inverse of the norm.
        assert!(o.output.preserve_animation);
    }
}
