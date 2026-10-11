import { describe, expect, it } from "vitest";
import type { CanvasLayer } from "../../types/canvas";
import {
  adjustCropRect,
  applyCropToSize,
  clampCropRect,
  clampLayerSide,
  exportRectOf,
  layersBBox,
  layoutPatches,
  moveCropRect,
  resizeLayerByHandle,
  resizeRotatedLayerByHandle,
  rotatedAabb,
} from "../canvas";

function layer(overrides: Partial<CanvasLayer> = {}): CanvasLayer {
  return {
    id: "l1",
    path: "C:/a.png",
    name: "a.png",
    src: "data:",
    x: 0,
    y: 0,
    width: 100,
    height: 50,
    rotation: 0,
    flipH: false,
    flipV: false,
    opacity: 100,
    visible: true,
    naturalWidth: 100,
    naturalHeight: 50,
    ...overrides,
  };
}

describe("rotatedAabb", () => {
  it("keeps the box at 0° and swaps sides at 90° (snapped, no fp speck)", () => {
    expect(rotatedAabb(100, 50, 0)).toEqual({ width: 100, height: 50 });
    expect(rotatedAabb(100, 50, 90)).toEqual({ width: 50, height: 100 });
    expect(rotatedAabb(100, 50, -90)).toEqual({ width: 50, height: 100 });
  });

  it("expands by (w+h)/√2 on both axes at 45°", () => {
    const { width, height } = rotatedAabb(100, 50, 45);
    expect(width).toBeCloseTo(150 * Math.SQRT1_2, 9);
    expect(height).toBeCloseTo(150 * Math.SQRT1_2, 9);
  });

  it("collapses non-finite input to 1×1", () => {
    expect(rotatedAabb(Number.NaN, 50, 0)).toEqual({ width: 1, height: 1 });
  });
});

describe("layersBBox", () => {
  it("covers visible layers and ignores hidden ones", () => {
    const layers = [
      layer({ id: "a", x: 0, y: 0, width: 100, height: 50 }),
      layer({ id: "hidden", x: 500, y: 500, visible: false }),
      layer({ id: "b", x: 60, y: 30, width: 40, height: 40 }),
    ];
    expect(layersBBox(layers)).toEqual({ x: 0, y: 0, width: 100, height: 70 });
  });

  it("is null with no visible layers", () => {
    expect(layersBBox([])).toBeNull();
    expect(layersBBox([layer({ visible: false })])).toBeNull();
  });
});

describe("exportRectOf", () => {
  it("prefers the selection and integer-aligns it", () => {
    const rect = exportRectOf([layer()], [10.4, 20.6, 30.2, 40.9]);
    expect(rect).toEqual({ x: 10, y: 20, width: 31, height: 41 });
  });

  it("rejects degenerate selections", () => {
    expect(exportRectOf([layer()], [0, 0, 0, 10])).toBeNull();
    expect(exportRectOf([layer()], [0, 0, Number.NaN, 10])).toBeNull();
  });

  it("falls back to the layer bbox", () => {
    expect(exportRectOf([layer()], null)).toEqual({
      x: 0,
      y: 0,
      width: 100,
      height: 50,
    });
    expect(exportRectOf([], null)).toBeNull();
  });
});

describe("clampLayerSide", () => {
  it("bounds to [1, 12000] and de-NaNs", () => {
    expect(clampLayerSide(0)).toBe(1);
    expect(clampLayerSide(-5)).toBe(1);
    expect(clampLayerSide(Number.NaN)).toBe(1);
    expect(clampLayerSide(99999)).toBe(12000);
    expect(clampLayerSide(300)).toBe(300);
  });
});

describe("layoutPatches", () => {
  it("tiles horizontally in z-order, centered on the cross axis", () => {
    const layers = [
      layer({ id: "a", y: 0, width: 100, height: 50 }),
      layer({ id: "b", x: -50, y: 0, width: 40, height: 80 }),
    ];
    const patches = layoutPatches(layers, "horizontal", 10);
    // Cross center = avg(0+25, 0+40) = 32.5.
    expect(patches).toEqual([
      { id: "a", x: 0, y: 7.5 }, // 32.5 - 25
      { id: "b", x: 110, y: -7.5 }, // 32.5 - 40
    ]);
  });

  it("tiles vertically below each other", () => {
    const layers = [
      layer({ id: "a", width: 100, height: 50 }),
      layer({ id: "b", width: 40, height: 80 }),
    ];
    const patches = layoutPatches(layers, "vertical", 0);
    // Cross center = avg(0+50, 0+20) = 35.
    expect(patches).toEqual([
      { id: "a", x: -15, y: 0 },
      { id: "b", x: 15, y: 50 },
    ]);
  });

  it("skips hidden layers and handles empty input", () => {
    expect(layoutPatches([], "horizontal", 0)).toEqual([]);
    const patches = layoutPatches([layer({ visible: false })], "horizontal", 0);
    expect(patches).toEqual([]);
  });
});

