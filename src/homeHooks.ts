import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type ArticleListItem } from "./api";
import {
  articleDifficulty,
  calibrateEdges,
  difficultyFromScore,
  difficultyScoreCache,
  type DifficultyLevel,
  type DifficultyPrefs,
} from "./difficulty";
import { articleNeedsCardZh } from "./homeDerived";
import { normalizeKey } from "./wordLevels";
import type { CefrLevel, FreqBand } from "./wordLevels";

/**
 * Home-domain hooks, split out of Home.tsx so the page stays a thin
 * orchestrator. Behavior-identical extractions; see each hook for the
 * original rationale.
 */

type BackfillOptions = {
  articles: ArticleListItem[];
  hasLlm: boolean;
  loading: boolean;
  /** Full reload (list refreshed after backfills land). */
  load: () => Promise<void>;
};

/**
 * Auto-backfill for LLM enrichment: one-line Chinese blurbs.
 */
export function useArticleBackfill({
  articles,
  hasLlm,
  loading,
  load,
}: BackfillOptions) {
  const didCards = useRef(false);
  const [cardFilling, setCardFilling] = useState(false);
  const [cardFillError, setCardFillError] = useState<string | null>(null);

  // New fetch context (filter/category change) re-arms the guard.
  useEffect(() => {
    didCards.current = false;
  }, [load]);

  const fillCards = useCallback(async () => {
    setCardFilling(true);
    setCardFillError(null);
    try {
      const n = await api.fillMissingCardZh();
      if (n > 0) await load();
    } catch (e) {
      didCards.current = false;
      setCardFillError(String(e));
    } finally {
      setCardFilling(false);
    }
  }, [load]);

  // Summary backfill — the user-visible one, so it gets its own shot.
  useEffect(() => {
    if (didCards.current) return;
    if (!hasLlm || loading || articles.length === 0) return;
    if (!articles.some(articleNeedsCardZh)) return;
    didCards.current = true;
    void fillCards();
  }, [articles, hasLlm, loading, fillCards]);

  /** Manual retry from the error banner; skips the once-guard. */
  const retryFillCards = useCallback(() => {
    didCards.current = true;
    void fillCards();
  }, [fillCards]);

  return { cardFilling, cardFillError, retryFillCards };
}

/**
 * Infinite scroll: a sentinel near the list bottom triggers `loadMore`.
 * Ref indirection keeps the observer callback on the latest closure.
 * Returns the ref to attach to the sentinel element.
 */
export function useInfiniteScroll(
  hasMore: boolean,
  loadMore: () => void | Promise<void>,
) {
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const loadMoreRef = useRef(loadMore);
  useEffect(() => {
    loadMoreRef.current = loadMore;
  });
  useEffect(() => {
    if (!hasMore) return;
    const el = sentinelRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) loadMoreRef.current();
      },
      { rootMargin: "600px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [hasMore]);
  return sentinelRef;
}

type DifficultyOptions = {
  articles: ArticleListItem[];
  learningTerms: string[];
  knownTerms: string[];
  cefrLevel: CefrLevel;
  freqBand: FreqBand;
};

/**
 * Local difficulty index per article: density of words above this learner's
 * word-frequency size (plus their learning terms). Bucket edges are calibrated
 * against the visible sample, so 简单/普通/较难 stay relative to what this
 * learner actually gets served.
 */
export function useHomeDifficulty({
  articles,
  learningTerms,
  knownTerms,
  cefrLevel,
  freqBand,
}: DifficultyOptions) {
  const difficultyPrefs = useMemo<DifficultyPrefs>(
    () => ({ cefrLevel, freqBand }),
    [cefrLevel, freqBand],
  );
  // Hoisted term sets: building them once per input change instead of once
  // per article (N× allocation + N× normalize). Content-stable via join key.
  const learningKey = useMemo(
    () => learningTerms.map(normalizeKey).filter((t) => t.length >= 2).sort().join("\n"),
    [learningTerms],
  );
  const knownKey = useMemo(
    () => knownTerms.map(normalizeKey).filter((t) => t.length >= 2).sort().join("\n"),
    [knownTerms],
  );
  // Per-article score cache: `loadMore` appends rows, so only genuinely new
  // (id, excerpt, prefs, vocab) tuples pay for tokenize + dict lookup.
  // Vocab identity is a short hash (not the raw terms) so cache keys stay small.
  // The cache is module-shared (difficultyScoreCache), not per mount: the
  // return-from-Reader pass over the same 60 rows is then all hits instead
  // of ~25ms of main-thread recompute.
  const vocabHash = useMemo(() => {
    const s = `${learningKey}\n${knownKey}`;
    let h = 0;
    for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
    return `${cefrLevel}|${freqBand}|${s.length}|${h.toString(36)}`;
  }, [learningKey, knownKey, cefrLevel, freqBand]);
  return useMemo(() => {
    const cache = difficultyScoreCache;
    const scored: { id: string; score: number | null }[] = [];
    for (const a of articles) {
      const ck = `${a.id}\n${vocabHash}`;
      const hit = cache.get(ck, a.excerpt);
      if (hit) {
        scored.push({ id: a.id, score: hit.score });
        continue;
      }
      const result = articleDifficulty(
        a.excerpt,
        learningTerms,
        difficultyPrefs,
        knownTerms,
      );
      const score = result?.score ?? null;
      cache.set(ck, a.excerpt, score);
      scored.push({ id: a.id, score });
    }
    const edges = calibrateEdges(
      scored.map((s) => s.score).filter((s): s is number => s !== null),
    );
    const byId = new Map<string, DifficultyLevel | null>();
    const counts = new Map<DifficultyLevel, number>();
    for (const { id, score } of scored) {
      const level = score === null ? null : difficultyFromScore(score, edges);
      byId.set(id, level);
      if (level) counts.set(level, (counts.get(level) ?? 0) + 1);
    }
    return { difficultyById: byId, levelCounts: counts };
    // vocabHash carries the vocab+prefs identity; raw arrays feed the compute.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [articles, difficultyPrefs, vocabHash]);
}
