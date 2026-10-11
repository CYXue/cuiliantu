//! The interactive canvas stage: pan/zoom viewport, layer dragging, corner
//! resize handles, and export-selection drawing.
//!
//! Coordinate systems:
//! * canvas space — the layer model's pixels (what gets exported)
//! * screen space — the container's CSS pixels
//! * viewport — `panX/panY/zoom` maps canvas → screen:
//!   `screen = canvas * zoom + pan`

import {
  createEffect,
  createSignal,
  For,
  on,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { useTranslation } from "../../i18n";
import { backgroundHexOf, canvasActions, store } from "../../store/canvasStore";
import type { CanvasLayer } from "../../types/canvas";
import {
  adjustCropRect,
  type CropHandle,
  type CropRect,
  clampCropRect,
  cropDisplayRect,
  exportRectOf,
  isFullFrameRect,
  layersBBox,
  moveCropRect,
  resizeLayerByHandle,
  resizeRotatedLayerByHandle,
} from "../../utils/canvas";

const MIN_ZOOM = 0.02;
const MAX_ZOOM = 8;
const ZOOM_WHEEL_FACTOR = 0.0015;

type HandleId = "nw" | "ne" | "se" | "sw";

type Interaction =
  | {
      kind: "pan";
      startClientX: number;
      startClientY: number;
      startPanX: number;
      startPanY: number;
    }
  | {
      kind: "move";
      id: string;
      startCanvasX: number;
      startCanvasY: number;
      startX: number;
      startY: number;
    }
  | { kind: "resize"; id: string; handle: HandleId; base: CanvasLayer }
  | { kind: "draw"; startCanvasX: number; startCanvasY: number }
  | { kind: "crop-resize"; id: string; handle: CropHandle; baseRect: CropRect }
  | {
      kind: "crop-move";
      id: string;
      startNx: number;
      startNy: number;
      baseRect: CropRect;
    }
  | { kind: "crop-draw"; id: string; startNx: number; startNy: number };

export function CanvasStage() {
  const { t } = useTranslation();
  let container: HTMLDivElement | undefined;

  const [viewport, setViewport] = createSignal({ panX: 0, panY: 0, zoom: 1 });
  // Draft selection while dragging a new one; the committed one lives in the
  // store.
  const [draftSelection, setDraftSelection] = createSignal<
    [number, number, number, number] | null
  >(null);

  let interaction: Interaction | null = null;

  // Viewport ↔ canvas transforms -------------------------------------------
  const toCanvas = (clientX: number, clientY: number) => {
    const rect = container?.getBoundingClientRect();
    const vp = viewport();
    if (!rect) return { x: 0, y: 0 };
    return {
      x: (clientX - rect.left - vp.panX) / vp.zoom,
      y: (clientY - rect.top - vp.panY) / vp.zoom,
    };
  };

  /** Center the visible layers in the viewport (never above 1:1). */
  const fitView = () => {
    if (!container) return;
    const box = layersBBox(store.layers) ?? {
      x: 0,
      y: 0,
      width: 800,
      height: 600,
    };
    const rect = container.getBoundingClientRect();
    const padding = 48;
    const zoom = Math.max(
      MIN_ZOOM,
      Math.min(
        1,
        (rect.width - padding * 2) / Math.max(1, box.width),
        (rect.height - padding * 2) / Math.max(1, box.height),
      ),
    );
    setViewport({
      zoom,
      panX: (rect.width - box.width * zoom) / 2 - box.x * zoom,
      panY: (rect.height - box.height * zoom) / 2 - box.y * zoom,
    });
  };

  // The toolbar/store can request a refit (after drops, layout, ...).
  createEffect(
    on(
      () => store.fitTick,
      () => fitView(),
    ),
  );

  // Pointer interactions ----------------------------------------------------
  const beginInteraction = (next: Interaction) => {
    interaction = next;
  };

  const onWindowPointerMove = (e: PointerEvent) => {
    if (!interaction || !container) return;
    // Snapshot: `interaction` is a mutable module-level let, and TS cannot
    // keep narrowing it across the setViewport closure below.
    const current = interaction;
    if (current.kind === "pan") {
      setViewport((vp) => ({
        ...vp,
        panX: current.startPanX + (e.clientX - current.startClientX),
        panY: current.startPanY + (e.clientY - current.startClientY),
      }));
      return;
    }
    const canvas = toCanvas(e.clientX, e.clientY);
    if (current.kind === "move") {
      canvasActions.updateLayer(current.id, {
        x: current.startX + (canvas.x - current.startCanvasX),
        y: current.startY + (canvas.y - current.startCanvasY),
      });
    } else if (current.kind === "resize") {
      const keepAspect = !e.shiftKey;
      // Rotated layers resize in their local frame (pointer inverse-rotated);
      // the axis-aligned fast path handles the common rotation === 0 case.
      const next =
        current.base.rotation !== 0
          ? resizeRotatedLayerByHandle(
              current.base,
              current.handle,
              canvas,
              keepAspect,
            )
          : resizeLayerByHandle(
              current.base,
              current.handle,
              canvas,
              keepAspect,
            );
      canvasActions.updateLayer(current.id, next);
    } else if (current.kind === "draw") {
      const x = Math.min(current.startCanvasX, canvas.x);
      const y = Math.min(current.startCanvasY, canvas.y);
      const w = Math.abs(canvas.x - current.startCanvasX);
      const h = Math.abs(canvas.y - current.startCanvasY);
      setDraftSelection([x, y, w, h]);
    } else if (current.kind === "crop-move") {
      const layer = store.layers.find((l) => l.id === current.id);
      if (!layer) return;
      const [nx, ny] = normalizedInLayer(layer, canvas);
      canvasActions.setDraftCrop(
        moveCropRect(
          current.baseRect,
          current.startNx,
          current.startNy,
          nx,
          ny,
        ),
      );
    } else if (current.kind === "crop-draw") {
      // Dragging OUTSIDE the frame (or inside a FULL-frame one) draws a
      // brand-new crop rect from the press point.
      const layer = store.layers.find((l) => l.id === current.id);
      if (!layer) return;
      const [nx, ny] = normalizedInLayer(layer, canvas);
      const x = Math.min(current.startNx, nx);
      const y = Math.min(current.startNy, ny);
      const w = Math.abs(nx - current.startNx);
      const h = Math.abs(ny - current.startNy);
      canvasActions.setDraftCrop(clampCropRect([x, y, w, h]));
    } else if (current.kind === "crop-resize") {
      const layer = store.layers.find((l) => l.id === current.id);
      if (!layer) return;
      const [nx, ny] = normalizedInLayer(layer, canvas);
      canvasActions.setDraftCrop(
        adjustCropRect(current.baseRect, current.handle, nx, ny),
      );
    }
  };

  const onWindowPointerUp = () => {
    if (!interaction) return;
    const current = interaction;
    interaction = null;
    if (current.kind === "draw") {
      const draft = draftSelection();
      setDraftSelection(null);
      // A click without a drag clears the selection instead of storing a
      // degenerate rect.
      if (draft && draft[2] >= 1 && draft[3] >= 1) {
        canvasActions.setSelection(draft);
      } else {
        canvasActions.setSelection(null);
      }
    }
  };

  const onWheel = (e: WheelEvent) => {
    if (!container) return;
    e.preventDefault();
    const rect = container.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    setViewport((vp) => {
      const zoom = Math.min(
        MAX_ZOOM,
        Math.max(MIN_ZOOM, vp.zoom * Math.exp(-e.deltaY * ZOOM_WHEEL_FACTOR)),
      );
      // Keep the canvas point under the cursor anchored while zooming.
      const ratio = zoom / vp.zoom;
      return {
        zoom,
        panX: px - (px - vp.panX) * ratio,
        panY: py - (py - vp.panY) * ratio,
      };
    });
  };

  onMount(() => {
    // Wheel zoom needs a non-passive listener to be able to preventDefault.
    container?.addEventListener("wheel", onWheel, { passive: false });
    window.addEventListener("pointermove", onWindowPointerMove);
    window.addEventListener("pointerup", onWindowPointerUp);
    onCleanup(() => {
      container?.removeEventListener("wheel", onWheel);
      window.removeEventListener("pointermove", onWindowPointerMove);
      window.removeEventListener("pointerup", onWindowPointerUp);
    });
  });

  // Derived geometry --------------------------------------------------------
  const selected = (): CanvasLayer | null =>
    store.layers.find((l) => l.id === store.selectedId) ?? null;

  const croppingLayer = (): CanvasLayer | null =>
    store.layers.find((l) => l.id === store.croppingId) ?? null;

  // Unified selection / crop box geometry. Every accessor here is a thunk ON
  // PURPOSE: the store mutates layer objects IN PLACE (produce + Object.assign),
  // so object references never change and a keyed render callback never
  // re-runs. Any value hoisted into a plain const freezes at its first
  // evaluation — the reads must happen inside tracked JSX expressions.
  const boxLayer = (): CanvasLayer | null =>
    croppingLayer() ?? (selected()?.visible ? selected() : null);

  const boxIsCrop = () => {
    const layer = boxLayer();
    return !!layer && store.croppingId === layer.id;
  };

  const boxDraft = (): CropRect =>
    (store.draftCrop ?? boxLayer()?.crop ?? [0, 0, 1, 1]) as CropRect;

  /** Display rect of the box in canvas space: the keep-region inside the
   * layer in crop mode, the full layer footprint in transform mode. */
  const boxRect = () => {
    const layer = boxLayer();
    if (!layer) return { x: 0, y: 0, width: 0, height: 0 };
    return boxIsCrop()
      ? cropDisplayRect(boxDraft(), layer.width, layer.height)
      : { x: 0, y: 0, width: layer.width, height: layer.height };
  };

  const board = () => exportRectOf(store.layers, store.selection);

  // "#rrggbb" for the current background mode, or null for transparent.
  const backgroundHex = () =>
    backgroundHexOf(store.backgroundMode, store.customColor);

  // Layer rendering: the display box shows the layer's CROP region of the
  // source stretched to width/height — background sizing mirrors what the
  // Rust compositor does (crop → resize), so WYSIWYG holds. In crop mode the
  // target layer renders its full image, unrotated and unflipped, so the
  // crop frame drags in plain image coordinates.
  const layerBoxStyle = (layer: CanvasLayer, forCrop: boolean) => {
    const crop: CropRect = forCrop || !layer.crop ? [0, 0, 1, 1] : layer.crop;
    const srcW = layer.naturalWidth || layer.width;
    const srcH = layer.naturalHeight || layer.height;
    const scale = layer.width / (crop[2] * srcW);
    return {
      left: `${layer.x}px`,
      top: `${layer.y}px`,
      width: `${layer.width}px`,
      height: `${layer.height}px`,
      transform: forCrop
        ? undefined
        : `rotate(${layer.rotation}deg) scaleX(${layer.flipH ? -1 : 1}) scaleY(${layer.flipV ? -1 : 1})`,
      opacity: String(layer.opacity / 100),
      "background-image": `url("${layer.src}")`,
      "background-size": `${srcW * scale}px ${srcH * scale}px`,
      "background-position": `${-crop[0] * srcW * scale}px ${-crop[1] * srcH * scale}px`,
      "background-repeat": "no-repeat",
    } as const;
  };

  // Canvas point → normalized position inside the layer's UNROTATED box.
  const normalizedInLayer = (
    layer: CanvasLayer,
    canvas: { x: number; y: number },
  ): [number, number] => {
    const nx = (canvas.x - layer.x) / Math.max(1, layer.width);
    const ny = (canvas.y - layer.y) / Math.max(1, layer.height);
    return [nx, ny];
  };

  // Layer pointer handlers --------------------------------------------------
  const onLayerPointerDown = (e: PointerEvent, layer: CanvasLayer) => {
    if (e.button !== 0) return; // middle button pans via the fallback
    // Crop mode: let the event bubble to the stage container, which routes
    // it to "draw a new crop rect" (frame-outside drag).
    if (store.croppingId) return;
    e.stopPropagation();
    canvasActions.selectLayer(layer.id);
    const canvas = toCanvas(e.clientX, e.clientY);
    beginInteraction({
      kind: "move",
      id: layer.id,
      startCanvasX: canvas.x,
      startCanvasY: canvas.y,
      startX: layer.x,
      startY: layer.y,
    });
  };

  const onHandlePointerDown = (e: PointerEvent, handle: HandleId) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const layer = selected();
    if (!layer) return;
    beginInteraction({
      kind: "resize",
      id: layer.id,
      handle,
      base: { ...layer },
    });
  };

  const onCropFramePointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const layer = croppingLayer();
    if (!layer) return;
    const baseRect = (store.draftCrop ??
      layer.crop ?? [0, 0, 1, 1]) as CropRect;
    const canvas = toCanvas(e.clientX, e.clientY);
    const [nx, ny] = normalizedInLayer(layer, canvas);
    // A drag on a FULL-frame (the initial box, or after Reset) draws a fresh
    // crop box from the press point (screenshot-tool gesture). Once a real box
    // exists, dragging inside it MOVES it instead.
    const isFullFrame = isFullFrameRect(baseRect);
    if (isFullFrame) {
      beginInteraction({
        kind: "crop-draw",
        id: layer.id,
        startNx: nx,
        startNy: ny,
      });
    } else {
      beginInteraction({
        kind: "crop-move",
        id: layer.id,
        startNx: nx,
        startNy: ny,
        baseRect,
      });
    }
  };

  const onCropHandlePointerDown = (e: PointerEvent, handle: CropHandle) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const layer = croppingLayer();
    if (!layer) return;
    const baseRect = (store.draftCrop ??
      layer.crop ?? [0, 0, 1, 1]) as CropRect;
    beginInteraction({ kind: "crop-resize", id: layer.id, handle, baseRect });
  };

  const onBackgroundPointerDown = (e: PointerEvent) => {
    if (e.button === 1 || e.button === 2) {
      // Middle/right button always pans.
      e.preventDefault();
      const vp = viewport();
      beginInteraction({
        kind: "pan",
        startClientX: e.clientX,
        startClientY: e.clientY,
        startPanX: vp.panX,
        startPanY: vp.panY,
      });
      return;
    }
    if (e.button !== 0) return;
    if (store.croppingId) {
      // Crop mode: only the frame (move/draw) and its handles (resize)
      // interact. Presses on the image outside the frame or on empty canvas
      // are deliberately ignored — no more accidental wipe-and-redraw. Use
      // the Reset button (or Esc) to start over.
      return;
    }
    const canvas = toCanvas(e.clientX, e.clientY);
    if (store.selectionMode) {
      canvasActions.selectLayer(null);
      beginInteraction({
        kind: "draw",
        startCanvasX: canvas.x,
        startCanvasY: canvas.y,
      });
      setDraftSelection([canvas.x, canvas.y, 0, 0]);
    } else {
      // Empty space drags the viewport; clicking (no drag) just deselects.
      canvasActions.selectLayer(null);
      const vp = viewport();
      beginInteraction({
        kind: "pan",
        startClientX: e.clientX,
        startClientY: e.clientY,
        startPanX: vp.panX,
        startPanY: vp.panY,
      });
    }
  };

  // Overlay chrome (selection frame, handles, crop frame) lives INSIDE the
  // transformed container, so the viewport zoom would shrink a 2px border to
  // a hairline at 20%. Every chrome size is multiplied by 1/zoom to stay a
  // constant on-screen size.
  const inv = () => 1 / viewport().zoom;

  const handlePositions = (
    invZ: number,
  ): Record<HandleId, { left: string; top: string }> => ({
    nw: { left: `${-6 * invZ}px`, top: `${-6 * invZ}px` },
    ne: { left: `calc(100% - ${6 * invZ}px)`, top: `${-6 * invZ}px` },
    se: {
      left: `calc(100% - ${6 * invZ}px)`,
      top: `calc(100% - ${6 * invZ}px)`,
    },
    sw: { left: `${-6 * invZ}px`, top: `calc(100% - ${6 * invZ}px)` },
  });

  const cropHandlePositions = (
    invZ: number,
  ): Record<CropHandle, { left: string; top: string }> => ({
    nw: { left: `${-5 * invZ}px`, top: `${-5 * invZ}px` },
    n: { left: `calc(50% - ${5 * invZ}px)`, top: `${-5 * invZ}px` },
    ne: { left: `calc(100% - ${5 * invZ}px)`, top: `${-5 * invZ}px` },
    e: { left: `calc(100% - ${5 * invZ}px)`, top: `calc(50% - ${5 * invZ}px)` },
    se: {
      left: `calc(100% - ${5 * invZ}px)`,
      top: `calc(100% - ${5 * invZ}px)`,
    },
    s: { left: `calc(50% - ${5 * invZ}px)`, top: `calc(100% - ${5 * invZ}px)` },
    sw: { left: `${-5 * invZ}px`, top: `calc(100% - ${5 * invZ}px)` },
    w: { left: `${-5 * invZ}px`, top: `calc(50% - ${5 * invZ}px)` },
  });

  const vp = () => viewport();
  const activeSelection = () => draftSelection() ?? store.selection;

  return (
    <div
      ref={container}
      class="clt-canvas-workspace relative flex-1 overflow-hidden rounded-2xl border border-slate-200 dark:border-slate-700"
      classList={{ "cursor-crosshair": store.selectionMode }}
      onPointerDown={onBackgroundPointerDown}
      onContextMenu={(e) => e.preventDefault()}
      role="application"
      aria-label={t("canvas.stage.aria")}
    >
      <div
        class="absolute left-0 top-0 origin-top-left"
        style={{
          transform: `translate(${vp().panX}px, ${vp().panY}px) scale(${vp().zoom})`,
        }}
      >
        {/* Export board: what you see is what gets written. Transparent
            background renders as a checkerboard. */}
        <Show when={board()}>
          {(b) => (
            <div
              class="absolute"
              style={{
                left: `${b().x}px`,
                top: `${b().y}px`,
                width: `${b().width}px`,
                height: `${b().height}px`,
                "z-index": -1,
              }}
            >
              <Show
                when={backgroundHex()}
                fallback={<div class="clt-checkerboard absolute inset-0" />}
              >
                <div
                  class="absolute inset-0"
                  style={{ background: backgroundHex() ?? "#ffffff" }}
                />
              </Show>
            </div>
          )}
        </Show>

        {/* Layers: array order = z-order. Background-box rendering mirrors
            the compositor (crop → stretch), so what you see is what is
            written. */}
        <For each={store.layers}>
          {(layer, index) => (
            <Show when={layer.visible}>
              <div
                class="clt-layer-image absolute select-none"
                style={{
                  ...layerBoxStyle(layer, store.croppingId === layer.id),
                  "z-index": String(index()),
                }}
                onPointerDown={(e: PointerEvent) =>
                  onLayerPointerDown(e, layer)
                }
              />
            </Show>
          )}
        </For>

        {/* Unified selection / crop box — the SAME rectangle follows the
            layer. Transform mode scales the layer (4 corner handles, no mask,
            rotates with the layer); crop mode selects the keep-region (8
            handles, dimmed mask outside, and the box may shrink inside the
            layer). At crop entry both coincide on the full image, so it reads
            as one box whose function flipped from resize to crop. All geometry
            goes through the box* thunks — plain consts here would freeze at
            their first evaluation (layers mutate in place). */}
        <Show when={boxLayer()} keyed>
          {(layer) => (
            <>
              {/* Crop mask: dim the layer outside the keep-region. */}
              <Show when={boxIsCrop()}>
                <div
                  class="absolute bg-black/45"
                  style={{
                    left: `${layer.x}px`,
                    top: `${layer.y}px`,
                    width: `${layer.width}px`,
                    height: `${boxRect().y}px`,
                    "z-index": "10002",
                  }}
                />
                <div
                  class="absolute bg-black/45"
                  style={{
                    left: `${layer.x}px`,
                    top: `${layer.y + boxRect().y + boxRect().height}px`,
                    width: `${layer.width}px`,
                    height: `${layer.height - boxRect().y - boxRect().height}px`,
                    "z-index": "10002",
                  }}
                />
                <div
                  class="absolute bg-black/45"
                  style={{
                    left: `${layer.x}px`,
                    top: `${layer.y + boxRect().y}px`,
                    width: `${boxRect().x}px`,
                    height: `${boxRect().height}px`,
                    "z-index": "10002",
                  }}
                />
                <div
                  class="absolute bg-black/45"
                  style={{
                    left: `${layer.x + boxRect().x + boxRect().width}px`,
                    top: `${layer.y + boxRect().y}px`,
                    width: `${layer.width - boxRect().x - boxRect().width}px`,
                    height: `${boxRect().height}px`,
                    "z-index": "10002",
                  }}
                />
              </Show>

              {/* The box: transform = full layer (rotates with it, no mask);
                  crop = keep-region (unrotated, masked outside). */}
              <div
                class="absolute"
                classList={{
                  "pointer-events-none": !boxIsCrop(),
                  "border-white": boxIsCrop(),
                  "border-indigo-500": !boxIsCrop(),
                  "cursor-move": boxIsCrop() && !isFullFrameRect(boxDraft()),
                  "cursor-crosshair":
                    boxIsCrop() && isFullFrameRect(boxDraft()),
                }}
                style={{
                  left: `${layer.x + boxRect().x}px`,
                  top: `${layer.y + boxRect().y}px`,
                  width: `${boxRect().width}px`,
                  height: `${boxRect().height}px`,
                  transform: boxIsCrop()
                    ? undefined
                    : `rotate(${layer.rotation}deg)`,
                  "border-width": `${2 * inv()}px`,
                  "z-index": boxIsCrop() ? "10003" : "10000",
                  "box-shadow": boxIsCrop()
                    ? `0 0 0 ${inv()}px rgba(99,102,241,0.9)`
                    : undefined,
                }}
                onPointerDown={boxIsCrop() ? onCropFramePointerDown : undefined}
              >
                <For
                  each={
                    boxIsCrop()
                      ? (Object.keys(
                          cropHandlePositions(inv()),
                        ) as CropHandle[])
                      : (Object.keys(handlePositions(inv())) as HandleId[])
                  }
                >
                  {(handle) => (
                    <div
                      class="clt-canvas-handle pointer-events-auto absolute rounded-full shadow"
                      classList={{
                        "border-indigo-500 bg-white": boxIsCrop(),
                        "border-white bg-indigo-500": !boxIsCrop(),
                      }}
                      style={{
                        ...(boxIsCrop()
                          ? cropHandlePositions(inv())[handle as CropHandle]
                          : handlePositions(inv())[handle as HandleId]),
                        width: `${(boxIsCrop() ? 10 : 12) * inv()}px`,
                        height: `${(boxIsCrop() ? 10 : 12) * inv()}px`,
                        "border-width": `${2 * inv()}px`,
                      }}
                      onPointerDown={(e: PointerEvent) =>
                        boxIsCrop()
                          ? onCropHandlePointerDown(e, handle as CropHandle)
                          : onHandlePointerDown(e, handle as HandleId)
                      }
                    />
                  )}
                </For>
              </div>
            </>
          )}
        </Show>

        {/* Export selection rectangle (draft while drawing, committed after). */}
        <Show when={activeSelection()}>
          {(sel) => (
            <div
              class="pointer-events-none absolute border-2 border-dashed border-amber-500"
              style={{
                left: `${sel()[0]}px`,
                top: `${sel()[1]}px`,
                width: `${sel()[2]}px`,
                height: `${sel()[3]}px`,
                "z-index": "10001",
              }}
            >
              <span class="clt-canvas-selection-label absolute left-1 top-1 rounded bg-amber-500 px-1 text-xs font-semibold text-white">
                {Math.round(sel()[2])} × {Math.round(sel()[3])}
              </span>
            </div>
          )}
        </Show>
      </div>

      {/* Zoom indicator. */}
      <div class="pointer-events-none absolute bottom-3 right-3 rounded-lg bg-black/60 px-2 py-1 text-xs font-semibold text-white">
        {Math.round(vp().zoom * 100)}%
      </div>

      <Show when={store.selectionMode}>
        <div class="pointer-events-none absolute left-3 top-3 rounded-lg bg-amber-500/90 px-2.5 py-1 text-xs font-semibold text-white shadow">
          {t("canvas.toolbar.selectionHint")}
        </div>
      </Show>

      {/* Crop mode floating bar: apply / cancel stay next to the frame. */}
      <Show when={croppingLayer()}>
        <div class="absolute left-1/2 top-3 z-10 flex -translate-x-1/2 items-center gap-2 rounded-xl bg-slate-900/85 px-3 py-2 shadow-lg">
          <span class="text-xs font-semibold text-white">
            {t("canvas.crop.hint")}
          </span>
          <button
            type="button"
            class="rounded-lg bg-indigo-500 px-3 py-1 text-xs font-semibold text-white hover:bg-indigo-600"
            onClick={() => canvasActions.applyCrop(store.croppingId ?? "")}
          >
            {t("canvas.crop.apply")}
          </button>
          <button
            type="button"
            class="rounded-lg bg-slate-700 px-3 py-1 text-xs font-semibold text-white hover:bg-slate-600"
            onClick={() => canvasActions.resetCrop(store.croppingId ?? "")}
          >
            {t("canvas.crop.reset")}
          </button>
          <button
            type="button"
            class="rounded-lg bg-slate-700 px-3 py-1 text-xs font-semibold text-white hover:bg-slate-600"
            onClick={() => canvasActions.cancelCrop()}
          >
            {t("canvas.crop.cancel")}
          </button>
        </div>
      </Show>
    </div>
  );
}
