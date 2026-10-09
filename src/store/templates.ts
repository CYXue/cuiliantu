import type { OptionsTemplate, ProcessingOptions } from "../types";
import { defaultOptions } from "./defaults";

// localStorage key for user-defined templates (v1 schema: OptionsTemplate[]).
// Legacy keys predate the renames (image-crunch → tucui → cuiliantu); they are
// migrated on first read so an existing install does not lose its templates.
const STORAGE_KEY = "cuiliantu-templates.v1";
const LEGACY_STORAGE_KEYS = [
  "tucui-templates.v1",
  "image-crunch-templates.v1",
] as const;

// Built-ins are full option snapshots, so applying one always yields a
// deterministic configuration regardless of the current settings.
// `name` holds an i18n key; the "builtin:" id prefix tells the UI apart
// from user-defined templates (whose name is plain text).
function builtin(
  id: string,
  nameKey: string,
  overrides: Partial<ProcessingOptions>,
): OptionsTemplate {
  return { id, name: nameKey, options: { ...defaultOptions, ...overrides } };
}

export const BUILTIN_TEMPLATES: OptionsTemplate[] = [
  // Migrated from the old hard-coded target-size presets.
  builtin("builtin:id-photo", "settings.templates.builtin.idPhoto", {
    format: "jpeg",
    width: 196,
    height: 250,
    fit_mode: "stretch",
    target_size_bytes: 50 * 1024,
  }),
  // Standard Chinese ID photo sizes at 300 dpi
  // (1-in: 25×35mm → 295×413, 2-in: 35×49mm → 413×579, small 2-in: 35×45mm → 413×531).
  // "cover" fills the exact pixel box without distortion (center crop),
  // which is what upload systems checking pixel dimensions expect.
  builtin("builtin:id-1inch-small", "settings.templates.builtin.id1inchSmall", {
    format: "jpeg",
    quality: 90,
    width: 260,
    height: 378,
    fit_mode: "cover",
  }),
  builtin("builtin:id-1inch", "settings.templates.builtin.id1inch", {
    format: "jpeg",
    quality: 90,
    width: 295,
    height: 413,
    fit_mode: "cover",
  }),
  builtin("builtin:id-1inch-large", "settings.templates.builtin.id1inchLarge", {
    format: "jpeg",
    quality: 90,
    width: 390,
    height: 567,
    fit_mode: "cover",
  }),
  builtin("builtin:id-2inch", "settings.templates.builtin.id2inch", {
    format: "jpeg",
    quality: 90,
    width: 413,
    height: 579,
    fit_mode: "cover",
  }),
  builtin("builtin:id-2inch-small", "settings.templates.builtin.id2inchSmall", {
    format: "jpeg",
    quality: 90,
    width: 413,
    height: 531,
    fit_mode: "cover",
  }),
  // Social-security / 2nd-gen ID card photo: 26×32mm at 350 dpi.
  builtin("builtin:id-sscard", "settings.templates.builtin.idSscard", {
    format: "jpeg",
    quality: 90,
    width: 358,
    height: 441,
    fit_mode: "cover",
  }),
  builtin("builtin:material", "settings.templates.builtin.material", {
    format: "jpeg",
    width: null,
    height: null,
    target_size_bytes: 300 * 1024,
  }),
  // Typical scenarios: web page, social sharing, lossless archive.
  // Width-only pixel resize keeps the aspect ratio (backend branch
  // `(Some(w), None)`), so 1920/1280 act as a longest-side cap.
  builtin("builtin:web", "settings.templates.builtin.web", {
    format: "webp",
    quality: 80,
    width: 1920,
    height: null,
    fit_mode: "contain",
  }),
  builtin("builtin:social", "settings.templates.builtin.social", {
    format: "jpeg",
    quality: 75,
    width: 1280,
    height: null,
    fit_mode: "contain",
  }),
  builtin("builtin:lossless", "settings.templates.builtin.lossless", {
    format: "png",
    compression: "lossless",
    target_size_bytes: null,
  }),
];

function isOptionsTemplate(value: unknown): value is OptionsTemplate {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.id === "string" &&
    typeof v.name === "string" &&
    typeof v.options === "object" &&
    v.options !== null
  );
}

// Moves templates saved under pre-rename keys to the current one, then drops
// the legacy entries. Returns the raw payload, or null when there is nothing
// usable to move (absent or already-corrupt data is not resurrected).
function migrateLegacyTemplates(): string | null {
  for (const key of LEGACY_STORAGE_KEYS) {
    const legacy = localStorage.getItem(key);
    if (!legacy) continue;
    try {
      const parsed: unknown = JSON.parse(legacy);
      if (!Array.isArray(parsed)) return null;
      localStorage.setItem(STORAGE_KEY, legacy);
      for (const k of LEGACY_STORAGE_KEYS) localStorage.removeItem(k);
      return legacy;
    } catch (error) {
      console.warn("Failed to migrate templates:", error);
      return null;
    }
  }
  return null;
}

// Guarded for non-browser environments (unit tests, no-window contexts).
export function loadCustomTemplates(): OptionsTemplate[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY) ?? migrateLegacyTemplates();
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isOptionsTemplate);
  } catch (error) {
    console.warn("Failed to load templates:", error);
    return [];
  }
}

export function persistCustomTemplates(list: OptionsTemplate[]) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch (error) {
    console.warn("Failed to save templates:", error);
  }
}

export function newTemplateId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") {
    return `tpl-${c.randomUUID()}`;
  }
  return `tpl-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
