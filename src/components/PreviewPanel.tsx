import { invoke } from "@tauri-apps/api/core";
import Cropper from "cropperjs";
import "cropperjs/dist/cropper.css";
import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { useTranslation } from "../i18n";
import { appActions, store } from "../store/useAppStore";
import { formatBytes } from "../utils/format";
import { sliderFillStyle } from "../utils/slider";
import { getTemplateImageBase64 } from "../utils/templateImage";
import { XIcon } from "./Icons";

interface PreviewData {
  before_base64: string;
  after_base64: string;
  before_size: number;
  after_size: number;
  width: number;
  height: number;
}

const round4 = (v: number) => Math.round(v * 10000) / 10000;

// System-default ratio presets for the drag box (null = free). Custom
// persistence flows through the existing OptionsTemplate snapshots, which
// already include `crop_rect`.
const CROP_ASPECTS: { key: string; label: string; ratio: number | null }[] = [
  { key: "free", label: "preview.cropFree", ratio: null },
  { key: "1:1", label: "1:1", ratio: 1 },
  { key: "3:4", label: "3:4", ratio: 3 / 4 },
  { key: "4:3", label: "4:3", ratio: 4 / 3 },
  { key: "2:3", label: "2:3", ratio: 2 / 3 },
  { key: "3:2", label: "3:2", ratio: 3 / 2 },
  { key: "9:16", label: "9:16", ratio: 9 / 16 },
  { key: "16:9", label: "16:9", ratio: 16 / 9 },
  { key: "id", label: "preview.cropIDPhoto", ratio: 5 / 7 },
];

