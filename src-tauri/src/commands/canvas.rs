//! Canvas compose: multi-image staging, layer transforms, region export.
//!
//! The frontend owns a free-form canvas where layers (backed by source image
//! files) are moved, scaled, rotated, reordered and finally composited into
//! one output image — optionally cropped to a user-drawn selection rect.
//! Two commands live here:
//!
//! * [`canvas_layer_thumbnail`] — a bounded-size PNG preview for on-canvas
//!   display (the final export always re-reads the original file, so the
//!   thumbnail size never limits output quality).
//! * [`compose_canvas`] — the full-quality composite + encode + write.
//!
//! Both verify every path against the session whitelist, keeping the canvas
//! surface under the same defense-in-depth rule as the rest of the app.
//!
//! Geometry contract (shared with the frontend's `utils/canvas.ts`):
//! a layer is the axis-aligned rect `(x, y, width, height)` in canvas
//! pixels; `rotation_degrees` rotates the layer content clockwise around its
//! center (CSS `rotate(θ)` on a y-down screen), applied AFTER the flips —
//! i.e. CSS `transform: rotate(θ) scaleX(±1) scaleY(±1)`.

use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;
use tauri::State;

use super::path_guard::PathWhitelist;
use crate::image::processor::ImageProcessor;

/// Longest side of the on-canvas display thumbnail. The canvas renders
/// layers at CSS-pixel scale, so 2048 is plenty even zoomed in, while the
/// base64 IPC payload stays a few MB at most.
const THUMBNAIL_MAX_SIDE: u32 = 2048;

/// Hard cap on any composed output side. Bounds the allocation of the target
/// canvas (and every intermediate rotated layer) to something the encoder
/// formats can still express — WebP tops out at 16383 px.
const MAX_COMPOSE_SIDE: f64 = 12000.0;

/// Hard cap on total composed pixels (~128 MP), same spirit as the decode
/// bomb guard: a hostile frontend must not be able to request a canvas that
/// allocates gigabytes before the first pixel is written.
const MAX_COMPOSE_PIXELS: f64 = 128_000_000.0;

/// Smallest layer box the compositor accepts. Prevents zero/negative sizes
/// from turning into degenerate `resize_exact(0, ..)` calls.
const MIN_LAYER_SIDE: f64 = 1.0;

/// A display thumbnail for one canvas layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerThumbnail {
    /// PNG, base64 (supports transparency, safe for any source format)
    pub base64_png: String,
    /// Original pixel size — the frontend uses it to derive the layer's
    /// default on-canvas box at 1:1 scale.
    pub natural_width: u32,
    pub natural_height: u32,
}

/// One layer of the composite request. Array order IS z-order (index 0 at
/// the bottom), matching the frontend layer list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeLayer {
    pub path: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub rotation_degrees: f64,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    /// 0..100 (100 = fully opaque)
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    /// Source crop `[fx, fy, fw, fh]` normalized 0..1 relative to the
    /// original image; `None` = whole image. Applied BEFORE the resize.
    #[serde(default)]
    pub crop: Option<[f64; 4]>,
}

fn default_opacity() -> f64 {
    100.0
}

/// Full composite request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeRequest {
    pub layers: Vec<ComposeLayer>,
    /// `#rrggbb`, or `None` for a transparent background (PNG/WebP only —
    /// JPEG output composites over white instead).
    #[serde(default)]
    pub background: Option<String>,
    /// Export rectangle `[x, y, w, h]` in canvas coordinates, or `None` to
    /// export the auto bounding box of all layers.
    #[serde(default)]
    pub export_rect: Option<[f64; 4]>,
    pub output_path: String,
    /// `"png" | "jpeg" | "webp"`
    pub format: String,
    /// 1..100, JPEG/WebP only.
    #[serde(default = "default_quality")]
    pub quality: u8,
}

fn default_quality() -> u8 {
    90
}

#[derive(Debug, Clone, Serialize)]
pub struct ComposeResult {
    pub output_path: String,
    pub width: u32,
    pub height: u32,
    pub output_size: u64,
}

