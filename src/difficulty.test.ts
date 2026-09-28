import { beforeAll, describe, expect, it } from "vitest";
import { ensureLexiconLoaded } from "./wordLevels";
import type { DifficultyPrefs } from "./difficulty";
import {
  articleDifficulty,
  calibrateEdges,
  DEFAULT_EDGES,
  difficultyClassName,
  difficultyFromScore,
  difficultyLabel,
  DifficultyScoreCache,
  sentenceFactor,
  tokenizeWords,
  wordDifficultyWeight,
} from "./difficulty";

const PREFS: DifficultyPrefs = { cefrLevel: "B1", freqBand: 3000 };

const EASY_WORDS = [
  "the", "and", "for", "you", "that", "with", "have", "this", "from",
  "they", "will", "would", "there", "their", "what", "about", "which",
  "when", "make", "time", "know", "take", "think", "good", "help", "like",
];

beforeAll(async () => {
  await ensureLexiconLoaded();
});

function repeatTo(words: string[], n: number): string {
  const out: string[] = [];
  while (out.length < n) out.push(...words);
  return out.slice(0, n).join(" ") + ".";
}

describe("tokenizeWords", () => {
  it("lowercases and keeps letters/apostrophes/hyphens", () => {
    expect(tokenizeWords("Hello, WORLD! It's fine.")).toEqual([
      "hello",
      "world",
      "it's",
      "fine",
    ]);
  });

  it("drops single-character tokens", () => {
    expect(tokenizeWords("a b c d")).toEqual([]);
  });
});

describe("wordDifficultyWeight", () => {
  it("is zero inside the comfort zone", () => {
    expect(wordDifficultyWeight("the", PREFS, false)).toBe(0);
    expect(wordDifficultyWeight("time", PREFS, false)).toBe(0);
  });

  it("charges more for words further outside the band", () => {
    const learning = wordDifficultyWeight("time", PREFS, true);
    expect(learning).toBe(1.5);
    // OOV words are weak evidence, not difficulty.
    expect(wordDifficultyWeight("zzzqqq", PREFS, false)).toBe(0.5);
  });

  it("charges CEFR-above words the same rule as the underline", () => {
    // A word inside the freq band but above the CEFR level is underlined
    // hard, so it must also weigh into the difficulty index.
    const above = wordDifficultyWeight("ubiquitous", PREFS, false);
    const atLevel = wordDifficultyWeight(
      "ubiquitous",
      { cefrLevel: "C2", freqBand: 3000 },
      false,
    );
    expect(above).toBeGreaterThan(0);
    expect(atLevel).toBeLessThanOrEqual(above);
  });
});

describe("sentenceFactor", () => {
  it("stays close to 1 and is clamped", () => {
    expect(sentenceFactor(18)).toBeCloseTo(1);
    expect(sentenceFactor(60)).toBeLessThanOrEqual(1.25);
    expect(sentenceFactor(2)).toBeGreaterThanOrEqual(0.9);
  });
});

describe("difficultyFromScore", () => {
  it("maps the score ramp to the five levels", () => {
    expect(difficultyFromScore(0.005)).toBe("easy");
    expect(difficultyFromScore(0.03)).toBe("normal");
    expect(difficultyFromScore(0.07)).toBe("hard");
    expect(difficultyFromScore(0.12)).toBe("harder");
    expect(difficultyFromScore(0.5)).toBe("hardest");
  });
});

describe("articleDifficulty", () => {
  it("returns null for tiny samples", () => {
    expect(articleDifficulty("the cat sat on the mat", [], PREFS)).toBeNull();
  });

  it("rates common prose as easy", () => {
    const result = articleDifficulty(repeatTo(EASY_WORDS, 120), [], PREFS);
    expect(result).not.toBeNull();
    expect(difficultyFromScore(result!.score)).toBe("easy");
  });

  it("rates dense hard vocabulary higher", () => {
    const hard = "ubiquitous bureaucratic idiosyncratic juxtaposition "
      .trim()
      .split(" ");
    const result = articleDifficulty(repeatTo(hard, 120), [], PREFS);
    expect(result).not.toBeNull();
    expect(["hard", "harder", "hardest"]).toContain(
      difficultyFromScore(result!.score),
    );
  });

  it("never gets easier when the freq band narrows", () => {
    const body = repeatTo(EASY_WORDS, 150);
    const wide = articleDifficulty(body, [], { cefrLevel: "B1", freqBand: 20000 });
    const narrow = articleDifficulty(body, [], { cefrLevel: "B1", freqBand: 1000 });
    expect(wide!.score).toBeLessThanOrEqual(narrow!.score);
  });

  it("never gets easier when the CEFR bar lowers", () => {
    const body = repeatTo(EASY_WORDS, 150);
    const high = articleDifficulty(body, [], { cefrLevel: "C2", freqBand: 3000 });
    const low = articleDifficulty(body, [], { cefrLevel: "A1", freqBand: 3000 });
    expect(high!.score).toBeLessThanOrEqual(low!.score);
  });

  it("calibrates edges to the library distribution and stays stable", () => {
    expect(calibrateEdges([])).toEqual(DEFAULT_EDGES);
    const small = Array.from({ length: 10 }, (_, i) => i / 100);
    expect(calibrateEdges(small)).toEqual(DEFAULT_EDGES);
    const wide = Array.from({ length: 200 }, (_, i) => (i / 200) * 0.4);
    const edges = calibrateEdges(wide);
    // Edges must be increasing and pulled away from the defaults.
    expect(edges[0]).toBeLessThan(edges[1]);
    expect(edges[1]).toBeLessThan(edges[2]);
    expect(edges[2]).toBeLessThan(edges[3]);
    expect(edges[3]).not.toBe(DEFAULT_EDGES[3]);
  });

  it("exposes label and css class", () => {
    expect(difficultyLabel("hardest")).toBe("极难");
    expect(difficultyClassName("hard")).toContain("d-hard");
  });
});

describe("DifficultyScoreCache", () => {
  it("hits on identical excerpt, misses when the body changed", () => {
    const cache = new DifficultyScoreCache();
    expect(cache.get("a", "body v1")).toBeUndefined();
    cache.set("a", "body v1", 0.5);
    expect(cache.get("a", "body v1")).toEqual({
      excerpt: "body v1",
      score: 0.5,
    });
    // Same id, refreshed body → miss (a re-score is due).
    expect(cache.get("a", "body v2")).toBeUndefined();
  });

  it("evicts oldest-inserted beyond the cap", () => {
    const cache = new DifficultyScoreCache(2);
    cache.set("a", "a", 1);
    cache.set("b", "b", 2);
    cache.set("c", "c", 3);
    expect(cache.size).toBe(2);
    expect(cache.get("a", "a")).toBeUndefined();
    expect(cache.get("c", "c")?.score).toBe(3);
  });

  it("clear drops everything", () => {
    const cache = new DifficultyScoreCache();
    cache.set("a", "a", 1);
    cache.clear();
    expect(cache.size).toBe(0);
    expect(cache.get("a", "a")).toBeUndefined();
  });
});
