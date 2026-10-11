// Pure geometry helpers for the canvas compose view.
//
// The rotated-AABB formula is the SAME one the Rust compositor uses
// (`src-tauri/src/commands/canvas.rs::rotated_aabb`, including the 90°
// snap) — keep the two in sync or selection handles drift off the exported
// pixels.
import type { CanvasAlignDirection, CanvasLayer } from "../types/canvas";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

// Mirror of the Rust MAX_COMPOSE_SIDE / MIN_LAYER_SIDE bounds.
export const MAX_LAYER_SIDE = 12000;
export const MIN_LAYER_SIDE = 1;

/** Smallest interactive box on canvas (below this a drag handle is unusable). */
export const MIN_UI_SIDE = 8;

/** `|sin|` and `|cos|` with multiples of 90° snapped to exact 0/1 — the
 * floating-point speck in `cos(90°)` would otherwise be amplified by ceil()
 * into an extra pixel row on export. */
export function absSinCos(degrees: number): { sin: number; cos: number } {
  const clean = (v: number): number => {
    const a = Math.abs(v);
    if (Math.abs(a - 1) < 1e-9) return 1;
    if (a < 1e-9) return 0;
    return a;
  };
  const r = (degrees * Math.PI) / 180;
  return { sin: clean(Math.sin(r)), cos: clean(Math.cos(r)) };
}

/** Size of the axis-aligned bounding box of a `w×h` box rotated clockwise
 * by `degrees`. Non-finite inputs collapse to 1. */
export function rotatedAabb(
  w: number,
  h: number,
  degrees: number,
): { width: number; height: number } {
  if (
    !w ||
    !h ||
    !Number.isFinite(w) ||
    !Number.isFinite(h) ||
    !Number.isFinite(degrees)
  ) {
    return { width: 1, height: 1 };
  }
  const { sin, cos } = absSinCos(degrees);
  return {
    width: w * cos + h * sin,
    height: w * sin + h * cos,
  };
}

export function layerCenter(layer: CanvasLayer): { x: number; y: number } {
  return { x: layer.x + layer.width / 2, y: layer.y + layer.height / 2 };
}

/** Tight bounding box over all VISIBLE layers (rotated AABBs), or null when
 * there is nothing on the canvas. */
export function layersBBox(layers: CanvasLayer[]): Rect | null {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  let any = false;
  for (const layer of layers) {
    if (!layer.visible) continue;
    any = true;
    const box = rotatedAabb(layer.width, layer.height, layer.rotation);
    const c = layerCenter(layer);
    minX = Math.min(minX, c.x - box.width / 2);
    minY = Math.min(minY, c.y - box.height / 2);
    maxX = Math.max(maxX, c.x + box.width / 2);
    maxY = Math.max(maxY, c.y + box.height / 2);
  }
  if (!any) return null;
  return {
    x: Math.floor(minX),
    y: Math.floor(minY),
    width: Math.max(1, Math.ceil(maxX - Math.floor(minX))),
    height: Math.max(1, Math.ceil(maxY - Math.floor(minY))),
  };
}

/** The rectangle a canvas export will produce: the user selection when set,
 * otherwise the tight bbox of visible layers; null when nothing to export. */
export function exportRectOf(
  layers: CanvasLayer[],
  selection: [number, number, number, number] | null,
): Rect | null {
  if (selection) {
    const [x, y, w, h] = selection;
    if (![x, y, w, h].every(Number.isFinite) || w < 1 || h < 1) return null;
    return {
      x: Math.floor(x),
      y: Math.floor(y),
      width: Math.ceil(w),
      height: Math.ceil(h),
    };
  }
  return layersBBox(layers);
}

export function clampLayerSide(value: number): number {
  if (!Number.isFinite(value)) return MIN_LAYER_SIDE;
  return Math.min(MAX_LAYER_SIDE, Math.max(MIN_LAYER_SIDE, value));
}

/**
 * One-click tiling: lay visible layers out in z-order (bottom of the list
 * = first) along the chosen axis, keeping each layer's own size, centered
 * on the cross axis. Returns position patches to apply.
 */
