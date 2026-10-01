//! Interest-based article ranking for the home list.
//!
//! Ported from src-tauri/core/src/rank.rs (now deleted). Deterministic,
//! explainable scoring: freshness × affinity + explicit signals (liked / read /
//! dwell) + a bounded daily exploration jitter so unseen articles still surface.

import type { ArticleListItem } from "./api/types";

/**
 * Aggregated open counts by source / category, plus the local semantic profile
 * (title terms from engaged articles) and the sidebar priority maps.
 */
export type Affinity = {
  sourceOpens: Record<string, number>;
  categoryOpens: Record<string, number>;
  /** term → engagement weight (liked = 2, read-to-end = 1). */
  termWeights: Record<string, number>;
  /** feed name → user-assigned sidebar priority within its category. */
  sourcePriority: Record<string, number>;
  /** category → highest priority among that category's feeds (the ceiling). */
  categoryPriorityMax: Record<string, number>;
};

export function emptyAffinity(): Affinity {
  return {
    sourceOpens: {},
    categoryOpens: {},
    termWeights: {},
    sourcePriority: {},
    categoryPriorityMax: {},
  };
}

/** Log-scaled affinity in [0, 1]; 10 opens saturate. */
export function affinityScore(opens: number): number {
  return opens <= 0 ? 0 : Math.log(opens + 1) / Math.log(10);
}

/**
 * English stopwords + reporting verbs: frequent in headlines but carrying no
 * topic signal. Kept small and obvious — this is a heuristic, not NLP.
 */
const STOPWORDS = new Set([
  "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "with", "as",
  "at", "by", "from", "is", "are", "was", "were", "be", "been", "it", "its",
  "that", "this", "these", "those", "will", "would", "could", "should",
  "has", "have", "had", "not", "but", "they", "their", "them", "he", "she",
  "we", "you", "s", "t", "say", "says", "said", "new", "over", "after",
  "before", "more", "most", "than", "into", "out", "up", "all", "also",
  "just", "like", "get", "amid", "among", "between", "while", "without",
  "what", "when", "how", "why", "who",
]);

/**
 * Topic-bearing terms of a headline/body: lowercase alphanumeric tokens,
 * ≥2 chars, no pure digits, no stopwords. Single set per article — repeats
 * within one article don't count twice.
 */
export function contentTerms(text: string): Set<string> {
  const out = new Set<string>();
  for (const tok of text.toLowerCase().split(/[^\p{L}\p{N}]+/u)) {
    if (
      [...tok].length >= 2 &&
      !/^[0-9]+$/.test(tok) &&
      !STOPWORDS.has(tok)
    ) {
      out.add(tok);
    }
  }
  return out;
}

/**
 * Build the semantic profile: accumulate engagement weights per title term.
 * `engaged` is [title, weight] with liked = 2.0, read-to-end = 1.0.
 */
export function buildTermWeights(engaged: [string, number][]): Record<string, number> {
  const weights: Record<string, number> = {};
  for (const [title, w] of engaged) {
    for (const term of contentTerms(title)) {
      weights[term] = (weights[term] ?? 0) + w;
    }
  }
  return weights;
}

/**
 * Sidebar-priority bonus in [0, 1]. `max` is that category's highest priority.
 * 0 when the source was never ordered (priority absent/0) or the ceiling is 0.
 */
export function priorityScore(priority: number, max: number): number {
  if (priority <= 0 || max <= 0) return 0;
  return Math.min(1, Math.max(0, priority / max));
}

/** Exponential freshness decay: 1.0 today, ~0.24 at 30 days, floor 0.05. */
export function freshness(ageDays: number): number {
  return Math.max(Math.exp(-Math.max(ageDays, 0) / 21), 0.05);
}

/** Body-length fit: very short and very long articles are worse reads. */
export function wordFit(wordCount: number): number {
  if (wordCount === 0) return 0; // unknown, no opinion
  if (wordCount < 150) return -0.6; // likely stub/teaser that slipped through
  if (wordCount <= 3000) return 0.4; // comfortable single-sitting read
  if (wordCount <= 6000) return 0.1;
  return -0.3; // heavy commitment
}

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const U64_MASK = 0xffffffffffffffffn;

/** FNV-1a 64-bit over UTF-8 bytes — matches the Rust `translate::fnv1a_64`. */
function fnv1a64(text: string): bigint {
  let hash = FNV_OFFSET;
  for (const byte of new TextEncoder().encode(text)) {
    hash ^= BigInt(byte);
    hash = (hash * FNV_PRIME) & U64_MASK;
  }
  return hash;
}

/**
 * Deterministic pseudo-random jitter in [-0.3, 0.3], re-seeded daily, so unseen
 * articles get exploration slots without shuffling on every render.
 */
export function explorationJitter(articleId: string, dayKey: number): number {
  const hash = fnv1a64(`${articleId}:${dayKey}`);
  return (Number(hash % 1000n) / 999) * 0.6 - 0.3;
}

/**
 * Age in days, preferring `published_at` then `fetched_at`; unparseable/missing
 * → 0 (now). Seconds are truncated toward zero before the day division, exactly
 * like the Rust `num_seconds() as f64 / 86_400.0`.
 */
export function articleAgeDays(
  publishedAt: string | null,
  fetchedAt: string,
  nowMs: number,
): number {
  const parse = (s: string): number | null => {
    const t = Date.parse(s);
    return Number.isNaN(t) ? null : t;
  };
  const ms = (publishedAt != null ? parse(publishedAt) : null) ?? parse(fetchedAt);
  if (ms == null) return 0;
  return Math.trunc((nowMs - ms) / 1000) / 86_400;
}

