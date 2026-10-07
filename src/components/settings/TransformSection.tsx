import { For, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { appActions, store } from "../../store/useAppStore";

const ROTATIONS = [0, 90, 180, 270];

export function TransformSection() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";
  const rotClass = (deg: number) =>
    `flex-1 text-xs font-medium py-1.5 rounded-lg transition-all duration-200 ${
      store.options.rotate === deg
        ? "bg-indigo-500 text-white shadow-sm"
        : "text-slate-500 hover:bg-slate-100"
    }`;

  return (
    <div class="space-y-3">
      <span class="text-sm font-medium text-slate-600">
        {t("settings.transform")}
      </span>

      <div class="flex bg-slate-100 rounded-lg p-0.5">
        <For each={ROTATIONS}>
          {(deg) => (
            <button
              type="button"
              class={rotClass(deg)}
              disabled={isProcessing()}
              onClick={() => appActions.setOptions({ rotate: deg })}
            >
              {deg === 0 ? t("settings.rotateNone") : `${deg}°`}
            </button>
          )}
        </For>
      </div>

      <div class="flex gap-6">
        <label class="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={store.options.flip_horizontal}
            onChange={(e) =>
              appActions.setOptions({
                flip_horizontal: e.currentTarget.checked,
              })
            }
            disabled={isProcessing()}
            class="custom-checkbox"
          />
          <span class="text-sm text-slate-700">{t("settings.flipH")}</span>
        </label>
        <label class="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={store.options.flip_vertical}
            onChange={(e) =>
              appActions.setOptions({ flip_vertical: e.currentTarget.checked })
            }
            disabled={isProcessing()}
            class="custom-checkbox"
          />
          <span class="text-sm text-slate-700">{t("settings.flipV")}</span>
        </label>
      </div>

      {/* Placeholder keeps the block honest when nothing is active */}
      <Show
        when={
          !store.options.rotate &&
          !store.options.flip_horizontal &&
          !store.options.flip_vertical
        }
      >
        <p class="text-xs text-slate-400">{t("settings.rotateNone")}</p>
      </Show>
    </div>
  );
}
