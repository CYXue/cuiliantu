import { render } from "solid-js/web";
import App from "./App";
import "./i18n";
import "./App.css";
import { initTheme } from "./theme";

// ---- Range slider fill sync -----------------------------------------------
// The slider CSS paints its filled portion either from the inline
// `sliderFillStyle(...)` background (the rule: EVERY slider sets it, see
// `utils/slider.ts` — it also tracks programmatic value changes) or, as a
// pure CSS fallback, from the `--value` custom property in `App.css`.
// Native <input type=range> has no CSS-only way to read its own value, so
// user-driven updates are synced through this delegated input listener.
// (A global MutationObserver used to sync sliders as they mounted, but every
// slider now carries the inline style, so it only added a document-wide
// childList scan on top of Solid's already-frequent DOM updates.)
function syncRangeFill(el: HTMLInputElement) {
  const min = parseFloat(el.min || "0");
  const max = parseFloat(el.max || "100");
  const v = Number.isFinite(el.valueAsNumber) ? el.valueAsNumber : min;
  const pct = max > min ? ((v - min) / (max - min)) * 100 : 0;
  el.style.setProperty("--value", `${Math.min(100, Math.max(0, pct))}%`);
}

document.addEventListener("input", (e) => {
  const t = e.target;
  if (t instanceof HTMLInputElement && t.type === "range") {
    syncRangeFill(t);
  }
});

// Apply the user's saved theme (light / dark / system) before first paint.
initTheme();

const root = document.getElementById("root");

if (root) {
  render(() => <App />, root);
}
