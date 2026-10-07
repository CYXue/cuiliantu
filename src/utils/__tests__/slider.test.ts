import { describe, expect, it } from "vitest";
import { sliderFillStyle } from "../slider";

describe("sliderFillStyle", () => {
  it("maps the value proportionally into [min, max]", () => {
    expect(sliderFillStyle(50, 0, 100)).toContain("50%");
    expect(sliderFillStyle(0, 0, 100)).toContain("0%");
    expect(sliderFillStyle(100, 0, 100)).toContain("100%");
  });

  it("offsets by min, not by zero", () => {
    // 100 inside [1, 400] is ~24.8% of the track, not 25% and not 100%.
    expect(sliderFillStyle(100, 1, 400)).toContain("24.8");
  });

  it("clamps out-of-range values", () => {
    expect(sliderFillStyle(-10, 0, 100)).toContain("0%");
    expect(sliderFillStyle(150, 0, 100)).toContain("100%");
  });

  it("degenerate range renders an empty fill", () => {
    expect(sliderFillStyle(7, 5, 5)).toContain("0%");
  });

  it("keeps the shared gradient definition", () => {
    // The colours must stay in sync with the `input[type="range"]` fallback
    // rule in App.css and with `syncRangeFill` in main.tsx.
    const style = sliderFillStyle(25, 0, 100);
    expect(style).toContain("var(--color-primary-500)");
    expect(style).toContain("var(--color-slate-200)");
    expect(style.startsWith("linear-gradient(to right,")).toBe(true);
  });
});
