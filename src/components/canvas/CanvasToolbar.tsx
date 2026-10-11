//! Canvas toolbar: add images, one-click tiling, background, selection mode,
//! clear, and the export controls.

import { open } from "@tauri-apps/plugin-dialog";
import { createSignal, For, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { canvasActions, store } from "../../store/canvasStore";
import { sanitizeError } from "../../utils/sanitize";
import {
  DownloadIcon,
  ImageIcon,
  PlusIcon,
  SpinnerIcon,
  TrashIcon,
} from "../Icons";
import { RangeRow } from "../settings/shared";
import { addImagesFromPaths } from "./canvasAdd";
import { type CanvasExportFormat, exportCanvas } from "./canvasExport";

const FORMATS: CanvasExportFormat[] = ["png", "jpeg", "webp"];

export function CanvasToolbar() {
  const { t } = useTranslation();
  const [gap, setGap] = createSignal(0);
  const [format, setFormat] = createSignal<CanvasExportFormat>("png");
  const [quality, setQuality] = createSignal(90);

  const hasLayers = () => store.layers.length > 0;
  const hasVisible = () => store.layers.some((l) => l.visible);

  const pickImages = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "Images",
            extensions: [
              "png",
              "jpg",
              "jpeg",
              "webp",
              "gif",
              "bmp",
              "tiff",
              "tif",
              "avif",
            ],
          },
        ],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      await addImagesFromPaths(paths);
    } catch (error) {
      console.error("Failed to add canvas images:", error);
      canvasActions.setExportError(
        `${t("errors.addFilesFailed")}: ${sanitizeError(String(error))}`,
      );
    }
  };

  const runExport = () => void exportCanvas(format(), quality(), t);

  const toolbarButton =
    "inline-flex items-center gap-1.5 rounded-lg px-3 py-2 text-sm font-medium transition-colors " +
    "bg-slate-100 text-slate-700 hover:bg-slate-200 dark:bg-slate-700 dark:text-slate-200 dark:hover:bg-slate-600 " +
    "disabled:cursor-not-allowed disabled:opacity-40";

  return (
    <div class="space-y-3">
      {/* Row 1: content actions */}
      <div class="flex flex-wrap items-center gap-2">
        <button
          type="button"
          class={toolbarButton}
          onClick={() => void pickImages()}
        >
          <PlusIcon class="h-4 w-4" />
          {t("canvas.toolbar.add")}
        </button>

        <div class="mx-1 h-6 w-px bg-slate-200 dark:bg-slate-700" />

        <button
          type="button"
          class={toolbarButton}
          disabled={!hasVisible()}
          onClick={() => canvasActions.alignLayers("horizontal", gap())}
        >
          {t("canvas.toolbar.alignH")}
        </button>
        <button
          type="button"
          class={toolbarButton}
          disabled={!hasVisible()}
          onClick={() => canvasActions.alignLayers("vertical", gap())}
        >
          {t("canvas.toolbar.alignV")}
        </button>
        <label class="flex items-center gap-1.5 text-xs text-slate-500">
          {t("canvas.toolbar.gap")}
          <input
            type="number"
            min={0}
            max={500}
            value={gap()}
            onInput={(e) =>
              setGap(
                Math.max(
                  0,
                  Math.min(500, parseInt(e.currentTarget.value, 10) || 0),
                ),
              )
            }
            class="w-16 rounded-lg border border-slate-200 bg-white px-2 py-1.5 text-sm text-slate-800 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-100"
          />
          <span>px</span>
        </label>

        <div class="mx-1 h-6 w-px bg-slate-200 dark:bg-slate-700" />

        {/* Background picker */}
        <label class="flex items-center gap-1.5 text-xs text-slate-500">
          {t("canvas.toolbar.background")}
          <select
            value={store.backgroundMode}
            onChange={(e) =>
              canvasActions.setBackgroundMode(
                e.currentTarget.value as typeof store.backgroundMode,
              )
            }
            class="rounded-lg border border-slate-200 bg-white px-2 py-1.5 text-sm text-slate-800 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-100"
          >
            <option value="transparent">
              {t("canvas.toolbar.bgTransparent")}
            </option>
            <option value="white">{t("canvas.toolbar.bgWhite")}</option>
            <option value="black">{t("canvas.toolbar.bgBlack")}</option>
            <option value="custom">{t("canvas.toolbar.bgCustom")}</option>
          </select>
        </label>
        <Show when={store.backgroundMode === "custom"}>
          <input
            type="color"
            value={store.customColor}
            onInput={(e) => canvasActions.setCustomColor(e.currentTarget.value)}
            class="h-8 w-10 cursor-pointer rounded border border-slate-200 dark:border-slate-600"
            aria-label={t("canvas.toolbar.bgCustom")}
          />
        </Show>

        <div class="mx-1 h-6 w-px bg-slate-200 dark:bg-slate-700" />

        <button
          type="button"
          class={`${toolbarButton} ${
            store.selectionMode
              ? "!bg-amber-500 !text-white hover:!bg-amber-600 dark:!bg-amber-500 dark:hover:!bg-amber-600"
              : ""
          }`}
          disabled={!hasVisible()}
          onClick={() => canvasActions.setSelectionMode(!store.selectionMode)}
        >
          {t("canvas.toolbar.selectionMode")}
        </button>
        <Show when={store.selection}>
          <button
            type="button"
            class={toolbarButton}
            onClick={() => canvasActions.setSelection(null)}
          >
            {t("canvas.toolbar.clearSelection")}
          </button>
        </Show>

        <div class="flex-1" />

        <button
          type="button"
          class={`${toolbarButton} !text-rose-600 hover:!bg-rose-50 dark:!text-rose-400 dark:hover:!bg-rose-950/40`}
          disabled={!hasLayers()}
          onClick={() => canvasActions.clearCanvas()}
        >
          <TrashIcon class="h-4 w-4" />
          {t("canvas.toolbar.clearCanvas")}
        </button>
      </div>

      {/* Row 2: export */}
      <div class="flex flex-wrap items-center gap-3 rounded-xl bg-slate-50 px-3 py-2.5 dark:bg-slate-800/60">
        <span class="flex items-center gap-1.5 text-sm font-semibold text-slate-700 dark:text-slate-200">
          <DownloadIcon class="h-4 w-4" />
          {t("canvas.export.title")}
        </span>

        <select
          value={format()}
          onChange={(e) =>
            setFormat(e.currentTarget.value as CanvasExportFormat)
          }
          disabled={store.exporting}
          class="rounded-lg border border-slate-200 bg-white px-2 py-1.5 text-sm text-slate-800 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-100"
          aria-label={t("canvas.export.format")}
        >
          <For each={FORMATS}>
            {(f) => <option value={f}>{f.toUpperCase()}</option>}
          </For>
        </select>

        <Show when={format() !== "png"}>
          <div class="w-44">
            <RangeRow
              label={t("canvas.export.quality")}
              min={1}
              max={100}
              value={quality()}
              disabled={store.exporting}
              onInput={setQuality}
            />
          </div>
        </Show>

        <button
          type="button"
          onClick={runExport}
          disabled={!hasVisible() || store.exporting}
          class={`
            ml-auto flex items-center gap-2 rounded-xl px-4 py-2 text-sm font-semibold text-white
            transition-all duration-200
            ${
              hasVisible() && !store.exporting
                ? "bg-gradient-to-r from-[#5b5bd6] to-[#6d56e0] shadow-md shadow-indigo-500/20 hover:shadow-lg hover:shadow-indigo-500/25"
                : "cursor-not-allowed bg-slate-300 text-slate-500 shadow-none dark:bg-slate-700 dark:text-slate-500"
            }
          `}
        >
          <Show
            when={!store.exporting}
            fallback={<SpinnerIcon class="h-4 w-4 animate-spin" />}
          >
            <ImageIcon class="h-4 w-4" />
          </Show>
          {store.exporting
            ? t("canvas.export.exporting")
            : t("canvas.export.button")}
        </button>

        <Show when={store.exportResult}>
          {(result) => (
            <span class="text-xs text-emerald-600 dark:text-emerald-400">
              ✓ {result().name} · {result().width}×{result().height} ·{" "}
              {(result().sizeBytes / 1024).toFixed(0)} KB
            </span>
          )}
        </Show>
        <Show when={store.exportError}>
          <span
            class="max-w-xs truncate text-xs text-rose-600 dark:text-rose-400"
            title={store.exportError ?? ""}
          >
            {store.exportError}
          </span>
        </Show>
      </div>
    </div>
  );
}
