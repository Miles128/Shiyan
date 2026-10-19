import { describe, expect, it } from "vitest";
import { BOOK_CATALOG, bookLength, formatWordCount } from "./catalog";
import {
  inferBookInterests,
  matchesDifficulty,
  searchBooks,
  type BookSearchQuery,
} from "./search";

function query(patch: Partial<BookSearchQuery> = {}): BookSearchQuery {
  return {
    text: "",
    difficulty: "all",
    interests: [],
    length: "all",
    userCefr: "B1",
    ...patch,
  };
}

describe("bookLength", () => {
  it("bands books by word count", () => {
    expect(bookLength(10600)).toBe("short");
    expect(bookLength(39999)).toBe("short");
    expect(bookLength(40000)).toBe("medium");
    expect(bookLength(79999)).toBe("medium");
    expect(bookLength(80000)).toBe("long");
    expect(bookLength(139999)).toBe("long");
    expect(bookLength(140000)).toBe("epic");
  });

  it("formats counts in 万", () => {
    expect(formatWordCount(122000)).toBe("约 12.2 万词");
    expect(formatWordCount(10600)).toBe("约 1.1 万词");
    expect(formatWordCount(9800)).toBe("约 9800 词");
  });
});

describe("matchesDifficulty", () => {
  it("treats the learner's level and one step easier as 适合我", () => {
    expect(matchesDifficulty("B1", "B1", "mine")).toBe(true);
    expect(matchesDifficulty("A2", "B1", "mine")).toBe(true);
    expect(matchesDifficulty("B2", "B1", "mine")).toBe(false);
  });

  it("keeps easier strictly below and stretch one or two above", () => {
    expect(matchesDifficulty("A2", "B2", "easier")).toBe(true);
    expect(matchesDifficulty("B2", "B2", "easier")).toBe(false);
    expect(matchesDifficulty("B2", "B1", "stretch")).toBe(true);
    expect(matchesDifficulty("C1", "B1", "stretch")).toBe(true);
    expect(matchesDifficulty("C2", "B1", "stretch")).toBe(false);
    expect(matchesDifficulty("A2", "B1", "stretch")).toBe(false);
  });

  it("accepts an exact CEFR chip", () => {
    expect(matchesDifficulty("C2", "B1", "C2")).toBe(true);
    expect(matchesDifficulty("B1", "B1", "C2")).toBe(false);
  });
});

describe("searchBooks", () => {
  it("filters by interest, length, and a title query together", () => {
    const hits = searchBooks(
      BOOK_CATALOG,
      query({
        text: "whale",
        interests: ["adventure"],
        length: "epic",
        difficulty: "C2",
      }),
    );
    expect(hits.map((book) => book.id)).toEqual(["moby-dick"]);
  });

  it("sorts closer to the learner's level first", () => {
    const hits = searchBooks(
      BOOK_CATALOG,
      query({ interests: ["science"], userCefr: "B1" }),
    );
    expect(hits[0]?.cefr === "B1" || hits[0]?.cefr === "A2").toBe(true);
    const origin = hits.find((book) => book.id === "origin");
    expect(hits.indexOf(origin!)).toBe(hits.length - 1);
  });

  it("returns nothing when the bands do not overlap", () => {
    const hits = searchBooks(
      BOOK_CATALOG,
      query({ difficulty: "A2", length: "epic" }),
    );
    expect(hits).toEqual([]);
  });
});

describe("inferBookInterests", () => {
  it("ranks interests from reading tags and boosts the top category", () => {
    expect(
      inferBookInterests({
        tags: ["climate", "climate", "election"],
        topCategory: "world",
      }),
    ).toEqual(["politics", "science"]);
  });

  it("ignores tags that are not book interests", () => {
    expect(inferBookInterests({ tags: ["chips", "newsletter"] })).toEqual([]);
  });
});
