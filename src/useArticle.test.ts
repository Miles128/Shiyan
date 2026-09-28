import { beforeEach, describe, expect, it } from "vitest";
import {
  articleViewState,
  getLastArticleId,
  lastArticlePath,
  loadScroll,
  rememberLastArticle,
  resetLastArticleMemory,
  saveScroll,
  translationsMap,
} from "./useArticle";

describe("articleViewState", () => {
  it("is loading while the first fetch is in flight", () => {
    expect(
      articleViewState({ loading: true, article: null, error: null }),
    ).toBe("loading");
  });

  it("is missing when fetch finished with no article", () => {
    expect(
      articleViewState({ loading: false, article: null, error: null }),
    ).toBe("missing");
  });

  it("is error when fetch failed before an article arrived", () => {
    expect(
      articleViewState({
        loading: false,
        article: null,
        error: "数据库未就绪",
      }),
    ).toBe("error");
  });

  it("is ready once an article is present, even if a later action failed", () => {
    expect(
      articleViewState({
        loading: false,
        article: { id: "a1" },
        error: "翻译失败",
      }),
    ).toBe("ready");
  });
});

describe("translationsMap", () => {
  it("indexes paragraph rows by scope_key", () => {
    expect(
      translationsMap([
        { scope_key: "0", translated_text: "你好" },
        { scope_key: "2", translated_text: "世界" },
      ]),
    ).toEqual({ "0": "你好", "2": "世界" });
  });
});

beforeEach(() => {
  resetLastArticleMemory();
});

describe("last article", () => {
  it("remembers the last opened article", () => {
    expect(getLastArticleId()).toBeNull();
    expect(lastArticlePath()).toBeNull();
    rememberLastArticle("a1");
    expect(getLastArticleId()).toBe("a1");
    expect(lastArticlePath()).toBe("/article/a1");
  });

  it("ignores empty ids", () => {
    rememberLastArticle("");
    expect(getLastArticleId()).toBeNull();
  });
});

describe("scroll memory", () => {
  it("round-trips a scroll offset per article", () => {
    expect(loadScroll("a1")).toBeNull();
    saveScroll("a1", 1234.6);
    expect(loadScroll("a1")).toBe(1235);
    expect(loadScroll("a2")).toBeNull();
  });

  it("clamps negatives", () => {
    saveScroll("a1", -50);
    expect(loadScroll("a1")).toBe(0);
  });
});
