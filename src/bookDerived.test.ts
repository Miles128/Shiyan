import { describe, expect, it } from "vitest";
import { bookMeta, formatAuthor } from "./bookDerived";

describe("formatAuthor", () => {
  it("flips last, first and drops life dates", () => {
    expect(formatAuthor("Melville, Herman, 1819-1891")).toBe("Herman Melville");
    expect(formatAuthor("Austen, Jane")).toBe("Jane Austen");
  });

  it("takes the first author of a list", () => {
    expect(formatAuthor("Eliot, George; Lewes, G. H.")).toBe("George Eliot");
  });

  it("leaves unrecognised shapes alone", () => {
    expect(formatAuthor("Anonymous")).toBe("Anonymous");
    expect(formatAuthor("")).toBe("");
  });
});

describe("bookMeta", () => {
  it("joins author and year", () => {
    expect(bookMeta("Melville, Herman, 1819-1891", "1851-11-14")).toBe(
      "Herman Melville · 1851",
    );
  });

  it("keeps whichever half exists", () => {
    expect(bookMeta("Austen, Jane", "")).toBe("Jane Austen");
    expect(bookMeta("", "1813")).toBe("1813");
    expect(bookMeta("", "")).toBe("");
  });
});
