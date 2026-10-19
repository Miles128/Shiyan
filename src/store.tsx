import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { api, defaultAppConfig, type AppConfig } from "./api";
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
  /** Learning words only (phrases excluded). Drives the daily study gate. */
  learningWordCount: number;
  /** True after the first vocab load attempt. */
  vocabReady: boolean;
  refreshLearningTerms: () => Promise<void>;
  /** Lowercased terms the learner marked as already known. */
  knownTerms: string[];
  refreshKnownTerms: () => Promise<void>;
  markKnown: (term: string) => Promise<void>;
  unmarkKnown: (term: string) => Promise<void>;
};

const VocabContext = createContext<VocabState | null>(null);

export function VocabProvider({ children }: { children: ReactNode }) {
  const [learningTerms, setLearningTerms] = useState<string[]>([]);
  const [learningWordCount, setLearningWordCount] = useState(0);
  const [vocabReady, setVocabReady] = useState(false);
  const [knownTerms, setKnownTerms] = useState<string[]>([]);

  const refreshLearningTerms = useCallback(async () => {
    try {
      const [words, phrases] = await Promise.all([
        api.listMemory("word", "learning"),
        api.listMemory("phrase", "learning"),
      ]);
      setLearningTerms([...words, ...phrases].map((v) => v.term));
      setLearningWordCount(words.length);
    } catch {
      // highlight list is optional
    } finally {
      setVocabReady(true);
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
      learningWordCount,
      vocabReady,
      refreshLearningTerms,
      knownTerms,
      refreshKnownTerms,
      markKnown,
      unmarkKnown,
    }),
    [
      learningTerms,
      learningWordCount,
      vocabReady,
      refreshLearningTerms,
      knownTerms,
      refreshKnownTerms,
      markKnown,
      unmarkKnown,
    ],
  );
  return (
    <VocabContext.Provider value={value}>{children}</VocabContext.Provider>
  );
}

export function useVocab(): VocabState {
  const ctx = useContext(VocabContext);
  if (!ctx) throw new Error("useVocab must be used within VocabProvider");
  return ctx;
}