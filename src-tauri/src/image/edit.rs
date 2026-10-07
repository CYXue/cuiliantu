//! Image editing primitives (P0 of `docs/IMAGE_EDITING_RESEARCH_2026-09-27.md`).
//!
//! Every function here is pure: it takes a [`DynamicImage`] and returns a new
//! one. [`super::processor`] decides *whether* an edit applies; this module
//! decides *how*. All ops that change colors return RGBA8 so a single pixel
//! type flows through the rest of the pipeline (the watermark overlay and
//! alpha-preserving encoders expect RGBA).

use image::{DynamicImage, Rgba, RgbaImage};

/// Crop a normalized fractional rectangle `[fx, fy, fw, fh]` (each `0.0..=1.0`,
/// relative to the current image). This is the backend of the interactive
/// drag-crop in the preview panel: the box is drawn on ONE image and stored as
/// fractions, so a batch run maps it proportionally onto every image.
///
/// Values are clamped into range; the rectangle is intersected with the image
/// bounds and guarded to at least 1×1 px, so a degenerate box can never panic
/// or produce an empty crop.
pub fn crop_fraction(img: &DynamicImage, rect: [f64; 4]) -> DynamicImage {
    let [fx, fy, fw, fh] = rect;
    let (w, h) = (img.width(), img.height());
    let clamp01 = |v: f64| v.clamp(0.0, 1.0);
    let fx = clamp01(fx);
    let fy = clamp01(fy);
    let fw = clamp01(fw);
    let fh = clamp01(fh);

    let x = (fx * w as f64).round() as u32;
    let y = (fy * h as f64).round() as u32;
    let cw = ((fw * w as f64).round() as u32).max(1);
    let ch = ((fh * h as f64).round() as u32).max(1);

    // Intersect with the image and keep at least 1×1 px.
    let cw = cw.min(w.saturating_sub(x)).max(1);
    let ch = ch.min(h.saturating_sub(y)).max(1);
    img.crop_imm(x, y, cw, ch)
}

/// Center-crop to a target aspect ratio (`rw:rh`), keeping the largest crop
/// that fits inside the current dimensions.
///
/// This is the batch-friendly replacement for a per-image interactive crop
/// rect (one crop ratio applied to many images of different sizes is what a
/// batch tool can actually offer — XnConvert ships the same interaction).
pub fn center_crop_to_ratio(img: &DynamicImage, rw: u32, rh: u32) -> DynamicImage {
    let (rw, rh) = (rw.max(1), rh.max(1));
    let (w, h) = (img.width(), img.height());
    let target = rw as f64 / rh as f64;
    let current = w as f64 / h as f64;

    let (cw, ch) = if current > target {
        // Too wide: cut width to the target ratio, keep the full height.
        let cw = (((h as f64) * target).floor() as u32).clamp(1, w);
        (cw, h)
    } else {
        // Too tall (or exact): cut height, keep the full width.
        let ch = ((w as f64 / target).floor() as u32).clamp(1, h);
        (w, ch)
    };

    let x = (w - cw) / 2;
    let y = (h - ch) / 2;
    img.crop_imm(x, y, cw, ch)
}

