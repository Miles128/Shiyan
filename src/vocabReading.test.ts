import { describe, expect, it } from "vitest";
import type { ArticleListItem } from "./api/types";
import { BOOK_CATALOG } from "./books/catalog";
import {
  collectStudyTerms,
  isRecentStudy,
  mergeTodayPicks,
  rankBooksForTerms,
  textHasTerm,
} from "./vocabReading";

const NOW = new Date("2026-10-19T12:00:00Z");

function memory(
  term: string,
  created_at: string,
  patch: { kind?: string; status?: string } = {},
) {
  return {
    kind: patch.kind ?? "word",
    term,
    status: patch.status ?? "learning",
    created_at,
  };
}

function article(id: string, rank: number, opened = false): ArticleListItem {
  return {
    id,
    url: `https://example.com/${id}`,
    title: id,
    source: "S",
    category: "tech",
    published_at: null,
    excerpt: "",
    fetched_at: "2026-10-01T00:00:00Z",
    origin: "rss",
    summary_zh: "",
    last_opened_at: opened ? "2026-10-02T00:00:00Z" : null,
    open_count: opened ? 1 : 0,
    word_count: 800,
    rank_score: rank,
    dwell_ms: 0,
    read_completed: false,
    liked: false,
    tags: [],
  };
}

describe("collectStudyTerms", () => {
  it("keeps recent learning words ahead of the older bank", () => {
    const terms = collectStudyTerms(
      [
        memory("estate", "2026-09-01T00:00:00Z"),
        memory("prejudice", "2026-10-18T00:00:00Z"),
        memory("the", "2026-10-18T00:00:00Z"),
        memory("on the table", "2026-10-18T00:00:00Z", { kind: "phrase" }),
        memory("mastered", "2026-10-18T00:00:00Z", { status: "mastered" }),
      ],
      NOW,
    );
    expect(terms.map((term) => term.term)).toEqual(["prejudice", "estate"]);
    expect(terms[0]?.recent).toBe(true);
    expect(terms[1]?.recent).toBe(false);
  });

  it("treats the last 14 days as recent", () => {
    expect(isRecentStudy("2026-10-05T12:00:00Z", NOW)).toBe(true);
    expect(isRecentStudy("2026-10-04T12:00:00Z", NOW)).toBe(false);
  });
});

describe("textHasTerm", () => {
  it("matches a whole word and skips inflections", () => {
    expect(textHasTerm("The tariff rose overnight.", "tariff")).toBe(true);
    expect(textHasTerm("tariffs rose", "tariff")).toBe(false);
  });
});

describe("rankBooksForTerms", () => {
  it("prefers a book that hits a recently learned word", () => {
    const ranked = rankBooksForTerms(
      BOOK_CATALOG,
      [
        { term: "prejudice", recent: true },
        { term: "whale", recent: false },
      ],
      2,
    );
    expect(ranked[0]?.book.id).toBe("pride");
    expect(ranked[0]?.matched).toContain("prejudice");
    expect(ranked.map((row) => row.book.id)).toContain("moby-dick");
  });

  it("returns nothing when the bank does not overlap the shelf", () => {
    expect(
      rankBooksForTerms(BOOK_CATALOG, [{ term: "qubit", recent: true }]),
    ).toEqual([]);
  });
});

describe("mergeTodayPicks", () => {
  it("puts unread vocab matches in front and fills with interest picks", () => {
    const picks = mergeTodayPicks({
      vocabMatched: [article("match-read", 9, true), article("match-new", 4)],
      interest: [article("interest", 8), article("match-new", 1)],
      max: 3,
      vocabSlots: 2,
    });
    expect(picks.map((item) => item.id)).toEqual([
      "match-new",
      "match-read",
      "interest",
    ]);
  });
});
