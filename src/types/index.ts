// Image formats
export type InputFormat = "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp";
export type OutputFormat = "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp";

// Compression type
export type CompressionType = "lossy" | "lossless";

// Resize interpretation (Converseen-style)
export type ResizeMode = "pixels" | "percent";

// How the image fits into the pixel box
export type FitMode = "stretch" | "contain" | "cover";

// Watermark anchor position
export type WatermarkPosition =
  | "top_left"
  | "top_right"
  | "bottom_right"
  | "bottom_left"
  | "center";

// Watermark: picture overlay or text (embedded Smiley Sans font)
export type WatermarkConfig =
  | {
      kind: "image";
      path: string;
      opacity: number;
      scale_percent: number;
      position: WatermarkPosition;
      margin_percent: number;
    }
  | {
      kind: "text";
      text: string;
      font_size: number;
      color: string;
      opacity: number;
      position: WatermarkPosition;
      margin_percent: number;
    };

export type ImageWatermark = Extract<WatermarkConfig, { kind: "image" }>;
export type TextWatermark = Extract<WatermarkConfig, { kind: "text" }>;

// Behaviour when an output file already exists
export type ConflictPolicy = "overwrite" | "auto_rename" | "skip";

// Processing options
export interface ProcessingOptions {
  format: OutputFormat;
  quality: number;
  width: number | null;
  height: number | null;
  keep_metadata: boolean;
  compression: CompressionType;
  // --- integrated features ---
  resize_mode: ResizeMode;
  resize_percent: number;
  fit_mode: FitMode;
  rotate: number;
  flip_horizontal: boolean;
  flip_vertical: boolean;
  watermark: WatermarkConfig | null;
  quantize_colors: number | null;
  filename_pattern: string;
  conflict_policy: ConflictPolicy;
  skip_if_larger: boolean;
  preserve_animation: boolean;
  // Target size in bytes: when set, JPEG output auto-bisects quality to fit
  // the budget; null uses the fixed `quality` slider.
  target_size_bytes: number | null;
  // --- Image editing (P0, docs/IMAGE_EDITING_RESEARCH_2026-09-27.md) ---
  // Interactive drag-crop rect [fx, fy, fw, fh] (normalized 0..1; a box drawn
  // on one image maps proportionally onto the whole batch); null = off.
  crop_rect: [number, number, number, number] | null;
  // Center-crop to a ratio [w, h] (e.g. [1,1] square); null = off.
  crop_ratio: [number, number] | null;
  // Fine rotation in degrees (-180..180, canvas auto-expands, transparent
  // corners); 0 = off.
  rotate_degrees: number;
  // Additive brightness -100..100 (0 = identity)
  brightness: number;
  // Contrast -100..100 (0 = identity)
  contrast: number;
  // Hue rotation in degrees -180..180 (0 = identity)
  hue: number;
  // USM sharpen amount 0..100 (0 = off)
  sharpen: number;
  // Gaussian blur sigma 0..50 (0 = off)
  blur: number;
  grayscale: boolean;
  invert: boolean;
  sepia: boolean;
  // Auto contrast (1%/99% percentile stretch)
  auto_contrast: boolean;
}

// Saved parameter set (template). Built-ins ship with the app (their `name`
// is an i18n key and the id is prefixed "builtin:"); custom ones are
// persisted to localStorage in array order.
export interface OptionsTemplate {
  id: string;
  name: string;
  options: ProcessingOptions;
}

// Processing result for single image
export interface ProcessingResult {
  original_path: string;
  output_path: string;
  original_size: number;
  output_size: number;
  reduction_percent: number;
  success: boolean;
  error: string | null;
  // True when nothing was written (not smaller / name conflict)
  skipped: boolean;
  // True when output fit the requested target size (JPEG target-size mode only)
  within_target: boolean;
}

// Batch processing statistics
export interface BatchStats {
  total_files: number;
  processed_files: number;
  successful_files: number;
  failed_files: number;
  total_original_size: number;
  total_output_size: number;
  overall_reduction_percent: number;
  average_reduction_percent: number;
  median_reduction_percent: number;
}

// Progress update
export interface ProgressUpdate {
  current: number;
  total: number;
  current_file: string;
  percent: number;
}

// Real image type detected from a file's signature bytes.
export type DetectedFormat =
  | "jpeg"
  | "png"
  | "gif"
  | "bmp"
  | "tiff"
  | "webp"
  | "heic"
  | "avif"
  | "jxl"
  | "unknown";

// Result of the `detect_file_type` backend command.
export interface FileTypeInfo {
  path: string;
  extension: string;
  detected_format: DetectedFormat;
  detected_mime: string;
  matches_extension: boolean;
  width: number | null;
  height: number | null;
  size_bytes: number;
}

// One entry of the batch `detect_file_types` backend command: either the
// detected info or an error message (one bad file must not fail the drop).
export interface FileTypeDetectResult {
  path: string;
  info?: FileTypeInfo;
  error?: string;
}

// File processing status
export type FileStatus = "pending" | "processing" | "completed" | "error";

// File item for UI
export interface FileItem {
  path: string;
  name: string;
  size: number;
  preview?: string;
  status: FileStatus;
  outputPath?: string;
  outputSize?: number;
  reductionPercent?: number;
  error?: string;
  // Nothing was written (not smaller / name conflict)
  skipped?: boolean;
  // Detected real type (from detect_file_type)
  detectedFormat?: DetectedFormat | null;
  extension?: string;
  matchesExtension?: boolean;
  detectedMime?: string;
  detectedWidth?: number | null;
  detectedHeight?: number | null;
  // Output exceeded the requested target size (JPEG target-size mode only)
  withinTarget?: boolean;
}

// App state
export type ProcessingState = "idle" | "processing" | "completed" | "error";
