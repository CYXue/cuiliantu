import i18n from "i18next";
import { createSignal, onCleanup } from "solid-js";

import en from "./locales/en.json";
import ja from "./locales/ja.json";
import zhCN from "./locales/zh-CN.json";

// Legacy keys from before the renames (image-crunch → tucui → cuiliantu);
// read oldest-to-newest so an existing install keeps its language instead of
// silently falling back to English.
const LEGACY_STORAGE_KEYS = ["tucui-lang", "image-crunch-lang"] as const;
const STORAGE_KEY = "cuiliantu-lang";

// One-shot migration: copy the newest value stored under a pre-rename key to
// the current one, then drop every legacy entry. Returns null when there is
// nothing to move.
function migrateLegacy(): string | null {
  if (typeof localStorage === "undefined") return null;
  for (const key of LEGACY_STORAGE_KEYS) {
    const legacy = localStorage.getItem(key);
    if (!legacy) continue;
    try {
      localStorage.setItem(STORAGE_KEY, legacy);
      for (const k of LEGACY_STORAGE_KEYS) localStorage.removeItem(k);
    } catch (error) {
      console.warn("Failed to migrate language preference:", error);
    }
    return legacy;
  }
  return null;
}

function detectDefaultLang(): string {
  const saved = localStorage.getItem(STORAGE_KEY) ?? migrateLegacy();
  if (saved) return saved;

  const nav = navigator.language?.toLowerCase() ?? "";
  if (nav.startsWith("zh")) return "zh-CN";
  if (nav.startsWith("ja")) return "ja";
  return "en";
}

i18n.init({
  resources: {
    en: { translation: en },
    ja: { translation: ja },
    "zh-CN": { translation: zhCN },
  },
  lng: detectDefaultLang(),
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

// Reflect the detected language on the document immediately — the
// `languageChanged` handler below only fires on later switches, and the
// HTML ships as lang="en".
document.documentElement.lang = i18n.language;

/**
 * SolidJS-friendly i18n hook.
 *
 * i18next is framework-agnostic here (no react-i18next). We expose a `t` that
 * reads a language signal so that changing language re-renders every call site.
 */
export function useTranslation() {
  const [lang, setLang] = createSignal(i18n.language);

  // Registered per-component (i18next is an app-wide singleton, so without an
  // `off` the listener would accumulate on every component mount). Clean it up
  // on unmount so re-mounting never leaves duplicates behind.
  const handleLangChanged = (l: string) => {
    setLang(l);
    localStorage.setItem(STORAGE_KEY, l);
    // Keep the document language in sync for screen readers and the
    // browser's font selection (the HTML ships as lang="en").
    document.documentElement.lang = l;
  };
  i18n.on("languageChanged", handleLangChanged);
  onCleanup(() => i18n.off("languageChanged", handleLangChanged));

  const t = (key: string, opts?: Record<string, unknown>): string => {
    // Reading the signal registers a dependency so the consuming component
    // re-renders when the language changes.
    lang();
    return i18n.t(key, opts) as string;
  };

  return { t, i18n, lang };
}

export default i18n;
