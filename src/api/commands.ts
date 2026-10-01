import type {
  AppConfig,
  Article,
  ArticleListItem,
  ArticleView,
  FeedCategory,
  FeedDiscoverCandidate,
  FeedSource,
  FeedValidation,
  FullTranslateResult,
  LearningStats,
  LookupEntry,
  MemoryItem,
  RankWindow,
  ReadingStats,
  RefreshResult,
  TranslationRow,
} from "./types";

/** Read-state filter; mirrors the backend's `ReadState`. */
export type ReadStateFilter = "all" | "unfinished" | "unread" | "reading" | "read";

/** Vocab entry kind; mirrors the backend's `MemoryKind`. */
export type MemoryKind = "word" | "phrase";

/**
 * The IPC contract: every Tauri command the frontend may call, with its
 * argument shape and return type in one place. `typedInvoke` keys off this, so
 * a misspelled command name or a wrong argument key fails `tsc` instead of
 * silently deserializing to `None` on the Rust side.
 *
 * Argument keys are lowerCamelCase — Tauri's `#[tauri::command]` macro maps
 * them onto snake_case Rust params. Fields *nested inside* an argument (e.g.
 * `add_memory`'s `input`) follow serde and stay snake_case; only the top-level
 * keys are Tauri's to translate.
 *
 * A command that takes no arguments declares `args: void`.
 */
export interface Commands {
  // ── articles ──────────────────────────────────────────────────────────
  list_rank_window: {
    args: {
      category: string | null;
      source: string | null;
      search: string | null;
      unreadOnly: boolean | null;
    };
    ret: RankWindow;
  };
  list_library: {
    args: {
      category: string | null;
      source: string | null;
      limit: number | null;
      offset: number | null;
      search: string | null;
      readState: ReadStateFilter | null;
      likedOnly: boolean | null;
    };
    ret: ArticleListItem[];
  };
  get_article_view: { args: { id: string }; ret: ArticleView | null };
  mark_article_opened: { args: { id: string }; ret: void };
  mark_article_progress: {
    args: { id: string; dwellMsDelta: number; readCompleted: boolean };
    ret: void;
  };
  set_article_liked: { args: { id: string; liked: boolean }; ret: void };
  get_learning_stats: { args: void; ret: LearningStats };
  get_reading_stats: { args: void; ret: ReadingStats };
  fill_missing_card_zh: { args: void; ret: number };
  import_article_url: { args: { url: string }; ret: Article };
  import_article_file: { args: { path: string }; ret: Article };
  repair_paragraphs: { args: { limit: number | null }; ret: number };
  translate_paragraph: {
    args: { articleId: string; paragraphIndex: number; text: string };
    ret: TranslationRow;
  };
  translate_selection: {
    args: { articleId: string; text: string };
    ret: TranslationRow;
  };
  translate_plain_text: { args: { text: string }; ret: string };
  translate_full_article: {
    args: { articleId: string };
    ret: FullTranslateResult;
  };

  // ── memory / vocab / known words / lookups ────────────────────────────
  add_memory: {
    args: {
      input: {
        kind: MemoryKind;
        term: string;
        context_sentence: string;
        article_id: string | null;
        definition_zh: string | null;
        word_type: string | null;
        collocations: string[] | null;
      };
    };
    ret: MemoryItem;
  };
  list_memory: {
    args: { kind: MemoryKind; status: string | null };
    ret: MemoryItem[];
  };
  due_memory: { args: { kind: MemoryKind }; ret: MemoryItem[] };
  review_memory: { args: { id: string; rating: string }; ret: MemoryItem };
  set_memory_status: { args: { id: string; status: string }; ret: void };
  delete_memory: { args: { id: string }; ret: void };
  export_memory_csv: { args: void; ret: string | null };
  list_known_words: { args: void; ret: string[] };
  add_known_word: { args: { term: string }; ret: void };
  remove_known_word: { args: { term: string }; ret: void };
  record_lookup: {
    args: { term: string; context: string | null; articleId: string | null };
    ret: void;
  };
  list_lookups: {
    args: { search: string | null; limit: number | null; offset: number | null };
    ret: LookupEntry[];
  };
  delete_lookup: { args: { id: number }; ret: void };
  clear_lookups: { args: void; ret: void };

  // ── feeds ─────────────────────────────────────────────────────────────
  list_feeds: { args: void; ret: FeedSource[] };
  set_feed_enabled: { args: { id: string; enabled: boolean }; ret: void };
  reorder_feeds: { args: { orderedIds: string[] }; ret: void };
  delete_feed_source: { args: { id: string }; ret: void };
  list_feed_categories: { args: void; ret: FeedCategory[] };
  list_source_article_counts: { args: void; ret: Record<string, number> };
  add_feed_category: { args: { label: string }; ret: FeedCategory };
  discover_feeds: {
    args: { categoryId: string };
    ret: FeedDiscoverCandidate[];
  };
  validate_feed: { args: { url: string }; ret: FeedValidation };
  subscribe_feed: {
    args: {
      input: {
        name: string;
        category: string;
        url: string;
        description: string | null;
      };
    };
    ret: FeedSource;
  };
  refresh_feeds: { args: void; ret: RefreshResult };
  cancel_refresh: { args: void; ret: void };

  // ── config ────────────────────────────────────────────────────────────
  get_config: { args: void; ret: AppConfig };
  save_config_cmd: { args: { cfg: AppConfig }; ret: void };

  // ── data ──────────────────────────────────────────────────────────────
  backup_database: { args: void; ret: string | null };
  restore_database: { args: void; ret: string };
}
