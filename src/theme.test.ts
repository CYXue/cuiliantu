import { beforeEach, describe, expect, it, vi } from "vitest";

const NEW_KEY = "cuiliantu-theme";
const PREV_KEY = "tucui-theme";
const OLDEST_KEY = "image-crunch-theme";

// The stored mode is read once at module init, so the module has to be
// re-imported after seeding localStorage for the migration to run again.
beforeEach(() => {
  localStorage.clear();
  vi.resetModules();
});

describe("theme storage (rename migration)", () => {
  it("moves a pre-rename theme preference to the new key", async () => {
    localStorage.setItem(PREV_KEY, "dark");

    const { getTheme } = await import("./theme");

    expect(getTheme()).toBe("dark");
    expect(localStorage.getItem(NEW_KEY)).toBe("dark");
    expect(localStorage.getItem(PREV_KEY)).toBeNull();
  });

  it("falls back to the oldest key when the newer one is absent", async () => {
    localStorage.setItem(OLDEST_KEY, "light");

    const { getTheme } = await import("./theme");

    expect(getTheme()).toBe("light");
    expect(localStorage.getItem(NEW_KEY)).toBe("light");
    expect(localStorage.getItem(OLDEST_KEY)).toBeNull();
  });

  it("falls back to system when nothing is stored", async () => {
    const { getTheme } = await import("./theme");

    expect(getTheme()).toBe("system");
  });

  it("still adopts the legacy value when persisting it fails", async () => {
    localStorage.setItem(PREV_KEY, "dark");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const setItem = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota exceeded");
      });

    const { getTheme } = await import("./theme");

    // The migration failed, but the preference read from the legacy key wins
    // over a silent fallback to "system".
    expect(getTheme()).toBe("dark");
    expect(warn).toHaveBeenCalled();
    setItem.mockRestore();
    warn.mockRestore();
  });
});

describe("theme switching and system sync", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
  });

  it("applies and persists the dark mode", async () => {
    const { setTheme } = await import("./theme");

    setTheme("dark");

    expect(localStorage.getItem(NEW_KEY)).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("removes the dark class for the light mode", async () => {
    const { setTheme } = await import("./theme");

    setTheme("light");

    expect(localStorage.getItem(NEW_KEY)).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("resolves system mode through matchMedia and follows OS changes", async () => {
    let matches = true;
    const listeners: Array<() => void> = [];
    const mqFactory = () => ({
      get matches() {
        return matches;
      },
      addEventListener: (_: string, cb: () => void) => listeners.push(cb),
      removeEventListener: () => {},
    });
    (window as unknown as Record<string, unknown>).matchMedia =
      vi.fn(mqFactory);

    const { setTheme, initTheme } = await import("./theme");
    setTheme("system");
    initTheme();
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    // The OS flips to light: the "system" mode must follow.
    matches = false;
    for (const cb of listeners) cb();
    expect(document.documentElement.classList.contains("dark")).toBe(false);

    delete (window as unknown as Record<string, unknown>).matchMedia;
  });
});
