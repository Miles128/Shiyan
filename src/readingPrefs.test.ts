import { describe, expect, it } from "vitest";
import {
  defaultReadingPrefs,
  normalizeReadingPrefs,
  readingCssVars,
  resolveReadingPrefs,
} from "./readingPrefs";

describe("normalizeReadingPrefs", () => {
  it("returns defaults for empty input", () => {
    expect(normalizeReadingPrefs(undefined)).toEqual(defaultReadingPrefs());
    expect(normalizeReadingPrefs({})).toEqual(defaultReadingPrefs());
  });

  it("keeps valid values", () => {
    expect(
      normalizeReadingPrefs({
        reader_font: "sans",
        reader_font_size: 22,
        reader_line_height: 1.9,
        reader_line_width: "narrow",
      }),
    ).toEqual({
      reader_font: "sans",
      reader_font_size: 22,
      reader_line_height: 1.9,
      reader_line_width: "narrow",
    });
  });

  it("falls back on invalid values", () => {
    expect(
      normalizeReadingPrefs({
        reader_font: "comic-sans" as never,
        reader_font_size: 13 as never,
        reader_line_height: 3 as never,
        reader_line_width: "huge" as never,
      }),
    ).toEqual(defaultReadingPrefs());
  });
});

describe("resolveReadingPrefs", () => {
  it("defaults to full width so the column tracks the window", () => {
    const resolved = resolveReadingPrefs(defaultReadingPrefs());
    expect(resolved.measure).toBe("100%");
    expect(resolved.fontSizePx).toBe(18);
    expect(resolved.lineHeight).toBe(1.75);
  });

  it("maps medium width to the 52rem cap when chosen", () => {
    const resolved = resolveReadingPrefs({ reader_line_width: "medium" });
    expect(resolved.measure).toBe("52rem");
  });

  it("exposes CSS variables", () => {
    const vars = readingCssVars(resolveReadingPrefs({ reader_font: "georgia" }));
    expect(vars["--reader-size" as never]).toBe("18px");
    expect(String(vars["--reader-font" as never])).toContain("Georgia");
  });
});
