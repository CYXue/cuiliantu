// Canvas compose view types.

// One free-form layer on the canvas, backed by a source image file.
//
// `x/y/width/height` describe the UNROTATED box in canvas pixels; `rotation`
// rotates the content clockwise around the box center (CSS `rotate(θ)`),
// applied after the flips — the same contract as the Rust compositor in
// `src-tauri/src/commands/canvas.rs`.
export interface CanvasLayer {
  id: string;
  // Source file path (full-quality read at export time)
  path: string;
  name: string;
  // Display-only data-URL thumbnail (bounded to 2048 px by the backend);
  // never used for export.
  src: string;
  x: number;
  y: number;
  width: number;
  height: number;
  rotation: number;
  flipH: boolean;
  flipV: boolean;
  // 0..100
  opacity: number;
  visible: boolean;
  // Source-pixel crop rectangle, normalized 0..1 relative to the original
  // image: [fx, fy, fw, fh]. null/undefined = show the whole image. The
  // compositor crops FIRST, then resizes to width/height.
  crop?: [number, number, number, number] | null;
  // Original pixel size of the source file
  naturalWidth: number;
  naturalHeight: number;
}

// Payload for adding a layer (position assigned by the store).
export interface CanvasLayerInput {
  path: string;
  name: string;
  src: string;
  width: number;
  height: number;
  naturalWidth: number;
  naturalHeight: number;
}

export type CanvasBackgroundMode = "transparent" | "white" | "black" | "custom";

export type CanvasAlignDirection = "horizontal" | "vertical";

// Top-level app view: the batch pipeline or the canvas compose stage.
export type ViewMode = "batch" | "canvas";

// Result line shown after a successful canvas export.
export interface CanvasExportResult {
  name: string;
  width: number;
  height: number;
  sizeBytes: number;
}
