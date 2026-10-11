import { createStore, produce } from "solid-js/store";
import type {
  CanvasAlignDirection,
  CanvasBackgroundMode,
  CanvasExportResult,
  CanvasLayer,
  CanvasLayerInput,
  ViewMode,
} from "../types/canvas";
import { applyCropToSize, layersBBox, layoutPatches } from "../utils/canvas";

export interface CanvasState {
  // Which top-level view the app shows.
  viewMode: ViewMode;
  // Array order IS z-order: index 0 renders at the bottom.
  layers: CanvasLayer[];
  selectedId: string | null;
  // User-drawn export rectangle in canvas coordinates; null = export the
  // auto bounding box.
  selection: [number, number, number, number] | null;
  // When true, dragging on empty canvas draws a new export selection.
  selectionMode: boolean;
  // Layer currently being cropped (crop overlay active on the stage); the
  // draft rect lives in the store so the stage, inspector and the Enter/Esc
  // shortcuts all see the same value.
  croppingId: string | null;
  draftCrop: [number, number, number, number] | null;
  // Bumped whenever the stage should refit its viewport (after drops).
  fitTick: number;
  backgroundMode: CanvasBackgroundMode;
  // Used when backgroundMode === "custom"; "#rrggbb".
  customColor: string;
  exporting: boolean;
  // Set after a successful export; cleared on the next one.
  exportResult: CanvasExportResult | null;
  exportError: string | null;
}

const CASCADE_STEP = 32;

const [store, setStore] = createStore<CanvasState>({
  viewMode: "batch",
  layers: [],
  selectedId: null,
  selection: null,
  selectionMode: false,
  croppingId: null,
  draftCrop: null,
  fitTick: 0,
  backgroundMode: "transparent",
  customColor: "#ffffff",
  exporting: false,
  exportResult: null,
  exportError: null,
});

export { store };

let nextLayerId = 1;

/**
 * Release the blob URL backing a layer's display image (no-op for data URLs
 * and in environments without the API, e.g. the vitest DOM shim).
 */
function revokeLayerSrc(layer: { src?: string }) {
  if (
    layer.src?.startsWith("blob:") &&
    typeof URL !== "undefined" &&
    typeof URL.revokeObjectURL === "function"
  ) {
    URL.revokeObjectURL(layer.src);
  }
}