export function layoutPatches(
  layers: CanvasLayer[],
  direction: CanvasAlignDirection,
  gap: number,
): { id: string; x: number; y: number }[] {
  const safeGap = Number.isFinite(gap) ? Math.max(0, gap) : 0;
  const visible = layers.filter((l) => l.visible);
  if (visible.length === 0) return [];

  const crossCenter =
    direction === "horizontal"
      ? visible.reduce((sum, l) => sum + l.y + l.height / 2, 0) / visible.length
      : visible.reduce((sum, l) => sum + l.x + l.width / 2, 0) / visible.length;

  // Walk layers in array (z) order along the layout axis.
  let cursor = 0;
  return visible.map((layer) => {
    let x: number;
    let y: number;
    if (direction === "horizontal") {
      x = cursor;
      y = crossCenter - layer.height / 2;
      cursor += layer.width + safeGap;
    } else {
      x = crossCenter - layer.width / 2;
      y = cursor;
      cursor += layer.height + safeGap;
    }
    return { id: layer.id, x: round2(x), y: round2(y) };
  });
}

function round2(v: number): number {
  return Math.round(v * 100) / 100;
}

/**
 * Handle-drag resize: scale a layer by dragging one of its four corner
 * handles (rotation must be 0 for handle dragging; callers hide handles
 * otherwise). `handle` names the grabbed corner; the OPPOSITE corner stays
 * anchored. Default keeps the aspect ratio — `free` (e.g. Shift) unlocks it.
 */
export function resizeLayerByHandle(
  layer: CanvasLayer,
  handle: "nw" | "ne" | "se" | "sw",
  pointer: { x: number; y: number },
  keepAspect: boolean,
): { x: number; y: number; width: number; height: number } {
  // Which side of the layer the (fixed) anchor sits on. Dragging a corner
  // pins the diagonally opposite corner.
  const anchorAt: Record<typeof handle, { right: boolean; bottom: boolean }> = {
    nw: { right: true, bottom: true },
    ne: { right: false, bottom: true },
    se: { right: false, bottom: false },
    sw: { right: true, bottom: false },
  };
  const a = anchorAt[handle];
  const anchor = {
    x: a.right ? layer.x + layer.width : layer.x,
    y: a.bottom ? layer.y + layer.height : layer.y,
  };

  const sx = Math.max(
    MIN_UI_SIDE / layer.width,
    Math.abs(pointer.x - anchor.x) / layer.width,
  );
  const sy = Math.max(
    MIN_UI_SIDE / layer.height,
    Math.abs(pointer.y - anchor.y) / layer.height,
  );
  const scale = keepAspect ? Math.max(sx, sy) : null;
  const width = clampLayerSide(
    scale !== null ? layer.width * scale : layer.width * sx,
  );
  const height = clampLayerSide(
    scale !== null ? layer.height * scale : layer.height * sy,
  );

  // The anchored corner keeps its canvas position; the box grows toward the
  // grabbed corner's direction.
  return {
    x: a.right ? anchor.x - width : anchor.x,
    y: a.bottom ? anchor.y - height : anchor.y,
    width,
    height,
  };
}

/**
 * Corner-handle resize for a ROTATED layer.
 *
 * The pointer is inverse-rotated into the layer's unrotated local frame
 * (center-preserving), the same axis-aligned math runs there, and the
 * resulting center displacement is rotated back into canvas space. The
 * layer's rotation is preserved.
 */
export function resizeRotatedLayerByHandle(
  layer: CanvasLayer,
  handle: "nw" | "ne" | "se" | "sw",
  pointer: { x: number; y: number },
  keepAspect: boolean,
): { x: number; y: number; width: number; height: number } {
  const c = layerCenter(layer);
  const rad = (layer.rotation * Math.PI) / 180;
  const cos = Math.cos(rad);
  const sin = Math.sin(rad);
  const dx = pointer.x - c.x;
  const dy = pointer.y - c.y;
  // Inverse of the clockwise screen-space rotation: the pointer as it would
  // be with the layer unrotated.
  const local = { x: cos * dx + sin * dy + c.x, y: -sin * dx + cos * dy + c.y };

  const next = resizeLayerByHandle(
    { ...layer, rotation: 0 },
    handle,
    local,
    keepAspect,
  );

  // The resize moved the center in the LOCAL frame; rotate that displacement
  // back so the visually anchored corner stays pinned.
  const cLocal = {
    x: layer.x + layer.width / 2,
    y: layer.y + layer.height / 2,
  };
  const newCLocal = { x: next.x + next.width / 2, y: next.y + next.height / 2 };
  const ddx = newCLocal.x - cLocal.x;
  const ddy = newCLocal.y - cLocal.y;
  return {
    x: c.x + (cos * ddx - sin * ddy) - next.width / 2,
    y: c.y + (sin * ddx + cos * ddy) - next.height / 2,
    width: next.width,
    height: next.height,
  };
}

