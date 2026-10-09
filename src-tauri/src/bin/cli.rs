//! Head-less command-line front-end for CuiLianTu.
//!
//! It reuses the exact same processing pipeline and batch runner as the GUI, so
//! a terminal user (or a build/script) gets identical output: formats, resize,
//! rotate, flip, watermark, quantize, real-size estimation, EXIF/ICC metadata
//! preservation, directory-tree mirroring and a CSV report.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::{Parser, ValueEnum};
use serde::Serialize;
use serde_json::Value;

use cuiliantu_lib::commands::image_commands::{
    get_image_files_impl, process_batch_with, write_report_csv, BatchStats, EventEmitter,
    ProgressUpdate,
};
use cuiliantu_lib::image::processor::{
    AdjustOptions, CompressionType, ConflictPolicy, CropOptions, FitMode, OutputFormat,
    OutputOptions, ProcessingOptions, ProcessingResult, ResizeOptions, TransformOptions,
    WatermarkConfig, WatermarkPosition,
};

// --- clap argument enums (the library enums are kept clap-free) -------------

#[derive(ValueEnum, Clone, Copy, Debug)]
enum FmtArg {
    Jpg,
    Png,
    Gif,
    Bmp,
    Tiff,
    Webp,
}

impl From<FmtArg> for OutputFormat {
    fn from(f: FmtArg) -> Self {
        match f {
            FmtArg::Jpg => OutputFormat::Jpeg,
            FmtArg::Png => OutputFormat::Png,
            FmtArg::Gif => OutputFormat::Gif,
            FmtArg::Bmp => OutputFormat::Bmp,
            FmtArg::Tiff => OutputFormat::Tiff,
            FmtArg::Webp => OutputFormat::WebP,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum FitArg {
    Stretch,
    Contain,
    Cover,
}

impl From<FitArg> for FitMode {
    fn from(f: FitArg) -> Self {
        match f {
            FitArg::Stretch => FitMode::Stretch,
            FitArg::Contain => FitMode::Contain,
            FitArg::Cover => FitMode::Cover,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ConflictArg {
    Overwrite,
    Rename,
    Skip,
}

impl From<ConflictArg> for ConflictPolicy {
    fn from(c: ConflictArg) -> Self {
        match c {
            ConflictArg::Overwrite => ConflictPolicy::Overwrite,
            ConflictArg::Rename => ConflictPolicy::AutoRename,
            ConflictArg::Skip => ConflictPolicy::Skip,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum PosArg {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
    Center,
}

impl From<PosArg> for WatermarkPosition {
    fn from(p: PosArg) -> Self {
        match p {
            PosArg::TopLeft => WatermarkPosition::TopLeft,
            PosArg::TopRight => WatermarkPosition::TopRight,
            PosArg::BottomRight => WatermarkPosition::BottomRight,
            PosArg::BottomLeft => WatermarkPosition::BottomLeft,
            PosArg::Center => WatermarkPosition::Center,
        }
    }
}

// `clap`'s `default_value_t` needs the enum to implement `Display`; the matched
// value is the variant name (case-insensitive), so lowercase strings work.
impl std::fmt::Display for FmtArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            FmtArg::Jpg => "jpg",
            FmtArg::Png => "png",
            FmtArg::Gif => "gif",
            FmtArg::Bmp => "bmp",
            FmtArg::Tiff => "tiff",
            FmtArg::Webp => "webp",
        };
        f.write_str(s)
    }
}

impl std::fmt::Display for FitArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            FitArg::Stretch => "stretch",
            FitArg::Contain => "contain",
            FitArg::Cover => "cover",
        };
        f.write_str(s)
    }
}

impl std::fmt::Display for ConflictArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConflictArg::Overwrite => "overwrite",
            ConflictArg::Rename => "rename",
            ConflictArg::Skip => "skip",
        };
        f.write_str(s)
    }
}

impl std::fmt::Display for PosArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PosArg::TopLeft => "topleft",
            PosArg::TopRight => "topright",
            PosArg::BottomRight => "bottomright",
            PosArg::BottomLeft => "bottomleft",
            PosArg::Center => "center",
        };
        f.write_str(s)
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "cuiliantu-cli",
    version,
    about = "Head-less batch image optimizer (CuiLianTu / 淬炼图)"
)]
struct Cli {
    /// Input files or directories (directories are scanned recursively).
    inputs: Vec<PathBuf>,

