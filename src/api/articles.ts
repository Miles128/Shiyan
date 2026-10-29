import { typedInvoke } from "./invoke";
import type { ReadStateFilter } from "./commands";
import type { ArticleListItem } from "./types";
import {
  buildTermWeights,
  dayKeyFromMs,
  pageRanked,
  rankArticles,
  type Affinity,
} from "../rank";

/**
 * Filter shared by both list commands — one place to add a field, instead of
 * repeating the backend's query struct in every caller.
 */
export type ArticleFilter = {
  source?: string;
  limit?: number;
  offset?: number;
  /** Server-side substring match over title / blurb / source. */
  search?: string;
};

/**
 * Tauri deserializes an absent argument as `None` — normalize once here
 * rather than at each call site.
 */
function filterArgs(filter: ArticleFilter) {
  return {
    source: filter.source ?? null,
    limit: filter.limit ?? null,
    offset: filter.offset ?? null,
    search: filter.search?.trim() ? filter.search.trim() : null,
  };
}

export const apiArticles = {
  /**
   * Interest-ranked digest window (home). The backend hands back one snapshot
   * (candidate window + affinity inputs); scoring and cursor paging run here in
   * `src/rank.ts`, so ranking tweaks no longer need a Rust rebuild.
   */
  listArticlesRanked: async (
    filter: ArticleFilter & {
      unreadOnly?: boolean;
      cursor?: { score: number; id: string } | null;
    },
  ): Promise<ArticleListItem[]> => {
    const w = await typedInvoke("list_rank_window", {
      source: filter.source ?? null,
      search: filter.search?.trim() ? filter.search.trim() : null,
      unreadOnly: filter.unreadOnly ?? null,
    });
    const affinity: Affinity = {
      sourceOpens: w.source_opens,
      categoryOpens: w.category_opens,
      termWeights: buildTermWeights(w.engaged_titles),
      sourcePriority: w.source_priority,
      categoryPriorityMax: w.category_priority_max,
    };
    const nowMs = Date.now();
    const ranked = rankArticles(w.items, affinity, nowMs, dayKeyFromMs(nowMs));
    return pageRanked(ranked, filter.cursor ?? null, filter.offset ?? 0, filter.limit ?? 60);
  },
  /** Plain newest-first library list with the full filter set. */
  listLibrary: (
    filter: ArticleFilter & {
      readState?: ReadStateFilter;
      likedOnly?: boolean;
    },
  ) =>
    typedInvoke("list_library", {
      ...filterArgs(filter),
      readState: filter.readState ?? null,
      likedOnly: filter.likedOnly ?? null,
    }),
  getArticleView: (id: string) => typedInvoke("get_article_view", { id }),
  markArticleOpened: (id: string) => typedInvoke("mark_article_opened", { id }),
  markArticleProgress: (id: string, dwellMsDelta: number, readCompleted: boolean) =>
    typedInvoke("mark_article_progress", {
      id,
      dwellMsDelta,
      readCompleted,
    }),
  setArticleLiked: (id: string, liked: boolean) =>
    typedInvoke("set_article_liked", { id, liked }),
  getLearningStats: () => typedInvoke("get_learning_stats"),
  getReadingStats: () => typedInvoke("get_reading_stats"),
  fillMissingCardZh: () => typedInvoke("fill_missing_card_zh"),
  translateParagraph: (articleId: string, paragraphIndex: number, text: string) =>
    typedInvoke("translate_paragraph", {
      articleId,
      paragraphIndex,
      text,
    }),
  translateSelection: (articleId: string, text: string) =>
    typedInvoke("translate_selection", { articleId, text }),
  translatePlainText: (text: string) => typedInvoke("translate_plain_text", { text }),
  translateFullArticle: (articleId: string) =>
    typedInvoke("translate_full_article", { articleId }),
};
