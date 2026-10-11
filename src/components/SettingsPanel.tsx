import { invoke } from "@tauri-apps/api/core";
import { createEffect, createSignal, For, Show } from "solid-js";
import { useTranslation } from "../i18n";
import { appActions, store } from "../store/useAppStore";
import type { CompressionType, OutputFormat } from "../types";
import { pickOutputDir } from "../utils/dialog";
import { parseSelectValue } from "../utils/parse";
import { sanitizeError } from "../utils/sanitize";
import { sliderFillStyle } from "../utils/slider";
import { FolderIcon, SettingsIcon } from "./Icons";
import { AdvancedSection } from "./settings/AdvancedSection";
import { EditSection } from "./settings/EditSection";
import { ResizeSection } from "./settings/ResizeSection";
import { RadioGroup } from "./settings/shared";
import { TemplateSection } from "./settings/TemplateSection";
import { TransformSection } from "./settings/TransformSection";
import { WatermarkSection } from "./settings/WatermarkSection";

const OUTPUT_FORMATS: OutputFormat[] = [
  "webp",
  "jpeg",
  "png",
  "gif",
  "bmp",
  "tiff",
];

export function SettingsPanel() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";

  // Target-size mode: when enabled, JPEG output auto-bisects quality to fit a
  // byte budget (the old separate "upload prep" mode, now folded in).
  const [kb, setKb] = createSignal(50);
  const targetEnabled = () => store.options.target_size_bytes !== null;

  // Keep the KB fallback in sync no matter what changed the target size
  // (manual input, template application) so re-enabling the toggle
  // restores the last used budget.
  createEffect(() => {
    const ts = store.options.target_size_bytes;
    if (ts !== null) setKb(Math.round(ts / 1024));
  });

  const handleSelectOutputDir = async () => {
    try {
      const dir = await pickOutputDir(t("settings.selectDir"));
      if (dir) {
        // Whitelist the output root so process commands accept writes there.
        await invoke("register_allowed_paths", { paths: [dir] });
        appActions.setOutputDir(dir);
      }
    } catch (error) {
      console.error("Failed to select directory:", error);
      // The output directory is mandatory for a run; a silent failure here
      // would leave the user stuck with an empty field and no hint.
      appActions.setError(
        `${t("errors.outputDirFailed")}: ${sanitizeError(String(error))}`,
      );
    }
  };

  return (
    <div class="card divide-y divide-slate-100 animate-slideUp">
      {/* Header */}
      <div class="p-4 pb-3">
        <div class="flex items-center gap-2">
          <div class="w-8 h-8 rounded-lg bg-indigo-50 flex items-center justify-center">
            <SettingsIcon class="w-4 h-4 text-indigo-500" />
          </div>
          <h3 class="font-semibold text-slate-800">{t("settings.title")}</h3>
        </div>
      </div>

      <div class="p-4 space-y-5">
        {/* Parameter templates: one-click built-ins + user-defined, persisted */}
        <TemplateSection />

        {/* Output Format */}
        <div class="space-y-2">
          <label
            class="text-sm font-medium text-slate-600"
            for="settings-format"
          >
            {t("settings.format")}
          </label>
          <select
            id="settings-format"
            value={store.options.format}
            onChange={(e) =>
              appActions.setOptions({
                format: parseSelectValue(e.currentTarget.value, [
                  "jpeg",
                  "png",
                  "gif",
                  "bmp",
                  "tiff",
                  "webp",
                ] as OutputFormat[]),
              })
            }
            disabled={isProcessing()}
            class={`
              w-full custom-select
              bg-slate-50 border border-slate-200 rounded-xl
              px-4 py-2.5 text-sm text-slate-700 font-medium
              transition-all duration-200
              disabled:opacity-50 disabled:cursor-not-allowed
            `}
          >
            <For each={OUTPUT_FORMATS}>
              {(format) => (
                <option value={format}>{format.toUpperCase()}</option>
              )}
            </For>
          </select>
        </div>

        {/* Quality */}
        <div class="space-y-3">
          <div class="flex justify-between items-center">
            <label
              class="text-sm font-medium text-slate-600"
              for="settings-quality"
            >
              {t("settings.quality")}
            </label>
            <span class="text-sm font-bold text-indigo-600 bg-indigo-50 px-2.5 py-1 rounded-lg">
              {store.options.quality}%
            </span>
          </div>
          <input
            id="settings-quality"
            type="range"
            min="1"
            max="100"
            value={store.options.quality}
            onInput={(e) =>
              appActions.setOptions({
                quality: parseInt(e.currentTarget.value, 10),
              })
            }
            disabled={isProcessing()}
            class="w-full disabled:opacity-50"
            style={{
              background: sliderFillStyle(store.options.quality, 1, 100),
            }}
          />
        </div>

        {/* Target size: auto-fit JPEG to a byte budget (upload-prep folded in) */}
        <div class="space-y-3 pt-4 border-t border-slate-100">
          <div class="flex justify-between items-start gap-4">
            <div>
              <span class="text-sm font-medium text-slate-600">
                {t("settings.targetSize")}
              </span>
              <p class="text-xs text-slate-400 mt-0.5 max-w-[16rem]">
                {t("settings.targetSizeDesc")}
              </p>
            </div>
            <label class="relative inline-flex items-center cursor-pointer flex-shrink-0">
              <input
                type="checkbox"
                class="sr-only peer"
                checked={targetEnabled()}
                onChange={(e) =>
                  appActions.setOptions({
                    target_size_bytes: e.currentTarget.checked
                      ? (kb() || 50) * 1024
                      : null,
                  })
                }
                disabled={isProcessing()}
              />
              <div class="w-10 h-6 bg-slate-200 peer-checked:bg-indigo-500 rounded-full transition-colors"></div>
            </label>
          </div>

          <Show when={targetEnabled()}>
            <div class="space-y-3">
              <div class="flex items-center gap-3">
                <input
                  type="number"
                  min="1"
                  value={Math.round(
                    (store.options.target_size_bytes ?? 0) / 1024,
                  )}
                  onInput={(e) => {
                    const v = parseInt(e.currentTarget.value, 10);
                    if (!Number.isNaN(v) && v > 0) {
                      setKb(v);
                      appActions.setOptions({ target_size_bytes: v * 1024 });
                    }
                  }}
                  disabled={isProcessing()}
                  class="w-28 custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm disabled:opacity-50"
                />
                <span class="text-sm text-slate-500">KB</span>
              </div>

              <Show when={store.options.format !== "jpeg"}>
                <p class="text-xs text-amber-600">
                  {t("settings.targetSizeJpegOnly")}
                </p>
              </Show>
            </div>
          </Show>
        </div>

        {/* Resize (pixels / percent + fit modes) */}
        <div class="pt-4 border-t border-slate-100">
          <ResizeSection />
        </div>

        {/* Rotate / Flip */}
        <div class="pt-4 border-t border-slate-100">
          <TransformSection />
        </div>

        {/* Image editing (crop / fine rotate / tonal / filters) */}
        <div class="pt-4 border-t border-slate-100">
          <EditSection />
        </div>

        {/* Picture watermark */}
        <div class="pt-4 border-t border-slate-100">
          <WatermarkSection />
        </div>

        {/* Advanced (quantize / animation / naming / conflicts) */}
        <div class="pt-4 border-t border-slate-100">
          <AdvancedSection />
        </div>

        {/* Metadata */}
        <div class="pt-4 border-t border-slate-100">
          <RadioGroup
            name="metadata"
            legend={t("settings.metadata")}
            value={store.options.keep_metadata ? "keep" : "remove"}
            options={[
              { value: "keep", label: "settings.keepMetadata" },
              { value: "remove", label: "settings.removeMetadata" },
            ]}
            disabled={isProcessing()}
            onChange={(v) =>
              appActions.setOptions({ keep_metadata: v === "keep" })
            }
          />
        </div>

        {/* Compression */}
        <RadioGroup
          name="compression"
          legend={t("settings.compression")}
          value={store.options.compression}
          options={[
            { value: "lossy" as CompressionType, label: "settings.lossy" },
            {
              value: "lossless" as CompressionType,
              label: "settings.lossless",
            },
          ]}
          disabled={isProcessing()}
          onChange={(v) => appActions.setOptions({ compression: v })}
        />

        {/* Output Directory */}
        <div class="space-y-2">
          <label
            class="text-sm font-medium text-slate-600"
            for="settings-output-dir"
          >
            {t("settings.outputDir")}
          </label>
          <div class="flex gap-2">
            <input
              id="settings-output-dir"
              type="text"
              value={store.outputDir}
              disabled={isProcessing()}
              class="flex-1 custom-input bg-slate-50 border border-slate-200 rounded-xl px-4 py-2.5 text-sm text-slate-600 disabled:opacity-50"
              readOnly
              placeholder={t("settings.selectDir")}
            />
            <button
              type="button"
              onClick={handleSelectOutputDir}
              disabled={isProcessing()}
              class={`
                flex items-center gap-2 px-4 py-2.5
                bg-slate-100 hover:bg-slate-200 border border-slate-200
                rounded-xl text-sm font-medium text-slate-700
                transition-all duration-200
                ${isProcessing() ? "opacity-50 cursor-not-allowed" : ""}
              `}
            >
              <FolderIcon class="w-4 h-4" />
              {t("settings.selectDir")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