describe("resizeLayerByHandle", () => {
  const l = layer({ x: 10, y: 10, width: 100, height: 50 });

  it("anchors the opposite corner when dragging se", () => {
    const next = resizeLayerByHandle(l, "se", { x: 110, y: 60 }, true);
    expect(next.x).toBe(10);
    expect(next.y).toBe(10);
    expect(next.width).toBe(100);
    expect(next.height).toBe(50);
  });

  it("grows from the sw anchor while dragging ne", () => {
    const next = resizeLayerByHandle(l, "ne", { x: 60, y: 40 }, true);
    // anchor = (10, 60); pointer at (60,40) → dx 50 / 100 = 0.5, dy 20/50=0.4
    // keepAspect → uniform scale 0.5 → 50×25 anchored at sw.
    expect(next.x).toBe(10);
    expect(next.y).toBe(35);
    expect(next.width).toBe(50);
    expect(next.height).toBe(25);
  });

  it("enforces a minimum on-canvas size", () => {
    const next = resizeLayerByHandle(l, "se", { x: 11, y: 11 }, false);
    expect(next.width).toBe(8);
    expect(next.height).toBe(8);
  });

  it("supports free (non-uniform) scaling", () => {
    const next = resizeLayerByHandle(l, "se", { x: 60, y: 40 }, false);
    expect(next.width).toBe(50);
    expect(next.height).toBe(30);
  });
});

describe("resizeRotatedLayerByHandle", () => {
  it("at 0° matches the unrotated resize", () => {
    const l = layer({ x: 10, y: 10, width: 100, height: 50 });
    const a = resizeLayerByHandle(l, "se", { x: 110, y: 60 }, true);
    const b = resizeRotatedLayerByHandle(l, "se", { x: 110, y: 60 }, true);
    expect(b).toEqual(a);
  });

  it("at 90° keeps the anchor corner pinned in canvas space", () => {
    // 10×100 layer rotated 90°: the canvas pointer projects onto the LOCAL
    // frame, where the ne drag runs its usual axis-aligned math.
    const l = layer({ x: 0, y: 0, width: 10, height: 100, rotation: 90 });
    const next = resizeRotatedLayerByHandle(l, "ne", { x: 50, y: 10 }, true);
    // Local pointer = (-35, 5); anchor sw (0,100): sx=3.5, sy=0.95 → uniform 3.5.
    expect(next.width).toBeCloseTo(35, 9);
    expect(next.height).toBeCloseTo(350, 9);
  });

  it("scales uniformly from the local-frame pointer projection", () => {
    const l = layer({ x: 0, y: 0, width: 100, height: 50, rotation: 30 });
    const next = resizeRotatedLayerByHandle(l, "se", { x: 150, y: 75 }, true);
    // Local pointer: rotate (150,75)-(50,25) by -30° → (161.60254, 18.30127).
    // scale = max(1.616..., 0.366...) = 1.616...
    expect(next.width).toBeCloseTo(161.60254, 5);
    expect(next.height).toBeCloseTo(80.80127, 5);
  });
});

describe("clampCropRect", () => {
  it("clamps to bounds and enforces a minimum size", () => {
    expect(clampCropRect([0, 0, 1, 1])).toEqual([0, 0, 1, 1]);
    expect(clampCropRect([-0.2, 0, 0.5, 0.5])).toEqual([0, 0, 0.5, 0.5]);
    // Minimum size wins: the origin is pushed back to make room for fw.
    const [fx, , fw] = clampCropRect([0.99, 0, 0.5, 0.5]) ?? [0, 0, 1, 1];
    expect(fx).toBeCloseTo(0.5, 9);
    expect(fw).toBeCloseTo(0.5, 9);
    expect(clampCropRect([0, 0, Number.NaN, 0.5])).toBeNull();
  });
});

describe("adjustCropRect / moveCropRect", () => {
  it("drags the east edge and never inverts below the minimum", () => {
    const next = adjustCropRect([0, 0, 1, 1], "e", 0.4, 0) ?? [0, 0, 1, 1];
    expect(next).toEqual([0, 0, 0.4, 1]);
    // Dragging e past the left edge clamps at the minimum width.
    const min = adjustCropRect([0, 0, 0.2, 1], "e", -5, 0) ?? [0, 0, 1, 1];
    expect(min[2]).toBeCloseTo(0.05, 9);
  });

  it("drags the nw corner symmetrically", () => {
    const next = adjustCropRect([0, 0, 1, 1], "nw", 0.2, 0.3) ?? [0, 0, 1, 1];
    expect(next[0]).toBeCloseTo(0.2, 9);
    expect(next[1]).toBeCloseTo(0.3, 9);
    expect(next[2]).toBeCloseTo(0.8, 9);
    expect(next[3]).toBeCloseTo(0.7, 9);
  });

  it("moves the frame within bounds", () => {
    expect(moveCropRect([0, 0, 0.5, 0.5], 0, 0, 0.25, 0.1)).toEqual([
      0.25, 0.1, 0.5, 0.5,
    ]);
    // Overshooting clamps at the image edge.
    expect(moveCropRect([0, 0, 0.5, 0.5], 0, 0, 2, -1)).toEqual([
      0.5, 0, 0.5, 0.5,
    ]);
  });
});

describe("applyCropToSize", () => {
  it("scales the display box by the fraction ratio", () => {
    const r = applyCropToSize(null, [0.5, 0.25, 0.5, 0.5], 200, 100);
    expect(r.width).toBe(100); // 200 × (0.5/1)
    expect(r.height).toBe(50); // 100 × (0.5/1)
    expect(r.crop).toEqual([0.5, 0.25, 0.5, 0.5]);
  });

  it("stacks on a previous crop via the ratio", () => {
    const r = applyCropToSize([0, 0, 0.5, 0.5], [0, 0, 0.25, 0.5], 100, 50);
    expect(r.width).toBe(50); // 100 × (0.25/0.5)
    expect(r.height).toBe(50); // 50 × (0.5/0.5)
  });

  it("collapses a full-frame crop back to null", () => {
    const r = applyCropToSize([0.1, 0.1, 0.8, 0.8], [0, 0, 1, 1], 100, 100);
    expect(r.crop).toBeNull();
  });
});