/**
 * Dwell-time signals (ms of visible, focused reading). Long dwell = real
 * engagement even if unfinished; a sub-30s bounce on an opened article is a
 * weak negative.
 */
export function dwellAdjustment(dwellMs: number, readCompleted: boolean): number {
  if (dwellMs >= 300_000) return readCompleted ? 0.8 : 0.6;
  if (dwellMs >= 30_000) return 0.2;
  if (dwellMs > 0 && !readCompleted) return -0.15; // bounced quickly
  return 0;
}

export function articleRankScore(
  article: ArticleListItem,
  affinity: Affinity,
  nowMs: number,
  dayKey: number,
): number {
  const age = articleAgeDays(article.published_at, article.fetched_at, nowMs);
  const fresh = freshness(age);
  let score = fresh;
  score += 0.6 * affinityScore(affinity.sourceOpens[article.source] ?? 0) * fresh;
  score += 0.4 * affinityScore(affinity.categoryOpens[article.category] ?? 0) * fresh;
  // Sidebar priority is an explicit within-category ordering: a strong,
  // freshness-modulated boost normalised by that category's ceiling.
  score +=
    1.5 *
    priorityScore(
      affinity.sourcePriority[article.source] ?? 0,
      affinity.categoryPriorityMax[article.category] ?? 0,
    ) *
    fresh;
  if (article.liked) score += 2.5;
  score += wordFit(article.word_count);
  if (article.open_count === 0) {
    score += explorationJitter(article.id, dayKey);
  } else {
    score -= article.read_completed ? 0.6 : 0.25;
    score += dwellAdjustment(article.dwell_ms, article.read_completed);
  }
  return score;
}

/**
 * Score + sort a window of list items (descending score, id as a stable
 * tie-break so cursor pagination has a total order), stamping `rank_score`.
 *
 * The semantic bonus is computed here (not in `articleRankScore`) because IDF
 * needs the whole window: rare-among-candidates profile terms count more than
 * terms every article carries.
 */
export function rankArticles(
  items: ArticleListItem[],
  affinity: Affinity,
  nowMs: number,
  dayKey: number,
): ArticleListItem[] {
  // Document frequency over the window (title + excerpt terms per article).
  const docCounts = new Map<string, number>();
  const itemTerms: Set<string>[] = items.map((item) => {
    const terms = contentTerms(item.title);
    for (const t of contentTerms(item.excerpt)) terms.add(t);
    for (const t of terms) docCounts.set(t, (docCounts.get(t) ?? 0) + 1);
    return terms;
  });
  const docs = items.length;
  const idf = (term: string): number => {
    const df = docCounts.get(term) ?? 0;
    return Math.max(Math.log((1 + docs) / (1 + df)), 0);
  };
  let mass = 0;
  for (const [term, w] of Object.entries(affinity.termWeights)) mass += w * idf(term);

  const ranked = items.map((item, i) => {
    let score = articleRankScore(item, affinity, nowMs, dayKey);
    // Semantic profile: engaged-title terms, IDF-weighted so generic terms
    // don't dominate. Only active once a profile exists.
    if (mass > 0) {
      let hit = 0;
      for (const term of itemTerms[i]) {
        hit += (affinity.termWeights[term] ?? 0) * idf(term);
      }
      score += 0.8 * Math.min(hit / mass, 1);
    }
    return { ...item, rank_score: score };
  });

  ranked.sort((a, b) => {
    const d = b.rank_score - a.rank_score;
    if (d !== 0) return d > 0 ? 1 : -1;
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
  });
  return ranked;
}

/**
 * Slice one page out of a ranked list. A cursor (score + id of the last shown
 * item) returns everything strictly after it, so inserts above the cursor no
 * longer shift later pages; without a cursor the legacy offset window applies.
 *
 * Score comparison is tolerance-based: `rank_score` crosses a JSON round-trip
 * between pages, and a 1ulp drift on an exact `==` used to skip or repeat the
 * boundary item. Within tolerance the `id` tiebreak decides.
 */
export function pageRanked(
  items: ArticleListItem[],
  cursor: { score: number; id: string } | null,
  offset: number,
  limit: number,
): ArticleListItem[] {
  // Two scores from the same computation that differ only by a JSON round-trip
  // land within a few ulps; 1e-9 relative covers that without blurring
  // genuinely distinct ranks.
  const sameScore = (a: number, b: number): boolean => {
    if (a === b) return true;
    const scale = Math.max(Math.abs(a), Math.abs(b), 1);
    return Math.abs(a - b) <= 1e-9 * scale;
  };
  let start: number;
  if (cursor) {
    const idx = items.findIndex(
      (a) =>
        (a.rank_score < cursor.score && !sameScore(a.rank_score, cursor.score)) ||
        (sameScore(a.rank_score, cursor.score) && a.id > cursor.id),
    );
    start = idx === -1 ? items.length : idx;
  } else {
    start = Math.min(offset, items.length);
  }
  return items.slice(start, start + limit);
}

/**
 * Days since 0001-01-01 (proleptic Gregorian, 0001-01-01 = day 1), matching
 * chrono's `num_days_from_ce()` for a UTC instant. The Unix epoch is day
 * 719163. Used only to seed the daily exploration jitter.
 */
export function dayKeyFromMs(nowMs: number): number {
  return Math.floor(nowMs / 86_400_000) + 719163;
}
