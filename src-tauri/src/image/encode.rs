//! Output encoding: per-format encoders, quantization and the GIF frame path.
//!
//! Split out of the former `processor.rs` monolith. This module implements the
//! encode half of `ImageProcessor`; option types and the pipeline stay in
//! `processor.rs`.

use color_quant::NeuQuant;
use image::codecs::gif::{GifDecoder, GifEncoder};
use image::{AnimationDecoder, DynamicImage, Frame, ImageFormat, RgbImage};
use std::io::{BufReader, Cursor};
use std::path::Path;

use super::processor::{CompressionType, OutputFormat, ProcessError, ProcessingOptions};

impl super::processor::ImageProcessor {
    /// NeuQuant color reduction for PNG/GIF output (Imagine-style).
    pub(super) fn apply_quantize(img: DynamicImage, options: &ProcessingOptions) -> DynamicImage {
        let Some(n) = options.output.quantize_colors else {
            return img;
        };
        if !matches!(options.output.format, OutputFormat::Png | OutputFormat::Gif) {
            return img;
        }

        let n = (n as usize).clamp(2, 256);
        let mut rgba = img.into_rgba8();
        {
            let flat: &[u8] = rgba.as_raw();
            let nq = NeuQuant::new(10, n, flat);
            for px in rgba.pixels_mut() {
                nq.map_pixel(&mut px.0);
            }
        }
        DynamicImage::ImageRgba8(rgba)
    }

    /// [`Self::encode_jpeg_rgb`] from a `DynamicImage`: converts to RGB8
    /// first (one full-image copy per call — the target-size bisection uses
    /// [`Self::encode_jpeg_rgb`] directly so it converts only once).
    pub(super) fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, ProcessError> {
        Self::encode_jpeg_rgb(&img.to_rgb8(), quality)
    }

    /// Encode a baseline RGB JPEG at a fixed quality over an
    /// already-converted RGB buffer: the target-size bisection re-encodes up
    /// to ~7 times, and converting to RGB8 on every iteration cloned the
    /// whole image each round.
    fn encode_jpeg_rgb(rgb: &RgbImage, quality: u8) -> Result<Vec<u8>, ProcessError> {
        let mut cursor = Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
        rgb.write_with_encoder(encoder)
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

        // Convert once; every bisection step reuses this buffer.
        let rgb = img.to_rgb8();
        let limit = limit as usize;
        let (mut lo, mut hi) = (MIN_SEARCH_QUALITY, MAX_SEARCH_QUALITY);
        let mut best: Option<(Vec<u8>, u8)> = None;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let encoded = Self::encode_jpeg_rgb(&rgb, mid)?;
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
                let bytes = Self::encode_jpeg_rgb(&rgb, MIN_SEARCH_QUALITY)?;
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
                Self::encode_jpeg(img, options.output.quality)
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

    /// Re-encode with the `image` crate's own encoders (PNG/GIF/BMP/TIFF).
    fn encode_native(img: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, ProcessError> {
        let mut cursor = Cursor::new(Vec::new());
        img.write_to(&mut cursor, format)
            .map_err(|e| ProcessError::WriteError(e.to_string()))?;
        Ok(cursor.into_inner())
    }

    /// Encode animated frames back into a GIF byte stream.
    pub(super) fn encode_frames_to_memory(frames: Vec<Frame>) -> Result<Vec<u8>, ProcessError> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut encoder = GifEncoder::new(&mut cursor);
            encoder
                .encode_frames(frames)
                .map_err(|e| ProcessError::WriteError(e.to_string()))?;
        }
        Ok(cursor.into_inner())
    }

    /// Decode an animated GIF into its frames (with per-frame disposal
    /// already applied by the `image` crate).
    pub(super) fn decode_gif_frames(path: &Path) -> Result<Vec<Frame>, ProcessError> {
        let file = std::fs::File::open(path).map_err(|e| ProcessError::ReadError(e.to_string()))?;
        let decoder = GifDecoder::new(BufReader::new(file))
            .map_err(|e| ProcessError::ReadError(e.to_string()))?;
        decoder
            .into_frames()
            .collect::<image::ImageResult<Vec<Frame>>>()
            .map_err(|e| ProcessError::ReadError(e.to_string()))
    }
}
