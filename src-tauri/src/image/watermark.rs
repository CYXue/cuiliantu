//! Watermark preparation and overlay.
//!
//! Split out of the former `processor.rs` monolith. The option types
//! (`WatermarkConfig`, `PreparedWatermark`, …) stay in `processor.rs` — this
//! module owns the *how*: font parsing, text rasterization, positioning and
//! the alpha overlay itself.

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont};
use image::{DynamicImage, GenericImageView};
use imageproc::drawing::{draw_text_mut, text_size};

use super::processor::{PreparedWatermark, ProcessingOptions, WatermarkConfig, WatermarkPosition};

/// The embedded Smiley Sans font (SIL OFL 1.1), baked in at compile time.
const WATERMARK_FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/SmileySans-Oblique.ttf");

/// Parse `"#RRGGBB"` / `"RRGGBB"`; anything invalid falls back to white.
pub(super) fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
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
pub(super) fn trim_transparent(img: &image::RgbaImage) -> Option<image::RgbaImage> {
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

/// Parse the embedded watermark font once. `render_text_stamp` runs on every
/// preview re-render (each drag of a watermark slider), and `try_from_slice`
/// re-parses the TTF table directory every time — the font bytes are static,
/// so the parsed `FontRef` can live for the whole process.
fn watermark_font() -> Option<&'static FontRef<'static>> {
    static FONT: std::sync::OnceLock<Option<FontRef<'static>>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(WATERMARK_FONT_BYTES).ok())
        .as_ref()
}

/// Rasterize text into a tight RGBA stamp (alpha = coverage).
///
/// Multi-line: `\n` starts a new line (`\r\n` is normalized). Each line is
/// placed on the font's natural line grid (`height()` + `line_gap()`); blank
/// lines keep their slot so user-intended spacing survives, and the trailing
/// padding is removed by the transparent-border trim.
pub(super) fn render_text_stamp(
    text: &str,
    font_size: u32,
    color: &str,
) -> Option<image::RgbaImage> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let font = watermark_font()?;
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

    let mut canvas = image::RgbaImage::new(
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

impl super::processor::ImageProcessor {
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

    /// Composite the prepared watermark onto the image.
    ///
    /// The overlay runs on an explicit RgbaImage (consumed from `img` — zero
    /// copy when the pipeline already handed us RGBA8): concrete-type overlay
    /// is a static dispatch over pixels AND guarantees consistent alpha
    /// blending, whereas blending onto e.g. an ImageRgb8 target would depend
    /// on `DynamicImage`'s per-variant `blend_pixel` behavior.
    pub(super) fn apply_watermark(
        img: DynamicImage,
        wm: Option<&PreparedWatermark>,
        options: &ProcessingOptions,
    ) -> DynamicImage {
        // Both the prepared stamp and its config must be present; anything
        // else means "no watermark configured".
        let (wm, cfg) = match (wm, options.watermark.as_ref()) {
            (Some(wm), Some(cfg)) => (wm, cfg),
            _ => return img,
        };
        // The opacity clamp lives here (not in the IPC layer) because options
        // can be rebuilt from user templates: a malicious or buggy caller
        // could pass e.g. 255 and produce an out-of-range alpha factor.
        let opacity = cfg.opacity().clamp(0, 100);
        if opacity == 0 {
            return img;
        }

        // Build the overlay: image watermarks scale relative to the base
        // width; text stamps are used at their rasterized size.
        let mut overlay: image::RgbaImage = match (wm, cfg) {
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

        let mut out = img.into_rgba8();
        image::imageops::overlay(&mut out, &overlay, x, y);
        DynamicImage::ImageRgba8(out)
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
}
