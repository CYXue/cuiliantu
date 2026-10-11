// Single source of truth = src-tauri/supported_extensions.json.
// The Rust side reads the exact same file via `include_str!`, so the
// extension lists can never drift between frontend and backend. Only derive
// the arrays here; do NOT hardcode extensions.
import extensions from "../../src-tauri/supported_extensions.json";

export const ACCEPTED_IMAGE_EXTENSIONS: string[] = extensions.input.map(
  (e) => e.ext,
);

export const WATERMARK_IMAGE_EXTENSIONS: string[] = [...extensions.watermark];

// ---- Shared magic numbers ---------------------------------------------------
// Preview panel: slider drags re-request the preview after this quiet period.
export const PREVIEW_DEBOUNCE_MS = 300;
// Watermark text inputs: same idea, shorter (typing, not dragging).
export const TEXT_SYNC_DEBOUNCE_MS = 250;
// FileList row entrance: per-row animation delay.
export const STAGGER_DELAY_MS = 30;
// Paste path: String.fromCharCode spread limit (stack safety on big buffers).
export const BASE64_CHUNK_SIZE = 0x8000;
// Frontend mirror of the backend `MAX_RESIZE_DIMENSION` (processor.rs): both
// clamps must agree so the displayed value matches what a batch run will use.
export const MAX_DIMENSION = 65535;
