//! Shared building blocks for the settings sections.
//!
//! Split out of `EditSection` (SliderRow) and `WatermarkSection` (RangeRow):
//! the two hand-rolled the identical label+range+value row with drifted
//! APIs. One row serves both, and the radio groups that used to be duplicated
//! inline in `SettingsPanel` live here as `<RadioGroup>`.
import { For } from "solid-js";
import { useTranslation } from "../../i18n";
import { sliderFillStyle } from "../../utils/slider";

/** Label + range + value row (the single version of SliderRow/RangeRow). */
export function RangeRow(props: {
  id?: string;
  label: string;
  min: number;
  max: number;
  step?: number;
  value: number;
  suffix?: string;
  disabled?: boolean;
  format?: (v: number) => string;
  onInput: (v: number) => void;
}) {
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
        value={props.value}
        onInput={(e) => props.onInput(parseFloat(e.currentTarget.value))}
        disabled={props.disabled}
        class="flex-1 disabled:opacity-50"
        style={{
          background: sliderFillStyle(props.value, props.min, props.max),
        }}
      />
      <span class="text-xs font-bold text-indigo-600 bg-indigo-50 px-2 py-0.5 rounded-lg w-12 text-center shrink-0">
        {props.format ? props.format(props.value) : props.value}
        {props.suffix}
      </span>
    </div>
  );
}

/** Labelled radio group (fieldset + legend + horizontal options). */
export function RadioGroup<T extends string>(props: {
  name: string;
  legend: string;
  value: T;
  options: { value: T; label: string }[];
  disabled?: boolean;
  onChange: (v: T) => void;
}) {
  const { t } = useTranslation();
  return (
    <fieldset class="space-y-2">
      <legend class="text-sm font-medium text-slate-600">{props.legend}</legend>
      <div class="flex gap-4">
        <For each={props.options}>
          {(opt) => (
            <label class="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name={props.name}
                checked={props.value === opt.value}
                onChange={() => props.onChange(opt.value)}
                disabled={props.disabled}
                class="custom-radio"
              />
              <span class="text-sm text-slate-700">{t(opt.label)}</span>
            </label>
          )}
        </For>
      </div>
    </fieldset>
  );
}