/// `|sin|` and `|cos|` of `degrees`, with multiples of 90° snapped to exact
/// 0/1 values.
///
/// Floating point makes `cos(90°)` = 6.1e-17 instead of 0; the bounding-box
/// `ceil` downstream would amplify that speck into a whole extra pixel row
/// (2×1 → 90° must stay 1×2, not balloon to 2×2).
fn abs_sin_cos(degrees: f64) -> (f64, f64) {
    let clean = |v: f64| -> f64 {
        let a = v.abs();
        if (a - 1.0).abs() < 1e-9 {
            1.0
        } else if a < 1e-9 {
            0.0
        } else {
            a
        }
    };
    let (s, c) = degrees.to_radians().sin_cos();
    (clean(s), clean(c))
}

/// Clockwise-rotated axis-aligned bounding box of a `w × h` box.
///
/// Shared geometry with the frontend: same formula in `utils/canvas.ts`,
/// so the on-canvas selection handles sit exactly where the export puts
/// pixels.
fn rotated_aabb(w: f64, h: f64, degrees: f64) -> (f64, f64) {
    if !w.is_finite() || !h.is_finite() || !degrees.is_finite() {
        return (1.0, 1.0);
    }
    let (sin, cos) = abs_sin_cos(degrees);
    (w * cos + h * sin, w * sin + h * cos)
}

/// Reject a non-finite input, mapping it to a bounded fallback.
///
/// `f64::clamp` is a no-op on NaN (every comparison is false), so NaN must
/// be filtered BEFORE any clamp — otherwise it flows through the pipeline
/// and poisons the first multiplication downstream.
fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// Clamp a layer side to `[MIN_LAYER_SIDE, MAX_COMPOSE_SIDE]`.
fn clamp_side(value: f64) -> f64 {
    finite_or(value, MIN_LAYER_SIDE).clamp(MIN_LAYER_SIDE, MAX_COMPOSE_SIDE)
}

