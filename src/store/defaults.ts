import type { ProcessingOptions } from "../types";

// Factory defaults for the processing options. Shared by the app store
// (initial state) and the built-in template definitions, so both stay in sync.
export const defaultOptions: ProcessingOptions = {
  format: "webp",
  quality: 80,
  width: null,
  height: null,
  keep_metadata: false,
  compression: "lossy",
  // Integrated features
  resize_mode: "pixels",
  resize_percent: 100,
  fit_mode: "stretch",
  rotate: 0,
  flip_horizontal: false,
  flip_vertical: false,
  watermark: null,
  quantize_colors: null,
  filename_pattern: "{name}",
  conflict_policy: "overwrite",
  skip_if_larger: false,
  preserve_animation: true,
  target_size_bytes: null,
  // Image editing (P0) — everything off by default
  crop_rect: null,
  crop_ratio: null,
  rotate_degrees: 0,
  brightness: 0,
  contrast: 0,
  hue: 0,
  sharpen: 0,
  blur: 0,
  grayscale: false,
  invert: false,
  sepia: false,
  auto_contrast: false,
};

// Merge a (possibly older, partial) template payload over the factory
// defaults. Applying a template is a full snapshot replace, so options that
// were introduced after the template was saved would otherwise come out as
// `undefined` and leak into IPC payloads.
export function mergeDefaults(
  partial: Partial<ProcessingOptions>,
): ProcessingOptions {
  return { ...defaultOptions, ...partial };
}
