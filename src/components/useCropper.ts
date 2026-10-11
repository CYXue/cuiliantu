//! Interactive drag-crop state machine for the preview panel.
//!
//! Extracted from `PreviewPanel`: the cropper instance, its mount ref, the
//! crop-mode/aspect/summary signals and every transition between them. The
//! component owns the compare-preview pipeline (`beforeUrl`, debounced
//! re-encode) and all markup; entering/exiting crop mode is a signal read
//! away, so the frozen-preview behavior is unchanged.
import type Cropper from "cropperjs";
import { createEffect, createSignal, onCleanup } from "solid-js";
import type { Translator } from "../i18n";
import { appActions, store } from "../store/useAppStore";

const round4 = (v: number) => Math.round(v * 10000) / 10000;

// cropperjs (~200 KB plus its CSS) used to sit on the static import graph of
// every launch, though only the drag-crop mode ever instantiates it. Load it
// lazily the first time a crop-mode image mounts instead.
let CropperCtor: typeof import("cropperjs").default | null = null;

async function loadCropper(): Promise<typeof import("cropperjs").default> {
  if (!CropperCtor) {
    const [mod] = await Promise.all([
      import("cropperjs"),
      import("cropperjs/dist/cropper.css"),
    ]);
    CropperCtor = mod.default;
  }
  return CropperCtor;
}

export function useCropper(t: Translator) {
  // True while the drag box is up: the compare preview is frozen (no
  // re-encode per drag tick); exiting re-runs the pipeline and shows the
  // cropped result immediately.
  const [cropMode, setCropMode] = createSignal(false);
  const [cropSummary, setCropSummary] = createSignal<string | null>(null);
  // Aspect-ratio preset constraining the drag box (null = free).
  const [cropAspect, setCropAspect] = createSignal<number | null>(null);

  let cropperImg: HTMLImageElement | undefined;
  let cropper: Cropper | null = null;

  const hasCropRect = () => store.options.crop_rect !== null;

  const formatCropSummary = (rect: [number, number, number, number]) =>
    t("preview.cropSummary", {
      x: Math.round(rect[0] * 100),
      y: Math.round(rect[1] * 100),
      w: Math.round(rect[2] * 100),
      h: Math.round(rect[3] * 100),
    });

  // Read the current box and mirror it into options + the summary line.
  const syncBox = () => {
    if (!cropper) return;
    const box = cropper.getData();
    const info = cropper.getImageData();
    const nw = info.naturalWidth || cropperImg?.naturalWidth || 0;
    const nh = info.naturalHeight || cropperImg?.naturalHeight || 0;
    if (!nw || !nh) return;
    const rect = [
      round4(box.x / nw),
      round4(box.y / nh),
      round4(box.width / nw),
      round4(box.height / nh),
    ] as [number, number, number, number];
    appActions.setOptions({ crop_rect: rect });
    setCropSummary(formatCropSummary(rect));
  };

  // Resize/re-center the box to the largest rect of the given ratio that fits
  // the image, so picking a preset gives immediate visual feedback.
  const applyMaxBox = (ratio: number) => {
    if (!cropper) return;
    const info = cropper.getImageData();
    const nw = info.naturalWidth || cropperImg?.naturalWidth || 0;
    const nh = info.naturalHeight || cropperImg?.naturalHeight || 0;
    if (!nw || !nh) return;
    let bw: number;
    let bh: number;
    if (nw / nh > ratio) {
      bh = nh;
      bw = nh * ratio;
    } else {
      bw = nw;
      bh = nw / ratio;
    }
    cropper.setData({
      x: (nw - bw) / 2,
      y: (nh - bh) / 2,
      width: bw,
      height: bh,
    });
  };

  const pickAspect = (ratio: number | null) => {
    setCropAspect(ratio);
    if (!cropper) return;
    // v1 uses NaN for "free".
    cropper.setAspectRatio(ratio ?? NaN);
    if (ratio != null) {
      applyMaxBox(ratio);
      syncBox();
    }
  };

  const destroyCropper = () => {
    cropper?.destroy();
    cropper = null;
  };
  onCleanup(destroyCropper);

  // Leaving the panel / switching files tears the cropper down and exits
  // crop mode (the fractions persist in options either way).
  createEffect((prevPath) => {
    const path = store.selectedPath;
    if (prevPath !== undefined && path !== prevPath) {
      setCropMode(false);
    }
    return path;
  }, undefined);

  /** JSX `ref` target for the BEFORE image the box is drawn on. */
  const cropperRef = (el: HTMLImageElement) => {
    cropperImg = el;
  };

  const initCropper = async () => {
    const imgEl = cropperImg;
    if (!imgEl) return;
    destroyCropper();
    const Ctor = await loadCropper();
    // The panel can unmount while the chunk loads (user toggled crop mode
    // back off before the first import resolved) — never attach to a dead
    // node, that would leak the cropper instance.
    if (cropperImg !== imgEl) return;
    const saved = store.options.crop_rect;
    cropper = new Ctor(imgEl, {
      viewMode: 1,
      autoCropArea: saved ? 1 : 0.8,
      // v1 uses NaN for "free".
      aspectRatio: cropAspect() ?? NaN,
      background: false,
      cropend: () => syncBox(),
    });
    // Restore a previously drawn box (fractions × natural size).
    if (saved) {
      const nw = imgEl.naturalWidth;
      const nh = imgEl.naturalHeight;
      if (nw && nh) {
        cropper.setData({
          x: saved[0] * nw,
          y: saved[1] * nh,
          width: saved[2] * nw,
          height: saved[3] * nh,
        });
        setCropSummary(formatCropSummary(saved));
      }
    } else if (cropAspect() != null) {
      // No saved box yet: start from the largest centered box of the
      // selected preset ratio.
      applyMaxBox(cropAspect() as number);
      syncBox();
    }
  };

  const exitCropMode = () => {
    setCropMode(false); // effect re-runs → pipeline preview shows the crop
  };

  const clearCrop = () => {
    cropper?.clear();
    appActions.setOptions({ crop_rect: null });
    setCropSummary(null);
  };

  const toggleCropMode = () => {
    if (cropMode()) {
      exitCropMode();
    } else {
      setCropMode(true);
    }
  };

  return {
    cropMode,
    exitCropMode,
    toggleCropMode,
    cropSummary,
    cropAspect,
    pickAspect,
    clearCrop,
    hasCropRect,
    cropperRef,
    initCropper,
  };
}
