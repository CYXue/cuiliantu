import { For, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { appActions, store } from "../../store/useAppStore";
import type { FitMode, ResizeMode } from "../../types";
import { sliderFillStyle } from "../../utils/slider";

export function ResizeSection() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";
  const isPercent = () => store.options.resize_mode === "percent";
  const modeClass = (mode: ResizeMode) =>
    `flex-1 text-xs font-medium py-1.5 rounded-lg transition-all duration-200 ${
      store.options.resize_mode === mode
        ? "bg-indigo-500 text-white shadow-sm"
        : "text-slate-500 hover:bg-slate-100"
    }`;
  const fitClass = (mode: FitMode) =>
    `flex-1 text-xs font-medium py-1.5 rounded-lg transition-all duration-200 ${
      store.options.fit_mode === mode
        ? "bg-indigo-500 text-white shadow-sm"
        : "text-slate-500 hover:bg-slate-100"
    }`;

  return (
    <div class="space-y-3">
      <div class="flex justify-between items-center">
        <span class="text-sm font-medium text-slate-600">
          {t("settings.resizeMode")}
        </span>
        <div class="flex bg-slate-100 rounded-lg p-0.5 w-44">
          <button
            type="button"
            class={modeClass("pixels")}
            disabled={isProcessing()}
            onClick={() => appActions.setOptions({ resize_mode: "pixels" })}
          >
            {t("settings.modePixels")}
          </button>
          <button
            type="button"
            class={modeClass("percent")}
            disabled={isProcessing()}
            onClick={() => appActions.setOptions({ resize_mode: "percent" })}
          >
            {t("settings.modePercent")}
          </button>
        </div>
      </div>

      {/* Percent mode: one scale slider, aspect ratio kept */}
      <Show when={isPercent()}>
        <div class="flex items-center gap-3">
          <input
            type="range"
            min="1"
            max="400"
            value={store.options.resize_percent}
            onInput={(e) =>
              appActions.setOptions({
                resize_percent: parseInt(e.currentTarget.value, 10),
              })
            }
            disabled={isProcessing()}
            class="flex-1 disabled:opacity-50"
            style={{
              background: sliderFillStyle(store.options.resize_percent, 1, 400),
            }}
          />
          <span class="text-sm font-bold text-indigo-600 bg-indigo-50 px-2.5 py-1 rounded-lg w-16 text-center">
            {store.options.resize_percent}%
          </span>
        </div>
      </Show>

      {/* Pixel mode: box + fit strategy */}
      <Show when={!isPercent()}>
        <div class="flex gap-3">
          <div class="flex-1">
            <label
              class="text-xs font-medium text-slate-500 mb-1 block"
              for="settings-width"
            >
              {t("settings.width")}
            </label>
            <input
              id="settings-width"
              type="number"
              min="1"
              max="65535"
              placeholder="px"
              value={store.options.width || ""}
              onInput={(e) => {
                const raw = e.currentTarget.value;
                if (!raw) {
                  appActions.setOptions({ width: null });
                  return;
                }
                const v = parseInt(raw, 10);
                // Clamp before it reaches the store: the backend applies the
                // same envelope, but the displayed value should match what a
                // batch run will actually use.
                if (!Number.isNaN(v)) {
                  appActions.setOptions({
                    width: Math.min(65535, Math.max(1, v)),
                  });
                }
              }}
              disabled={isProcessing()}
              class="w-full custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm disabled:opacity-50"
            />
          </div>
          <div class="flex-1">
            <label
              class="text-xs font-medium text-slate-500 mb-1 block"
              for="settings-height"
            >
              {t("settings.height")}
            </label>
            <input
              id="settings-height"
              type="number"
              min="1"
              max="65535"
              placeholder="px"
              value={store.options.height || ""}
              onInput={(e) => {
                const raw = e.currentTarget.value;
                if (!raw) {
                  appActions.setOptions({ height: null });
                  return;
                }
                const v = parseInt(raw, 10);
                if (!Number.isNaN(v)) {
                  appActions.setOptions({
                    height: Math.min(65535, Math.max(1, v)),
                  });
                }
              }}
              disabled={isProcessing()}
              class="w-full custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm disabled:opacity-50"
            />
          </div>
        </div>
      </Show>

      <Show
        when={
          !isPercent() &&
          store.options.width !== null &&
          store.options.height !== null
        }
      >
        <div class="space-y-2">
          <span class="text-xs font-medium text-slate-500">
            {t("settings.fitMode")}
          </span>
          <div class="flex bg-slate-100 rounded-lg p-0.5">
            <For each={["stretch", "contain", "cover"] as FitMode[]}>
              {(mode) => (
                <button
                  type="button"
                  class={fitClass(mode)}
                  disabled={isProcessing()}
                  onClick={() => appActions.setOptions({ fit_mode: mode })}
                >
                  {t(
                    `settings.fit${mode.charAt(0).toUpperCase()}${mode.slice(1)}`,
                  )}
                </button>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
}
