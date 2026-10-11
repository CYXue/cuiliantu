//! Property editor for the selected layer: exact position/size, rotation,
//! flips, opacity, and a reset-transform shortcut.

import { createMemo, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { canvasActions, store } from "../../store/canvasStore";
import { clampLayerSide } from "../../utils/canvas";
import { RangeRow } from "../settings/shared";

function NumberField(props: {
  label: string;
  value: number;
  min: number;
  max: number;
  disabled?: boolean;
  onCommit: (v: number) => void;
}) {
  return (
    <label class="flex items-center gap-1.5">
      <span class="w-6 shrink-0 text-xs font-medium text-slate-500">
        {props.label}
      </span>
      <input
        type="number"
        value={Math.round(props.value * 100) / 100}
        min={props.min}
        max={props.max}
        disabled={props.disabled}
        onChange={(e) => {
          const v = parseFloat(e.currentTarget.value);
          if (Number.isFinite(v)) props.onCommit(v);
        }}
        // Keep the field editable while selected; refresh from the store on
        // blur instead of fighting the user's typing with reactive resets.
        class="w-20 rounded-lg border border-slate-200 bg-white px-2 py-1.5 text-sm text-slate-800 disabled:opacity-50 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-100"
      />
    </label>
  );
}

export function LayerInspector() {
  const { t } = useTranslation();
  const layer = createMemo(
    () => store.layers.find((l) => l.id === store.selectedId) ?? null,
  );

  const update = (patch: Parameters<typeof canvasActions.updateLayer>[1]) => {
    const current = layer();
    if (current) canvasActions.updateLayer(current.id, patch);
  };

  const toggleButton =
    "rounded-lg px-2.5 py-1.5 text-xs font-semibold transition-colors " +
    "bg-slate-100 text-slate-600 hover:bg-slate-200 dark:bg-slate-700 dark:text-slate-300 dark:hover:bg-slate-600";
  const activeButton =
    "rounded-lg px-2.5 py-1.5 text-xs font-semibold transition-colors " +
    "bg-indigo-500 text-white hover:bg-indigo-600";

  return (
    <div>
      <h3 class="mb-2 px-1 text-xs font-bold uppercase tracking-wide text-slate-400">
        {t("canvas.inspector.title")}
      </h3>
      <Show
        when={layer()}
        fallback={
          <p class="px-1 py-4 text-center text-xs text-slate-400">
            {t("canvas.inspector.empty")}
          </p>
        }
      >
        {(l) => (
          <div class="space-y-3">
            <div class="flex flex-wrap gap-2">
              <NumberField
                label="X"
                value={l().x}
                min={-100000}
                max={100000}
                onCommit={(v) => update({ x: v })}
              />
              <NumberField
                label="Y"
                value={l().y}
                min={-100000}
                max={100000}
                onCommit={(v) => update({ y: v })}
              />
            </div>
            <div class="flex flex-wrap gap-2">
              <NumberField
                label="W"
                value={l().width}
                min={1}
                max={12000}
                onCommit={(v) => update({ width: clampLayerSide(v) })}
              />
              <NumberField
                label="H"
                value={l().height}
                min={1}
                max={12000}
                onCommit={(v) => update({ height: clampLayerSide(v) })}
              />
            </div>

            <RangeRow
              label={t("canvas.inspector.rotation")}
              min={-180}
              max={180}
              step={1}
              value={l().rotation}
              suffix="°"
              onInput={(v) => update({ rotation: v })}
            />

            <div class="flex items-center gap-2">
              <button
                type="button"
                class={l().flipH ? activeButton : toggleButton}
                onClick={() => update({ flipH: !l().flipH })}
              >
                {t("canvas.inspector.flipH")}
              </button>
              <button
                type="button"
                class={l().flipV ? activeButton : toggleButton}
                onClick={() => update({ flipV: !l().flipV })}
              >
                {t("canvas.inspector.flipV")}
              </button>
              <span class="flex-1" />
              <button
                type="button"
                class={toggleButton}
                onClick={() =>
                  update({ rotation: 0, flipH: false, flipV: false })
                }
              >
                {t("canvas.inspector.resetTransform")}
              </button>
            </div>

            <div class="flex items-center gap-2">
              <button
                type="button"
                class={
                  store.croppingId === l().id ? activeButton : toggleButton
                }
                onClick={() => {
                  if (store.croppingId === l().id) canvasActions.cancelCrop();
                  else canvasActions.startCrop(l().id);
                }}
              >
                {t("canvas.inspector.crop")}
              </button>
              <Show
                when={l().crop}
                fallback={
                  <span class="text-[11px] text-slate-400">
                    {t("canvas.crop.none")}
                  </span>
                }
              >
                <button
                  type="button"
                  class={toggleButton}
                  onClick={() => canvasActions.applyCrop(l().id, [0, 0, 1, 1])}
                >
                  {t("canvas.crop.reset")}
                </button>
              </Show>
            </div>

            <RangeRow
              label={t("canvas.inspector.opacity")}
              min={0}
              max={100}
              value={l().opacity}
              suffix="%"
              onInput={(v) => update({ opacity: v })}
            />

            <p class="px-1 text-[11px] leading-4 text-slate-400">
              {l().naturalWidth} × {l().naturalHeight} px ·{" "}
              {t("canvas.inspector.source")}
            </p>
          </div>
        )}
      </Show>
    </div>
  );
}