/// Bilinear RGBA sample with alpha-premultiplied edges.
///
/// Premultiplication avoids the dark halo a naive per-channel lerp leaves
/// where a fully transparent neighbor borders an opaque pixel. Coordinates
/// more than half a pixel outside the image sample as fully transparent —
/// that is what fills the expanded corners of a rotated layer.
fn sample_bilinear(img: &RgbaImage, fx: f64, fy: f64) -> [u8; 4] {
    let (w, h) = img.dimensions();
    let wf = w as f64;
    let hf = h as f64;
    if !fx.is_finite() || !fy.is_finite() {
        return [0, 0, 0, 0];
    }
    // More than half a pixel past the edge → outside the source entirely.
    if fx < -0.5 || fy < -0.5 || fx > wf - 0.5 || fy > hf - 0.5 {
        return [0, 0, 0, 0];
    }
    let clx = fx.clamp(0.0, wf - 1.0);
    let cly = fy.clamp(0.0, hf - 1.0);
    let x0 = clx.floor() as u32;
    let y0 = cly.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = clx - x0 as f64;
    let ty = cly - y0 as f64;

    let corners = [
        img.get_pixel(x0, y0).0,
        img.get_pixel(x1, y0).0,
        img.get_pixel(x0, y1).0,
        img.get_pixel(x1, y1).0,
    ];
    // Premultiply, lerp in premultiplied space, then unpremultiply.
    let mut acc = [0.0f64; 4];
    for (i, c) in corners.iter().enumerate() {
        let a = c[3] as f64 / 255.0;
        let wx = if i & 1 == 0 { 1.0 - tx } else { tx };
        let wy = if i < 2 { 1.0 - ty } else { ty };
        let wgt = wx * wy;
        acc[0] += c[0] as f64 * a * wgt;
        acc[1] += c[1] as f64 * a * wgt;
        acc[2] += c[2] as f64 * a * wgt;
        acc[3] += a * wgt;
    }
    if acc[3] <= f64::EPSILON {
        return [0, 0, 0, 0];
    }
    let inv = 1.0 / acc[3];
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = (acc[c] * inv * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (acc[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

/// Rotate `img` clockwise by `degrees` around its center, expanding the
/// output to the rotated bounding box so no pixel is clipped.
///
/// `image` has no arbitrary-angle rotate with expansion, and the composition
/// geometry must match the frontend's CSS rendering exactly — so this is an
/// inverse-mapping resample instead of a crate call.
fn rotate_expand(img: &RgbaImage, degrees: f64) -> RgbaImage {
    let (w, h) = img.dimensions();
    let wf = w as f64;
    let hf = h as f64;
    let (bbox_w, bbox_h) = rotated_aabb(wf, hf, degrees);
    let bbox_w = bbox_w.ceil().max(1.0);
    let bbox_h = bbox_h.ceil().max(1.0);
    let mut out = RgbaImage::new(bbox_w as u32, bbox_h as u32);

    let theta = degrees.to_radians();
    let (sin, cos) = theta.sin_cos();
    // Screen coordinates are y-down, so this matrix (x' = x·cos − y·sin,
    // y' = x·sin + y·cos) maps input → output as a CLOCKWISE rotation,
    // matching CSS `rotate(θ)`. The loop below applies the inverse.
    let (cx_in, cy_in) = (wf / 2.0, hf / 2.0);
    let (cx_out, cy_out) = (bbox_w / 2.0, bbox_h / 2.0);
    for oy in 0..out.height() {
        for ox in 0..out.width() {
            let dx = ox as f64 + 0.5 - cx_out;
            let dy = oy as f64 + 0.5 - cy_out;
            // Inverse rotation: p_in = R(-θ)·(p_out − c_out) + c_in
            let sx = cos * dx + sin * dy + cx_in;
            let sy = -sin * dx + cos * dy + cy_in;
            out.put_pixel(ox, oy, Rgba(sample_bilinear(img, sx - 0.5, sy - 0.5)));
        }
    }
    out
}

/// Scale the layer's alpha channel by `opacity` (0..100).
fn apply_opacity(img: &mut RgbaImage, opacity: f64) {
    if !opacity.is_finite() || (opacity - 100.0).abs() < f64::EPSILON {
        return;
    }
    let a = (opacity / 100.0).clamp(0.0, 1.0) as f32;
    for px in img.pixels_mut() {
        px.0[3] = (px.0[3] as f32 * a).round() as u8;
    }
}

/// Decode one source file, apply the layer's box/flip/rotate/opacity, and
/// report where the result lands on the canvas.
///
/// Returns `(content, paste_x, paste_y)` — the paste origin is the TOP-LEFT
/// of the rotated bounding box, derived from the layer's CENTER (which is
/// the one point flips and rotations leave invariant). `i64` allows partial
/// off-canvas placement; `overlay` clips it.
fn prepare_layer(img: DynamicImage, layer: &ComposeLayer) -> (RgbaImage, i64, i64) {
    let width = clamp_side(layer.width).round().max(1.0) as u32;
    let height = clamp_side(layer.height).round().max(1.0) as u32;

    // Source crop FIRST (normalized rect -> integer pixels), then the box
    // resize maps the cropped region onto the layer's display size — matching
    // the frontend's background-size/position rendering exactly.
    let mut img = img;
    if let Some([fx, fy, fw, fh]) = layer.crop {
        let (sw, sh) = (img.width(), img.height());
        let valid = [fx, fy, fw, fh]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0)
            && fw > f64::EPSILON
            && fh > f64::EPSILON;
        if valid {
            let x = ((fx * sw as f64).round() as u32).min(sw.saturating_sub(1));
            let y = ((fy * sh as f64).round() as u32).min(sh.saturating_sub(1));
            let cw = ((fw * sw as f64).round() as u32).clamp(1, sw - x);
            let ch = ((fh * sh as f64).round() as u32).clamp(1, sh - y);
            img = img.crop_imm(x, y, cw, ch);
        }
    }

    let filter = image::imageops::FilterType::Lanczos3;
    let mut content = img.resize_exact(width, height, filter).to_rgba8();

    // Flip FIRST, then rotate — matching CSS `rotate(θ) scaleX(±1)`, where
    // transforms read right-to-left.
    if layer.flip_h {
        image::imageops::flip_horizontal_in_place(&mut content);
    }
    if layer.flip_v {
        image::imageops::flip_vertical_in_place(&mut content);
    }
    let rotation = finite_or(layer.rotation_degrees, 0.0);
    if rotation.rem_euclid(360.0).abs() > f64::EPSILON {
        content = rotate_expand(&content, rotation);
    }
    apply_opacity(&mut content, layer.opacity);

    // The layer's center in canvas space, then back out half the FINAL
    // content size (for a rotated layer the content box is the AABB).
    let (cw, ch) = (content.width() as f64, content.height() as f64);
    let cx = finite_or(layer.x, 0.0) + clamp_side(layer.width) / 2.0;
    let cy = finite_or(layer.y, 0.0) + clamp_side(layer.height) / 2.0;
    let px = (cx - cw / 2.0).round() as i64;
    let py = (cy - ch / 2.0).round() as i64;
    (content, px, py)
}

/// Resolve the export rectangle: either the given selection or the tight
/// bounding box over all layers' rotated AABBs. Integer-aligned.
fn resolve_export_rect(
    layers: &[ComposeLayer],
    export_rect: Option<[f64; 4]>,
) -> Result<[i64; 4], String> {
    if let Some(rect) = export_rect {
        let [x, y, w, h] = rect;
        if ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return Err("Invalid export rectangle".to_string());
        }
        // Checked BEFORE the min-side clamp: a zero/negative selection is a
        // user mistake to report, not something to silently widen.
        if w < 1.0 || h < 1.0 {
            return Err("Export area is empty".to_string());
        }
        let rx = x.floor();
        let ry = y.floor();
        return Ok([rx as i64, ry as i64, w.ceil() as i64, h.ceil() as i64]);
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for layer in layers {
        let (bw, bh) = rotated_aabb(
            clamp_side(layer.width),
            clamp_side(layer.height),
            layer.rotation_degrees,
        );
        let cx = finite_or(layer.x, 0.0) + clamp_side(layer.width) / 2.0;
        let cy = finite_or(layer.y, 0.0) + clamp_side(layer.height) / 2.0;
        min_x = min_x.min(cx - bw / 2.0);
        min_y = min_y.min(cy - bh / 2.0);
        max_x = max_x.max(cx + bw / 2.0);
        max_y = max_y.max(cy + bh / 2.0);
    }
    // max_x/max_y come from finite_or-guarded values, so no NaN can reach
    // the clamp (clamp would return NaN instead of bounding it).
    let w = (max_x - min_x).ceil().clamp(1.0, MAX_COMPOSE_SIDE);
    let h = (max_y - min_y).ceil().clamp(1.0, MAX_COMPOSE_SIDE);
    Ok([
        min_x.floor() as i64,
        min_y.floor() as i64,
        w as i64,
        h as i64,
    ])
}

/// Parse `#rrggbb` (or `#rgb`) into an RGBA quad with full alpha.
fn parse_hex_color(color: &str) -> Option<[u8; 4]> {
    fn byte(hex: &str) -> Option<u8> {
        u8::from_str_radix(hex, 16).ok()
    }
    fn double(c: &str) -> Option<u8> {
        byte(&format!("{c}{c}"))
    }
    let hex = color.trim().trim_start_matches('#');
    let (r, g, b) = match hex.len() {
        6 => (byte(&hex[0..2])?, byte(&hex[2..4])?, byte(&hex[4..6])?),
        3 => (
            double(&hex[0..1])?,
            double(&hex[1..2])?,
            double(&hex[2..3])?,
        ),
        _ => return None,
    };
    Some([r, g, b, 255])
}

/// Encode the composed canvas. PNG keeps alpha; JPEG composites over the
/// (opaque) background; WebP encodes lossily at `quality`.
fn encode_output(img: &RgbaImage, format: &str, quality: u8) -> Result<(Vec<u8>, String), String> {
    let quality = quality.clamp(1, 100);
    match format {
        "jpeg" | "jpg" => {
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            let mut cursor = Cursor::new(Vec::new());
            let encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
            rgb.write_with_encoder(encoder)
                .map_err(|e| format!("Failed to encode JPEG: {e}"))?;
            Ok((cursor.into_inner(), "jpg".to_string()))
        }
        "webp" => {
            let (width, height) = img.dimensions();
            let encoded = webp::Encoder::from_rgba(img, width, height)
                .encode(quality as f32)
                .to_vec();
            Ok((encoded, "webp".to_string()))
        }
        _ => {
            // PNG (default branch also guards against unknown format strings)
            let dyn_img = DynamicImage::ImageRgba8(img.clone());
            let mut cursor = Cursor::new(Vec::new());
            dyn_img
                .write_to(&mut cursor, image::ImageFormat::Png)
                .map_err(|e| format!("Failed to encode PNG: {e}"))?;
            Ok((cursor.into_inner(), "png".to_string()))
        }
    }
}

/// Pure composite core (no filesystem): validate sizes, paint the background,
/// paste every layer in z-order into `[rect.x, rect.y, rect.w, rect.h]`.
fn compose_to_image(
    layers: &[ComposeLayer],
    sources: Vec<DynamicImage>,
    rect: [i64; 4],
    background: Option<[u8; 4]>,
) -> Result<RgbaImage, String> {
    let [rx, ry, rw, rh] = rect;
    if rw < 1 || rh < 1 {
        return Err("Export area is empty".to_string());
    }
    let (rw, rh) = (rw as u32, rh as u32);
    if rw as f64 * rh as f64 > MAX_COMPOSE_PIXELS {
        return Err(format!(
            "Export area too large: {} MP exceeds the {} MP limit",
            (rw as u64 * rh as u64) / 1_000_000,
            MAX_COMPOSE_PIXELS as u64 / 1_000_000,
        ));
    }

    let mut canvas = match background {
        Some([r, g, b, a]) => RgbaImage::from_pixel(rw, rh, Rgba([r, g, b, a])),
        None => RgbaImage::new(rw, rh),
    };

    for (layer, img) in layers.iter().zip(sources) {
        let (content, px, py) = prepare_layer(img, layer);
        // Shift from canvas coordinates into export-rect coordinates.
        image::imageops::overlay(&mut canvas, &content, px - rx, py - ry);
    }
    Ok(canvas)
}

/// Display thumbnail for one canvas layer: orientation-corrected, bounded to
/// [`THUMBNAIL_MAX_SIDE`], PNG/base64.
#[tauri::command]
pub async fn canvas_layer_thumbnail(
    path: String,
    whitelist: State<'_, PathWhitelist>,
) -> Result<LayerThumbnail, String> {
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || canvas_layer_thumbnail_impl(&path, &whitelist))
        .await
        .map_err(|e| format!("Canvas task failed: {e}"))?
}

fn canvas_layer_thumbnail_impl(
    path: &str,
    whitelist: &PathWhitelist,
) -> Result<LayerThumbnail, String> {
    whitelist.ensure_allowed(Path::new(path))?;
    let bytes = std::fs::read(Path::new(path)).map_err(|e| format!("Failed to read file: {e}"))?;
    let img = ImageProcessor::decode_bytes_with_orientation(&bytes)
        .map_err(|e| format!("Failed to decode image: {e}"))?;
    let natural_width = img.width();
    let natural_height = img.height();
    let display = if img.width() > THUMBNAIL_MAX_SIDE || img.height() > THUMBNAIL_MAX_SIDE {
        // resize() keeps the aspect ratio and never upscales.
        img.resize(
            THUMBNAIL_MAX_SIDE,
            THUMBNAIL_MAX_SIDE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };
    let mut cursor = Cursor::new(Vec::new());
    display
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode thumbnail: {e}"))?;
    Ok(LayerThumbnail {
        base64_png: base64::engine::general_purpose::STANDARD.encode(cursor.into_inner()),
        natural_width,
        natural_height,
    })
}

/// Composite all layers (z-order = array order), crop to the export rect and
/// write the encoded output file.
#[tauri::command]
pub async fn compose_canvas(
    request: ComposeRequest,
    whitelist: State<'_, PathWhitelist>,
) -> Result<ComposeResult, String> {
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || compose_canvas_impl(&request, &whitelist))
        .await
        .map_err(|e| format!("Canvas task failed: {e}"))?
}

fn compose_canvas_impl(
    request: &ComposeRequest,
    whitelist: &PathWhitelist,
) -> Result<ComposeResult, String> {
    if request.layers.is_empty() {
        return Err("Canvas has no layers to export".to_string());
    }
    // Whitelist every source up-front: a 10-layer composite must not export
    // 9 layers and then die on an unauthorized 10th.
    for layer in &request.layers {
        whitelist.ensure_allowed(Path::new(&layer.path))?;
    }
    // The output path arrives from a save dialog and is registered by the
    // frontend right before this call (register and validate stay separate
    // steps, same discipline as the CSV report export).
    whitelist.ensure_allowed(Path::new(&request.output_path))?;

    let format = request.format.to_lowercase();
    // JPEG has no alpha: force an opaque background (white unless the user
    // picked one) so transparent canvas areas don't turn black.
    let background = match (format.as_str(), request.background.as_deref()) {
        ("jpeg", Some(c)) => parse_hex_color(c),
        ("jpeg", None) => Some([255, 255, 255, 255]),
        (_, Some(c)) => parse_hex_color(c),
        (_, None) => None,
    };

    let rect = resolve_export_rect(&request.layers, request.export_rect)?;
    let mut sources = Vec::with_capacity(request.layers.len());
    for layer in &request.layers {
        let bytes = std::fs::read(Path::new(&layer.path))
            .map_err(|e| format!("Failed to read file: {e}"))?;
        let img = ImageProcessor::decode_bytes_with_orientation(&bytes)
            .map_err(|e| format!("Failed to decode image: {e}"))?;
        sources.push(img);
    }

    let canvas = compose_to_image(&request.layers, sources, rect, background)?;
    let (bytes, _ext) = encode_output(&canvas, &format, request.quality)?;
    std::fs::write(Path::new(&request.output_path), bytes)
        .map_err(|e| format!("Failed to write output: {e}"))?;

    let meta = std::fs::metadata(Path::new(&request.output_path))
        .map_err(|e| format!("Failed to stat output: {e}"))?;
    Ok(ComposeResult {
        output_path: request.output_path.clone(),
        width: canvas.width(),
        height: canvas.height(),
        output_size: meta.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("clt-canvas-{}-{}-{}", name, std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
        dir
    }

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, Rgba(rgba)))
    }

    fn layer(path: &str, x: f64, y: f64, w: f64, h: f64) -> ComposeLayer {
        ComposeLayer {
            path: path.to_string(),
            x,
            y,
            width: w,
            height: h,
            rotation_degrees: 0.0,
            flip_h: false,
            flip_v: false,
            opacity: 100.0,
            crop: None,
        }
    }

    #[test]
    fn rotated_aabb_matches_trig() {
        // 0° keeps the box, 90° swaps the sides.
        assert_eq!(rotated_aabb(100.0, 50.0, 0.0), (100.0, 50.0));
        let (w, h) = rotated_aabb(100.0, 50.0, 90.0);
        assert!((w - 50.0).abs() < 1e-9 && (h - 100.0).abs() < 1e-9);
        // 45° expands by √2/2·(w+h) on both axes.
        let (w, h) = rotated_aabb(100.0, 50.0, 45.0);
        let expected = (100.0 + 50.0) * std::f64::consts::FRAC_1_SQRT_2;
        assert!((w - expected).abs() < 1e-9 && (h - expected).abs() < 1e-9);
        // Negative rotation mirrors positive (|sin|/|cos|).
        let (w1, h1) = rotated_aabb(100.0, 50.0, -30.0);
        let (w2, h2) = rotated_aabb(100.0, 50.0, 30.0);
        assert!((w1 - w2).abs() < 1e-9 && (h1 - h2).abs() < 1e-9);
    }

    #[test]
    fn rotate_expand_90_moves_right_edge_to_bottom() {
        // 2×1: left red, right blue. CSS rotate(90deg) is clockwise, so the
        // right (blue) side lands on the BOTTOM of the 1×2 output.
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        let out = rotate_expand(&img, 90.0);
        assert_eq!((out.width(), out.height()), (1, 2));
        assert_eq!(out.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(out.get_pixel(0, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn rotate_expand_180_flips() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        let out = rotate_expand(&img, 180.0);
        assert_eq!((out.width(), out.height()), (2, 1));
        assert_eq!(out.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(out.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn compose_two_layers_z_order_and_bbox() {
        // Red 8×8 at (0,0) below blue 4×4 at (6,6): blue spans 6..10, so the
        // tight bbox is 10×10 and the 6..8 overlap shows blue.
        let layers = vec![
            layer("r", 0.0, 0.0, 8.0, 8.0),
            layer("b", 6.0, 6.0, 4.0, 4.0),
        ];
        let sources = vec![solid(8, 8, [255, 0, 0, 255]), solid(4, 4, [0, 0, 255, 255])];
        let rect = resolve_export_rect(&layers, None).unwrap();
        assert_eq!(rect, [0, 0, 10, 10]);
        let img = compose_to_image(&layers, sources, rect, None).unwrap();
        assert_eq!((img.width(), img.height()), (10, 10));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        // Overlap (7,7) and blue-only corner (9,9) both render blue.
        assert_eq!(img.get_pixel(7, 7).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(9, 9).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(5, 5).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(9, 5).0, [0, 0, 0, 0]);
        // With a white background the empty corner is white.
        let sources = vec![solid(8, 8, [255, 0, 0, 255]), solid(4, 4, [0, 0, 255, 255])];
        let img = compose_to_image(&layers, sources, rect, Some([255, 255, 255, 255])).unwrap();
        assert_eq!(img.get_pixel(9, 5).0, [255, 255, 255, 255]);
    }

    #[test]
    fn export_rect_crops_and_shifts() {
        let layers = vec![
            layer("r", 0.0, 0.0, 8.0, 8.0),
            layer("b", 4.0, 4.0, 4.0, 4.0),
        ];
        let rect = resolve_export_rect(&layers, Some([2.0, 2.0, 6.0, 6.0])).unwrap();
        assert_eq!(rect, [2, 2, 6, 6]);
        let sources = vec![solid(8, 8, [255, 0, 0, 255]), solid(4, 4, [0, 0, 255, 255])];
        let img = compose_to_image(&layers, sources, rect, None).unwrap();
        assert_eq!((img.width(), img.height()), (6, 6));
        // Canvas (2,2) → local (0,0): still red. Canvas (5,5) → local (3,3): blue.
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(3, 3).0, [0, 0, 255, 255]);
    }

    #[test]
    fn opacity_blends_over_lower_layer() {
        let layers = vec![
            layer("r", 0.0, 0.0, 8.0, 8.0),
            ComposeLayer {
                opacity: 50.0,
                ..layer("b", 2.0, 2.0, 4.0, 4.0)
            },
        ];
        let sources = vec![solid(8, 8, [255, 0, 0, 255]), solid(4, 4, [0, 0, 255, 255])];
        let rect = resolve_export_rect(&layers, None).unwrap();
        let img = compose_to_image(&layers, sources, rect, None).unwrap();
        // 50% blue over red → a magenta-ish blend. The exact channel values
        // depend on the over-compositor's rounding; assert the semantics
        // (half red + half blue, fully opaque) with a ±2 tolerance.
        let px = img.get_pixel(4, 4).0;
        assert!((px[0] as i32 - 127).abs() <= 2, "red channel: {px:?}");
        assert!(px[1] <= 2, "green channel: {px:?}");
        assert!((px[2] as i32 - 128).abs() <= 2, "blue channel: {px:?}");
        assert!(px[3] >= 250, "alpha channel: {px:?}");
        // Outside the blue layer, red stays untouched.
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn rotation_expands_export_bbox() {
        // A 100×50 layer rotated 45° needs the √2-expanded AABB.
        let layers = vec![ComposeLayer {
            rotation_degrees: 45.0,
            ..layer("r", 0.0, 0.0, 100.0, 50.0)
        }];
        let rect = resolve_export_rect(&layers, None).unwrap();
        let expected = ((100.0 + 50.0) * std::f64::consts::FRAC_1_SQRT_2).ceil() as i64;
        assert_eq!(rect[2], expected);
        assert_eq!(rect[3], expected);
    }

    #[test]
    fn nan_inputs_are_bounded_not_propagated() {
        // NaN must not flow through: clamped sides and a valid export rect.
        let layers = vec![layer("r", f64::NAN, 0.0, f64::NAN, 8.0)];
        let rect = resolve_export_rect(&layers, None).unwrap();
        assert!(rect[2] >= 1 && rect[3] >= 1);
        let sources = vec![solid(8, 8, [255, 0, 0, 255])];
        let img = compose_to_image(&layers, sources, rect, None).unwrap();
        assert_eq!(img.width(), rect[2] as u32);
    }

    #[test]
    fn crop_takes_left_half_before_resize() {
        // A 2×1 red|blue source, cropped to the LEFT half and resized into a
        // 4×4 box: every pixel must be red (no blue bleeds in).
        let mut l = layer("r", 0.0, 0.0, 4.0, 4.0);
        l.crop = Some([0.0, 0.0, 0.5, 1.0]);
        let layers = vec![l];
        let mut src = RgbaImage::new(2, 1);
        src.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        src.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        let sources = vec![DynamicImage::ImageRgba8(src)];
        let rect = resolve_export_rect(&layers, None).unwrap();
        let img = compose_to_image(&layers, sources, rect, None).unwrap();
        for px in img.pixels() {
            assert_eq!(px.0, [255, 0, 0, 255], "left-half crop must be all red");
        }
    }

    #[test]
    fn crop_ignores_invalid_rects() {
        // Out-of-range fractions fall back to "no crop": the full image shows.
        let mut l = layer("r", 0.0, 0.0, 4.0, 4.0);
        l.crop = Some([f64::NAN, 0.0, 2.0, 1.0]);
        let layers = vec![l];
        let sources = vec![solid(2, 1, [255, 0, 0, 255])];
        let rect = resolve_export_rect(&layers, None).unwrap();
        let img = compose_to_image(&layers, sources, rect, None).unwrap();
        assert_eq!(img.get_pixel(3, 3).0, [255, 0, 0, 255]);
    }

    #[test]
    fn export_rect_rejects_non_positive_size() {
        let layers = vec![layer("r", 0.0, 0.0, 8.0, 8.0)];
        assert!(resolve_export_rect(&layers, Some([0.0, 0.0, 0.0, 8.0])).is_err());
        assert!(resolve_export_rect(&layers, Some([0.0, 0.0, -5.0, 8.0])).is_err());
    }

    #[test]
    fn hex_color_parsing() {
        assert_eq!(parse_hex_color("#ff0000"), Some([255, 0, 0, 255]));
        assert_eq!(parse_hex_color("00ff00"), Some([0, 255, 0, 255]));
        assert_eq!(parse_hex_color("#abc"), Some([170, 187, 204, 255]));
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(parse_hex_color(""), None);
    }

    #[test]
    fn compose_canvas_impl_rejects_unauthorized_paths() {
        let dir = scratch("guard");
        let whitelist = PathWhitelist::default();
        let req = ComposeRequest {
            layers: vec![layer(
                dir.join("x.png").to_string_lossy().as_ref(),
                0.0,
                0.0,
                8.0,
                8.0,
            )],
            background: None,
            export_rect: None,
            output_path: dir.join("out.png").to_string_lossy().to_string(),
            format: "png".to_string(),
            quality: 90,
        };
        // Nothing registered → every path is outside the whitelist.
        let err = compose_canvas_impl(&req, &whitelist).unwrap_err();
        assert!(err.contains("outside"), "got: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compose_canvas_impl_end_to_end_writes_file() {
        let dir = scratch("e2e");
        let src = dir.join("src.png");
        solid(8, 8, [255, 0, 0, 255]).save(&src).unwrap();

        let whitelist = PathWhitelist::default();
        whitelist.register(&[src.to_string_lossy().to_string()]);
        let out = dir.join("out.png");
        // register() whitelists the parent dir too, so the unregistered
        // output path inside it is accepted — this is the documented
        // dialog-save flow (frontend registers, backend validates).
        let req = ComposeRequest {
            layers: vec![ComposeLayer {
                rotation_degrees: 45.0,
                ..layer(src.to_string_lossy().as_ref(), 0.0, 0.0, 16.0, 16.0)
            }],
            background: Some("#ffffff".to_string()),
            export_rect: None,
            output_path: out.to_string_lossy().to_string(),
            format: "png".to_string(),
            quality: 90,
        };
        let result = compose_canvas_impl(&req, &whitelist).expect("compose must succeed");
        assert_eq!(result.width, result.height, "45° AABB must be square");
        assert!(result.output_size > 0);
        assert!(out.exists(), "output file must be written");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
