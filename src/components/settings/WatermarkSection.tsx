import { open } from "@tauri-apps/plugin-dialog";
import { onCleanup, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { appActions, store } from "../../store/useAppStore";
import type {
  ImageWatermark,
  TextWatermark,
  WatermarkPosition,
} from "../../types";
import { WATERMARK_IMAGE_EXTENSIONS } from "../../utils/constants";
import { basename } from "../../utils/path";
import { sliderFillStyle } from "../../utils/slider";

// Text watermark font size bounds. These MUST stay in sync with the backend
// clamp in `processor.rs` (the same 8..=400 band) — the Rust side is the
// safety net, this constant is the single source for the UI.
const FONT_SIZE_MIN = 8;
const FONT_SIZE_MAX = 400;

// Kept local to the section on purpose: the store only holds the current
// config, defaults live next to the UI that creates them.
const DEFAULT_IMAGE_WM: ImageWatermark = {
  kind: "image",
  path: "",
  opacity: 80,
  scale_percent: 25,
  position: "bottom_right",
  margin_percent: 3,
};

const DEFAULT_TEXT_WM: TextWatermark = {
  kind: "text",
  text: "",
  font_size: 48,
  color: "#ffffff",
  opacity: 80,
  position: "bottom_right",
  margin_percent: 3,
};

export function WatermarkSection() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";
  const enabled = () => store.options.watermark !== null;
  const imgCfg = (): ImageWatermark | null => {
    const c = store.options.watermark;
    return c?.kind === "image" ? c : null;
  };
  const txtCfg = (): TextWatermark | null => {
    const c = store.options.watermark;
    return c?.kind === "text" ? c : null;
  };

  const toggle = (on: boolean) => {
    appActions.setOptions({ watermark: on ? { ...DEFAULT_IMAGE_WM } : null });
  };

  const switchKind = (kind: "image" | "text") => {
    if (store.options.watermark?.kind === kind) return;
    appActions.setOptions({
      watermark:
        kind === "image" ? { ...DEFAULT_IMAGE_WM } : { ...DEFAULT_TEXT_WM },
    });
  };

  const patchImage = (partial: Partial<ImageWatermark>) => {
    const c = store.options.watermark;
    if (c?.kind !== "image") return;
    appActions.setOptions({ watermark: { ...c, ...partial } });
  };

  const patchText = (partial: Partial<TextWatermark>) => {
    const c = store.options.watermark;
    if (c?.kind !== "text") return;
    appActions.setOptions({ watermark: { ...c, ...partial } });
  };

  // Text input is uncontrolled and synced to the store with a short debounce:
  // a controlled binding would write the store on every keystroke and fight
  // the IME (Chinese composition) plus move the caret. The debounce keeps
  // typing smooth and still feeds the preview/conversion right after.
  let textTimer: ReturnType<typeof setTimeout> | undefined;
  let pendingText: string | null = null;
  const queueTextSync = (v: string) => {
    pendingText = v;
    clearTimeout(textTimer);
    textTimer = setTimeout(() => {
      const value = pendingText;
      pendingText = null;
      if (value !== null) patchText({ text: value });
    }, 250);
  };
  onCleanup(() => {
    clearTimeout(textTimer);
    if (pendingText !== null) patchText({ text: pendingText });
  });

  const pickImage = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          { name: "Images", extensions: [...WATERMARK_IMAGE_EXTENSIONS] },
        ],
      });
      if (typeof selected === "string") {
        patchImage({ path: selected });
      }
    } catch (error) {
      console.error("Failed to pick watermark image:", error);
    }
  };

  const kindClass = (kind: "image" | "text") =>
    `flex-1 text-xs font-medium py-1.5 rounded-lg transition-all duration-200 ${
      store.options.watermark?.kind === kind
        ? "bg-indigo-500 text-white shadow-sm"
        : "text-slate-500 hover:bg-slate-100"
    }`;

  return (
    <div class="space-y-3">
      <label class="flex items-center gap-3 cursor-pointer">
        <input
          type="checkbox"
          checked={enabled()}
          onChange={(e) => toggle(e.currentTarget.checked)}
          disabled={isProcessing()}
          class="custom-checkbox"
        />
        <span class="text-sm font-medium text-slate-600">
          {t("settings.watermarkEnable")}
        </span>
      </label>

      <Show when={enabled()}>
        <div class="space-y-3 animate-fadeIn">
          {/* Kind switch: picture / text */}
          <div class="flex bg-slate-100 rounded-lg p-0.5">
            <button
              type="button"
              class={kindClass("image")}
              disabled={isProcessing()}
              onClick={() => switchKind("image")}
            >
              {t("settings.kindImage")}
            </button>
            <button
              type="button"
              class={kindClass("text")}
              disabled={isProcessing()}
              onClick={() => switchKind("text")}
            >
              {t("settings.kindText")}
            </button>
          </div>

          {/* Picture watermark form */}
          <Show when={imgCfg()}>
            {(c) => (
              <div class="space-y-3">
                <div class="flex items-center gap-2">
                  <span class="text-xs font-medium text-slate-500 w-16 shrink-0">
                    {t("settings.watermarkImage")}
                  </span>
                  <input
                    type="text"
                    readOnly
                    value={
                      c().path
                        ? basename(c().path)
                        : t("settings.watermarkPickFirst")
                    }
                    class="flex-1 custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-1.5 text-xs text-slate-600"
                  />
                  <button
                    type="button"
                    onClick={pickImage}
                    disabled={isProcessing()}
                    class="px-3 py-1.5 bg-slate-100 hover:bg-slate-200 border border-slate-200 rounded-xl text-xs font-medium text-slate-700 transition-all duration-200 disabled:opacity-50"
                  >
                    {t("settings.watermarkPick")}
                  </button>
                </div>

                <RangeRow
                  label={t("settings.opacity")}
                  min={1}
                  max={100}
                  value={c().opacity}
                  suffix=""
                  disabled={isProcessing()}
                  onInput={(v) => patchImage({ opacity: v })}
                />
                <RangeRow
                  label={t("settings.scale")}
                  min={2}
                  max={100}
                  value={c().scale_percent}
                  suffix="%"
                  disabled={isProcessing()}
                  onInput={(v) => patchImage({ scale_percent: v })}
                />
                <PositionSelect
                  value={c().position}
                  disabled={isProcessing()}
                  onChange={(p) => patchImage({ position: p })}
                />
                <RangeRow
                  label={t("settings.margin")}
                  min={0}
                  max={20}
                  value={c().margin_percent}
                  suffix="%"
                  disabled={isProcessing()}
                  onInput={(v) => patchImage({ margin_percent: v })}
                />
              </div>
            )}
          </Show>

          {/* Text watermark form (embedded Smiley Sans) */}
          <Show when={txtCfg()}>
            {(c) => (
              <div class="space-y-3">
                <div class="space-y-1">
                  <label
                    class="text-xs font-medium text-slate-500"
                    for="wm-text"
                  >
                    {t("settings.watermarkText")}
                  </label>
                  <textarea
                    id="wm-text"
                    rows="2"
                    ref={(el) => {
                      // One-time seed: the textarea stays uncontrolled so the
                      // IME (Chinese composition) and caret are never fought.
                      el.value = c().text;
                    }}
                    onInput={(e) => queueTextSync(e.currentTarget.value)}
                    disabled={isProcessing()}
                    placeholder={t("settings.watermarkText")}
                    class="w-full custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm resize-none disabled:opacity-50"
                  />
                  <p class="text-xs text-slate-400">
                    {t("settings.watermarkTextHint")}
                  </p>
                </div>

                <div class="flex items-center gap-3">
                  <span class="text-xs font-medium text-slate-500 w-16 shrink-0">
                    {t("settings.fontSize")}
                  </span>
                  <input
                    type="number"
                    min={FONT_SIZE_MIN}
                    max={FONT_SIZE_MAX}
                    value={c().font_size}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      if (!Number.isNaN(n)) {
                        patchText({
                          font_size: Math.min(
                            FONT_SIZE_MAX,
                            Math.max(FONT_SIZE_MIN, n),
                          ),
                        });
                      }
                    }}
                    disabled={isProcessing()}
                    class="w-20 custom-input bg-slate-50 border border-slate-200 rounded-xl px-2 py-1.5 text-xs disabled:opacity-50"
                  />
                  <span class="text-xs font-medium text-slate-500 ml-2">
                    {t("settings.color")}
                  </span>
                  <input
                    type="color"
                    value={c().color}
                    onInput={(e) => patchText({ color: e.currentTarget.value })}
                    disabled={isProcessing()}
                    class="w-9 h-7 cursor-pointer bg-transparent disabled:opacity-50"
                    aria-label={t("settings.color")}
                  />
                </div>

                <RangeRow
                  label={t("settings.opacity")}
                  min={1}
                  max={100}
                  value={c().opacity}
                  suffix=""
                  disabled={isProcessing()}
                  onInput={(v) => patchText({ opacity: v })}
                />
                <PositionSelect
                  value={c().position}
                  disabled={isProcessing()}
                  onChange={(p) => patchText({ position: p })}
                />
                <RangeRow
                  label={t("settings.margin")}
                  min={0}
                  max={20}
                  value={c().margin_percent}
                  suffix="%"
                  disabled={isProcessing()}
                  onInput={(v) => patchText({ margin_percent: v })}
                />

                <p class="text-xs text-slate-400">{t("settings.fontNote")}</p>
              </div>
            )}
          </Show>
        </div>
      </Show>
    </div>
  );
}