// --- Layer cropping --------------------------------------------------------

/** Smallest crop fraction per axis (prevents cropping to nothing). */
export const MIN_CROP_FRACTION = 0.05;

export type CropRect = [number, number, number, number]; // [fx, fy, fw, fh] 0..1

/** Clamp a crop rect to valid normalized bounds; null when degenerate.
 * The minimum size wins: if the origin leaves no room for it, the origin is
 * pushed back so `fw/fh ≥ MIN_CROP_FRACTION` always holds. */
export function clampCropRect(rect: CropRect): CropRect | null {
  let [fx, fy, fw, fh] = rect;
  if (![fx, fy, fw, fh].every((v) => Number.isFinite(v))) return null;
  fw = Math.min(1, Math.max(MIN_CROP_FRACTION, fw));
  fh = Math.min(1, Math.max(MIN_CROP_FRACTION, fh));
  fx = Math.min(Math.max(0, fx), 1 - fw);
  fy = Math.min(Math.max(0, fy), 1 - fh);
  return [fx, fy, fw, fh];
}

/** True when a crop rect covers the full image (a no-op crop). Used to decide
 * whether a drag on the frame draws a fresh box (full) or moves it. */
export function isFullFrameRect(rect: CropRect | null | undefined): boolean {
  const [fx, fy, fw, fh] = rect ?? [0, 0, 1, 1];
  return fx <= 1e-9 && fy <= 1e-9 && fw >= 1 - 1e-9 && fh >= 1 - 1e-9;
}

/** Screen-space rect of a crop within a layer box of `boxW × boxH` (the
 * unrotated layer frame — the crop UI always works unrotated). */
export function cropDisplayRect(
  crop: CropRect | null | undefined,
  boxW: number,
  boxH: number,
) {
  const [fx, fy, fw, fh] = crop ?? [0, 0, 1, 1];
  return { x: fx * boxW, y: fy * boxH, width: fw * boxW, height: fh * boxH };
}

/** Apply a new crop: the display box keeps pixel density, so its size scales
 * by the crop-fraction ratio. A full-frame crop collapses to null (storage). */
export function applyCropToSize(
  prev: CropRect | null | undefined,
  next: CropRect,
  width: number,
  height: number,
): { crop: CropRect | null; width: number; height: number } {
  const [, , fwPrev, fhPrev] = prev ?? [0, 0, 1, 1];
  const [, , fwNext, fhNext] = next;
  const fullFrame =
    fwNext >= 1 - 1e-9 &&
    fhNext >= 1 - 1e-9 &&
    next[0] <= 1e-9 &&
    next[1] <= 1e-9;
  return {
    crop: fullFrame ? null : next,
    width: clampLayerSide((width * fwNext) / fwPrev),
    height: clampLayerSide((height * fhNext) / fhPrev),
  };
}

/** Crop-overlay drag handles: 4 corners + 4 edges. */
export type CropHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

/** Resize a normalized crop rect by dragging `handle` to the pointer's
 * normalized position. Clamped to the image bounds with a minimum size. */
export function adjustCropRect(
  base: CropRect,
  handle: CropHandle,
  nx: number,
  ny: number,
): CropRect | null {
  const MIN = MIN_CROP_FRACTION;
  const [fx, fy, fw, fh] = base;
  const right = fx + fw;
  const bottom = fy + fh;
  const px = Math.min(1, Math.max(0, nx));
  const py = Math.min(1, Math.max(0, ny));
  let left = fx;
  let top = fy;
  let r = right;
  let b = bottom;
  if (handle.includes("w")) left = Math.min(px, r - MIN);
  if (handle.includes("e")) r = Math.max(px, left + MIN);
  if (handle.includes("n")) top = Math.min(py, b - MIN);
  if (handle.includes("s")) b = Math.max(py, top + MIN);
  return clampCropRect([left, top, r - left, b - top]);
}

/** Translate a normalized crop rect (pointer delta in normalized units). */
export function moveCropRect(
  base: CropRect,
  startNx: number,
  startNy: number,
  nx: number,
  ny: number,
): CropRect {
  const dx = nx - startNx;
  const dy = ny - startNy;
  const fx = Math.min(Math.max(0, base[0] + dx), 1 - base[2]);
  const fy = Math.min(Math.max(0, base[1] + dy), 1 - base[3]);
  return [fx, fy, base[2], base[3]];
}
