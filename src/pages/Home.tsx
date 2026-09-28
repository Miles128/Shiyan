import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, ArticleListItem, LearningStats } from "../api";
import {
  fetchQuery,
  invalidateQueries,
  peekQuery,
} from "../query";
import {
  applyDifficultyOrder,
  articleNeedsCardZh,
  homeListKey,
  pickTopArticles,
} from "../homeDerived";
import { useArticleBackfill, useHomeDifficulty, useInfiniteScroll } from "../homeHooks";
import { formatLearningInsight } from "../learningStats";
import {
  DIFFICULTY_LEVELS,
  difficultyLabel,
  type DifficultyLevel,
} from "../difficulty";
import { useAppConfig, useSearchQuery, useShell, useVocab } from "../store";
import { useToast } from "../components/Toaster";
import { emitEvent, onEvent } from "../events";
import {
  ensureLexiconLoaded,
  isCefrLevel,
  isFreqBand,
  type CefrLevel,
  type FreqBand,
} from "../wordLevels";
import ArticleRow from "../components/ArticleRow";
import { lastArticlePath } from "../useArticle";
import WordPopoverShell from "../components/WordPopoverShell";
import {
  bundledGloss,
  cachedTranslation,
  rememberTranslation,
} from "../wordResolve";
import { useTts } from "../useTts";
import { useWordPopover } from "../useWordPopover";

const PAGE_SIZE = 60;
/** 今日推荐: the first N ranked unread articles, shown expanded. */
const TOP_PICKS = 10;

/** 未完成 (default) / 未读 / 在读 / 已读 / 全部. */
type ReadFilter = "unfinished" | "unread" | "reading" | "read" | "all";

/** Filter-panel state as one object: reset = one assignment, no setter juggling.
 *  Tags and source live in the shell (sidebar owns them); the panel keeps the
 *  reading-state / favourite / difficulty refinement. */
type Filters = {
  read: ReadFilter;
  likedOnly: boolean;
  level: DifficultyLevel | "all";
};

const DEFAULT_FILTERS: Filters = {
  read: "unfinished",
  likedOnly: false,
  level: "all",
};

