import { describe, expect, it } from "vitest";
import type { ArticleListItem } from "./api/types";
import {
  affinityScore,
  articleRankScore,
  buildTermWeights,
  contentTerms,
  dwellAdjustment,
  emptyAffinity,
  explorationJitter,
  freshness,
  pageRanked,
  priorityScore,
  rankArticles,
  wordFit,
  type Affinity,
} from "./rank";

// Ported 1:1 from src-tauri/core/src/rank.rs `mod tests`. These assertions are
// the behavior spec for the TS port; the Rust module is being deleted.

const DAY_MS = 86_400_000;

function item(id: string): ArticleListItem {
  return {
    id,
    url: `https://example.com/${id}`,
    title: id,
    source: "Test",
    category: "tech",
    published_at: null,
    excerpt: "",
    fetched_at: "2020-01-01T00:00:00Z",
    origin: "rss",
    summary_zh: "",
    last_opened_at: null,
    open_count: 0,
    word_count: 800,
    rank_score: 0,
    dwell_ms: 0,
    read_completed: false,
    liked: false,
  };
}

function scored(id: string, score: number): ArticleListItem {
  return { ...item(id), rank_score: score };
}

function titled(id: string, title: string, nowMs: number): ArticleListItem {
  return {
    ...item(id),
    title,
    fetched_at: new Date(nowMs).toISOString(),
    open_count: 1,
  };
}

describe("freshness", () => {
  it("decays with age", () => {
    expect(Math.abs(freshness(0) - 1)).toBeLessThan(1e-9);
    expect(freshness(7)).toBeLessThan(freshness(1));
    expect(freshness(400)).toBeGreaterThanOrEqual(0.05); // never decays to zero
  });
});

describe("affinityScore", () => {
  it("saturates and handles zero", () => {
    expect(affinityScore(0)).toBe(0);
    expect(affinityScore(3)).toBeGreaterThan(affinityScore(1));
    expect(affinityScore(9)).toBeGreaterThanOrEqual(1);
    expect(affinityScore(100)).toBeGreaterThanOrEqual(1);
  });
});

describe("articleRankScore", () => {
  it("liked and completed dominate", () => {
    const now = Date.now();
    const affinity = emptyAffinity();
    const base = item("a");
    const liked = { ...item("b"), liked: true };
    const completed = { ...item("c"), open_count: 1, read_completed: true };
    expect(articleRankScore(liked, affinity, now, 0)).toBeGreaterThan(
      articleRankScore(base, affinity, now, 0),
    );
    expect(articleRankScore(completed, affinity, now, 0)).toBeLessThan(
      articleRankScore(base, affinity, now, 0),
    );
  });

  it("higher priority source ranks above same-freshness rival", () => {
    const now = Date.now();
    const affinity: Affinity = {
      ...emptyAffinity(),
      sourcePriority: { Favorite: 30, Rival: 20 },
      categoryPriorityMax: { tech: 30 },
    };
    const fav = {
      ...item("fav"),
      source: "Favorite",
      fetched_at: new Date(now).toISOString(),
      open_count: 1,
    };
    const riv = {
      ...item("riv"),
      source: "Rival",
      fetched_at: new Date(now).toISOString(),
      open_count: 1,
    };
    expect(articleRankScore(fav, affinity, now, 1)).toBeGreaterThan(
      articleRankScore(riv, affinity, now, 1),
    );
  });

  it("priority is normalised within category", () => {
    const now = Date.now();
    const affinity: Affinity = {
      ...emptyAffinity(),
      sourcePriority: { TechTop: 10, WorldTop: 2 },
      categoryPriorityMax: { tech: 10, world: 2 },
    };
    const tech = {
      ...item("t"),
      source: "TechTop",
      category: "tech",
      open_count: 1,
      fetched_at: new Date(now).toISOString(),
    };
    const world = {
      ...item("w"),
      source: "WorldTop",
      category: "world",
      open_count: 1,
      fetched_at: new Date(now).toISOString(),
    };
    expect(
      Math.abs(
        articleRankScore(tech, affinity, now, 1) -
          articleRankScore(world, affinity, now, 1),
      ),
    ).toBeLessThan(1e-9);
  });

  it("affinity boosts familiar sources", () => {
    const now = Date.now();
    const affinity: Affinity = { ...emptyAffinity(), sourceOpens: { Test: 50 } };
    const a = { ...item("fam"), fetched_at: new Date(now).toISOString() };
    const b = {
      ...item("other"),
      fetched_at: new Date(now).toISOString(),
      source: "Other",
    };
    const scoreFam = articleRankScore(a, affinity, now, 1);
    const scoreOther = articleRankScore(b, affinity, now, 1);
    expect(scoreFam).toBeGreaterThan(scoreOther);
    expect(scoreFam - scoreOther).toBeGreaterThan(0.5);
  });
});

describe("wordFit", () => {
  it("penalizes stubs and marathons", () => {
    expect(wordFit(80)).toBeLessThan(0);
    expect(wordFit(1200)).toBeGreaterThan(wordFit(9000));
    expect(wordFit(800)).toBeGreaterThan(wordFit(100));
    expect(wordFit(0)).toBe(0);
  });
});

describe("explorationJitter", () => {
  it("is bounded and day-seeded", () => {
    const j1 = explorationJitter("abc", 20_000);
    const j1Again = explorationJitter("abc", 20_000);
    expect(j1).toBe(j1Again); // same day is stable
    expect(j1).toBeGreaterThanOrEqual(-0.3);
    expect(j1).toBeLessThanOrEqual(0.3);
    const differs = Array.from({ length: 20 }, (_, d) => d).some(
      (d) => explorationJitter("abc", d) !== explorationJitter("abc", d + 1),
    );
    expect(differs).toBe(true);
  });
});

