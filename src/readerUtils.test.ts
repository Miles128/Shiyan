import { describe, expect, it } from "vitest";
import type { TranslateProgress } from "./api";
import {
  applyTranslateProgress,
  categoryLabel,
  clipContext,
  findContext,
  shouldRenderMarkdown,
  translateProgressLabel,
  urlLooksLikeMarkdown,
} from "./readerUtils";

describe("categoryLabel", () => {
  const cats = [
    { id: "tech", label: "科技" },
    { id: "world", label: "国际" },
  ];

  it("uses the API label when present", () => {
    expect(categoryLabel("tech", cats)).toBe("科技");
  });

  it("falls back to the raw id", () => {
    expect(categoryLabel("custom", cats)).toBe("custom");
  });
});

describe("findContext", () => {
  const paras = [
    "Hello world.",
    "The committee reached a serendipity of sorts.",
  ];

  it("returns the paragraph that contains the term", () => {
    expect(findContext(paras, "Serendipity")).toBe(paras[1]);
  });

  it("returns the term when no paragraph matches", () => {
    expect(findContext(paras, "quantum")).toBe("quantum");
  });

  it("returns a short window around the term for long paragraphs", () => {
    const long =
      "It was the best of times, it was the worst of times. ".repeat(20) +
      "Here lies serendipity, and much more follows after it in this very long paragraph.";
    const ctx = findContext([long], "serendipity");
    expect(ctx.length).toBeLessThan(160);
    expect(ctx).toContain("serendipity");
  });

  it("marks both cut ends with an ellipsis", () => {
    const long = `${"filler ".repeat(40)}serendipity${" trailer".repeat(40)}`;
    const ctx = findContext([long], "serendipity");
    expect(ctx.startsWith("…")).toBe(true);
    expect(ctx.endsWith("…")).toBe(true);
    expect(ctx).toContain("serendipity");
  });

  it("adds no ellipsis when the whole paragraph fits", () => {
    expect(findContext(["Hello world."], "world")).toBe("Hello world.");
  });
});

describe("clipContext", () => {
  it("keeps short sentences untouched", () => {
    expect(clipContext("Short and sweet.")).toBe("Short and sweet.");
  });

  it("clips long text to the last sentence end inside the window", () => {
    const clipped = clipContext(
      "First idea stands here. " + "Second sentence keeps going far beyond the limit. ",
      40,
    );
    expect(clipped.length).toBeLessThanOrEqual(41);
    expect(clipped.endsWith("…")).toBe(true);
    expect(clipped.startsWith("First")).toBe(true);
  });

  it("collapses whitespace", () => {
    expect(clipContext("a\n\n   b")).toBe("a b");
  });
});

function progress(partial: Partial<TranslateProgress> = {}): TranslateProgress {
  return {
    article_id: "a1",
    current: 1,
    total: 4,
    scope_key: "0",
    translated_text: "译文",
    done: false,
    ...partial,
  };
}

describe("applyTranslateProgress", () => {
  it("merges a finished paragraph into the cache", () => {
    const next = applyTranslateProgress({}, progress({ scope_key: "2" }));
    expect(next).toEqual({ "2": "译文" });
  });

  it("ignores the terminal done event", () => {
    const prev = { "0": "已有" };
    expect(
      applyTranslateProgress(
        prev,
        progress({ scope_key: "", translated_text: "", done: true }),
      ),
    ).toBe(prev);
  });
});

describe("translateProgressLabel", () => {
  it("shows current over total while in flight", () => {
    expect(translateProgressLabel(progress({ current: 3, total: 10 }))).toBe(
      "翻译中 3/10",
    );
  });

  it("hides when done or missing", () => {
    expect(translateProgressLabel(null)).toBeNull();
    expect(
      translateProgressLabel(progress({ done: true, scope_key: "" })),
    ).toBeNull();
  });
});

describe("markdown rendering", () => {
  it("detects markdown from the URL, falling back to the raw string", () => {
    expect(urlLooksLikeMarkdown("https://ex.com/notes.md")).toBe(true);
    expect(urlLooksLikeMarkdown("https://ex.com/a.markdown?x=1")).toBe(true);
    // Unparseable as a URL: the heuristic still sees the extension.
    expect(urlLooksLikeMarkdown("notes.md")).toBe(true);
    expect(urlLooksLikeMarkdown("https://ex.com/story.html")).toBe(false);
  });

  it("renders markdown when either the URL or the body signals it", () => {
    expect(shouldRenderMarkdown("https://ex.com/a.md", "plain")).toBe(true);
    // Body signals: one strong hit (code fence) or two weak ones (headings).
    expect(shouldRenderMarkdown("https://ex.com/a.html", "```\ncode\n```")).toBe(
      true,
    );
    expect(shouldRenderMarkdown("https://ex.com/a.html", "# One\n\n# Two")).toBe(
      true,
    );
    expect(shouldRenderMarkdown("https://ex.com/a.html", "plain")).toBe(false);
  });
});
