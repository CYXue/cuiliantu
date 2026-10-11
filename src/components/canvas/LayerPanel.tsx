//! Layer list: z-order display (top of the list = topmost layer), per-layer
//! visibility/removal, and z-order buttons for the selected layer.

import { createMemo, For, type JSX, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { canvasActions, store } from "../../store/canvasStore";
import { ChevronDownIcon, ChevronUpIcon, TrashIcon } from "../Icons";

export function LayerPanel() {
  const { t } = useTranslation();

  // Render in reverse array order so the topmost layer tops the list.
  const ordered = createMemo(() => [...store.layers].reverse());

  return (
    <div class="flex min-h-0 flex-1 flex-col">
      <h3 class="mb-2 px-1 text-xs font-bold uppercase tracking-wide text-slate-400">
        {t("canvas.layers.title")}
        <span class="ml-1 font-medium text-slate-400">
          ({store.layers.length})
        </span>
      </h3>
      <Show
        when={store.layers.length > 0}
        fallback={
          <p class="px-1 py-6 text-center text-xs text-slate-400">
            {t("canvas.layers.empty")}
          </p>
        }
      >
        <ul class="min-h-0 flex-1 space-y-1 overflow-y-auto pr-0.5">
          <For each={ordered()}>
            {(layer) => {
              const arrayIndex = () =>
                store.layers.findIndex((l) => l.id === layer.id);
              const selected = () => store.selectedId === layer.id;
              return (
                <li>
                  <div
                    class="group flex items-center gap-2 rounded-lg border px-2 py-1.5 transition-colors"
                    classList={{
                      "border-indigo-300 bg-indigo-50 dark:border-indigo-500/60 dark:bg-indigo-950/40":
                        selected(),
                      "border-transparent hover:bg-slate-100 dark:hover:bg-slate-800":
                        !selected(),
                      "opacity-50": !layer.visible,
                    }}
                  >
                    <button
                      type="button"
                      class="flex min-w-0 flex-1 items-center gap-2 text-left"
                      onClick={() => canvasActions.selectLayer(layer.id)}
                    >
                      <img
                        src={layer.src}
                        alt=""
                        class="h-8 w-8 shrink-0 rounded border border-slate-200 object-contain dark:border-slate-600"
                      />
                      <span class="min-w-0 flex-1 truncate text-xs text-slate-700 dark:text-slate-200">
                        {layer.name}
                      </span>
                      <span class="shrink-0 text-[10px] text-slate-400">
                        {Math.round(layer.width)}×{Math.round(layer.height)}
                      </span>
                    </button>
                    <div class="flex shrink-0 items-center gap-0.5">
                      <button
                        type="button"
                        title={
                          layer.visible
                            ? t("canvas.layers.hide")
                            : t("canvas.layers.show")
                        }
                        class="rounded p-1 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700 dark:hover:text-slate-200"
                        onClick={() =>
                          canvasActions.toggleLayerVisible(layer.id)
                        }
                      >
                        <Show when={layer.visible} fallback={<EyeOffGlyph />}>
                          <EyeGlyph />
                        </Show>
                      </button>
                      <button
                        type="button"
                        title={t("canvas.layers.remove")}
                        class="rounded p-1 text-slate-400 hover:bg-rose-50 hover:text-rose-600 dark:hover:bg-rose-950/40"
                        onClick={() => canvasActions.removeLayer(layer.id)}
                      >
                        <TrashIcon class="h-3.5 w-3.5" />
                      </button>
                    </div>
                  </div>
                  {/* Z-order buttons under the selected layer. */}
                  <Show when={selected()}>
                    <div class="mt-1 flex items-center gap-1 px-1">
                      <MiniButton
                        label={t("canvas.layers.toFront")}
                        disabled={arrayIndex() === store.layers.length - 1}
                        onClick={() => canvasActions.sendLayerToFront(layer.id)}
                      >
                        ⤒
                      </MiniButton>
                      <MiniButton
                        label={t("canvas.layers.moveUp")}
                        disabled={arrayIndex() === store.layers.length - 1}
                        onClick={() => canvasActions.moveLayer(layer.id, 1)}
                      >
                        <ChevronUpIcon class="h-3.5 w-3.5" />
                      </MiniButton>
                      <MiniButton
                        label={t("canvas.layers.moveDown")}
                        disabled={arrayIndex() <= 0}
                        onClick={() => canvasActions.moveLayer(layer.id, -1)}
                      >
                        <ChevronDownIcon class="h-3.5 w-3.5" />
                      </MiniButton>
                      <MiniButton
                        label={t("canvas.layers.toBack")}
                        disabled={arrayIndex() <= 0}
                        onClick={() => canvasActions.sendLayerToBack(layer.id)}
                      >
                        ⤓
                      </MiniButton>
                    </div>
                  </Show>
                </li>
              );
            }}
          </For>
        </ul>
      </Show>
    </div>
  );
}

function MiniButton(props: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: JSX.Element;
}) {
  return (
    <button
      type="button"
      title={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
      class="flex h-6 flex-1 items-center justify-center rounded-md text-slate-500 transition-colors hover:bg-slate-200 disabled:cursor-not-allowed disabled:opacity-30 dark:text-slate-400 dark:hover:bg-slate-700"
    >
      {props.children}
    </button>
  );
}

function EyeGlyph() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      class="h-3.5 w-3.5"
      aria-hidden="true"
    >
      <path
        d="M1 12s4-7 11-7 11 7 11 7-4 7-11 7S1 12 1 12z"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

function EyeOffGlyph() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      class="h-3.5 w-3.5"
      aria-hidden="true"
    >
      <path
        d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <path d="M1 1l22 22" stroke-linecap="round" />
    </svg>
  );
}
