import type { ArticleListItem } from "./api/types";
import type { DifficultyLevel } from "./difficulty";

// Home 列表的全部派生逻辑：摘要/长度标签、
// 今日推荐、难度加权排序。保持纯函数，供 Home / ArticleRow 消费。

/** Titles stay English now — only the Chinese synopsis is generated. */
export function articleListBlurb(input: {
  summary_zh: string;
  excerpt?: string;
}): string {
  return input.summary_zh.trim();
}

/** Titles stay English now — only the Chinese synopsis is generated. */
export function articleNeedsCardZh(input: { summary_zh: string }): boolean {
  return !input.summary_zh.trim();
}

/** Reading-length band: 很短 <600 / 短 600 / 中 900 / 长 1200 / 很长 1800 / 极长 3000. */
export function articleLengthLabel(wordCount: number): string {
  if (!Number.isFinite(wordCount) || wordCount <= 0) return "";
  if (wordCount < 600) return "很短";
  if (wordCount < 900) return "短";
  if (wordCount < 1200) return "中";
  if (wordCount < 1800) return "长";
  if (wordCount < 3000) return "很长";
  return "极长";
}

/** Max picks per category, so one busy section can't fill the whole row. */
const PER_CATEGORY_CAP = 3;

/**
 * "今日推荐": top unread articles, spread across categories.
 * The backend ranks by interest; taking a raw prefix let the largest
 * category (world) dominate, so picks are capped per category first and
 * backfilled from the remaining ranked order.
 */
export function pickTopArticles(
  articles: ArticleListItem[],
  max: number = 5,
): ArticleListItem[] {
  const unread = articles.filter((a) => !a.last_opened_at);
  const picks: ArticleListItem[] = [];
  const used = new Set<string>();
  const counts = new Map<string, number>();

  for (const a of unread) {
    if ((counts.get(a.category) ?? 0) >= PER_CATEGORY_CAP) continue;
    picks.push(a);
    used.add(a.id);
    counts.set(a.category, (counts.get(a.category) ?? 0) + 1);
    if (picks.length >= max) return picks;
  }

  // Fewer categories than the cap allows: fill from the rest of the order.
  for (const a of unread) {
    if (used.has(a.id)) continue;
    picks.push(a);
    if (picks.length >= max) break;
  }
  return picks;
}

/**
 * Difficulty fit on top of the backend interest rank. The peak is "正常"
 * (normal): weight falls off with distance from it, so the adjacent
 * easy/hard stay positive and the extremes sink.
 */
export function difficultyAdjustment(level: DifficultyLevel | null): number {
  switch (level) {
    case "normal":
      return 0.5;
    case "hard":
      return 0.25;
    case "easy":
      return 0.1;
    case "harder":
      return -0.2;
    case "hardest":
      return -0.5;
    default:
      return 0;
  }
}

export function adjustedScore(
  article: ArticleListItem,
  level: DifficultyLevel | null,
): number {
  return article.rank_score + difficultyAdjustment(level);
}

/** Re-order a ranked list by difficulty-adjusted score (stable, non-mutating). */
export function applyDifficultyOrder(
  articles: ArticleListItem[],
  difficultyById: Map<string, DifficultyLevel | null>,
): ArticleListItem[] {
  return articles
    .map((a) => ({
      a,
      score: adjustedScore(a, difficultyById.get(a.id) ?? null),
    }))
    .sort((x, y) => y.score - x.score)
    .map((row) => row.a);
}

/** Server-query inputs that define the Home page-0 window. Difficulty level
 *  is deliberately absent: it refines client-side (matchesLevel), so it must
 *  not split the key space. Constants (limit, unreadOnly) are omitted too. */
export type HomeListKeyInput = {
  showPicks: boolean;
  focusSource: string | null;
  read: string;
  likedOnly: boolean;
  search: string | undefined;
};

/**
 * Query-cache key segments for the page-0 window. Single definition shared
 * by the Home memo and the state lazy-initializer: two spellings of the same
 * key would silently halve the hit rate (peek misses on every remount).
 */
export function homeListKey(input: HomeListKeyInput): readonly unknown[] {
  if (input.showPicks) {
    return ["articles", "ranked", { search: input.search ?? null }];
  }
  return [
    "articles",
    "library",
    {
      source: input.focusSource ?? null,
      read: input.read,
      likedOnly: input.likedOnly,
      search: input.search ?? null,
    },
  ];
}
