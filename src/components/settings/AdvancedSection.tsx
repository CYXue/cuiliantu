import { Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { appActions, store } from "../../store/useAppStore";

export function AdvancedSection() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";
  const quantizeOn = () => store.options.quantize_colors !== null;

  return (
    <div class="space-y-3">
      <span class="text-sm font-medium text-slate-600">
        {t("settings.advanced")}
      </span>

      {/* Color quantization (PNG/GIF) */}
      <div class="space-y-2">
        <label class="flex items-center gap-3 cursor-pointer">
          <input
            type="checkbox"
            checked={quantizeOn()}
            onChange={(e) =>
              appActions.setOptions({
                quantize_colors: e.currentTarget.checked ? 256 : null,
              })
            }
            disabled={isProcessing()}
            class="custom-checkbox"
          />
          <span class="text-sm text-slate-700">
            {t("settings.quantizeEnable")}
          </span>
        </label>
        <Show when={quantizeOn()}>
          <div class="flex items-center gap-3 pl-7 animate-fadeIn">
            <span class="text-xs font-medium text-slate-500 shrink-0">
              {t("settings.quantizeColors")}
            </span>
            <input
              type="number"
              min="2"
              max="256"
              value={store.options.quantize_colors ?? 256}
              onInput={(e) => {
                const n = parseInt(e.currentTarget.value, 10);
                if (!Number.isNaN(n)) {
                  appActions.setOptions({
                    quantize_colors: Math.min(256, Math.max(2, n)),
                  });
                }
              }}
              disabled={isProcessing()}
              class="w-20 custom-input bg-slate-50 border border-slate-200 rounded-xl px-2 py-1.5 text-xs disabled:opacity-50"
            />
          </div>
        </Show>
      </div>

      {/* GIF animation preservation */}
      <label class="flex items-center gap-3 cursor-pointer">
        <input
          type="checkbox"
          checked={store.options.preserve_animation}
          onChange={(e) =>
            appActions.setOptions({
              preserve_animation: e.currentTarget.checked,
            })
          }
          disabled={isProcessing()}
          class="custom-checkbox"
        />
        <span class="text-sm text-slate-700">
          {t("settings.preserveAnimation")}
        </span>
      </label>

      {/* Only save when smaller */}
      <label class="flex items-center gap-3 cursor-pointer">
        <input
          type="checkbox"
          checked={store.options.skip_if_larger}
          onChange={(e) =>
            appActions.setOptions({ skip_if_larger: e.currentTarget.checked })
          }
          disabled={isProcessing()}
          class="custom-checkbox"
        />
        <span class="text-sm text-slate-700">{t("settings.skipIfLarger")}</span>
      </label>

      {/* Filename pattern */}
      <div class="space-y-1">
        <label
          class="text-xs font-medium text-slate-500"
          for="settings-filename-pattern"
        >
          {t("settings.filenamePattern")}
        </label>
        <input
          id="settings-filename-pattern"
          type="text"
          value={store.options.filename_pattern}
          onInput={(e) =>
            appActions.setOptions({ filename_pattern: e.currentTarget.value })
          }
          disabled={isProcessing()}
          placeholder="{name}"
          class="w-full custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm disabled:opacity-50"
        />
        <p class="text-xs text-slate-400">
          {t("settings.filenamePatternHint")}
        </p>
      </div>

      {/* Conflict policy */}
      <div class="space-y-1">
        <span class="text-xs font-medium text-slate-500">
          {t("settings.conflictPolicy")}
        </span>
        <select
          value={store.options.conflict_policy}
          onChange={(e) =>
            appActions.setOptions({
              conflict_policy: e.currentTarget
                .value as typeof store.options.conflict_policy,
            })
          }
          disabled={isProcessing()}
          class="w-full custom-select bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm text-slate-700 disabled:opacity-50"
        >
          <option value="overwrite">{t("settings.conflictOverwrite")}</option>
          <option value="auto_rename">
            {t("settings.conflictAutoRename")}
          </option>
          <option value="skip">{t("settings.conflictSkip")}</option>
        </select>
      </div>
    </div>
  );
}
