import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import { useNavigate, useParams } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api, type FeedCategory, type TranslateProgress } from "../api";
import { invalidateQueries } from "../query";
import {
  readingCssVars,
  resolveReadingPrefs,
  type ResolvedReading,
} from "../readingPrefs";
import { AnnotatedPara } from "../annotateText";
import { bundledGloss, rememberTranslation } from "../wordResolve";
import WordPopoverShell from "../components/WordPopoverShell";
import ReaderTypePanel from "../components/ReaderTypePanel";
import ReaderParagraph from "../components/ReaderParagraph";
import { useEscapeKey } from "../useEscapeKey";
import { useAppConfig, useVocab } from "../store";
import { useToast } from "../components/Toaster";
import { loadScroll, rememberLastArticle, saveScroll, useArticle } from "../useArticle";
import { useTts } from "../useTts";
import { useWordPopover } from "../useWordPopover";
import {
  applyTranslateProgress,
  categoryLabel,
  findContext,
  shouldRenderMarkdown,
  translateProgressLabel,
} from "../readerUtils";
import {
  ensureChunksLoaded,
  ensureLexiconLoaded,
  isCefrLevel,
  isFreqBand,
  lookupWord,
  type DifficultyPrefs,
} from "../wordLevels";

/** Short stable hash for paragraph keys (avoids index-only key reuse). */
function paraKey(id: string | undefined, index: number, text: string): string {
  let h = 0;
  for (let i = 0; i < Math.min(text.length, 64); i++) {
    h = (h * 31 + text.charCodeAt(i)) | 0;
  }
  return `${id ?? "noid"}:${index}:${h.toString(36)}`;
}

function IconBack() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M19 12H5" />
      <path d="m12 19-7-7 7-7" />
    </svg>
  );
}

function IconStar() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26" />
    </svg>
  );
}

function IconStarFilled() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26" />
    </svg>
  );
}

function IconVolume() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M11 5 6 9H2v6h4l5 4z" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
      <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
    </svg>
  );
}

function IconStop() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </svg>
  );
}

function IconTranslate() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="m5 8 6 6" />
      <path d="m4 14 6-6 2-3" />
      <path d="M2 5h12" />
      <path d="M7 2h1" />
      <path d="m22 22-5-10-5 10" />
      <path d="M14 18h6" />
    </svg>
  );
}

function IconEyeOff() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M9.88 9.88a3 3 0 1 0 4.24 4.24" />
      <path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" />
      <path d="M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" />
      <line x1="2" x2="22" y1="2" y2="22" />
    </svg>
  );
}