/// Rotate clockwise by `degrees` with canvas auto-expansion: the output canvas
/// grows to the bounding box of the rotated rectangle and the corners become
/// transparent. Exact 90/180/270 inputs shortcut to the lossless fast path.
///
/// Implementation note: `imageproc::rotate_about_center` keeps the input
/// dimensions (corners get clipped), so we first paste the source centered on
/// a transparent canvas of `max(source, expanded)` size — the centers then
/// coincide — rotate about that shared center, and crop the expanded bounding
/// box back out. Pasting can never produce a negative offset because the
/// canvas is at least as large as the source on both axes.
pub fn rotate_with_expansion(img: &DynamicImage, degrees: f64) -> DynamicImage {
    let deg = degrees.rem_euclid(360.0);
    if deg.abs() < 1e-6 {
        return img.clone();
    }
    // Lossless quarter turns.
    for (angle, quarter) in [(90.0, 1u32), (180.0, 2), (270.0, 3)] {
        if (deg - angle).abs() < 1e-6 {
            return match quarter {
                1 => img.rotate90(),
                2 => img.rotate180(),
                _ => img.rotate270(),
            };
        }
    }

    let theta = (deg as f32).to_radians();
    let (w, h) = (img.width() as f64, img.height() as f64);
    let sin_a = theta.sin().abs() as f64;
    let cos_a = theta.cos().abs() as f64;
    // Bounding box of the rotated rectangle.
    let nw = ((w * cos_a + h * sin_a).ceil() as u32).max(1);
    let nh = ((w * sin_a + h * cos_a).ceil() as u32).max(1);

    let base = img.to_rgba8();
    let cw = nw.max(base.width());
    let ch = nh.max(base.height());
    let mut canvas = RgbaImage::new(cw, ch);
    let ox = ((cw - base.width()) / 2) as i64;
    let oy = ((ch - base.height()) / 2) as i64;
    image::imageops::overlay(&mut canvas, &base, ox, oy);

    use imageproc::geometric_transformations::{rotate_about_center, Interpolation};
    let rotated = rotate_about_center(&canvas, theta, Interpolation::Bilinear, Rgba([0, 0, 0, 0]));

    // Crop the expanded bounding box out of the (possibly larger) canvas.
    let x = (cw - nw) / 2;
    let y = (ch - nh) / 2;
    DynamicImage::ImageRgba8(image::imageops::crop_imm(&rotated, x, y, nw, nh).to_image())
}

/// Map RGB per pixel, preserving alpha. Returns RGBA8.
///
/// Why not `imageops::brighten`/`contrast`/`huerotate` directly? Those use
/// `pixel.map`, which walks **every** subpixel including alpha — a
/// half-transparent PNG would get its alpha re-scaled too. These helpers keep
/// alpha bit-exact, which is what an editor must do.
fn map_rgb(img: &DynamicImage, f: impl Fn(u8, u8, u8) -> (u8, u8, u8)) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    for px in rgba.pixels_mut() {
        let [r, g, b, a] = px.0;
        let (r, g, b) = f(r, g, b);
        *px = Rgba([r, g, b, a]);
    }
    DynamicImage::ImageRgba8(rgba)
}

fn clamp8(v: f32) -> u8 {
    // Round, don't truncate: an identity pass through f32 math must land on
    // the same byte (199.99998 must become 200, not 199).
    v.clamp(0.0, 255.0).round() as u8
}

/// Additive brightness per channel (`-255..=255`), alpha preserved.
pub fn brighten_rgb(img: &DynamicImage, add: i32) -> DynamicImage {
    let add = add as f32;
    map_rgb(img, |r, g, b| {
        (
            clamp8(r as f32 + add),
            clamp8(g as f32 + add),
            clamp8(b as f32 + add),
        )
    })
}

/// Contrast around mid-gray, `-100..=100`, `0` = identity. Uses the
/// `imageops::contrast` curve (`percent = ((100 + c) / 100)²`) but on RGB
/// only, so alpha survives.
pub fn contrast_rgb(img: &DynamicImage, contrast: f32) -> DynamicImage {
    let percent = ((100.0 + contrast) / 100.0).powi(2);
    map_rgb(img, |r, g, b| {
        let ch = |v: u8| clamp8(((v as f32 / 255.0 - 0.5) * percent + 0.5) * 255.0);
        (ch(r), ch(g), ch(b))
    })
}

