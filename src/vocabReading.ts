import type { ArticleListItem, MemoryItem } from "./api/types";
import { BOOK_CATALOG, type CatalogBook } from "./books/catalog";

/** Recently saved words count more than the rest of the learning bank. */
export const RECENT_TERM_DAYS = 14;
export const RECENT_TERM_WEIGHT = 3;
export const BANK_TERM_WEIGHT = 1;
const MAX_TERMS = 40;

const STOPWORDS = new Set([
  "the", "and", "for", "that", "with", "from", "this", "have", "was", "were",
  "are", "you", "your", "not", "but", "his", "her", "she", "they", "them",
  "its", "been", "will", "would", "there", "their", "what", "when", "which",
  "about", "into", "than", "then", "some", "just", "had", "has", "him", "our",
  "out", "can", "all", "one", "who", "how", "did", "get", "got",
]);

export type StudyTerm = {
  term: string;
  recent: boolean;
};

export function isRecentStudy(createdAt: string, now: Date, days = RECENT_TERM_DAYS): boolean {
  const stamp = Date.parse(createdAt);
  if (!Number.isFinite(stamp)) return false;
  return now.getTime() - stamp <= days * 86_400_000;
}

/** Learning-bank words, recent ones first. Phrases and tiny function words drop out. */
export function collectStudyTerms(
  items: Pick<MemoryItem, "kind" | "term" | "status" | "created_at">[],
  now: Date,
): StudyTerm[] {
  const ranked: { term: StudyTerm; created: string }[] = [];
  const seen = new Set<string>();
  const sorted = [...items].sort((a, b) => b.created_at.localeCompare(a.created_at));
  for (const item of sorted) {
    if (item.kind !== "word" || item.status !== "learning") continue;
    const term = item.term.trim().toLowerCase();
    if (!/^[a-z][a-z'-]{2,39}$/.test(term)) continue;
    if (STOPWORDS.has(term) || seen.has(term)) continue;
    seen.add(term);
    ranked.push({
      term: { term, recent: isRecentStudy(item.created_at, now) },
      created: item.created_at,
    });
  }
  ranked.sort((a, b) => Number(b.term.recent) - Number(a.term.recent) || b.created.localeCompare(a.created));
  return ranked.slice(0, MAX_TERMS).map((row) => row.term);
}

export function termWeight(term: StudyTerm): number {
  return term.recent ? RECENT_TERM_WEIGHT : BANK_TERM_WEIGHT;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Whole-word hit. `tariff` does not match `tariffs`. */
export function textHasTerm(haystack: string, term: string): boolean {
  if (!term) return false;
  const re = new RegExp(`(?:^|[^a-z])${escapeRegExp(term)}(?:$|[^a-z])`, "i");
  return re.test(haystack);
}

export type RankedBook = {
  book: CatalogBook;
  score: number;
  matched: string[];
};

/** Books whose distinctive vocabulary overlaps the learner's words. */
export function rankBooksForTerms(
  books: CatalogBook[],
  terms: StudyTerm[],
  limit = 3,
): RankedBook[] {
  if (terms.length === 0 || limit <= 0) return [];
  const ranked: RankedBook[] = [];
  for (const book of books) {
    const hay = `${book.title} ${book.terms.join(" ")}`.toLowerCase();
    const matched: string[] = [];
    let score = 0;
    for (const term of terms) {
      if (!textHasTerm(hay, term.term)) continue;
      matched.push(term.term);
      score += termWeight(term);
    }
    if (score > 0) ranked.push({ book, score, matched });
  }
  ranked.sort(
    (a, b) => b.score - a.score || a.book.title.localeCompare(b.book.title),
  );
  return ranked.slice(0, limit);
}

export function rankCatalogForTerms(terms: StudyTerm[], limit = 3): RankedBook[] {
  return rankBooksForTerms(BOOK_CATALOG, terms, limit);
}

/**
 * Today's row: vocab-matching articles take the first slots (recent words
 * already weighted into `rank_score` by the caller), then the interest
 * ranking fills whatever is left.
 */
export function mergeTodayPicks(input: {
  interest: ArticleListItem[];
  vocabMatched: ArticleListItem[];
  max?: number;
  vocabSlots?: number;
}): ArticleListItem[] {
  const max = input.max ?? 10;
  const vocabSlots = input.vocabSlots ?? 4;
  const used = new Set<string>();
  const out: ArticleListItem[] = [];
  const vocab = [...input.vocabMatched].sort((a, b) => {
    const unread = Number(!a.last_opened_at) - Number(!b.last_opened_at);
    if (unread !== 0) return -unread;
    return b.rank_score - a.rank_score;
  });
  for (const article of vocab) {
    if (out.length >= Math.min(max, vocabSlots)) break;
    if (used.has(article.id)) continue;
    used.add(article.id);
    out.push(article);
  }
  for (const article of input.interest) {
    if (out.length >= max) break;
    if (used.has(article.id)) continue;
    used.add(article.id);
    out.push(article);
  }
  return out;
}
