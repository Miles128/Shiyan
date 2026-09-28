import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { api, defaultAppConfig, type AppConfig, type FeedSource } from "./api";
import { onEvent } from "./events";
import { invalidateQueries } from "./query";
import { normalizeReadingPrefs } from "./readingPrefs";
import { isCefrLevel, isFreqBand, normalizeKey } from "./wordLevels";

/** Merge raw config over defaults, clamping difficulty fields. */
export function normalizeConfig(raw: AppConfig): AppConfig {
  return {
    ...defaultAppConfig(),
    ...raw,
    cefr_level: isCefrLevel(raw.cefr_level) ? raw.cefr_level : "B1",
    freq_band: isFreqBand(raw.freq_band) ? raw.freq_band : 3000,
    ...normalizeReadingPrefs(raw),
  };
}

export type ConfigLoadAttempt =
  | { ok: true; loaded: AppConfig }
  | { ok: false; message: string };

/** First get_config attempt always yields `ready`, so the app is never stuck on a silent failure. */
export function applyConfigLoadResult(
  previous: AppConfig,
  result: ConfigLoadAttempt,
): { cfg: AppConfig; ready: true; loadError: string | null } {
  if (result.ok) {
    return {
      cfg: normalizeConfig(result.loaded),
      ready: true,
      loadError: null,
    };
  }
  return { cfg: previous, ready: true, loadError: result.message };
}

type AppConfigState = {
  /** Current config. Starts as defaults; becomes the loaded file once `ready`. */
  cfg: AppConfig;
  /** True after the first `get_config` attempt, success or failure. */
  ready: boolean;
  /** Set when the last load failed; cleared on success or save. */
  loadError: string | null;
  save: (next: AppConfig) => Promise<void>;
  refresh: () => Promise<void>;
};

const AppConfigContext = createContext<AppConfigState | null>(null);