    /// Output directory.
    #[arg(short, long)]
    output: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = FmtArg::Webp)]
    format: FmtArg,

    /// Quality 0-100 for lossy JPEG/WebP.
    #[arg(long, default_value_t = 80)]
    quality: u8,

    /// Use lossless WebP instead of quality-based encoding.
    #[arg(long)]
    lossless: bool,

    /// Resize width in pixels (omit to keep original width).
    #[arg(long)]
    width: Option<u32>,

    /// Resize height in pixels (omit to keep original height).
    #[arg(long)]
    height: Option<u32>,

    /// Resize by a percentage of the original (overrides --width/--height).
    #[arg(long)]
    percent: Option<f64>,

    /// Fit strategy for pixel resize.
    #[arg(long, value_enum, default_value_t = FitArg::Stretch)]
    fit: FitArg,

    /// Rotate clockwise: 0/90/180/270.
    #[arg(long, default_value_t = 0)]
    rotate: u32,

    /// Flip horizontally.
    #[arg(long)]
    flip_h: bool,

    /// Flip vertically.
    #[arg(long)]
    flip_v: bool,

    /// Reduce to N colors (2-256) for PNG/GIF output.
    #[arg(long)]
    quantize: Option<u32>,

    /// Text watermark.
    #[arg(long)]
    watermark_text: Option<String>,

    /// Image watermark path (PNG with alpha recommended).
    #[arg(long)]
    watermark_image: Option<PathBuf>,

    /// Watermark opacity 0-100.
    #[arg(long, default_value_t = 100)]
    watermark_opacity: u8,

    /// Watermark scale percent (image watermark: relative to base width).
    #[arg(long, default_value_t = 20)]
    watermark_scale: u32,

    /// Watermark margin percent.
    #[arg(long, default_value_t = 3)]
    watermark_margin: u32,

    /// Watermark position.
    #[arg(long, value_enum, default_value_t = PosArg::BottomRight)]
    watermark_pos: PosArg,

    /// What to do when the output file already exists.
    #[arg(long, value_enum, default_value_t = ConflictArg::Overwrite)]
    conflict: ConflictArg,

    /// Only write the output when it is smaller than the original.
    #[arg(long)]
    skip_if_larger: bool,

    /// Preserve EXIF/ICC metadata (JPEG output only).
    #[arg(long)]
    keep_metadata: bool,

    /// Center-crop to an aspect ratio before resizing, e.g. "1:1", "4:3", "16:9".
    #[arg(long)]
    crop_ratio: Option<String>,

    /// Interactive-style drag-crop as normalized fractions "fx,fy,fw,fh"
    /// (0..1, e.g. "0.1,0,0.8,0.9"), mapped proportionally onto every image.
    #[arg(long)]
    crop_rect: Option<String>,

    /// Fine rotation in degrees (-180..180); the canvas auto-expands.
    #[arg(long, default_value_t = 0.0)]
    rotate_degrees: f32,

    /// Additive brightness (-100..100).
    #[arg(long, default_value_t = 0)]
    brightness: i32,

    /// Contrast (-100..100; 0 = off).
    #[arg(long, default_value_t = 0)]
    contrast: i32,

    /// Hue rotation in degrees (-180..180).
    #[arg(long, default_value_t = 0)]
    hue: i32,

    /// USM sharpen amount (0..100).
    #[arg(long, default_value_t = 0)]
    sharpen: u8,

    /// Gaussian blur sigma (0 = off).
    #[arg(long, default_value_t = 0.0)]
    blur: f32,

    #[arg(long)]
    grayscale: bool,

    #[arg(long)]
    invert: bool,

    #[arg(long)]
    sepia: bool,

    /// Percentile (1%/99%) contrast stretch.
    #[arg(long)]
    auto_contrast: bool,

    /// Mirror the input directory tree under the output directory.
    #[arg(long)]
    mirror: bool,

    /// Target output size in KB; the JPEG quality is bisected to fit (JPEG only).
    #[arg(long)]
    target_kb: Option<u64>,

    /// Write a CSV processing report to this path when done.
    #[arg(long)]
    report: Option<PathBuf>,

    /// Parallel jobs (0 = auto-tune to the machine).
    #[arg(long, default_value_t = 0)]
    jobs: usize,
}