describe("dwellAdjustment", () => {
  it("signals engagement", () => {
    expect(dwellAdjustment(0, false)).toBe(0);
    expect(dwellAdjustment(10_000, false)).toBe(-0.15); // quick bounce
    expect(dwellAdjustment(60_000, false)).toBe(0.2); // read a minute
    expect(dwellAdjustment(400_000, false)).toBe(0.6); // long dwell
    expect(dwellAdjustment(400_000, true)).toBe(0.8); // long dwell, finished
  });
});

describe("priorityScore", () => {
  it("is normalised and bounded", () => {
    expect(priorityScore(0, 10)).toBe(0);
    expect(priorityScore(10, 0)).toBe(0);
    expect(Math.abs(priorityScore(10, 10) - 1)).toBeLessThan(Number.EPSILON);
    expect(Math.abs(priorityScore(5, 10) - 0.5)).toBeLessThan(Number.EPSILON);
    expect(priorityScore(20, 10)).toBe(1); // clamped
  });
});

describe("rankArticles", () => {
  it("is descending and stamps scores", () => {
    const now = Date.now();
    const affinity = emptyAffinity();
    const fresh = { ...item("fresh"), fetched_at: new Date(now).toISOString() };
    const old = {
      ...item("old"),
      fetched_at: new Date(now - 120 * DAY_MS).toISOString(),
      open_count: 2,
      read_completed: true,
    };
    const ranked = rankArticles([old, fresh], affinity, now, 1);
    expect(ranked[0].id).toBe("fresh");
    expect(ranked[0].rank_score).toBeGreaterThan(ranked[1].rank_score);
    expect(ranked[0].rank_score).not.toBe(0);
  });

  it("ties break by id for cursor stability", () => {
    const now = Date.now();
    const affinity = emptyAffinity();
    const items = [item("b"), item("a"), item("c")].map((i) => ({
      ...i,
      open_count: 1,
    }));
    const ranked = rankArticles(items, affinity, now, 1);
    expect(ranked.map((i) => i.id)).toEqual(["a", "b", "c"]);
  });

  it("semantic bonus boosts profile-matching articles", () => {
    const now = Date.now();
    const affinity: Affinity = {
      ...emptyAffinity(),
      termWeights: buildTermWeights([
        ["Central bank holds interest rates", 2.0],
      ]),
    };
    const matching = titled("m", "Interest rates and the central bank outlook", now);
    const plain = titled("p", "Night trains return across quiet borders", now);
    const ranked = rankArticles([plain, matching], affinity, now, 1);
    expect(ranked[0].id).toBe("m");
    expect(ranked[0].rank_score - ranked[1].rank_score).toBeGreaterThan(0.2);
  });

  it("no profile means no semantic bonus", () => {
    const now = Date.now();
    const affinity = emptyAffinity();
    const a = titled("a", "Central bank holds interest rates", now);
    const b = titled("b", "Central bank holds interest rates", now);
    const ranked = rankArticles([a, b], affinity, now, 1);
    expect(Math.abs(ranked[0].rank_score - ranked[1].rank_score)).toBeLessThan(1e-9);
  });
});

describe("pageRanked", () => {
  it("cursor page skips inserts above", () => {
    const ranked = [
      scored("fresh", 3.0),
      scored("b", 2.0),
      scored("c", 2.0),
      scored("d", 1.0),
    ];
    const page2 = pageRanked(ranked.map((r) => ({ ...r })), { score: 2.0, id: "b" }, 0, 10);
    expect(page2.map((i) => i.id)).toEqual(["c", "d"]);

    const withInsert = [scored("new", 9.0), ...ranked.map((r) => ({ ...r }))];
    const page2Again = pageRanked(withInsert, { score: 2.0, id: "b" }, 0, 10);
    expect(page2Again.map((i) => i.id)).toEqual(["c", "d"]);

    const offsetPage = pageRanked(ranked.map((r) => ({ ...r })), null, 1, 2);
    expect(offsetPage.map((i) => i.id)).toEqual(["b", "c"]);
  });

  it("cursor tolerates json round-trip drift", () => {
    const ranked = [
      scored("fresh", 3.0),
      scored("b", 2.0),
      scored("c", 2.0),
      scored("d", 1.0),
    ];
    const drifted = 2.0 + Number.EPSILON;
    const page = pageRanked(ranked, { score: drifted, id: "b" }, 0, 10);
    expect(page.map((i) => i.id)).toEqual(["c", "d"]);
  });
});

describe("contentTerms", () => {
  it("drops stopwords, digits and singles", () => {
    const terms = contentTerms("The Fed holds rates steady in 2026, officials say");
    expect(terms.has("fed")).toBe(true);
    expect(terms.has("rates")).toBe(true);
    expect(terms.has("officials")).toBe(true);
    expect(terms.has("the")).toBe(false); // stopword
    expect(terms.has("in")).toBe(false); // stopword
    expect(terms.has("2026")).toBe(false); // pure digits
    expect(terms.has("say")).toBe(false); // reporting verb
  });
});

describe("buildTermWeights", () => {
  it("accumulates engagement weights", () => {
    const weights = buildTermWeights([
      ["Central bank holds rates", 2.0],
      ["Bank earnings beat estimates", 1.0],
    ]);
    expect(weights["bank"]).toBe(3.0);
    expect(weights["rates"]).toBe(2.0);
    expect("the" in weights).toBe(false);
  });
});