export default function Reader() {
  const { id } = useParams();
  const navigate = useNavigate();
  const {
    article,
    paragraphs,
    translations,
    setTranslations,
    error,
    setError,
    view,
  } = useArticle(id);
  const [lexReady, setLexReady] = useState(false);
  const [showFullZh, setShowFullZh] = useState(false);
  const [visibleParas, setVisibleParas] = useState<Record<number, boolean>>({});
  const [busyFull, setBusyFull] = useState(false);
  const [fullProgress, setFullProgress] = useState<TranslateProgress | null>(null);
  const [busyPara, setBusyPara] = useState<number | null>(null);
  const [categories, setCategories] = useState<FeedCategory[]>([]);
  const [likedOverride, setLikedOverride] = useState<boolean | null>(null);
  const [typeOpen, setTypeOpen] = useState(false);
  const [showDone, setShowDone] = useState(false);
  const typePanelRef = useRef<HTMLDivElement | null>(null);
  const typeBtnRef = useRef<HTMLButtonElement | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const clickGuardRef = useRef(false);
  const readCompletedRef = useRef(false);
  /** Accumulated visible+focused dwell, feeds stats and interest ranking. */
  const dwellMsRef = useRef(0);
  const atBottomRef = useRef(false);

  const tts = useTts();
  const wordToast = useToast();
  const { speaking, speakTarget, startSpeak, stopSpeak } = tts;
  const { cfg } = useAppConfig();
  const {
    learningTerms: vocabTerms,
    knownTerms,
    refreshLearningTerms,
    markKnown,
    unmarkKnown,
  } = useVocab();
  const prefs: DifficultyPrefs = useMemo(
    () => ({
      cefrLevel: isCefrLevel(cfg.cefr_level) ? cfg.cefr_level : "B1",
      freqBand: isFreqBand(cfg.freq_band) ? cfg.freq_band : 3000,
    }),
    [cfg.cefr_level, cfg.freq_band],
  );
  const reading: ResolvedReading = useMemo(() => resolveReadingPrefs(cfg), [cfg]);
  const { setPopover, showMeaning, mount: popoverMount } = useWordPopover({
    articleId: id ?? null,
    tts,
    localGloss: (term) => lookupWord(term)?.zh ?? bundledGloss(term),
    translate: async (term) => {
      if (!id) throw new Error("文章未加载");
      const row = await api.translateSelection(id, term);
      // Feed the shared session cache so the Home list (and re-selects)
      // reuse this translation without another LLM call.
      rememberTranslation(term, row.translated_text);
      return row.translated_text;
    },
    contextFor: (source, term) => findContext(paragraphs, source ?? term),
    onError: (m) => setError(m),
    onSuccess: (m) => wordToast.ok(m),
    onVocabAdded: () => void refreshLearningTerms(),
    knownTerms,
    markKnown,
    unmarkKnown,
  });

  useEffect(() => {
    Promise.all([ensureLexiconLoaded(), ensureChunksLoaded()])
      .then(() => setLexReady(true))
      .catch(() => setLexReady(true));
    void api.listFeedCategories().then(setCategories).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<TranslateProgress>("translate-progress", (event) => {
      const next = event.payload;
      if (next.article_id !== id) return;
      setTranslations((map) => applyTranslateProgress(map, next));
      if (next.done) {
        setFullProgress(null);
        return;
      }
      setFullProgress(next);
      setShowFullZh(true);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [id, setTranslations]);

  useEffect(() => {
    function onDocMouseDown(e: globalThis.MouseEvent) {
      if (!rootRef.current?.contains(e.target as Node)) {
        setPopover(null);
      }
      // Close the typography panel on any click outside it and its toggle.
      if (
        !typePanelRef.current?.contains(e.target as Node) &&
        !typeBtnRef.current?.contains(e.target as Node)
      ) {
        setTypeOpen(false);
      }
    }
    document.addEventListener("mousedown", onDocMouseDown);
    return () => document.removeEventListener("mousedown", onDocMouseDown);
  }, [setPopover]);

  useEscapeKey(typeOpen, () => setTypeOpen(false));

  const title = useMemo(() => article?.title ?? "阅读", [article]);
  const liked = likedOverride ?? article?.liked ?? false;

  /** Finish = reached the bottom. Dwell still accrues for stats/ranking,
   *  but it no longer gates the read flag — the old words/200-minutes
   *  threshold meant fast readers never got marked 已读 at all. */
  const tryComplete = useCallback(() => {
    if (!id || readCompletedRef.current) return;
    if (!atBottomRef.current) return;
    readCompletedRef.current = true;
    setShowDone(true);
    // Read-state feeds the Home filters: bust the list window so back-nav
    // paints fresh instead of a stale row that the revalidate then yanks.
    // (Dwell-only flushes below skip this — churn, and they self-heal.)
    void api
      .markArticleProgress(id, 0, true)
      .then(() => invalidateQueries(["articles"]))
      .catch(() => undefined);
  }, [id]);

  // 读完提示播完即撤；切文章时清掉，避免把上一篇的残影带过来。
  useEffect(() => {
    if (!showDone) return;
    const t = window.setTimeout(() => setShowDone(false), 2800);
    return () => window.clearTimeout(t);
  }, [showDone]);

  // Reading-time tracking: flush while the window is visible AND focused,
  // on losing focus/hiding, and on unmount. Capped per flush so sleep/resume
  // can't inflate it.
  useEffect(() => {
    if (!id) return;
    dwellMsRef.current = 0;
    atBottomRef.current = false;
    let flushedAt = Date.now();
    const flush = () => {
      const now = Date.now();
      const delta = Math.min(now - flushedAt, 60_000);
      flushedAt = now;
      if (delta >= 1000) {
        dwellMsRef.current += delta;
        void api
          .markArticleProgress(id, delta, readCompletedRef.current)
          .catch(() => undefined);
        tryComplete();
      }
    };
    const isCounting = () => !document.hidden && document.hasFocus();
    const flushIfCounting = () => {
      if (isCounting()) flush();
    };
    const flushIfNotCounting = () => {
      if (!isCounting()) flush();
    };
    const timer = window.setInterval(flushIfCounting, 15_000);
    document.addEventListener("visibilitychange", flushIfNotCounting);
    window.addEventListener("blur", flushIfNotCounting);
    const onFocus = () => {
      flushedAt = Date.now();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", flushIfNotCounting);
      window.removeEventListener("blur", flushIfNotCounting);
      window.removeEventListener("focus", onFocus);
      if (!document.hidden && document.hasFocus()) flush();
    };
  }, [id, tryComplete]);

  // Remember the article so other pages can offer "continue reading".
  useEffect(() => {
    if (id) rememberLastArticle(id);
  }, [id]);

  // Restore the previous scroll offset once the body is on screen.
  const restoredRef = useRef<string | null>(null);
  useEffect(() => {
    if (!id || view !== "ready" || restoredRef.current === id) return;
    restoredRef.current = id;
    const y = loadScroll(id);
    if (y && y > 0) {
      requestAnimationFrame(() => window.scrollTo({ top: y, behavior: "auto" }));
    }
  }, [id, view]);

  // Read-to-the-end detection + scroll memory.
  useEffect(() => {
    if (!id) return;
    readCompletedRef.current = false;
    atBottomRef.current = false;
    setLikedOverride(null);
    setShowDone(false);
    // Reset translation-visibility state when navigating to a different article.
    setShowFullZh(false);
    setVisibleParas({});
    setBusyPara(null);
    setFullProgress(null);
    // Gate detection on the body being on screen. While the article is still
    // loading the document is short, so `scrollHeight - 400` is already under
    // the fold: an ungated check self-completes on mount, latches, and marks
    // every opened article read (and fires the toast invisibly) before it is
    // actually read. Re-runs when `view` flips to ready, which re-arms it.
    if (view !== "ready") return;
    let lastSaved = 0;
    const onScroll = () => {
      const now = Date.now();
      if (now - lastSaved > 500) {
        lastSaved = now;
        saveScroll(id, window.scrollY);
      }
      const doc = document.documentElement;
      atBottomRef.current =
        window.innerHeight + window.scrollY >= doc.scrollHeight - 400;
      if (atBottomRef.current) tryComplete();
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => {
      window.removeEventListener("scroll", onScroll);
      saveScroll(id, window.scrollY);
    };
  }, [id, tryComplete, view]);

  function toggleLiked() {
    if (!id) return;
    const next = !(likedOverride ?? article?.liked ?? false);
    setLikedOverride(next);
    // Same story as tryComplete: the ★ flag lives in the cached list rows.
    api
      .setArticleLiked(id, next)
      .then(() => invalidateQueries(["articles"]))
      .catch(() => setLikedOverride(null));
  }

  const asMarkdown = useMemo(() => {
    if (!article) return false;
    const body = paragraphs.length > 0 ? paragraphs.join("\n\n") : article.content_text;
    return shouldRenderMarkdown(article.url, body);
  }, [article, paragraphs]);

  function speakArticle() {
    if (speaking && speakTarget?.kind === "article") {
      stopSpeak();
      return;
    }
    if (paragraphs.length === 0) return;
    if (!startSpeak({ kind: "article" }, paragraphs)) {
      wordToast.err("当前环境无语音引擎，无法朗读");
    }
  }

  function speakParagraph(index: number) {
    if (speaking && speakTarget?.kind === "paragraph" && speakTarget.index === index) {
      stopSpeak();
      return;
    }
    const text = paragraphs[index];
    if (!text) return;
    if (!startSpeak({ kind: "paragraph", index }, [text])) {
      wordToast.err("当前环境无语音引擎，无法朗读");
    }
  }

  async function translatePara(index: number) {
    if (!id) return;
    if (visibleParas[index] && translations[String(index)]) {
      setVisibleParas((v) => ({ ...v, [index]: false }));
      return;
    }
    if (translations[String(index)]) {
      setVisibleParas((v) => ({ ...v, [index]: true }));
      return;
    }
    setBusyPara(index);
    setError(null);
    try {
      const row = await api.translateParagraph(id, index, paragraphs[index]);
      setTranslations((t) => ({ ...t, [String(index)]: row.translated_text }));
      setVisibleParas((v) => ({ ...v, [index]: true }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyPara(null);
    }
  }

  // Stable per-paragraph handlers so `ReaderParagraph(memo)` only re-renders
  // the paragraph whose props actually changed (TTS/translation toggles no
  // longer invalidate the whole article via inline closures).
  const handleTranslatePara = useCallback(
    (index: number) => void translatePara(index),
    // translatePara reads latest id/paragraphs/translations via closure;
    // re-created when those change, stable across TTS/speaking renders.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [id, paragraphs, translations, visibleParas],
  );
  const handleSpeakParagraph = useCallback(
    (index: number) => speakParagraph(index),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [speaking, speakTarget, paragraphs, startSpeak, stopSpeak],
  );

  const onHardWordClick = useCallback(
    function onHardWordClick(info: {
      term: string;
      display: string;
      zh?: string;
      clientX: number;
      clientY: number;
    }) {
      clickGuardRef.current = true;
      void showMeaning({
        text: info.term,
        x: info.clientX || 80,
        y: info.clientY || 120,
        bundledZh: info.zh,
      });
    },
    [showMeaning],
  );

  const annotateChildren = useCallback(
    (children: ReactNode): ReactNode => {
      if (!lexReady) return children;
      const annotate = (text: string, key?: number) => (
        <AnnotatedPara
          key={key}
          text={text}
          prefs={prefs}
          learningTerms={vocabTerms}
          knownTerms={knownTerms}
          onHardClick={onHardWordClick}
          showGloss={cfg.show_hard_word_gloss}
        />
      );
      if (typeof children === "string") return annotate(children);
      if (Array.isArray(children)) {
        return children.map((child, i) =>
          typeof child === "string" ? annotate(child, i) : child,
        );
      }
      return children;
    },
    [lexReady, prefs, vocabTerms, knownTerms, onHardWordClick, cfg.show_hard_word_gloss],
  );

  async function toggleFullTranslation() {
    if (!id) return;
    if (showFullZh) {
      setShowFullZh(false);
      return;
    }
    setBusyFull(true);
    setError(null);
    try {
      const result = await api.translateFullArticle(id);
      setTranslations((prev) => {
        const map: Record<string, string> = { ...prev };
        result.rows.forEach((r) => {
          map[r.scope_key] = r.translated_text;
        });
        return map;
      });
      if (result.errors.length > 0) {
        setError(`部分段落翻译失败（${result.errors.length} 段），其余译文已就绪。`);
      }
      setShowFullZh(true);
      const all: Record<number, boolean> = {};
      paragraphs.forEach((_, i) => {
        all[i] = true;
      });
      setVisibleParas(all);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyFull(false);
    }
  }

  async function onMouseUp(e: MouseEvent) {
    if (clickGuardRef.current) {
      clickGuardRef.current = false;
      return;
    }
    const sel = window.getSelection();
    const text = sel?.toString().trim() ?? "";
    if (!text) {
      setPopover(null);
      return;
    }
    if (text.length > 120) {
      // 选中整段走段落 gutter 的译按钮；浮窗只做词/短语。
      setPopover({
        x: e.clientX,
        y: e.clientY,
        text: text.slice(0, 20) + "…",
        source: text,
        loading: false,
        error: "所选超过 120 字，请用段落左侧的 译 按钮翻译整段。",
      });
      return;
    }
    await showMeaning({ text, x: e.clientX, y: e.clientY });
  }

  if (view !== "ready" || !article) {
    return (
      <div className="page">
        {view === "error" && error ? (
          <p className="banner err">{error}</p>
        ) : view === "missing" ? (
          <p className="muted">找不到这篇文章。</p>
        ) : (
          <p className="muted">加载中…</p>
        )}
      </div>
    );
  }

  const articleSpeaking = speaking && speakTarget?.kind === "article";

  return (
    <div
      className={`page reader${reading.fullWidth ? " reader-full" : ""}`}
      ref={rootRef}
      style={readingCssVars(reading, cfg.show_hard_word_gloss)}
    >

      {error && <p className="banner err">{error}</p>}
      {articleSpeaking && (
        <p className="muted" role="status">
          正在朗读全文，再次点击标题栏朗读按钮停止。
        </p>
      )}

      <article className="article-body" onMouseUp={onMouseUp}>
        <div className="reader-title-row">
          <h1>
            {lexReady ? (
              <AnnotatedPara
                text={title}
                prefs={prefs}
                learningTerms={vocabTerms}
                knownTerms={knownTerms}
                onHardClick={onHardWordClick}
              />
            ) : (
              title
            )}
          </h1>
          <div className="page-header-actions">
            <button
              className="icon-btn"
              type="button"
              onClick={() => navigate("/")}
              title="返回主界面"
              aria-label="返回主界面"
            >
              <IconBack />
            </button>
            <button
              className="icon-btn"
              type="button"
              onClick={toggleLiked}
              title={liked ? "取消收藏，之后不再优先推荐同类文章" : "收藏，之后优先推荐同类文章"}
              aria-label={liked ? "取消收藏" : "收藏"}
              aria-pressed={liked}
            >
              {liked ? <IconStarFilled /> : <IconStar />}
            </button>
            <button
              className="icon-btn"
              type="button"
              onClick={speakArticle}
              disabled={paragraphs.length === 0}
              title={articleSpeaking ? "停止朗读" : "朗读全文"}
              aria-label={articleSpeaking ? "停止朗读" : "朗读全文"}
            >
              {articleSpeaking ? <IconStop /> : <IconVolume />}
            </button>
            {busyFull ? (
              <button
                className="icon-btn spin"
                type="button"
                disabled
                aria-busy="true"
                title={translateProgressLabel(fullProgress) ?? "正在翻译…"}
                aria-label={translateProgressLabel(fullProgress) ?? "正在翻译"}
              >
                <IconTranslate />
              </button>
            ) : (
              <button
                className="icon-btn"
                type="button"
                onClick={() => void toggleFullTranslation()}
                title={showFullZh ? "隐藏译文" : "全文翻译"}
                aria-label={showFullZh ? "隐藏译文" : "全文翻译"}
                aria-pressed={showFullZh}
              >
                {showFullZh ? <IconEyeOff /> : <IconTranslate />}
              </button>
            )}
            <button
              ref={typeBtnRef}
              className={`icon-btn type-toggle${typeOpen ? " active" : ""}`}
              type="button"
              onClick={() => setTypeOpen((v) => !v)}
              title="排版设置"
              aria-label="排版设置"
              aria-expanded={typeOpen}
            >
              Aa
            </button>
            {typeOpen && (
              <div ref={typePanelRef} className="type-panel-anchor">
                <ReaderTypePanel />
              </div>
            )}
          </div>
        </div>
        <div className="reader-heading">
          {article.summary_zh && (
            <p className="article-summary-zh">{article.summary_zh}</p>
          )}
          <p className="muted">
            {article.source} · {categoryLabel(article.category, categories)} · 难度{" "}
            {prefs.cefrLevel} / {prefs.freqBand / 1000}k
          </p>
        </div>
        {paragraphs.map((p, i) => (
          <ReaderParagraph
            key={paraKey(id, i, p)}
            text={p}
            paraIndex={i}
            asMarkdown={asMarkdown}
            annotateChildren={annotateChildren}
            zhVisible={!!((showFullZh || visibleParas[i]) && translations[String(i)])}
            zhText={translations[String(i)]}
            translating={busyPara === i}
            visiblePara={!!visibleParas[i]}
            paraSpeaking={
              !!(speaking && speakTarget?.kind === "paragraph" && speakTarget.index === i)
            }
            onTranslate={handleTranslatePara}
            onSpeak={handleSpeakParagraph}
          />
        ))}
      </article>

      <p className="muted source-link">
        原文：{" "}
        {article.url.startsWith("file://") ? (
          <span>本地导入 · {article.title}</span>
        ) : (
          <a href={article.url} target="_blank" rel="noreferrer">
            {article.url}
          </a>
        )}
      </p>

      {showDone && (
        <div className="read-done" role="status" aria-live="polite">
          已看完
        </div>
      )}

      {popoverMount && <WordPopoverShell {...popoverMount} />}
    </div>
  );
}