// Small building blocks so both watermark kinds share identical controls.

function RangeRow(props: {
  label: string;
  min: number;
  max: number;
  value: number;
  suffix: string;
  disabled: boolean;
  onInput: (v: number) => void;
}) {
  return (
    <div class="flex items-center gap-3">
      <span class="text-xs font-medium text-slate-500 w-16 shrink-0">
        {props.label}
      </span>
      <input
        type="range"
        min={props.min}
        max={props.max}
        value={props.value}
        onInput={(e) => props.onInput(parseInt(e.currentTarget.value, 10))}
        disabled={props.disabled}
        class="flex-1 disabled:opacity-50"
        style={{
          background: sliderFillStyle(props.value, props.min, props.max),
        }}
      />
      <span class="text-xs font-bold text-indigo-600 bg-indigo-50 px-2 py-0.5 rounded-lg w-12 text-center">
        {props.value}
        {props.suffix}
      </span>
    </div>
  );
}

function PositionSelect(props: {
  value: WatermarkPosition;
  disabled: boolean;
  onChange: (p: WatermarkPosition) => void;
}) {
  const { t } = useTranslation();
  return (
    <div class="flex items-center gap-2">
      <span class="text-xs font-medium text-slate-500 w-16 shrink-0">
        {t("settings.position")}
      </span>
      <select
        value={props.value}
        onChange={(e) =>
          props.onChange(e.currentTarget.value as WatermarkPosition)
        }
        disabled={props.disabled}
        class="flex-1 custom-select bg-slate-50 border border-slate-200 rounded-xl px-3 py-1.5 text-xs text-slate-700 disabled:opacity-50"
      >
        <option value="top_left">{t("settings.posTopLeft")}</option>
        <option value="top_right">{t("settings.posTopRight")}</option>
        <option value="center">{t("settings.posCenter")}</option>
        <option value="bottom_left">{t("settings.posBottomLeft")}</option>
        <option value="bottom_right">{t("settings.posBottomRight")}</option>
      </select>
    </div>
  );
}
