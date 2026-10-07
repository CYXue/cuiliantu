import { render } from "solid-js/web";
import App from "./App";
import "./i18n";
import "./App.css";
import { initTheme } from "./theme";

// ---- Range slider fill sync -----------------------------------------------
// The slider CSS paints its filled portion from a `--value` custom property.
// Native <input type=range> has no CSS-only way to read its own value, so a
// single global sync keeps EVERY slider (current and future) correct:
// fills track the value relative to [min, max], no per-component code.
function syncRangeFill(el: HTMLInputElement) {
  const min = parseFloat(el.min || "0");
  const max = parseFloat(el.max || "100");
  const v = Number.isFinite(el.valueAsNumber) ? el.valueAsNumber : min;
  const pct = max > min ? ((v - min) / (max - min)) * 100 : 0;
  el.style.setProperty("--value", `${Math.min(100, Math.max(0, pct))}%`);
}

function syncAllRanges(root: Document | Element) {
  root
    .querySelectorAll<HTMLInputElement>('input[type="range"]')
    .forEach(syncRangeFill);
}

document.addEventListener("input", (e) => {
  const t = e.target;
  if (t instanceof HTMLInputElement && t.type === "range") {
    syncRangeFill(t);
  }
});

// Sliders mounted later (Show-conditioned panels) get synced on insertion.
new MutationObserver((mutations) => {
  for (const m of mutations) {
    m.addedNodes.forEach((n) => {
      if (n instanceof HTMLInputElement && n.type === "range") {
        syncRangeFill(n);
      } else if (n instanceof HTMLElement) {
        syncAllRanges(n);
      }
    });
  }
}).observe(document.body, { childList: true, subtree: true });

syncAllRanges(document);

// Apply the user's saved theme (light / dark / system) before first paint.
initTheme();

const root = document.getElementById("root");

if (root) {
  render(() => <App />, root);
}
