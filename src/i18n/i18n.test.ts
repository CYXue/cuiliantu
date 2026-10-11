import { describe, expect, it } from "vitest";

import en from "../i18n/locales/en.json";
import ja from "../i18n/locales/ja.json";
import zhCN from "../i18n/locales/zh-CN.json";

// The app ships three locales and switches between them at runtime. A key
// present in `en` but missing from `zh-CN` does not fail the build — i18next
// silently falls back to the English string, so the UI degrades to a mixed
// language with no error anywhere. That is exactly the kind of failure a test
// has to catch, because nothing else will.

type Catalog = { [key: string]: string | Catalog };

function flatten(catalog: Catalog, prefix = ""): string[] {
  return Object.entries(catalog).flatMap(([key, value]) =>
    typeof value === "string"
      ? [`${prefix}${key}`]
      : flatten(value, `${prefix}${key}.`),
  );
}

const LOCALES = { en, ja, "zh-CN": zhCN } as const;

describe("i18n locale catalogs", () => {
  it("exposes a non-trivial number of keys", () => {
    // Guards the checks below against silently comparing empty objects: if
    // every catalog were empty, "all locales match" would pass for the wrong
    // reason.
    expect(flatten(en as Catalog).length).toBeGreaterThan(100);
  });

  for (const [name, catalog] of Object.entries(LOCALES)) {
    it(`${name} has exactly the same keys as en`, () => {
      const expected = flatten(en as Catalog).sort();
      const actual = flatten(catalog as Catalog).sort();
      expect(actual).toEqual(expected);
    });
  }

  it("has no empty translation strings", () => {
    // An empty value renders as a blank label or button — the key looks
    // present to every key-parity check above, so only this catches it.
    for (const [name, catalog] of Object.entries(LOCALES)) {
      const empties: string[] = [];
      const walk = (node: Catalog, path = ""): void => {
        for (const [key, value] of Object.entries(node)) {
          const full = `${path}${key}`;
          if (typeof value === "string") {
            if (value.trim() === "") empties.push(full);
          } else {
            walk(value as Catalog, `${full}.`);
          }
        }
      };
      walk(catalog as Catalog);
      expect({ name, empties }).toEqual({ name, empties: [] });
    }
  });

  it("keeps interpolation placeholders identical across locales", () => {
    // The real silent-failure mode for a translation catalog: a locale drops
    // or misspells a `{{name}}`-style placeholder. Key parity still passes, the
    // string count still matches, and the UI renders literal `{{count}}` to the
    // user at runtime. Compare the placeholder *sets* per key.
    const placeholders = (value: string): string[] =>
      (value.match(/\{\{\s*[\w.]+\s*\}\}/g) ?? [])
        .map((m) => m.replace(/\s+/g, ""))
        .sort();

    const collect = (catalog: Catalog, prefix = ""): Map<string, string[]> => {
      const map = new Map<string, string[]>();
      for (const [key, value] of Object.entries(catalog)) {
        const path = `${prefix}${key}`;
        if (typeof value === "string") {
          map.set(path, placeholders(value));
        } else {
          for (const [k, v] of collect(value as Catalog, `${path}.`)) {
            map.set(k, v);
          }
        }
      }
      return map;
    };

    const enPlaceholders = collect(en as Catalog);
    for (const [name, catalog] of Object.entries(LOCALES)) {
      if (name === "en") continue;
      const mismatched: string[] = [];
      for (const [key, expected] of enPlaceholders) {
        const actual = collect(catalog as Catalog).get(key) ?? [];
        if (expected.join("|") !== actual.join("|")) mismatched.push(key);
      }
      expect({ name, mismatched }).toEqual({ name, mismatched: [] });
    }
  });
});

// ---- Runtime behavior -------------------------------------------------------
//
// The module initializes i18next at import time from localStorage /
// navigator.language, so every case here re-imports the module with a seeded
// (or cleared) storage to exercise the detection path itself.

import { afterEach, beforeEach, vi } from "vitest";

const LANG_KEY = "cuiliantu-lang";

beforeEach(() => {
  localStorage.clear();
  vi.resetModules();
});

afterEach(() => {
  // Restore the real detection for whatever runs after this suite.
  vi.unstubAllGlobals?.();
});

describe("i18n runtime behavior", () => {
  it("adopts a stored language and switches at runtime", async () => {
    localStorage.setItem(LANG_KEY, "ja");
    const i18n = (await import("../i18n")).default;
    expect(i18n.language).toBe("ja");

    await i18n.changeLanguage("zh-CN");
    expect(i18n.language).toBe("zh-CN");
    // Persistence itself happens in the languageChanged handler that
    // `useTranslation` registers per component; the component-level test in
    // ipc-smoke covers that wiring.
  });

  it("migrates a pre-rename language preference", async () => {
    localStorage.setItem("tucui-lang", "ja");

    const i18n = (await import("../i18n")).default;

    expect(i18n.language).toBe("ja");
    expect(localStorage.getItem(LANG_KEY)).toBe("ja");
    expect(localStorage.getItem("tucui-lang")).toBeNull();
  });

  it("detects zh-CN from the browser language when nothing is stored", async () => {
    vi.stubGlobal("navigator", { language: "zh-CN" });

    const i18n = (await import("../i18n")).default;

    expect(i18n.language).toBe("zh-CN");
  });

  it("detects ja from the browser language when nothing is stored", async () => {
    vi.stubGlobal("navigator", { language: "ja-JP" });

    const i18n = (await import("../i18n")).default;

    expect(i18n.language).toBe("ja");
  });

  it("falls back to en for an unrecognized browser language", async () => {
    vi.stubGlobal("navigator", { language: "fr-FR" });

    const i18n = (await import("../i18n")).default;

    expect(i18n.language).toBe("en");
  });
});