export const canvasActions = {
  setViewMode(mode: ViewMode) {
    setStore("viewMode", mode);
  },

  // Layers -----------------------------------------------------------------
  addLayers(inputs: CanvasLayerInput[]) {
    if (inputs.length === 0) return;
    // Cascade new layers to the bottom-right of the current content so
    // nothing lands stacked in a single pile; layers added in ONE drop are
    // spread by their own width so a batch lands side by side, not stacked.
    const box = layersBBox(store.layers);
    let cursorX = box ? box.x + box.width + CASCADE_STEP : 0;
    const cursorY = box ? box.y : 0;
    setStore(
      "layers",
      produce((layers) => {
        for (const input of inputs) {
          const width = Math.max(1, Math.round(input.width));
          const height = Math.max(1, Math.round(input.height));
          layers.push({
            id: `clt-layer-${nextLayerId++}`,
            path: input.path,
            name: input.name,
            src: input.src,
            x: cursorX,
            y: cursorY,
            width,
            height,
            rotation: 0,
            flipH: false,
            flipV: false,
            opacity: 100,
            visible: true,
            naturalWidth: input.naturalWidth,
            naturalHeight: input.naturalHeight,
          });
          cursorX += width + CASCADE_STEP;
        }
      }),
    );
    // Select the last added layer.
    setStore("selectedId", store.layers[store.layers.length - 1]?.id ?? null);
  },

  selectLayer(id: string | null) {
    setStore("selectedId", id);
  },

  updateLayer(
    id: string,
    patch: Partial<Omit<CanvasLayer, "id" | "path" | "src">>,
  ) {
    setStore(
      "layers",
      produce((layers) => {
        const layer = layers.find((l) => l.id === id);
        if (layer) Object.assign(layer, patch);
      }),
    );
  },

  removeLayer(id: string) {
    setStore(
      "layers",
      produce((layers) => {
        const i = layers.findIndex((l) => l.id === id);
        if (i >= 0) {
          revokeLayerSrc(layers[i]);
          layers.splice(i, 1);
        }
      }),
    );
    if (store.selectedId === id) {
      setStore("selectedId", null);
    }
  },

  clearCanvas() {
    for (const layer of store.layers) revokeLayerSrc(layer);
    setStore({
      layers: [],
      selectedId: null,
      selection: null,
      exportResult: null,
      exportError: null,
    });
  },

  // Z-order ----------------------------------------------------------------
  moveLayer(id: string, direction: -1 | 1) {
    setStore(
      "layers",
      produce((layers) => {
        const i = layers.findIndex((l) => l.id === id);
        const j = i + direction;
        if (i < 0 || j < 0 || j >= layers.length) return;
        [layers[i], layers[j]] = [layers[j], layers[i]];
      }),
    );
  },
  sendLayerToFront(id: string) {
    setStore(
      "layers",
      produce((layers) => {
        const i = layers.findIndex((l) => l.id === id);
        if (i < 0 || i === layers.length - 1) return;
        const [layer] = layers.splice(i, 1);
        layers.push(layer);
      }),
    );
  },
  sendLayerToBack(id: string) {
    setStore(
      "layers",
      produce((layers) => {
        const i = layers.findIndex((l) => l.id === id);
        if (i <= 0) return;
        const [layer] = layers.splice(i, 1);
        layers.unshift(layer);
      }),
    );
  },

  toggleLayerVisible(id: string) {
    setStore(
      "layers",
      produce((layers) => {
        const layer = layers.find((l) => l.id === id);
        if (layer) layer.visible = !layer.visible;
      }),
    );
  },

  // One-click tiling (the "merge horizontally/vertically" feature).
  alignLayers(direction: CanvasAlignDirection, gap: number) {
    const patches = layoutPatches(store.layers, direction, gap);
    if (patches.length === 0) return;
    setStore(
      "layers",
      produce((layers) => {
        const byId = new Map(patches.map((p) => [p.id, p]));
        for (const layer of layers) {
          const patch = byId.get(layer.id);
          if (patch) {
            layer.x = patch.x;
            layer.y = patch.y;
          }
        }
      }),
    );
    // A layout change invalidates a drawn selection's meaning.
    setStore("selection", null);
  },

  // Selection --------------------------------------------------------------
  setSelection(rect: [number, number, number, number] | null) {
    setStore("selection", rect);
  },
  setSelectionMode(mode: boolean) {
    setStore("selectionMode", mode);
    if (!mode) setStore("selection", null);
  },
  // Ask the stage to refit its viewport to the content.
  requestFit() {
    setStore("fitTick", store.fitTick + 1);
  },

  // Crop mode ---------------------------------------------------------------
  startCrop(id: string) {
    setStore({
      croppingId: id,
      selectionMode: false,
      selectedId: id,
      draftCrop: null,
    });
  },
  cancelCrop() {
    setStore({ croppingId: null, draftCrop: null });
  },
  /** Reset the crop frame to the full image (stays in crop mode) so the user
   * can draw a fresh box. This is the only way to redraw from scratch now that
   * pressing outside the frame no longer wipes the current crop. */
  resetCrop(id: string) {
    if (store.croppingId !== id) return;
    setStore("draftCrop", [0, 0, 1, 1]);
  },
  setDraftCrop(rect: [number, number, number, number] | null) {
    setStore("draftCrop", rect);
  },
  // Commit the drafted crop: keep pixel density (display size scales with
  // the crop-fraction ratio), keep the box origin.
  applyCrop(id: string, next?: [number, number, number, number]) {
    const layer = store.layers.find((l) => l.id === id);
    const rect = next ?? store.draftCrop ?? layer?.crop ?? null;
    setStore({ croppingId: null, draftCrop: null });
    if (!layer || !rect) return;
    const applied = applyCropToSize(
      layer.crop,
      rect,
      layer.width,
      layer.height,
    );
    canvasActions.updateLayer(id, applied);
  },

  // Background -------------------------------------------------------------
  setBackgroundMode(mode: CanvasBackgroundMode) {
    setStore("backgroundMode", mode);
  },
  setCustomColor(color: string) {
    setStore("customColor", color);
  },

  // Export -----------------------------------------------------------------
  setExporting(exporting: boolean) {
    setStore("exporting", exporting);
  },
  setExportResult(result: CanvasExportResult | null) {
    setStore("exportResult", result);
    setStore("exportError", null);
  },
  setExportError(error: string | null) {
    setStore("exportError", error);
    if (error) setStore("exportResult", null);
  },
};

// Hex color for the current background mode, or null for transparent.
export function backgroundHexOf(
  mode: CanvasBackgroundMode,
  customColor: string,
): string | null {
  switch (mode) {
    case "white":
      return "#ffffff";
    case "black":
      return "#000000";
    case "custom":
      return /^#[0-9a-fA-F]{6}$/.test(customColor) ? customColor : null;
    default:
      return null;
  }
}