/// Event emitter that prints progress to stderr and collects per-file results
/// so a CSV report can be written afterwards.
struct CliEmitter {
    results: Arc<Mutex<Vec<ProcessingResult>>>,
}

impl EventEmitter for CliEmitter {
    fn emit_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String> {
        let value: Value = match serde_json::to_value(&payload) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
        match event {
            "processing-progress" => {
                if let Ok(p) = serde_json::from_value::<ProgressUpdate>(value) {
                    eprint!("\r[{}/{}] {}", p.current, p.total, p.current_file);
                }
            }
            "processing-result" => {
                if let Ok(r) = serde_json::from_value::<ProcessingResult>(value) {
                    self.results.lock().unwrap().push(r);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Build the library [`ProcessingOptions`] from the parsed CLI arguments.
fn build_options(cli: &Cli) -> ProcessingOptions {
    // The pipeline clamps these too (defense-in-depth), but the CLI should
    // already deliver in-domain values: `--percent -50` or `--quality 200`
    // would otherwise produce surprising output instead of an error.
    let (resize_mode, resize_percent) = if let Some(p) = cli.percent {
        // `f64::clamp` is a no-op for NaN (every comparison is false), so a
        // non-finite value has to be rejected before clamping or it poisons
        // every later multiplication in the resizer.
        let pct = if p.is_finite() {
            p.clamp(1.0, 1000.0)
        } else {
            100.0
        };
        (cuiliantu_lib::image::processor::ResizeMode::Percent, pct)
    } else {
        (cuiliantu_lib::image::processor::ResizeMode::Pixels, 100.0)
    };

    ProcessingOptions {
        output: OutputOptions {
            format: cli.format.into(),
            quality: cli.quality.min(100),
            compression: if cli.lossless {
                CompressionType::Lossless
            } else {
                CompressionType::Lossy
            },
            filename_pattern: "{name}".to_string(),
            conflict_policy: cli.conflict.into(),
            skip_if_larger: cli.skip_if_larger,
            preserve_animation: true,
            target_size_bytes: cli.target_kb.map(|kb| kb * 1024),
            quantize_colors: cli.quantize,
            mirror_subdirs: cli.mirror,
            keep_metadata: cli.keep_metadata,
        },
        resize: ResizeOptions {
            width: cli.width,
            height: cli.height,
            resize_mode,
            resize_percent,
            fit_mode: cli.fit.into(),
        },
        transform: TransformOptions {
            rotate: cli.rotate,
            flip_horizontal: cli.flip_h,
            flip_vertical: cli.flip_v,
            // NaN survives `clamp` and would make `rem_euclid` in the rotation
            // produce nonsense, so non-finite input falls back to "no rotation".
            rotate_degrees: if cli.rotate_degrees.is_finite() {
                cli.rotate_degrees.clamp(-180.0, 180.0)
            } else {
                0.0
            },
        },
        crop: CropOptions {
            crop_ratio: cli.crop_ratio.as_deref().and_then(parse_ratio),
            crop_rect: cli.crop_rect.as_deref().and_then(parse_fraction_rect),
        },
        adjust: AdjustOptions {
            brightness: cli.brightness.clamp(-100, 100),
            contrast: cli.contrast.clamp(-100, 100),
            hue: cli.hue.clamp(-180, 180),
            sharpen: cli.sharpen.min(100),
            // `f64::clamp` cannot tame NaN, so non-finite floats are replaced
            // with "off" values here rather than reaching the pixel pipeline.
            blur: if cli.blur.is_finite() {
                cli.blur.clamp(0.0, 50.0)
            } else {
                0.0
            },
            grayscale: cli.grayscale,
            invert: cli.invert,
            sepia: cli.sepia,
            auto_contrast: cli.auto_contrast,
        },
        watermark: match (&cli.watermark_text, &cli.watermark_image) {
            (Some(text), _) if !text.trim().is_empty() => Some(WatermarkConfig::Text {
                text: text.clone(),
                font_size: 48,
                color: "#FFFFFF".to_string(),
                opacity: cli.watermark_opacity,
                position: cli.watermark_pos.into(),
                margin_percent: cli.watermark_margin,
            }),
            (_, Some(path)) => Some(WatermarkConfig::Image {
                path: path.to_string_lossy().to_string(),
                opacity: cli.watermark_opacity,
                scale_percent: cli.watermark_scale,
                position: cli.watermark_pos.into(),
                margin_percent: cli.watermark_margin,
            }),
            _ => None,
        },
    }
}

/// Parse an "W:H" aspect-ratio string like `"1:1"` or `"16:9"` into `(u32, u32)`.
fn parse_ratio(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once(':')?;
    let w = w.trim().parse::<u32>().ok()?;
    let h = h.trim().parse::<u32>().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// Parse a normalized crop rect `"fx,fy,fw,fh"` (each 0..1) into `[f64; 4]`.
fn parse_fraction_rect(s: &str) -> Option<[f64; 4]> {
    let parts: Vec<f64> = s
        .split(',')
        .map(|p| p.trim().parse::<f64>().ok())
        .collect::<Option<_>>()?;
    // The arity check must come *before* indexing. A short input like
    // `--crop-rect 0,0,1` used to index past the end and panic the CLI, so the
    // length is verified first and only then unpacked.
    if parts.len() != 4 {
        return None;
    }
    if !parts
        .iter()
        .all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0)
    {
        return None;
    }
    Some([parts[0], parts[1], parts[2], parts[3]])
}

fn main() {
    let cli = Cli::parse();

    // Fail fast on out-of-domain values instead of silently ignoring them:
    // `--rotate 45` would otherwise produce an unrotated output that looks
    // like success.
    if ![0, 90, 180, 270].contains(&cli.rotate) {
        eprintln!(
            "error: --rotate must be one of 0, 90, 180, 270 (got {})",
            cli.rotate
        );
        std::process::exit(2);
    }

    if cli.inputs.is_empty() {
        eprintln!("error: no input files or directories given");
        std::process::exit(2);
    }

    let inputs: Vec<String> = cli
        .inputs
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let images = match get_image_files_impl(inputs) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            eprintln!("error: no supported images found in the given inputs");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("error: failed to collect images: {e}");
            std::process::exit(1);
        }
    };

    let options = build_options(&cli);
    let emitter = CliEmitter {
        results: Arc::new(Mutex::new(Vec::new())),
    };
    let log = |message: String| eprintln!("[clt] {message}");

    eprintln!(
        "Processing {} image(s) -> {}",
        images.len(),
        cli.output.display()
    );
    let stats: BatchStats = match process_batch_with(
        &emitter,
        images,
        cli.output.to_string_lossy().to_string(),
        options,
        cli.jobs,
        &log,
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\nerror: batch failed: {e}");
            std::process::exit(1);
        }
    };

    eprintln!(); // end the progress line
    println!(
        "Done. files={} success={} failed={} overall_reduction={:.1}% avg_reduction={:.1}%",
        stats.total_files,
        stats.successful_files,
        stats.failed_files,
        stats.overall_reduction_percent,
        stats.average_reduction_percent
    );

    if let Some(report) = &cli.report {
        let results = emitter.results.lock().unwrap();
        if let Err(e) = write_report_csv(&results, report) {
            eprintln!("error: failed to write report: {e}");
        } else {
            println!("Report written to {}", report.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use cuiliantu_lib::image::processor::ResizeMode;

    /// A `Cli` with clap's own defaults (output dir + no inputs), so the tests
    /// exercise the same default path a bare invocation takes.
    fn default_cli() -> Cli {
        Cli::parse_from(["cuiliantu-cli", "-o", "out"])
    }

    #[test]
    fn cli_definition_is_valid() {
        // Catches contradictory `#[arg]` attributes and invalid value parsers
        // at test time rather than on the user's first run.
        Cli::command().debug_assert();
    }

    // --- parse_ratio ------------------------------------------------------

    #[test]
    fn ratio_accepts_well_formed_input() {
        assert_eq!(parse_ratio("1:1"), Some((1, 1)));
        assert_eq!(parse_ratio("16:9"), Some((16, 9)));
        // Whitespace around the parts is tolerated.
        assert_eq!(parse_ratio(" 4 : 3 "), Some((4, 3)));
    }

    #[test]
    fn ratio_rejects_garbage_and_zero() {
        // A zero dimension would make the aspect-ratio math divide by zero
        // downstream, so it must never reach the pipeline.
        assert_eq!(parse_ratio("0:1"), None);
        assert_eq!(parse_ratio("1:0"), None);
        assert_eq!(parse_ratio("16-9"), None, "wrong separator");
        assert_eq!(parse_ratio(""), None);
        assert_eq!(parse_ratio("a:b"), None);
        assert_eq!(parse_ratio("-1:1"), None, "negative");
        assert_eq!(parse_ratio("1:2:3"), None, "extra segment");
    }

    // --- parse_fraction_rect ----------------------------------------------

    #[test]
    fn crop_rect_accepts_four_normalized_values() {
        assert_eq!(parse_fraction_rect("0,0,1,1"), Some([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(
            parse_fraction_rect("0.1,0.2,0.5,0.6"),
            Some([0.1, 0.2, 0.5, 0.6])
        );
        assert_eq!(
            parse_fraction_rect(" 0 , 0 , .5 , .5 "),
            Some([0.0, 0.0, 0.5, 0.5])
        );
    }

    #[test]
    fn crop_rect_rejects_out_of_range_and_malformed() {
        // Values outside 0..1 would place the crop outside the image; NaN and
        // inf are what the pipeline's own `is_finite` guard screens for, so
        // the parser must not let them through either.
        assert_eq!(parse_fraction_rect("0,0,1.5,1"), None, "width > 1");
        assert_eq!(parse_fraction_rect("-0.1,0,1,1"), None, "negative origin");
        assert_eq!(parse_fraction_rect("0,0,NaN,1"), None);
        assert_eq!(parse_fraction_rect("0,0,inf,1"), None);
        // Wrong arity is the most common typo (`--crop-rect 0,0,1`).
        assert_eq!(parse_fraction_rect("0,0,1"), None, "three values");
        assert_eq!(parse_fraction_rect("0,0,1,1,1"), None, "five values");
        assert_eq!(parse_fraction_rect(""), None);
        assert_eq!(parse_fraction_rect("a,b,c,d"), None);
    }

    // --- build_options ----------------------------------------------------
    //
    // The CLI must hand the pipeline in-domain values: the library clamps too,
    // but silently turning `--quality 200` into a surprising output instead of
    // an error is exactly what these guards prevent.

    #[test]
    fn defaults_are_in_domain() {
        let o = build_options(&default_cli());
        assert_eq!(o.output.quality, 80);
        assert!(o.output.quality <= 100);
        assert_eq!(o.output.filename_pattern, "{name}");
        assert_eq!(o.resize.resize_mode, ResizeMode::Pixels);
        assert_eq!(o.resize.resize_percent, 100.0);
        assert!(o.watermark.is_none());
        assert_eq!(o.output.format, OutputFormat::WebP);
    }

    #[test]
    fn percent_selects_percent_resize_mode_and_clamps() {
        let mut cli = default_cli();
        cli.percent = Some(150.0);
        let o = build_options(&cli);
        assert_eq!(o.resize.resize_mode, ResizeMode::Percent);
        assert_eq!(o.resize.resize_percent, 150.0);

        // Out-of-domain percentages are clamped into 1..=1000 rather than
        // producing a zero/negative scale.
        cli.percent = Some(-50.0);
        assert_eq!(build_options(&cli).resize.resize_percent, 1.0);
        cli.percent = Some(9999.0);
        assert_eq!(build_options(&cli).resize.resize_percent, 1000.0);

        // A NaN percent would poison every later multiplication.
        cli.percent = Some(f64::NAN);
        let o = build_options(&cli);
        assert!(
            o.resize.resize_percent.is_finite(),
            "NaN percent must not reach the pipeline"
        );
    }

    #[test]
    fn adjustment_values_are_clamped_to_their_documented_ranges() {
        let mut cli = default_cli();
        cli.brightness = 500;
        cli.contrast = -500;
        cli.hue = 900;
        cli.sharpen = 255;
        cli.blur = 1.0e9;
        cli.rotate_degrees = 3600.0;

        let o = build_options(&cli);
        assert_eq!(o.adjust.brightness, 100, "-100..100");
        assert_eq!(o.adjust.contrast, -100, "-100..100");
        assert_eq!(o.adjust.hue, 180, "-180..180");
        assert_eq!(o.adjust.sharpen, 100, "0..100");
        assert_eq!(o.adjust.blur, 50.0, "0..50");
        assert_eq!(o.transform.rotate_degrees, 180.0, "-180..180");
    }

    #[test]
    fn non_finite_floats_fall_back_to_off_instead_of_poisoning_the_pipeline() {
        // `f64::clamp` is a no-op for NaN (all comparisons are false), so a
        // non-finite value used to travel straight into the pixel pipeline and
        // turn every later multiplication into NaN. Same for infinities, which
        // `clamp` does bound but to an absurd value.
        let mut cli = default_cli();
        cli.blur = f32::NAN;
        cli.rotate_degrees = f32::NAN;
        cli.percent = Some(f64::INFINITY);

        let o = build_options(&cli);
        for (label, v) in [
            ("blur", o.adjust.blur),
            ("rotate_degrees", o.transform.rotate_degrees),
        ] {
            assert!(v.is_finite(), "{label} must be finite, got {v}");
        }
        assert!(
            o.resize.resize_percent.is_finite(),
            "resize_percent must be finite, got {}",
            o.resize.resize_percent
        );
        assert_eq!(o.adjust.blur, 0.0, "NaN blur = no blur");
        assert_eq!(
            o.transform.rotate_degrees, 0.0,
            "NaN rotation = no rotation"
        );
        assert_eq!(o.resize.resize_percent, 100.0, "infinite percent = default");
    }

    #[test]
    fn watermark_is_only_built_from_a_non_empty_value() {
        let mut cli = default_cli();
        // Blank text must not produce a stamp; the image branch is the
        // fallback when both are present.
        cli.watermark_text = Some("   ".to_string());
        assert!(build_options(&cli).watermark.is_none());

        cli.watermark_text = Some("(c) Me".to_string());
        assert!(matches!(
            build_options(&cli).watermark,
            Some(WatermarkConfig::Text { .. })
        ));

        cli.watermark_text = None;
        cli.watermark_image = Some(PathBuf::from("wm.png"));
        assert!(matches!(
            build_options(&cli).watermark,
            Some(WatermarkConfig::Image { .. })
        ));
    }

    #[test]
    fn target_kb_becomes_bytes() {
        let mut cli = default_cli();
        cli.target_kb = Some(50);
        assert_eq!(
            build_options(&cli).output.target_size_bytes,
            Some(50 * 1024)
        );
        cli.target_kb = None;
        assert_eq!(build_options(&cli).output.target_size_bytes, None);
    }

    // --- CliEmitter -------------------------------------------------------

    /// The emitter must survive a batch: these payloads serialize today, but a
    /// future type change must not panic the CLI mid-run.
    #[test]
    fn cli_emitter_collects_results() {
        let emitter = CliEmitter {
            results: Arc::new(Mutex::new(Vec::new())),
        };
        let result = ProcessingResult {
            original_path: "a.png".to_string(),
            output_path: "a.webp".to_string(),
            original_size: 100,
            output_size: 40,
            reduction_percent: 60.0,
            success: true,
            error: None,
            skipped: false,
            within_target: true,
        };
        assert!(emitter.emit_event("processing-result", &result).is_ok());
        assert!(emitter
            .emit_event(
                "processing-progress",
                ProgressUpdate {
                    current: 1,
                    total: 1,
                    current_file: "a.png".to_string(),
                    percent: 100.0,
                }
            )
            .is_ok());
        // Unknown events are ignored rather than treated as errors.
        assert!(emitter.emit_event("something-else", 1u8).is_ok());

        let collected = emitter.results.lock().unwrap();
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].output_path, "a.webp");
    }
}
