import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import type { WordPopoverShellProps } from "./components/WordPopoverShell";
import type { Popover } from "./components/SelectionPopover";
import type { SpeakTarget } from "./useTts";
import { useEscapeKey } from "./useEscapeKey";
import { createKnownToggle } from "./knownWords";
import { ensureDetailsLoaded, lookupDetail, prepareLookup } from "./wordResolve";

type TtsState = {
  speaking: boolean;
  speakTarget: SpeakTarget | null;
  /** Returns false when the engine is unavailable (caller shows feedback). */
  startSpeak: (target: SpeakTarget, chunks: string[]) => boolean;
  stopSpeak: () => void;
};

export type WordPopoverConfig = {
  /** Article the selection belongs to; null on list pages. */
  articleId: string | null;
  tts: TtsState;
  /** Page-specific local gloss (the reader layers in its CEFR lexicon). */
  localGloss?: (term: string) => string | undefined;
  /** Translate a term (list pages = plain text, reader = per-article cache). */
  translate: (term: string) => Promise<string>;
  /** Sentence stored alongside a saved word/phrase. */
  contextFor: (source: string | undefined, term: string) => string;
  onError: (message: string) => void;
  /** Success feedback (e.g. 已加入生词库); hosts usually route this to useToast(). */
  onSuccess?: (message: string) => void;
  onVocabAdded?: () => void;
  /** 已认识词库与其增删：浮窗的「认识 / 不认识」开关，两个页面同一份。 */
  knownTerms: string[];
  markKnown: (term: string) => Promise<void>;
  unmarkKnown: (term: string) => Promise<void>;
};

/**
 * Shared selection-to-translation popover: local dictionary → bundled gloss →
 * LLM, plus save-to-vocab / save-to-phrase. Used by Home and Reader.
 */
export function useWordPopover(config: WordPopoverConfig) {
  const [popover, setPopover] = useState<Popover | null>(null);
  // Latest config in a ref so the returned callbacks stay stable.
  const cfg = useRef(config);
  useEffect(() => {
    cfg.current = config;
  }, [config]);

  const closePopover = useCallback(() => setPopover(null), []);
  useEscapeKey(popover != null, closePopover);

  const showMeaning = useCallback(
    async (opts: { text: string; x: number; y: number; bundledZh?: string }) => {
      const { text, x, y, bundledZh } = opts;
      const { term: ruleTerm, source } = prepareLookup(text);
      let detail: ReturnType<typeof lookupDetail> = null;
      try {
        await ensureDetailsLoaded();
        detail = lookupDetail(source) ?? lookupDetail(ruleTerm);
      } catch {
        // details are optional — fall through to the local gloss / LLM
      }
      const term = detail?.lemma || ruleTerm;
      // Record the lookup (fire-and-forget): reviewable history for the
      // Vocab page. Covers dictionary hits and AI translations alike.
      void api
        .recordLookup(term, cfg.current.contextFor(source, term), cfg.current.articleId)
        .catch(() => undefined);
      if (detail) {
        setPopover({ x, y, text: term, source, detail, origin: "local", loading: false });
        return;
      }
      const fromLocal = bundledZh || cfg.current.localGloss?.(term);
      if (fromLocal) {
        setPopover({
          x,
          y,
          text: term,
          source,
          translation: fromLocal,
          origin: "local",
          loading: false,
        });
        return;
      }
      setPopover({ x, y, text: term, source, loading: true });
      try {
        const translated = await cfg.current.translate(term);
        setPopover((p) =>
          p && p.text === term
            ? { ...p, translation: translated, origin: "ai" as const, loading: false }
            : p,
        );
      } catch (err) {
        setPopover((p) =>
          p && p.text === term ? { ...p, error: String(err), loading: false } : p,
        );
      }
    },
    [],
  );

  const speakWord = useCallback((text: string) => {
    const { speaking, speakTarget, startSpeak, stopSpeak } = cfg.current.tts;
    if (speaking && speakTarget?.kind === "word") {
      stopSpeak();
      return;
    }
    if (!text.trim()) return;
    if (!startSpeak({ kind: "word" }, [text])) {
      cfg.current.onError("当前环境无语音引擎，无法朗读");
    }
  }, []);

  const addToVocab = useCallback(async () => {
    if (!popover) return;
    const c = cfg.current;
    try {
      await api.addMemory({
        kind: "word",
        term: popover.text,
        contextSentence: c.contextFor(popover.source, popover.text),
        articleId: c.articleId,
        definitionZh: popover.translation ?? null,
      });
      c.onSuccess?.(`已加入生词库：${popover.text}`);
      setPopover(null);
      c.onVocabAdded?.();
    } catch (e) {
      c.onError(String(e));
    }
  }, [popover]);

  const addToPhrase = useCallback(async () => {
    if (!popover) return;
    const c = cfg.current;
    try {
      await api.addMemory({
        kind: "phrase",
        term: popover.text,
        contextSentence: c.contextFor(popover.source, popover.text),
        articleId: c.articleId,
      });
      c.onSuccess?.(`已加入短语：${popover.text}`);
      setPopover(null);
    } catch (e) {
      c.onError(String(e));
    }
  }, [popover]);

  // Both pages keep their own popover wiring alive: one shared 已认识 toggle
  // and one prop bundle ready to spread into the shell, so a new host cannot
  // forget one of the nine callbacks.
  const { knownTerms, markKnown, unmarkKnown } = config;
  const toggleKnown = useMemo(
    () =>
      createKnownToggle({
        knownTerms,
        markKnown,
        unmarkKnown,
        onError: (m) => cfg.current.onError(m),
      }),
    [knownTerms, markKnown, unmarkKnown],
  );

  const mount: WordPopoverShellProps | null = popover
    ? {
        popover,
        speaking: config.tts.speaking,
        speakTarget: config.tts.speakTarget,
        knownTerms,
        onSpeakWord: speakWord,
        onAddVocab: () => void addToVocab(),
        onAddPhrase: () => void addToPhrase(),
        onToggleKnown: (term) => void toggleKnown(term),
        onClose: closePopover,
      }
    : null;

  return {
    popover,
    setPopover,
    closePopover,
    showMeaning,
    speakWord,
    addToVocab,
    addToPhrase,
    mount,
  };
}