/// Hue rotation in degrees using the CSS/SVG `feColorMatrix hueRotate`
/// matrix on RGB (alpha preserved). The matrix rows each sum to 1, so
/// grays and the 0° input are exact identities.
pub fn hue_rotate_rgb(img: &DynamicImage, degrees: f32) -> DynamicImage {
    let theta = degrees.to_radians();
    let (sin, cos) = theta.sin_cos();
    // Third row (luminance weights) of the standard matrix.
    const LR: f32 = 0.213;
    const LG: f32 = 0.715;
    const LB: f32 = 0.072;
    let m = [
        [
            LR + cos * (1.0 - LR) + sin * -LR,
            LG + cos * -LG + sin * -LG,
            LB + cos * -LB + sin * (1.0 - LB),
        ],
        [
            LR + cos * -LR + sin * 0.143,
            LG + cos * (1.0 - LG) + sin * 0.140,
            LB + cos * -LB + sin * -0.283,
        ],
        [
            LR + cos * -LR + sin * -(1.0 - LR),
            LG + cos * -LG + sin * LG,
            LB + cos * (1.0 - LB) + sin * LB,
        ],
    ];
    map_rgb(img, |r, g, b| {
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        (
            clamp8(m[0][0] * rf + m[0][1] * gf + m[0][2] * bf),
            clamp8(m[1][0] * rf + m[1][1] * gf + m[1][2] * bf),
            clamp8(m[2][0] * rf + m[2][1] * gf + m[2][2] * bf),
        )
    })
}

/// Invert RGB channels, alpha preserved.
pub fn invert_rgb(img: &DynamicImage) -> DynamicImage {
    map_rgb(img, |r, g, b| (255 - r, 255 - g, 255 - b))
}

/// Classic sepia color matrix applied per pixel (alpha preserved).
pub fn apply_sepia(img: &DynamicImage) -> DynamicImage {
    let rgba = img.to_rgba8();
    let mut out = rgba;
    for px in out.pixels_mut() {
        let [r, g, b, a] = px.0;
        let (r, g, b) = (r as f32, g as f32, b as f32);
        let nr = (0.393 * r + 0.769 * g + 0.189 * b)
            .round()
            .clamp(0.0, 255.0);
        let ng = (0.349 * r + 0.686 * g + 0.168 * b)
            .round()
            .clamp(0.0, 255.0);
        let nb = (0.272 * r + 0.534 * g + 0.131 * b)
            .round()
            .clamp(0.0, 255.0);
        *px = Rgba([nr as u8, ng as u8, nb as u8, a]);
    }
    DynamicImage::ImageRgba8(out)
}