export default function Home() {
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const navigate = useNavigate();
  const { cfg } = useAppConfig();
  const {
    rerankNonce,
    filtersOpen,
    focusSource,
    setFocusSource,
  } = useShell();
  const { query, setQuery } = useSearchQuery();
  // Debounced server-side search: the backend filters title/blurb/source so
  // searching reaches past the loaded window. 300ms keeps keystrokes smooth.
  const [debouncedQuery, setDebouncedQuery] = useState(query);
  useEffect(() => {
    const t = window.setTimeout(() => setDebouncedQuery(query), 300);
    return () => window.clearTimeout(t);
  }, [query]);
  const serverSearch = debouncedQuery.trim() ? debouncedQuery.trim() : undefined;
  /** Archive filters (merged in from the old Library page). */
  const [filters, setFilters] = useState<Filters>(DEFAULT_FILTERS);
  const patchFilters = useCallback(
    (patch: Partial<Filters>) => setFilters((f) => ({ ...f, ...patch })),
    [],
  );
  /** True reset for the filtered-empty state: panel + source + search. */
  const resetAllFilters = useCallback(() => {
    setFilters(DEFAULT_FILTERS);
    setFocusSource(null);
    setQuery("");
  }, [setFocusSource, setQuery]);
  const toast = useToast();
  const [learningStats, setLearningStats] = useState<LearningStats | null>(null);

  const {
    learningTerms,
    knownTerms,
    refreshLearningTerms,
    markKnown,
    unmarkKnown,
  } = useVocab();
  const tts = useTts();
  const freqBand: FreqBand = isFreqBand(cfg.freq_band) ? cfg.freq_band : 3000;
  const cefrLevel: CefrLevel = isCefrLevel(cfg.cefr_level)
    ? cfg.cefr_level
    : "B1";
  const hasLlm = Boolean(cfg.api_key?.trim());

  /** Main-list model (single focused list, never a multi-source page):
   *  - 今日推荐 (default): no source focused and no other filter active.
   *  - source view: a sidebar source is focused → that source's articles.
   *  - archive view: some refinement (status/收藏/难度) active with no
   *    focused source → a flat filtered list. */
  const hasOtherFilter =
    filters.read !== "unfinished" ||
    filters.likedOnly ||
    filters.level !== "all" ||
    serverSearch != null;
  const showPicks = focusSource === null && !hasOtherFilter;

  const fetchPage = useCallback(
    (offset: number, cursor?: { score: number; id: string } | null) =>
      showPicks
        ? api.listArticlesRanked({
            unreadOnly: true,
            limit: PAGE_SIZE,
            offset,
            cursor: cursor ?? null,
            search: serverSearch,
          })
        : api.listLibrary({
            category: undefined,
            source: focusSource ?? undefined,
            readState: filters.read,
            likedOnly: filters.likedOnly,
            limit: PAGE_SIZE,
            offset,
            search: serverSearch,
          }),
    [showPicks, focusSource, filters, serverSearch],
  );

  // Cache key for the page-0 window (pagination appends stay local state —
  // only the base window is shared). Single definition in homeDerived: the
  // state initializer below must spell it identically or peeks miss.
  const listKey = useMemo(
    () =>
      homeListKey({
        showPicks,
        focusSource,
        read: filters.read,
        likedOnly: filters.likedOnly,
        search: serverSearch,
      }),
    [showPicks, focusSource, filters, serverSearch],
  );

  // First paint comes from the cache when present: returning from an article
  // renders rows immediately instead of flashing the empty state, and the
  // fresh fetch in load() replaces them. Lazy initializer re-runs per
  // mount, so every return re-peeks.
  const [articles, setArticles] = useState<ArticleListItem[]>(
    () => peekQuery<ArticleListItem[]>(listKey) ?? [],
  );
  const [hasMore, setHasMore] = useState(
    () => (peekQuery<ArticleListItem[]>(listKey)?.length ?? 0) >= PAGE_SIZE,
  );

  // Stale-response guard: rapid filter/source changes fire overlapping loads;
  // only the newest call may touch state.
  const loadSeq = useRef(0);
  const load = useCallback(async () => {
    const seq = ++loadSeq.current;
    // Stale-while-revalidate: remounts (Reader → Home) paint the cached
    // window instantly instead of flashing 加载中…; the fresh fetch below
    // always runs and replaces it. Filter changes miss the cache (new key)
    // and keep today's loading flash.
    const snap = peekQuery<ArticleListItem[]>(listKey);
    if (snap) {
      setArticles(snap);
      setHasMore(snap.length >= PAGE_SIZE);
    } else {
      setLoading(true);
    }
    try {
      const [list, stats] = await Promise.all([
        fetchQuery<ArticleListItem[]>(listKey, () => fetchPage(0)),
        // Stats move slowly; a minute of freshness skips a refetch on every
        // filter change. Refresh events invalidate (see below).
        fetchQuery<LearningStats | null>(
          ["learning-stats"],
          () => api.getLearningStats().catch(() => null),
          { staleTime: 60_000 },
        ),
        ensureLexiconLoaded().catch(() => undefined),
      ]);
      if (seq !== loadSeq.current) return;
      setArticles(list);
      setHasMore(list.length >= PAGE_SIZE);
      setLearningStats(stats);
    } catch (e) {
      if (seq === loadSeq.current) toast.err(String(e));
    } finally {
      if (seq === loadSeq.current) setLoading(false);
    }
  }, [fetchPage, listKey, toast]);

  useEffect(() => {
    void load();
  }, [load]);

  // A sidebar priority reorder bumps rerankNonce → re-fetch the ranked list.
  // Skip the initial mount (already covered by the load effect above).
  const rerankMounted = useRef(false);
  useEffect(() => {
    if (!rerankMounted.current) {
      rerankMounted.current = true;
      return;
    }
    void load();
    // Only rerankNonce should retrigger this; `load` is read fresh each render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rerankNonce]);

  const { cardFilling, cardFillError, retryFillCards } = useArticleBackfill({
    articles,
    hasLlm,
    loading,
    load,
  });

  const loadMoreRef = useRef(false);
  // Latest list key, synced every render (effect, never during render):
  // a page fetched under the old filter must not append into the new list.
  const listKeyRef = useRef(listKey);
  useEffect(() => {
    listKeyRef.current = listKey;
  });
  async function loadMore() {
    if (loadMoreRef.current) return;
    loadMoreRef.current = true;
    const keyAtStart = listKeyRef.current;
    setLoadingMore(true);
    try {
      // Cursor pagination for the ranked 今日推荐 feed: immune to inserts above
      // the cursor. Source/archive views keep offset paging.
      const last = articles.length > 0 ? articles[articles.length - 1] : null;
      const next = await fetchPage(
        articles.length,
        showPicks && last ? { score: last.rank_score, id: last.id } : null,
      );
      // Filter/source changed mid-flight: drop the stale page instead of
      // mixing rows from two queries. (load() has the same guard via loadSeq;
      // loadMore appends, so it needs the key check instead.)
      if (listKeyRef.current !== keyAtStart) return;
      setArticles((prev) => {
        const seen = new Set(prev.map((a) => a.id));
        return prev.concat(next.filter((a) => !seen.has(a.id)));
      });
      setHasMore(next.length >= PAGE_SIZE);
    } catch (e) {
      toast.err(String(e));
    } finally {
      loadMoreRef.current = false;
      setLoadingMore(false);
    }
  }

  // Refresh runs from the topbar; Home reloads on the shared event below.

  /** True when any loaded row still lacks a Chinese blurb (drives the hint). */
  const needsCardZh = useMemo(
    () => articles.some(articleNeedsCardZh),
    [articles],
  );

  // Client-side refinement over the server-filtered page (title / blurb /
  // source). The backend `search` already scopes to the whole library;
  // this only guards against stale rows while the debounced query is in flight.
  const matchesQuery = useCallback(
    (a: ArticleListItem) => {
      const q = query.trim().toLowerCase();
      if (!q) return true;
      return (
        a.title.toLowerCase().includes(q) ||
        a.summary_zh.toLowerCase().includes(q) ||
        a.source.toLowerCase().includes(q)
      );
    },
    [query],
  );

  // Local difficulty index per article — see useHomeDifficulty.
  const { difficultyById, levelCounts } = useHomeDifficulty({
    articles,
    learningTerms,
    knownTerms,
    cefrLevel,
    freqBand,
  });

  const matchesLevel = useCallback(
    (a: ArticleListItem) =>
      filters.level === "all" || difficultyById.get(a.id) === filters.level,
    [filters.level, difficultyById],
  );

  /** Flat list for the source / archive views (difficulty + search refined). */
  const visible = useMemo(
    () => articles.filter((a) => matchesLevel(a) && matchesQuery(a)),
    [articles, matchesLevel, matchesQuery],
  );

  // Difficulty fit nudges the 今日推荐 ranking: the sweet spot (a few new words
  // per paragraph) floats up, word walls and trivially-easy pieces sink.
  const orderedArticles = useMemo(
    () => applyDifficultyOrder(visible, difficultyById),
    [visible, difficultyById],
  );
  const topPicks = useMemo(
    () => pickTopArticles(orderedArticles, TOP_PICKS),
    [orderedArticles],
  );

  // Collapsing the filter panel only folds it; values are kept until the
  // explicit 清除筛选 button (which resets everything, see resetAllFilters).

  /** Two modes only: 今日推荐 (default) vs 全部文章 (any source/filter/search
   *  active — a focused source is just one filter among others). */
  const displayList = showPicks ? topPicks : visible;
  const listHeading = showPicks ? "今日推荐" : "全部文章";

  // The top bar drives refresh + feed management; Home only reacts.
  // Refresh failures also persist as a banner (same style as card-fill errors)
  // so the cause survives the toast.
  const [refreshError, setRefreshError] = useState<string | null>(null);
  useEffect(() => {
    return onEvent("shiyan:refreshed", (detail) => {
      const result = detail.result;
      if (detail.error || !result) {
        setRefreshError(detail.error ?? "刷新失败");
        return;
      }
      setRefreshError(null);
      // Stats are cached for a minute — a refresh moves them, so bust.
      // (The article window always revalidates on load; its peek is only
      // an instant stale paint, replaced by the fresh fetch in load().)
      invalidateQueries(["learning-stats"]);
      toast.ok(
        `新增 ${result.added_or_updated}` +
          (result.skipped_existing ? ` · 已有 ${result.skipped_existing}` : "") +
          (result.skipped_duplicate ? ` · 去重 ${result.skipped_duplicate}` : "") +
          (result.purged_teasers ? ` · 清理残篇 ${result.purged_teasers}` : "") +
          (result.purged_old ? ` · 过期清理 ${result.purged_old}` : "") +
          (result.feeds_unchanged ? ` · ${result.feeds_unchanged} 源无更新` : "") +
          (result.titles_translated ? ` · 补简介 ${result.titles_translated}` : "") +
          (result.errors.length ? ` · ${result.errors.length} 个问题` : ""),
      );
      if (result.errors.length) {
        toast.err(
          "刷新问题：" +
            result.errors.slice(0, 3).join("；") +
            (result.errors.length > 3 ? ` 等 ${result.errors.length} 条` : ""),
        );
      }
      void load();
    });
  }, [load, toast]);

  // Selection-to-translate on the home list (titles / summaries).
  const { popover, showMeaning, mount: popoverMount } = useWordPopover({
    articleId: null,
    tts,
    localGloss: (term) => bundledGloss(term) ?? cachedTranslation(term),
    translate: async (term) => {
      const translated = await api.translatePlainText(term);
      rememberTranslation(term, translated);
      return translated;
    },
    contextFor: (source, term) => source ?? term,
    onError: (m) => toast.err(m),
    onSuccess: (m) => toast.ok(m),
    onVocabAdded: () => void refreshLearningTerms(),
    knownTerms,
    markKnown,
    unmarkKnown,
  });

  // Keyboard flow (j/k/Enter/o): navigate exactly what is on screen.
  const [selectedId, setSelectedId] = useState<string | null>(null);

  // Infinite scroll only makes sense while there is something on screen to
  // read; with everything collapsed it would just stream in new (collapsed)
  // boards. Re-arms as soon as one board is expanded again.
  const sentinelRef = useInfiniteScroll(
    hasMore && displayList.length > 0,
    loadMore,
  );

  useEffect(() => {
    function onNavKey(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const key = e.key;
      if (key !== "j" && key !== "k" && key !== "Enter" && key !== "o") return;
      const t = e.target as HTMLElement | null;
      const tag = t?.tagName;
      if (
        tag === "INPUT" ||
        tag === "TEXTAREA" ||
        tag === "SELECT" ||
        t?.isContentEditable
      ) {
        return;
      }
      // Don't hijack Enter on a focused button/link (it would double-act),
      // and not while the selection popover is open.
      if ((key === "Enter" || key === "o") && (tag === "BUTTON" || tag === "A")) {
        return;
      }
      if (popover || displayList.length === 0) return;
      e.preventDefault();
      if (key === "j" || key === "k") {
        setSelectedId((prev) => {
          const idx = prev ? displayList.findIndex((a) => a.id === prev) : -1;
          const next =
            key === "j"
              ? Math.min(displayList.length - 1, idx + 1)
              : Math.max(0, idx - 1);
          return displayList[next]?.id ?? null;
        });
        return;
      }
      // Enter/o: with a selection open it; without one, select the first row.
      const target = selectedId
        ? displayList.find((a) => a.id === selectedId)
        : displayList[0];
      if (selectedId && target) navigate(`/article/${target.id}`);
      else if (target) setSelectedId(target.id);
    }
    window.addEventListener("keydown", onNavKey);
    return () => window.removeEventListener("keydown", onNavKey);
  }, [displayList, popover, selectedId, navigate]);

  // Keep the keyboard-selected row in view.
  useEffect(() => {
    if (!selectedId) return;
    document
      .querySelector(`[data-article-row="${selectedId}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selectedId]);

  async function onPageMouseUp(e: React.MouseEvent) {
    const sel = window.getSelection();
    const text = sel?.toString().trim() ?? "";
    if (!text || text.length > 120) {
      return;
    }
    await showMeaning({ text, x: e.clientX, y: e.clientY });
  }

  const resumePath = lastArticlePath();

  return (
    <div className="page home-page" onMouseUp={(e) => void onPageMouseUp(e)}>
      {filtersOpen && (
        <div className="library-filters">
          <div className="filter-row">
            <div className="filter-group">
              {(
                [
                  ["unfinished", "未完成"],
                  ["unread", "未读"],
                  ["read", "已读"],
                  ["all", "全部"],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  className={filters.read === id ? "tag-chip active" : "tag-chip"}
                  onClick={() => patchFilters({ read: id })}
                >
                  {label}
                </button>
              ))}
              <button
                type="button"
                className={filters.likedOnly ? "tag-chip active" : "tag-chip"}
                onClick={() =>
                  // Opening the ★ shelf switches read to 全部: liked
                  // articles you already finished (read_completed = 1) are
                  // excluded by the default 未完成 filter, which reads as
                  // "收藏丢失". Closing ★ keeps the current read filter.
                  setFilters((f) =>
                    f.likedOnly
                      ? { ...f, likedOnly: false }
                      : { ...f, likedOnly: true, read: "all" },
                  )
                }
              >
                ★ 收藏
              </button>
            </div>
            {hasOtherFilter && (
              <button
                type="button"
                className="tag-chip clear filter-clear"
                onClick={resetAllFilters}
              >
                清除筛选
              </button>
            )}
          </div>

          <div className="filter-row">
            <select
              className="filter-select"
              value={filters.level}
              onChange={(e) =>
                patchFilters({ level: e.target.value as DifficultyLevel | "all" })
              }
            >
              <option value="all">全部难度</option>
              {DIFFICULTY_LEVELS.map((level) => (
                <option key={level} value={level}>
                  {difficultyLabel(level)}
                  {levelCounts.get(level) ? ` (${levelCounts.get(level)})` : ""}
                </option>
              ))}
            </select>
          </div>
        </div>
      )}

      {!hasLlm && needsCardZh && (
        <p className="muted">设置里填 API Key 后，列表会自动补一两句中文简介。</p>
      )}
      {hasLlm && cardFilling && <p className="muted">正在补中文简介…</p>}
      {refreshError && (
        <p className="banner err with-action">
          <span>刷新失败：{refreshError}</span>
          <button
            type="button"
            className="btn small"
            onClick={() => {
              setRefreshError(null);
              // Same path as the topbar refresh button: invoke + shared event.
              void api
                .refreshFeeds()
                .then((result) => emitEvent("shiyan:refreshed", { result }))
                .catch((e) =>
                  emitEvent("shiyan:refreshed", { error: String(e) }),
                );
            }}
          >
            重试
          </button>
        </p>
      )}
      {cardFillError && (
        <p className="banner err with-action">
          <span>简介未生成：{cardFillError}</span>
          <button
            type="button"
            className="btn small"
            onClick={retryFillCards}
          >
            重试
          </button>
        </p>
      )}

      {loading && <p className="muted">加载中…</p>}
      {!loading && articles.length === 0 && (
        <div className="empty">
          <p>
            还没有文章。点上方「刷新」拉取订阅；也可以用右上角按钮导入文件，
            或在 设置 → 订阅 里粘贴文章链接。
          </p>
        </div>
      )}
      {!loading && articles.length > 0 && displayList.length === 0 && (
        <div className="empty">
          <p>没有符合条件的文章。</p>
          <button type="button" className="btn small" onClick={resetAllFilters}>
            清除全部筛选
          </button>
        </div>
      )}

      {displayList.length > 0 && (
        <>
          <div className="list-heading-row">
            <h2 className="list-heading">
              {listHeading}
              {focusSource && <span className="muted"> · {focusSource}</span>}
            </h2>
            <div className="tabs-right">
              {learningStats && (
                <span className="learning-insight-inline">
                  {formatLearningInsight(learningStats)} ·{" "}
                  <Link to="/stats">统计</Link>
                </span>
              )}
              {resumePath && (
                <button
                  type="button"
                  className="linklike"
                  onClick={() => navigate(resumePath)}
                >
                  继续阅读
                </button>
              )}
              {focusSource && (
                <button
                  type="button"
                  className="linklike"
                  onClick={() => setFocusSource(null)}
                >
                  回到今日推荐
                </button>
              )}
            </div>
          </div>
          <ul className="article-list library-list">
            {displayList.map((a) => (
              <ArticleRow
                key={a.id}
                article={a}
                difficulty={difficultyById.get(a.id) ?? null}
                showSource={!focusSource && !showPicks}
                highlighted={a.id === selectedId}
              />
            ))}
          </ul>
        </>
      )}

      {popoverMount && <WordPopoverShell {...popoverMount} />}

      {/* 今日推荐恒 10 篇封顶：该模式不挂无限滚动哨兵，避免空转加载 */}
      {hasMore && !showPicks && (
        <div ref={sentinelRef}>
          {loadingMore && <p className="muted">加载中…</p>}
        </div>
      )}
    </div>
  );
}

