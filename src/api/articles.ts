import { invoke } from "@tauri-apps/api/core";
import type {
  Article,
  ArticleListItem,
  ArticleView,
  FullTranslateResult,
  LearningStats,
  ReadingStats,
  TranslationRow,
} from "./types";

/** Read-state filter; mirrors the backend's `ReadState`. */
type ReadStateFilter = "all" | "unfinished" | "unread" | "reading" | "read";

/**
 * Filter shared by both list commands — one place to add a field, instead of
 * repeating the backend's query struct in every caller.
 */
export type ArticleFilter = {
  category?: string;
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
    category: filter.category ?? null,
    source: filter.source ?? null,
    limit: filter.limit ?? null,
    offset: filter.offset ?? null,
    search: filter.search?.trim() ? filter.search.trim() : null,
  };
}

export const apiArticles = {
  /** Interest-ranked digest window (home). */
  listArticlesRanked: (
    filter: ArticleFilter & {
      unreadOnly?: boolean;
      cursor?: { score: number; id: string } | null;
    },
  ) =>
    invoke<ArticleListItem[]>("list_articles_ranked", {
      ...filterArgs(filter),
      unreadOnly: filter.unreadOnly ?? null,
      cursorScore: filter.cursor?.score ?? null,
      cursorId: filter.cursor?.id ?? null,
    }),
  /** Plain newest-first library list with the full filter set. */
  listLibrary: (
    filter: ArticleFilter & {
      readState?: ReadStateFilter;
      likedOnly?: boolean;
    },
  ) =>
    invoke<ArticleListItem[]>("list_library", {
      ...filterArgs(filter),
      readState: filter.readState ?? null,
      likedOnly: filter.likedOnly ?? null,
    }),
  getArticleView: (id: string) =>
    invoke<ArticleView | null>("get_article_view", { id }),
  markArticleOpened: (id: string) =>
    invoke<void>("mark_article_opened", { id }),
  markArticleProgress: (id: string, dwellMsDelta: number, readCompleted: boolean) =>
    invoke<void>("mark_article_progress", {
      id,
      dwellMsDelta,
      readCompleted,
    }),
  setArticleLiked: (id: string, liked: boolean) =>
    invoke<void>("set_article_liked", { id, liked }),
  getLearningStats: () => invoke<LearningStats>("get_learning_stats"),
  getReadingStats: () => invoke<ReadingStats>("get_reading_stats"),
  fillMissingCardZh: () => invoke<number>("fill_missing_card_zh"),
  importArticleUrl: (url: string) =>
    invoke<Article>("import_article_url", { url }),
  importArticleFile: (path: string) =>
    invoke<Article>("import_article_file", { path }),
  /** Re-fetch articles whose stored body lost paragraph breaks. */
  repairParagraphs: (limit?: number) =>
    invoke<number>("repair_paragraphs", { limit: limit ?? null }),
  translateParagraph: (
    articleId: string,
    paragraphIndex: number,
    text: string,
  ) =>
    invoke<TranslationRow>("translate_paragraph", {
      articleId,
      paragraphIndex,
      text,
    }),
  translateSelection: (articleId: string, text: string) =>
    invoke<TranslationRow>("translate_selection", { articleId, text }),
  translatePlainText: (text: string) =>
    invoke<string>("translate_plain_text", { text }),
  translateFullArticle: (articleId: string) =>
    invoke<FullTranslateResult>("translate_full_article", { articleId }),
};