/// Percentile contrast stretch: build a luma histogram, find the 1% / 99%
/// percentiles (`lo`, `hi`) and remap every RGB channel linearly from
/// `[lo, hi]` onto the full `[0, 255]` range. Alpha is preserved. A single
/// luma histogram (rather than per-channel) avoids introducing color casts.
///
/// Already full-range images map to themselves (identity), so the op is safe
/// to leave enabled.
pub fn auto_contrast_stretch(img: &DynamicImage) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return img.clone();
    }

    let mut hist = [0u64; 256];
    for px in rgba.pixels() {
        let [r, g, b, _] = px.0;
        let luma = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64).round() as usize;
        hist[luma.min(255)] += 1;
    }

    let total = (w as u64) * (h as u64);
    let low_cut = (total as f64 * 0.01).floor();
    let high_cut = (total as f64 * 0.99).floor();

    let mut cum = 0u64;
    let mut lo = 0usize;
    for (v, &count) in hist.iter().enumerate() {
        cum += count;
        if cum as f64 >= low_cut.max(1.0) {
            lo = v;
            break;
        }
    }
    cum = 0;
    let mut hi = 255usize;
    for (v, &count) in hist.iter().enumerate() {
        cum += count;
        if cum as f64 >= high_cut.max(1.0) {
            hi = v;
            break;
        }
    }

    if hi <= lo + 1 {
        return img.clone(); // flat or single-tone image: nothing to stretch
    }

    let scale = 255.0 / (hi - lo) as f32;
    let mut out = rgba;
    for px in out.pixels_mut() {
        let [r, g, b, a] = px.0;
        let stretch = |v: u8| -> u8 {
            let stretched = (v as f32 - lo as f32) * scale;
            stretched.round().clamp(0.0, 255.0) as u8
        };
        *px = Rgba([stretch(r), stretch(g), stretch(b), a]);
    }
    DynamicImage::ImageRgba8(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    /// A solid-color image of the given size.
    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, Rgba(rgba)))
    }

    #[test]
    fn fractional_crop_maps_clamps_and_intersects() {
        // Full frame is identity.
        let img = solid(200, 100, [5, 6, 7, 255]);
        assert_eq!(
            crop_fraction(&img, [0.0, 0.0, 1.0, 1.0]).dimensions(),
            (200, 100)
        );

        // Left half of 200x100 → 100x100.
        let out = crop_fraction(&img, [0.0, 0.0, 0.5, 1.0]);
        assert_eq!(out.dimensions(), (100, 100));

        // Out-of-range values clamp into 0..=1 (fw>1 stops at the image edge).
        let out = crop_fraction(&img, [-0.5, -0.5, 2.0, 2.0]);
        assert_eq!(out.dimensions(), (200, 100));

        // A box hanging off the bottom-right edge is intersected, not padded.
        let out = crop_fraction(&img, [0.75, 0.75, 0.5, 0.5]);
        assert_eq!(out.dimensions(), (50, 25));

        // A degenerate (zero-area) box still yields 1x1 instead of panicking.
        let out = crop_fraction(&img, [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(out.dimensions(), (1, 1));
    }

    #[test]
    fn center_crop_keeps_ratio_and_contents() {
        // 400x200 (2:1) cropped to 1:1 must be 200x200, centered.
        let img = solid(400, 200, [10, 20, 30, 255]);
        let out = center_crop_to_ratio(&img, 1, 1);
        assert_eq!((out.width(), out.height()), (200, 200));

        // 200x400 cropped to 16:9 → width kept, height cut to 200/16*9=112.
        let out = center_crop_to_ratio(&solid(200, 400, [0, 0, 0, 255]), 16, 9);
        assert_eq!((out.width(), out.height()), (200, 112));

        // Extreme ratio clamps to at least 1px.
        let out = center_crop_to_ratio(&solid(10, 10, [0, 0, 0, 255]), 1000, 1);
        assert_eq!((out.width(), out.height()), (10, 1));
    }

    #[test]
    fn fine_rotation_expands_canvas_and_snaps_quarter_turns() {
        // 100x50 rotated 30°: bbox = 100·cos30 + 50·sin30 ≈ 111.6, 50·cos30 + 100·sin30 ≈ 93.3
        let img = solid(100, 50, [255, 0, 0, 255]);
        let out = rotate_with_expansion(&img, 30.0);
        assert_eq!(out.width(), 112);
        assert_eq!(out.height(), 94); // ceil(93.30) = 94
                                      // Corner must be transparent (canvas expansion), center opaque.
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.get_pixel(56, 47).0, [255, 0, 0, 255]);

        // Exact quarter turns take the lossless path (dimensions swap/persist).
        let out = rotate_with_expansion(&solid(100, 50, [1, 2, 3, 255]), 90.0);
        assert_eq!((out.width(), out.height()), (50, 100));
        let out = rotate_with_expansion(&solid(100, 50, [1, 2, 3, 255]), 180.0);
        assert_eq!((out.width(), out.height()), (100, 50));

        // 0° is an identity clone.
        let img = solid(30, 40, [9, 9, 9, 255]);
        let out = rotate_with_expansion(&img, 0.0);
        assert_eq!((out.width(), out.height()), (30, 40));
        assert_eq!(out.get_pixel(0, 0).0, [9, 9, 9, 255]);

        // 360° normalizes to identity.
        let out = rotate_with_expansion(&img, 360.0);
        assert_eq!((out.width(), out.height()), (30, 40));
    }

    #[test]
    fn sepia_matches_known_matrix_on_primary_colors() {
        // Pure red (255,0,0) → (0.393·255, 0.349·255, 0.272·255) = (100, 89, 69).
        let out = apply_sepia(&solid(1, 1, [255, 0, 0, 255]));
        assert_eq!(out.get_pixel(0, 0).0, [100, 89, 69, 255]);

        // Pure blue (0,0,255) → (0.189·255, 0.168·255, 0.131·255) = (48, 43, 33).
        let out = apply_sepia(&solid(1, 1, [0, 0, 255, 255]));
        assert_eq!(out.get_pixel(0, 0).0, [48, 43, 33, 255]);

        // Alpha preserved.
        let out = apply_sepia(&solid(1, 1, [255, 255, 255, 128]));
        assert_eq!(out.get_pixel(0, 0).0[3], 128);
    }

    #[test]
    fn auto_contrast_stretches_narrow_range_to_full() {
        // Gray ramp compressed into 100..154 (55 levels): after a 1%/99%
        // stretch it must reach (near) 0..255.
        let mut img = RgbaImage::new(256, 4);
        for (x, _, px) in img.enumerate_pixels_mut() {
            let v = (100 + (x % 55)) as u8;
            *px = Rgba([v, v, v, 255]);
        }
        let img = DynamicImage::ImageRgba8(img);
        let out = auto_contrast_stretch(&img);
        let (min, max) = out
            .to_rgba8()
            .pixels()
            .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p.0[0]), hi.max(p.0[0])));
        assert!(min <= 2, "dark end must stretch to ~0, got {min}");
        assert!(max >= 253, "bright end must stretch to ~255, got {max}");

        // An already full-range ramp is (close to) identity.
        let mut full = RgbaImage::new(256, 1);
        for (x, _, px) in full.enumerate_pixels_mut() {
            *px = Rgba([x as u8, x as u8, x as u8, 255]);
        }
        let out = auto_contrast_stretch(&DynamicImage::ImageRgba8(full));
        assert_eq!(out.get_pixel(0, 0).0[0], 0);
        assert_eq!(out.get_pixel(255, 0).0[0], 255);

        // Alpha survives.
        let out = auto_contrast_stretch(&solid(2, 2, [50, 50, 50, 77]));
        assert_eq!(out.get_pixel(0, 0).0[3], 77);
    }

    #[test]
    fn color_ops_preserve_alpha_and_identity() {
        let img = solid(2, 2, [200, 100, 50, 96]);

        // Alpha must survive every RGB-only op.
        for out in [
            brighten_rgb(&img, 40),
            contrast_rgb(&img, 60.0),
            hue_rotate_rgb(&img, 120.0),
            invert_rgb(&img),
        ] {
            assert_eq!(out.get_pixel(0, 0).0[3], 96, "alpha must be preserved");
        }

        // Zero-valued tonal edits are exact identities.
        assert_eq!(brighten_rgb(&img, 0).get_pixel(0, 0).0, [200, 100, 50, 96]);
        assert_eq!(
            contrast_rgb(&img, 0.0).get_pixel(0, 0).0,
            [200, 100, 50, 96]
        );
        assert_eq!(
            hue_rotate_rgb(&img, 0.0).get_pixel(0, 0).0,
            [200, 100, 50, 96]
        );

        // Inversion of a primary color.
        let red = solid(1, 1, [255, 0, 0, 255]);
        assert_eq!(invert_rgb(&red).get_pixel(0, 0).0, [0, 255, 255, 255]);

        // Brightness clamps at both ends.
        let black = solid(1, 1, [0, 0, 0, 255]);
        assert_eq!(brighten_rgb(&black, -30).get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(
            brighten_rgb(&black, 128).get_pixel(0, 0).0,
            [128, 128, 128, 255]
        );

        // Grays are hue-rotation invariant (matrix rows each sum to 1).
        let gray = solid(1, 1, [128, 128, 128, 255]);
        assert_eq!(
            hue_rotate_rgb(&gray, 73.0).get_pixel(0, 0).0,
            [128, 128, 128, 255]
        );
    }
}
