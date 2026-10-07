// Single source of truth = src-tauri/supported_extensions.json.
// The Rust side reads the exact same file via `include_str!`, so the
// extension lists can never drift between frontend and backend. Only derive
// the arrays here; do NOT hardcode extensions.
import extensions from "../../src-tauri/supported_extensions.json";

export const ACCEPTED_IMAGE_EXTENSIONS: string[] = extensions.input.map(
  (e) => e.ext,
);

export const WATERMARK_IMAGE_EXTENSIONS: string[] = [...extensions.watermark];
