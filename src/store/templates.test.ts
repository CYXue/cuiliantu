import { beforeEach, describe, expect, it } from "vitest";
import type { OptionsTemplate } from "../types";
import { loadCustomTemplates } from "./templates";

const NEW_KEY = "cuiliantu-templates.v1";
const PREV_KEY = "tucui-templates.v1";
const OLDEST_KEY = "image-crunch-templates.v1";

function template(id: string): OptionsTemplate {
  return {
    id,
    name: `Template ${id}`,
    options: { format: "jpeg", quality: 80 },
  } as OptionsTemplate;
}

beforeEach(() => {
  localStorage.clear();
});

describe("loadCustomTemplates (rename migration)", () => {
  it("moves templates saved under a pre-rename key to the new one", () => {
    localStorage.setItem(
      PREV_KEY,
      JSON.stringify([template("a"), template("b")]),
    );

    const loaded = loadCustomTemplates();

    expect(loaded.map((t) => t.id)).toEqual(["a", "b"]);
    expect(localStorage.getItem(NEW_KEY)).toBe(
      JSON.stringify([template("a"), template("b")]),
    );
    expect(localStorage.getItem(PREV_KEY)).toBeNull();
  });

  it("prefers the newest legacy key when both generations exist", () => {
    localStorage.setItem(OLDEST_KEY, JSON.stringify([template("oldest")]));
    localStorage.setItem(PREV_KEY, JSON.stringify([template("prev")]));

    expect(loadCustomTemplates().map((t) => t.id)).toEqual(["prev"]);
    expect(localStorage.getItem(NEW_KEY)).toBe(
      JSON.stringify([template("prev")]),
    );
    expect(localStorage.getItem(PREV_KEY)).toBeNull();
    expect(localStorage.getItem(OLDEST_KEY)).toBeNull();
  });

  it("does not resurrect corrupt legacy data", () => {
    localStorage.setItem(PREV_KEY, JSON.stringify({ not: "an array" }));

    expect(loadCustomTemplates()).toEqual([]);
    expect(localStorage.getItem(NEW_KEY)).toBeNull();
    expect(localStorage.getItem(PREV_KEY)).not.toBeNull();
  });

  it("keeps the current key when it already holds templates", () => {
    localStorage.setItem(NEW_KEY, JSON.stringify([template("new")]));
    localStorage.setItem(PREV_KEY, JSON.stringify([template("old")]));

    expect(loadCustomTemplates().map((t) => t.id)).toEqual(["new"]);
    expect(localStorage.getItem(NEW_KEY)).toBe(
      JSON.stringify([template("new")]),
    );
  });

  it("returns an empty list when nothing is stored", () => {
    expect(loadCustomTemplates()).toEqual([]);
  });
});
