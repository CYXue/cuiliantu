/**
 * Single source of truth for the range-slider track fill.
 *
 * `SettingsPanel` (quality), `EditSection` (`SliderRow`) and the
 * `input[type="range"]` rule in `App.css` used to hand-roll the same
 * `linear-gradient(...)` independently — three copies that had already drifted
 * in how they derived the fill percentage. Both TSX sides now call this helper
 * reactively (so programmatic value changes — template apply, reset — repaint
 * correctly), while `main.tsx`'s global `--value` sync plus the `App.css`
 * gradient stay as the fallback for any slider rendered without an inline
 * style.
 *
 * The math mirrors `syncRangeFill` in `main.tsx` exactly: percentage of the
 * value inside `[min, max]`, clamped to `0..=100`.
 */
export function sliderFillStyle(
  value: number,
  min: number,
  max: number,
): string {
  const pct = max > min ? ((value - min) / (max - min)) * 100 : 0;
  const fill = Math.min(100, Math.max(0, pct));
  return `linear-gradient(to right, var(--color-primary-500) 0%, var(--color-primary-500) ${fill}%, var(--color-slate-200) ${fill}%, var(--color-slate-200) 100%)`;
}