export function PreviewPanel() {
  const { t } = useTranslation();
  const [data, setData] = createSignal<PreviewData | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  const [slider, setSlider] = createSignal(50);
  // True when previewing the built-in template instead of a selected file.
  const [usingTemplate, setUsingTemplate] = createSignal(false);

  // --- Interactive drag-crop -------------------------------------------------
  // The box is drawn on the BEFORE image and stored as normalized fractions
  // (crop_rect in ProcessingOptions), so one drag maps proportionally onto
  // every image of the batch. While the mode is on, the compare preview is
  // frozen (no re-encode per drag tick); exiting the mode re-runs the
  // pipeline and shows the cropped result immediately.
  const [cropMode, setCropMode] = createSignal(false);
  // Last successfully previewed BEFORE image (stable under option changes).
  const [beforeUrl, setBeforeUrl] = createSignal<string | null>(null);
  const [cropSummary, setCropSummary] = createSignal<string | null>(null);
  // Aspect-ratio preset constraining the drag box (null = free).
  const [cropAspect, setCropAspect] = createSignal<number | null>(null);
  let cropperImg: HTMLImageElement | undefined;
  let cropper: Cropper | null = null;

  const hasCropRect = () => store.options.crop_rect !== null;

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
    setCropSummary(
      t("preview.cropSummary", {
        x: Math.round(rect[0] * 100),
        y: Math.round(rect[1] * 100),
        w: Math.round(rect[2] * 100),
        h: Math.round(rect[3] * 100),
      }),
    );
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
    const path = selectedPath();
    if (prevPath !== undefined && path !== prevPath) {
      setCropMode(false);
    }
    return path;
  }, undefined);

  const initCropper = () => {
    const imgEl = cropperImg;
    if (!imgEl) return;
    destroyCropper();
    const saved = store.options.crop_rect;
    cropper = new Cropper(imgEl, {
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
        setCropSummary(
          t("preview.cropSummary", {
            x: Math.round(saved[0] * 100),
            y: Math.round(saved[1] * 100),
            w: Math.round(saved[2] * 100),
            h: Math.round(saved[3] * 100),
          }),
        );
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
      setFailed(false);
      setCropMode(true);
    }
  };

  const selectedPath = () => store.selectedPath;
  // Serializing the options gives the effect a single dependency on any
  // option change, so the preview follows the settings live.
  const optionsKey = () => JSON.stringify(store.options);

  // Monotonic request id: only the newest effect run may write results,
  // so a slow older encode can never overwrite (or clear) a newer preview.
  let seq = 0;

  createEffect(() => {
    const path = selectedPath();
    optionsKey(); // register dependency

    const mySeq = ++seq;

    // While the drag-crop box is being moved, freeze the compare preview:
    // the fractions land in options and take effect on exit. (cropMode is
    // read here on purpose so toggling it re-runs this effect.)
    if (cropMode()) {
      setLoading(false);
      return;
    }

    setLoading(true);
    setFailed(false);
    // Drop the previous preview payload right away: this is the resource
    // release path (previews are base64 data URLs, nothing to revoke), and
    // it prevents stale content from lingering while the new one encodes.
    setData(null);

    // Debounce: dragging sliders shouldn't re-encode on every tick.
    const timer = setTimeout(async () => {
      try {
        const result: PreviewData =
          path != null
            ? await invoke<PreviewData>("preview_image", {
                path,
                options: store.options,
              })
            : await invoke<PreviewData>("preview_image_data", {
                inputBase64: getTemplateImageBase64(),
                options: store.options,
              });
        if (mySeq !== seq) return; // superseded by a newer preview request
        setUsingTemplate(path == null);
        setBeforeUrl(result.before_base64);
        setData(result);
      } catch (error) {
        if (mySeq !== seq) return; // superseded: ignore the stale failure
        console.error("Preview failed:", error);
        setData(null);
        setFailed(true);
      } finally {
        // A superseded request must not clear the newer run's loading state.
        if (mySeq === seq) setLoading(false);
      }
    }, 300);
    onCleanup(() => clearTimeout(timer));
  });

  const savings = () => {
    const d = data();
    if (!d || d.before_size === 0) return 0;
    return ((d.before_size - d.after_size) / d.before_size) * 100;
  };

  return (
    <div class="card p-4 animate-slideUp">
      {/* Header */}
      <div class="flex justify-between items-center mb-3">
        <div class="flex items-center gap-2">
          <h3 class="font-semibold text-slate-800 dark:text-slate-100">
            {t("preview.title")}
          </h3>
          <Show when={usingTemplate()}>
            <span class="text-xs font-medium px-2 py-0.5 rounded-full bg-indigo-50 text-indigo-600 dark:bg-indigo-500/20 dark:text-indigo-300">
              {t("preview.templateBadge")}
            </span>
          </Show>
        </div>
        <div class="flex items-center gap-2">
          <button
            type="button"
            onClick={toggleCropMode}
            aria-pressed={cropMode()}
            class={`text-xs font-medium px-2.5 py-1.5 rounded-lg border transition-colors ${
              cropMode()
                ? "bg-indigo-500 border-indigo-500 text-white"
                : "bg-slate-100 border-transparent text-slate-600 hover:bg-indigo-50 hover:text-indigo-600 dark:bg-slate-700 dark:text-slate-300 dark:hover:bg-slate-600"
            }`}
          >
            {t("preview.cropToggle")}
          </button>
          <Show when={selectedPath()}>
            <button
              type="button"
              onClick={() => appActions.setSelectedPath(null)}
              class="p-1.5 hover:bg-slate-100 dark:hover:bg-slate-700 rounded-lg transition-colors"
              aria-label={t("preview.close")}
            >
              <XIcon class="w-4 h-4 text-slate-500 dark:text-slate-400" />
            </button>
          </Show>
        </div>
      </div>

      {/* Body */}
      <Show
        when={!failed()}
        fallback={
          <p class="text-sm text-rose-600 py-6 text-center">
            {t("preview.failed")}
          </p>
        }
      >
        {/* Drag-crop mode: draw the box on the before image */}
        <Show when={cropMode()}>
          <div class="space-y-3">
            <Show
              when={beforeUrl()}
              fallback={
                <p class="text-sm text-slate-400 py-6 text-center animate-pulse">
                  {t("preview.loading")}
                </p>
              }
            >
              <div class="w-full rounded-xl overflow-hidden bg-slate-100 dark:bg-slate-800 select-none">
                <img
                  ref={cropperImg}
                  src={`data:image/png;base64,${beforeUrl()}`}
                  alt={t("preview.original")}
                  class="block max-w-full max-h-80"
                  onLoad={initCropper}
                  draggable={false}
                />
              </div>
              {/* System-default ratio presets for the drag box */}
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-xs font-medium text-slate-500 dark:text-slate-400 mr-1">
                  {t("preview.cropAspectLabel")}
                </span>
                <For each={CROP_ASPECTS}>
                  {(a) => (
                    <button
                      type="button"
                      onClick={() => pickAspect(a.ratio)}
                      aria-pressed={
                        (cropAspect() ?? null) === a.ratio ||
                        (a.ratio === null && cropAspect() === null)
                      }
                      class={`text-xs font-medium px-2 py-1 rounded-lg border transition-colors ${
                        (cropAspect() ?? null) === a.ratio ||
                        (a.ratio === null && cropAspect() === null)
                          ? "bg-indigo-500 border-indigo-500 text-white"
                          : "bg-slate-100 border-transparent text-slate-600 hover:bg-indigo-50 hover:text-indigo-600 dark:bg-slate-700 dark:text-slate-300"
                      }`}
                    >
                      {t(a.label)}
                    </button>
                  )}
                </For>
              </div>
              <div class="flex items-center justify-between gap-3">
                <p class="text-xs text-slate-500 dark:text-slate-400 min-w-0 truncate">
                  {cropSummary() ?? t("preview.cropHint")}
                </p>
                <div class="flex gap-2 shrink-0">
                  <Show when={hasCropRect()}>
                    <button
                      type="button"
                      onClick={clearCrop}
                      class="text-xs font-medium px-2.5 py-1.5 rounded-lg bg-slate-100 text-slate-600 hover:bg-slate-200 dark:bg-slate-700 dark:text-slate-300"
                    >
                      {t("preview.cropClear")}
                    </button>
                  </Show>
                  <button
                    type="button"
                    onClick={exitCropMode}
                    class="text-xs font-medium px-2.5 py-1.5 rounded-lg bg-indigo-500 text-white hover:bg-indigo-400"
                  >
                    {t("preview.cropDone")}
                  </button>
                </div>
              </div>
              <p class="text-xs text-slate-400 dark:text-slate-500">
                {t("preview.cropBatchHint")}
              </p>
            </Show>
          </div>
        </Show>

        {/* Compare preview (hidden while the crop box is up) */}
        <Show when={!cropMode()}>
          <Show
            when={!loading()}
            fallback={
              <p class="text-sm text-slate-400 py-6 text-center animate-pulse">
                {t("preview.loading")}
              </p>
            }
          >
            <Show when={data()}>
              {(d) => (
                <div class="space-y-3">
                  {/* Comparison viewport: before is clipped to the slider,
                      after fills the whole box. */}
                  <div
                    class="relative w-full rounded-xl overflow-hidden bg-slate-100 dark:bg-slate-800 select-none"
                    style={{ height: "320px" }}
                  >
                    <img
                      src={`data:image/png;base64,${d().after_base64}`}
                      alt={t("preview.result")}
                      class="absolute inset-0 w-full h-full object-contain"
                      draggable={false}
                    />
                    <img
                      src={`data:image/png;base64,${d().before_base64}`}
                      alt={t("preview.original")}
                      class="absolute inset-0 w-full h-full object-contain"
                      draggable={false}
                      style={{ "clip-path": `inset(0 ${100 - slider()}% 0 0)` }}
                    />
                    {/* Divider */}
                    <div
                      class="absolute top-0 bottom-0 w-0.5 bg-white shadow-[0_0_4px_rgba(0,0,0,0.4)] pointer-events-none"
                      style={{ left: `${slider()}%` }}
                    />
                    {/* Labels */}
                    <span class="absolute top-2 left-2 px-2 py-0.5 text-xs font-medium bg-black/50 text-white rounded-md">
                      {t("preview.original")}
                    </span>
                    <span class="absolute top-2 right-2 px-2 py-0.5 text-xs font-medium bg-black/50 text-white rounded-md">
                      {t("preview.result")}
                    </span>
                  </div>

                  {/* Slider control */}
                  <input
                    type="range"
                    min="0"
                    max="100"
                    value={slider()}
                    onInput={(e) =>
                      setSlider(parseInt(e.currentTarget.value, 10))
                    }
                    class="w-full"
                    aria-label={t("preview.title")}
                    style={{ background: sliderFillStyle(slider(), 0, 100) }}
                  />

                  {/* Real sizes from the same encode path as the batch run */}
                  <div class="flex items-center justify-center gap-3 text-sm text-slate-500 bg-slate-50/70 dark:bg-slate-800/70 rounded-xl py-2">
                    <span>{formatBytes(d().before_size)}</span>
                    <span class="text-slate-300 dark:text-slate-600">→</span>
                    <span
                      class={
                        d().after_size <= d().before_size
                          ? "font-semibold text-emerald-600 dark:text-emerald-400"
                          : "font-semibold text-rose-600 dark:text-rose-400"
                      }
                    >
                      {formatBytes(d().after_size)}
                    </span>
                    <span
                      class={`px-2 py-0.5 rounded-md text-xs font-bold ${
                        savings() >= 0
                          ? "bg-emerald-50 text-emerald-600 dark:bg-emerald-500/15 dark:text-emerald-300"
                          : "bg-rose-50 text-rose-600 dark:bg-rose-500/15 dark:text-rose-300"
                      }`}
                    >
                      {savings() >= 0
                        ? t("preview.saved", { percent: savings().toFixed(1) })
                        : t("preview.larger", {
                            percent: (-savings()).toFixed(1),
                          })}
                    </span>
                  </div>

                  <p class="text-xs text-slate-400 dark:text-slate-500 text-center">
                    {d().width} × {d().height} px
                  </p>

                  <Show when={usingTemplate()}>
                    <p class="text-xs text-slate-400 dark:text-slate-500 text-center">
                      {t("preview.templateHint")}
                    </p>
                  </Show>
                </div>
              )}
            </Show>
          </Show>
        </Show>
      </Show>
    </div>
  );
}
