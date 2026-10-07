import { createSignal } from "solid-js";

export type ThemeMode = "light" | "dark" | "system";

// Legacy keys from before the renames (image-crunch → tucui → cuiliantu);
// read oldest-to-newest so an existing install keeps its theme instead of
// silently falling back to "system".
const LEGACY_STORAGE_KEYS = ["tucui-theme", "image-crunch-theme"] as const;
const STORAGE_KEY = "cuiliantu-theme";

function prefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    !!window.matchMedia &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

function resolve(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") return prefersDark() ? "dark" : "light";
  return mode;
}

function apply(mode: ThemeMode): void {
  const dark = resolve(mode) === "dark";
  document.documentElement.classList.toggle("dark", dark);
}

function readInitial(): ThemeMode {
  if (typeof localStorage === "undefined") return "system";
  const stored = localStorage.getItem(STORAGE_KEY) ?? migrateLegacyTheme();
  if (stored === "light" || stored === "dark" || stored === "system") {
    return stored;
  }
  return "system";
}

// One-shot migration: the newest legacy value is copied to the current key,
// then every legacy entry is dropped.
function migrateLegacyTheme(): string | null {
  for (const key of LEGACY_STORAGE_KEYS) {
    const legacy = localStorage.getItem(key);
    if (!legacy) continue;
    try {
      localStorage.setItem(STORAGE_KEY, legacy);
      for (const k of LEGACY_STORAGE_KEYS) localStorage.removeItem(k);
    } catch (error) {
      console.warn("Failed to migrate theme preference:", error);
    }
    return legacy;
  }
  return null;
}

const [theme, setThemeSignal] = createSignal<ThemeMode>(readInitial());

/** Reactive accessor for the UI (light | dark | system). */
export function getTheme() {
  return theme();
}

/** Switch theme, persist the choice, and apply it to <html>. */
export function setTheme(mode: ThemeMode): void {
  setThemeSignal(mode);
  if (typeof localStorage !== "undefined") {
    localStorage.setItem(STORAGE_KEY, mode);
  }
  apply(mode);
}

/** Apply the stored theme on startup and keep "system" in sync with OS. */
export function initTheme(): void {
  apply(theme());
  if (typeof window !== "undefined" && window.matchMedia) {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (theme() === "system") apply("system");
    };
    // Safari < 14 uses addListener; modern browsers use addEventListener.
    if (mq.addEventListener) mq.addEventListener("change", onChange);
    else if (mq.addListener) mq.addListener(onChange);
  }
}