export function AppConfigProvider({ children }: { children: ReactNode }) {
  const [cfg, setCfg] = useState<AppConfig>(() => defaultAppConfig());
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const loaded = await api.getConfig();
      const next = applyConfigLoadResult(defaultAppConfig(), {
        ok: true,
        loaded,
      });
      setCfg(next.cfg);
      setReady(next.ready);
      setLoadError(next.loadError);
    } catch (e) {
      const next = applyConfigLoadResult(defaultAppConfig(), {
        ok: false,
        message: String(e),
      });
      setReady(next.ready);
      setLoadError(next.loadError);
    }
  }, []);

  const save = useCallback(async (next: AppConfig) => {
    await api.saveConfig(next);
    setCfg(normalizeConfig(next));
    setReady(true);
    setLoadError(null);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const value = useMemo(
    () => ({ cfg, ready, loadError, save, refresh }),
    [cfg, ready, loadError, save, refresh],
  );
  return (
    <AppConfigContext.Provider value={value}>
      {children}
    </AppConfigContext.Provider>
  );
}

export function useAppConfig(): AppConfigState {
  const ctx = useContext(AppConfigContext);
  if (!ctx) throw new Error("useAppConfig must be used within AppConfigProvider");
  return ctx;
}

type VocabState = {
  /** Term strings of everything currently in `learning` status. */
  learningTerms: string[];
  refreshLearningTerms: () => Promise<void>;
  /** Lowercased terms the learner marked as already known. */
  knownTerms: string[];
  refreshKnownTerms: () => Promise<void>;
  markKnown: (term: string) => Promise<void>;
  unmarkKnown: (term: string) => Promise<void>;
};

const VocabContext = createContext<VocabState | null>(null);

type LearningState = Pick<VocabState, "learningTerms" | "refreshLearningTerms">;
type KnownState = Pick<
  VocabState,
  "knownTerms" | "refreshKnownTerms" | "markKnown" | "unmarkKnown"
>;

const LearningContext = createContext<LearningState | null>(null);
const KnownContext = createContext<KnownState | null>(null);

export function VocabProvider({ children }: { children: ReactNode }) {
  const [learningTerms, setLearningTerms] = useState<string[]>([]);
  const [knownTerms, setKnownTerms] = useState<string[]>([]);

  const refreshLearningTerms = useCallback(async () => {
    try {
      const [words, phrases] = await Promise.all([
        api.listMemory("word", "learning"),
        api.listMemory("phrase", "learning"),
      ]);
      setLearningTerms([...words, ...phrases].map((v) => v.term));
    } catch {
      // highlight list is optional
    }
  }, []);

  const refreshKnownTerms = useCallback(async () => {
    try {
      setKnownTerms(await api.listKnownWords());
    } catch {
      // optional
    }
  }, []);

  const markKnown = useCallback(async (term: string) => {
    const key = normalizeKey(term);
    if (!key) return;
    await api.addKnownWord(key);
    setKnownTerms((prev) => (prev.includes(key) ? prev : [...prev, key]));
  }, []);

  const unmarkKnown = useCallback(async (term: string) => {
    const key = normalizeKey(term);
    await api.removeKnownWord(key);
    setKnownTerms((prev) => prev.filter((t) => t !== key));
  }, []);

  useEffect(() => {
    void refreshLearningTerms();
    void refreshKnownTerms();
  }, [refreshLearningTerms, refreshKnownTerms]);

  const value = useMemo(
    () => ({
      learningTerms,
      refreshLearningTerms,
      knownTerms,
      refreshKnownTerms,
      markKnown,
      unmarkKnown,
    }),
    [learningTerms, refreshLearningTerms, knownTerms, refreshKnownTerms, markKnown, unmarkKnown],
  );
  // Split values so learning-only consumers don't re-render on known changes
  // and vice versa. `VocabContext` stays for existing `useVocab` callers.
  const learningValue = useMemo(
    () => ({ learningTerms, refreshLearningTerms }),
    [learningTerms, refreshLearningTerms],
  );
  const knownValue = useMemo(
    () => ({ knownTerms, refreshKnownTerms, markKnown, unmarkKnown }),
    [knownTerms, refreshKnownTerms, markKnown, unmarkKnown],
  );
  return (
    <VocabContext.Provider value={value}>
      <LearningContext.Provider value={learningValue}>
        <KnownContext.Provider value={knownValue}>
          {children}
        </KnownContext.Provider>
      </LearningContext.Provider>
    </VocabContext.Provider>
  );
}

export function useVocab(): VocabState {
  const ctx = useContext(VocabContext);
  if (!ctx) throw new Error("useVocab must be used within VocabProvider");
  return ctx;
}

/** Subscribes only to learning terms — won't re-render on known changes. */
export function useLearning(): LearningState {
  const ctx = useContext(LearningContext);
  if (!ctx) throw new Error("useLearning must be used within VocabProvider");
  return ctx;
}

/** Subscribes only to known terms — won't re-render on learning changes. */
export function useKnown(): KnownState {
  const ctx = useContext(KnownContext);
  if (!ctx) throw new Error("useKnown must be used within VocabProvider");
  return ctx;
}

/**
 * Shell state shared between the global Sidebar + top-bar search and the Home
 * list: the draggable source-priority list, the tag filter (owned by the
 * sidebar so it stays visible while reading), and a rerank nonce bumped after
 * a priority change so Home reloads.
 *
 * The search box deliberately lives in its own context (see
 * [`useSearchQuery`]): it changes on every keystroke, and the sidebar has no
 * use for it — sharing one value meant re-rendering 430 lines of draggable
 * rows per character typed.
 */
type ShellState = {
  /** Feeds in priority order (highest first) — the sidebar source list. */
  feeds: FeedSource[];
  reloadFeeds: () => Promise<void>;
  /** Persist a new top-to-bottom order, then signal Home to re-rank. */
  commitFeedOrder: (orderedIds: string[]) => Promise<void>;
  /** bump after reorder → Home reloads its ranked list. */
  rerankNonce: number;

  /** Archive filter panel (source/level/read/liked) — toggled from Home. */
  filtersOpen: boolean;
  setFiltersOpen: (open: boolean) => void;

  /** Source currently focused in the main list; null = the 今日推荐 default. */
  focusSource: string | null;
  setFocusSource: (source: string | null) => void;
};

const ShellContext = createContext<ShellState | null>(null);

type FeedsState = Pick<
  ShellState,
  "feeds" | "reloadFeeds" | "commitFeedOrder" | "rerankNonce"
>;
type FilterState = Pick<
  ShellState,
  | "filtersOpen"
  | "setFiltersOpen"
  | "focusSource"
  | "setFocusSource"
>;

const FeedsContext = createContext<FeedsState | null>(null);
const FilterContext = createContext<FilterState | null>(null);

export function ShellProvider({ children }: { children: ReactNode }) {
  const [feeds, setFeeds] = useState<FeedSource[]>([]);
  const [query, setQuery] = useState("");
  const [rerankNonce, setRerankNonce] = useState(0);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [focusSource, setFocusSource] = useState<string | null>(null);

  const reloadFeeds = useCallback(async () => {
    try {
      const list = await api.listFeeds();
      // list_feeds already sorts by priority DESC; keep enabled + disabled so the
      // sidebar can grey out muted sources without a second fetch.
      setFeeds(list);
    } catch {
      // feed list is optional; Home still renders.
    }
  }, []);

  useEffect(() => {
    void reloadFeeds();
  }, [reloadFeeds]);

  // Refresh can change the feed list (enable/disable, one-shot cleanups);
  // re-read it so the sidebar stays in sync without a full app reload.
  // The shell outlives Home across routes, so it also busts the cached
  // learning stats here: a refresh that finishes while Home is unmounted
  // would otherwise leave 60s-stale stats on return.
  useEffect(() => {
    return onEvent("shiyan:refreshed", () => {
      invalidateQueries(["learning-stats"]);
      void reloadFeeds();
    });
  }, [reloadFeeds]);

  const commitFeedOrder = useCallback(
    async (orderedIds: string[]) => {
      // Optimistically reflect the new order immediately.
      setFeeds((prev) => {
        const byId = new Map(prev.map((f) => [f.id, f]));
        const next: FeedSource[] = [];
        for (const id of orderedIds) {
          const f = byId.get(id);
          if (f) {
            next.push(f);
            byId.delete(id);
          }
        }
        // Any feed not in the payload keeps relative order at the bottom.
        return [...next, ...prev.filter((f) => byId.has(f.id))];
      });
      try {
        await api.reorderFeeds(orderedIds);
        await reloadFeeds();
        setRerankNonce((n) => n + 1);
      } catch (e) {
        // Reload the persisted truth so the UI never drifts from the DB, then
        // surface the failure so the caller can explain it to the user.
        await reloadFeeds();
        throw e;
      }
    },
    [reloadFeeds],
  );

  const value = useMemo(
    () => ({
      feeds,
      reloadFeeds,
      commitFeedOrder,
      rerankNonce,
      filtersOpen,
      setFiltersOpen,
      focusSource,
      setFocusSource,
    }),
    [
      feeds,
      reloadFeeds,
      commitFeedOrder,
      rerankNonce,
      filtersOpen,
      focusSource,
      setFocusSource,
    ],
  );

  const feedsValue = useMemo(
    () => ({ feeds, reloadFeeds, commitFeedOrder, rerankNonce }),
    [feeds, reloadFeeds, commitFeedOrder, rerankNonce],
  );
  const filterValue = useMemo(
    () => ({
      filtersOpen,
      setFiltersOpen,
      focusSource,
      setFocusSource,
    }),
    [
      filtersOpen,
      focusSource,
      setFocusSource,
    ],
  );

  const queryValue = useMemo(() => ({ query, setQuery }), [query]);

  return (
    <ShellContext.Provider value={value}>
      <FeedsContext.Provider value={feedsValue}>
        <FilterContext.Provider value={filterValue}>
          <SearchQueryContext.Provider value={queryValue}>
            {children}
          </SearchQueryContext.Provider>
        </FilterContext.Provider>
      </FeedsContext.Provider>
    </ShellContext.Provider>
  );
}

export function useShell(): ShellState {
  const ctx = useContext(ShellContext);
  if (!ctx) throw new Error("useShell must be used within ShellProvider");
  return ctx;
}

/** Subscribes only to feeds/order — won't re-render on tag/filter changes. */
export function useFeeds(): FeedsState {
  const ctx = useContext(FeedsContext);
  if (!ctx) throw new Error("useFeeds must be used within ShellProvider");
  return ctx;
}

/** Subscribes only to filters/tags/focus — won't re-render on feeds changes. */
export function useFilters(): FilterState {
  const ctx = useContext(FilterContext);
  if (!ctx) throw new Error("useFilters must be used within ShellProvider");
  return ctx;
}

const SearchQueryContext = createContext<{
  query: string;
  setQuery: (q: string) => void;
} | null>(null);

/** Top-bar search text, applied by Home as a client-side filter. */
export function useSearchQuery() {
  const ctx = useContext(SearchQueryContext);
  if (!ctx) throw new Error("useSearchQuery must be used within ShellProvider");
  return ctx;
}