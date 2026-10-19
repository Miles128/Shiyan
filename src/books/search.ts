import {
  bookLength,
  interestLabel,
  type BookCefr,
  type BookInterestId,
  type BookLengthId,
  type CatalogBook,
} from "./catalog";

const CEFR_ORDER = ["A1", "A2", "B1", "B2", "C1", "C2"] as const;

export type DifficultyChoice =
  | "mine"
  | "easier"
  | "stretch"
  | "all"
  | BookCefr;

export type BookSearchQuery = {
  text: string;
  difficulty: DifficultyChoice;
  /** Empty means every interest. */
  interests: BookInterestId[];
  length: BookLengthId | "all";
  userCefr: string;
};

export function cefrIndex(level: string): number {
  const idx = CEFR_ORDER.indexOf(level as (typeof CEFR_ORDER)[number]);
  return idx < 0 ? CEFR_ORDER.indexOf("B1") : idx;
}

/** How the book's CEFR sits against the learner: negative = easier. */
export function cefrDistance(bookCefr: string, userCefr: string): number {
  return cefrIndex(bookCefr) - cefrIndex(userCefr);
}

export function matchesDifficulty(
  bookCefr: string,
  userCefr: string,
  choice: DifficultyChoice,
): boolean {
  if (choice === "all") return true;
  if (choice === "mine" || choice === "easier" || choice === "stretch") {
    const distance = cefrDistance(bookCefr, userCefr);
    if (choice === "mine") return distance === 0 || distance === -1;
    if (choice === "easier") return distance <= -1;
    return distance >= 1 && distance <= 2;
  }
  return bookCefr === choice;
}

export function searchBooks(
  books: CatalogBook[],
  query: BookSearchQuery,
): CatalogBook[] {
  const needle = query.text.trim().toLowerCase();
  const interestSet = new Set(query.interests);
  const filtered = books.filter((book) => {
    if (!matchesDifficulty(book.cefr, query.userCefr, query.difficulty)) return false;
    if (query.length !== "all" && bookLength(book.words) !== query.length) return false;
    if (interestSet.size > 0 && !book.interests.some((id) => interestSet.has(id))) {
      return false;
    }
    if (!needle) return true;
    const hay = [
      book.title,
      book.author,
      book.summaryZh,
      book.terms.join(" "),
      book.interests.map(interestLabel).join(" "),
    ]
      .join(" ")
      .toLowerCase();
    return hay.includes(needle);
  });
  const userIdx = cefrIndex(query.userCefr);
  return filtered.sort((a, b) => {
    const da = Math.abs(cefrIndex(a.cefr) - userIdx);
    const db = Math.abs(cefrIndex(b.cefr) - userIdx);
    if (da !== db) return da - db;
    if (a.words !== b.words) return a.words - b.words;
    return a.title.localeCompare(b.title);
  });
}

const TAG_INTEREST: Record<string, BookInterestId> = {
  politics: "politics",
  political: "politics",
  election: "politics",
  elections: "politics",
  government: "politics",
  war: "politics",
  diplomacy: "politics",
  science: "science",
  climate: "science",
  space: "science",
  health: "science",
  technology: "science",
  tech: "science",
  ai: "science",
  research: "science",
  history: "history",
  culture: "society",
  society: "society",
  business: "society",
  economy: "society",
  finance: "society",
  markets: "society",
  environment: "nature",
  nature: "nature",
  wildlife: "nature",
  sports: "adventure",
  crime: "mystery",
  education: "society",
  philosophy: "philosophy",
};

const CATEGORY_INTEREST: Record<string, BookInterestId> = {
  tech: "science",
  finance: "society",
  world: "politics",
};

/**
 * Personal interests from what the learner already reads: topic tags, plus
 * the category they open most. Most frequent first, capped.
 */
export function inferBookInterests(input: {
  tags: string[];
  topCategory?: string | null;
}): BookInterestId[] {
  const counts = new Map<BookInterestId, number>();
  const bump = (id: BookInterestId, by: number) => {
    counts.set(id, (counts.get(id) ?? 0) + by);
  };
  for (const raw of input.tags) {
    const id = TAG_INTEREST[raw.trim().toLowerCase()];
    if (id) bump(id, 1);
  }
  const category = input.topCategory?.trim().toLowerCase() ?? "";
  const fromCategory = CATEGORY_INTEREST[category];
  if (fromCategory) bump(fromCategory, 2);
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, 3)
    .map(([id]) => id);
}
