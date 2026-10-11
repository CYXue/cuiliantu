import { beforeEach, describe, expect, it } from "vitest";
import type { CanvasLayerInput } from "../types/canvas";
import { backgroundHexOf, canvasActions, store } from "./canvasStore";

function input(name: string, width = 100, height = 50): CanvasLayerInput {
  return {
    path: `C:/img/${name}`,
    name,
    src: `data:image/png;base64,${name}`,
    width,
    height,
    naturalWidth: width * 2,
    naturalHeight: height * 2,
  };
}

beforeEach(() => {
  canvasActions.clearCanvas();
});

describe("canvasStore layers", () => {
  it("adds layers cascading to the right of the current bbox", () => {
    canvasActions.addLayers([input("a.png", 100, 50)]);
    canvasActions.addLayers([input("b.png", 40, 40)]);
    expect(store.layers).toHaveLength(2);
    // First at origin, second 32px right of the first bbox.
    expect(store.layers[0]).toMatchObject({
      x: 0,
      y: 0,
      width: 100,
      height: 50,
    });
    expect(store.layers[1].x).toBe(132);
    // The newest layer is selected.
    expect(store.selectedId).toBe(store.layers[1].id);
  });

  it("cascade places the next drop to the right of everything", () => {
    canvasActions.addLayers([input("a.png"), input("b.png", 30, 30)]);
    canvasActions.addLayers([input("c.png")]);
    expect(store.layers[2].x).toBe(132 + 30 + 32);
  });

  it("updateLayer patches only the target", () => {
    canvasActions.addLayers([input("a.png"), input("b.png")]);
    canvasActions.updateLayer(store.layers[0].id, {
      rotation: 45,
      opacity: 60,
    });
    expect(store.layers[0].rotation).toBe(45);
    expect(store.layers[0].opacity).toBe(60);
    expect(store.layers[1].rotation).toBe(0);
  });

  it("removeLayer clears the selection when needed", () => {
    canvasActions.addLayers([input("a.png")]);
    const id = store.layers[0].id;
    canvasActions.selectLayer(id);
    canvasActions.removeLayer(id);
    expect(store.layers).toHaveLength(0);
    expect(store.selectedId).toBeNull();
  });

  it("z-order helpers reorder the array", () => {
    canvasActions.addLayers([input("a.png"), input("b.png"), input("c.png")]);
    const [a, b, c] = store.layers.map((l) => l.id);
    canvasActions.moveLayer(b, 1);
    expect(store.layers.map((l) => l.id)).toEqual([a, c, b]);
    canvasActions.sendLayerToBack(b);
    expect(store.layers.map((l) => l.id)).toEqual([b, a, c]);
    canvasActions.sendLayerToFront(b);
    expect(store.layers.map((l) => l.id)).toEqual([a, c, b]);
  });

  it("alignLayers repositions visible layers and drops the selection", () => {
    canvasActions.addLayers([input("a.png", 100, 50), input("b.png", 40, 80)]);
    canvasActions.setSelection([0, 0, 10, 10]);
    canvasActions.alignLayers("horizontal", 10);
    // Cross center = avg(0+25, 0+40) = 32.5.
    expect(store.layers[0]).toMatchObject({ x: 0, y: 7.5 });
    expect(store.layers[1]).toMatchObject({ x: 110, y: -7.5 });
    expect(store.selection).toBeNull();
  });

  it("alignLayers skips hidden layers", () => {
    canvasActions.addLayers([input("a.png"), input("b.png")]);
    canvasActions.toggleLayerVisible(store.layers[1].id);
    canvasActions.alignLayers("horizontal", 0);
    // Hidden layer keeps its position untouched.
    expect(store.layers[1].x).toBe(132);
  });
});

describe("canvasStore selection & background", () => {
  it("stores and clears the selection rect", () => {
    canvasActions.setSelection([1, 2, 3, 4]);
    expect(store.selection).toEqual([1, 2, 3, 4]);
    canvasActions.setSelection(null);
    expect(store.selection).toBeNull();
  });

  it("backgroundHexOf validates the custom color", () => {
    expect(backgroundHexOf("white", "#123456")).toBe("#ffffff");
    expect(backgroundHexOf("black", "")).toBe("#000000");
    expect(backgroundHexOf("custom", "#12ab34")).toBe("#12ab34");
    expect(backgroundHexOf("custom", "red")).toBeNull();
    expect(backgroundHexOf("transparent", "#ffffff")).toBeNull();
  });
});

describe("canvasStore export state", () => {
  it("result and error are mutually exclusive", () => {
    canvasActions.setExportResult({
      name: "out.png",
      width: 10,
      height: 10,
      sizeBytes: 5,
    });
    expect(store.exportResult).not.toBeNull();
    expect(store.exportError).toBeNull();
    canvasActions.setExportError("boom");
    expect(store.exportResult).toBeNull();
    expect(store.exportError).toBe("boom");
  });

  it("clearCanvas resets everything", () => {
    canvasActions.addLayers([input("a.png")]);
    canvasActions.setExporting(true);
    canvasActions.clearCanvas();
    expect(store.layers).toHaveLength(0);
    expect(store.selectedId).toBeNull();
    expect(store.exporting).toBe(true); // exporting is not cleared mid-run by design
  });
});
