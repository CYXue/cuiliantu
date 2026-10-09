import { For, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { appActions, store } from "../../store/useAppStore";
import { sliderFillStyle } from "../../utils/slider";

const CROP_PRESETS: {
  key: string;
  label: string;
  ratio: [number, number] | null;
}[] = [
  { key: "none", label: "settings.edit.cropNone", ratio: null },
  { key: "1:1", label: "1:1", ratio: [1, 1] },
  { key: "4:3", label: "4:3", ratio: [4, 3] },
  { key: "3:2", label: "3:2", ratio: [3, 2] },
  { key: "16:9", label: "16:9", ratio: [16, 9] },
  { key: "9:16", label: "9:16", ratio: [9, 16] },
];

const FILTERS: { key: "grayscale" | "invert" | "sepia"; label: string }[] = [
  { key: "grayscale", label: "settings.edit.filterGray" },
  { key: "invert", label: "settings.edit.filterInvert" },
  { key: "sepia", label: "settings.edit.filterSepia" },
];

export function EditSection() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";
  const o = () => store.options;

  const cropValue = () => {
    const c = o().crop_ratio;
    if (!c) return "none";
    const hit = CROP_PRESETS.find(
      (p) => p.ratio && p.ratio[0] === c[0] && p.ratio[1] === c[1],
    );
    return hit ? hit.key : "none";
  };

  const setCrop = (key: string) => {
    const preset = CROP_PRESETS.find((p) => p.key === key);
    const ratio = preset?.ratio
      ? ([...preset.ratio] as [number, number])
      : null;
    appActions.setOptions({ crop_ratio: ratio });
  };

  const setNumber =
    <K extends keyof ReturnType<typeof o>>(key: K) =>
    (v: number) =>
      appActions.setOptions({ [key]: v } as Partial<ReturnType<typeof o>>);

  const chipClass = (active: boolean) =>
    `text-xs font-medium px-2.5 py-1.5 rounded-lg border transition-colors disabled:opacity-50 ${
      active
        ? "bg-indigo-500 border-indigo-500 text-white"
        : "bg-slate-100 border-transparent text-slate-600 hover:bg-indigo-50 hover:text-indigo-600"
    }`;

  const anyFilterActive = () =>
    o().grayscale || o().invert || o().sepia || o().auto_contrast;

  return (
    <div class="space-y-3">
      <span class="text-sm font-medium text-slate-600">
        {t("settings.edit.title")}
      </span>

      {/* Ratio crop (batch-friendly: one ratio, many images) */}
      <Show when={o().crop_rect}>
        <p class="text-xs text-indigo-600 dark:text-indigo-300">
          {t("settings.edit.cropRectActive")}
        </p>
      </Show>
      <div class="flex items-center gap-3">
        <label
          class="text-xs font-medium text-slate-500 shrink-0"
          for="edit-crop"
        >
          {t("settings.edit.crop")}
        </label>
        <select
          id="edit-crop"
          value={cropValue()}
          onChange={(e) => setCrop(e.currentTarget.value)}
          disabled={isProcessing()}
          class="flex-1 custom-select bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm text-slate-700 disabled:opacity-50"
        >
          <For each={CROP_PRESETS}>
            {(p) => <option value={p.key}>{t(p.label)}</option>}
          </For>
        </select>
      </div>

      <SliderRow
        id="edit-rotate"
        label={t("settings.edit.rotateFine")}
        min={-180}
        max={180}
        step={1}
        value={() => o().rotate_degrees}
        onSet={setNumber("rotate_degrees")}
        format={(v) => `${v}°`}
      />
      <SliderRow
        id="edit-brightness"
        label={t("settings.edit.brightness")}
        min={-100}
        max={100}
        step={1}
        value={() => o().brightness}
        onSet={setNumber("brightness")}
      />
      <SliderRow
        id="edit-contrast"
        label={t("settings.edit.contrast")}
        min={-100}
        max={100}
        step={1}
        value={() => o().contrast}
        onSet={setNumber("contrast")}
      />
      <SliderRow
        id="edit-hue"
        label={t("settings.edit.hue")}
        min={-180}
        max={180}
        step={1}
        value={() => o().hue}
        onSet={setNumber("hue")}
      />
      <SliderRow
        id="edit-sharpen"
        label={t("settings.edit.sharpen")}
        min={0}
        max={100}
        step={1}
        value={() => o().sharpen}
        onSet={setNumber("sharpen")}
      />
      <SliderRow
        id="edit-blur"
        label={t("settings.edit.blur")}
        min={0}
        max={20}
        step={0.5}
        value={() => o().blur}
        onSet={setNumber("blur")}
      />

      {/* Style filters (multi-toggle) */}
      <div class="flex flex-wrap items-center gap-1.5">
        <For each={FILTERS}>
          {(f) => (
            <button
              type="button"
              class={chipClass(o()[f.key])}
              aria-pressed={o()[f.key]}
              disabled={isProcessing()}
              onClick={() => appActions.setOptions({ [f.key]: !o()[f.key] })}
            >
              {t(f.label)}
            </button>
          )}
        </For>
      </div>

      {/* Auto contrast */}
      <label class="flex items-center gap-2 cursor-pointer">
        <input
          type="checkbox"
          checked={o().auto_contrast}
          onChange={(e) =>
            appActions.setOptions({ auto_contrast: e.currentTarget.checked })
          }
          disabled={isProcessing()}
          class="custom-checkbox"
        />
        <span class="text-sm text-slate-700">
          {t("settings.edit.autoContrast")}
        </span>
      </label>
      <Show when={o().auto_contrast}>
        <p class="text-xs text-slate-400 -mt-1">
          {t("settings.edit.autoContrastDesc")}
        </p>
      </Show>

      {/* Keeps the block honest when nothing is active */}
      <Show
        when={
          o().crop_rect === null &&
          o().crop_ratio === null &&
          o().rotate_degrees === 0 &&
          o().brightness === 0 &&
          o().contrast === 0 &&
          o().hue === 0 &&
          o().sharpen === 0 &&
          o().blur === 0 &&
          !anyFilterActive()
        }
      >
        <p class="text-xs text-slate-400">{t("settings.edit.none")}</p>
      </Show>
    </div>
  );
}

function SliderRow(props: {
  id: string;
  label: string;
  min: number;
  max: number;
  step: number;
  value: () => number;
  onSet: (v: number) => void;
  format?: (v: number) => string;
}) {
  const isProcessing = () => store.processingState === "processing";
  return (
    <div class="flex items-center gap-3">
      <label
        class="text-xs font-medium text-slate-500 w-16 shrink-0"
        for={props.id}
      >
        {props.label}
      </label>
      <input
        type="range"
        id={props.id}
        min={props.min}
        max={props.max}
        step={props.step}
        value={props.value()}
        onInput={(e) => props.onSet(parseFloat(e.currentTarget.value))}
        disabled={isProcessing()}
        class="flex-1 disabled:opacity-50"
        style={{
          background: sliderFillStyle(props.value(), props.min, props.max),
        }}
      />
      <span class="text-xs font-bold text-indigo-600 bg-indigo-50 px-2 py-0.5 rounded-lg w-12 text-center shrink-0">
        {props.format ? props.format(props.value()) : props.value()}
      </span>
    </div>
  );
}
