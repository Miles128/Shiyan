import { useCallback, useEffect, useRef } from "react";
import { api } from "./api";
import { invalidateQueries } from "./query";
import { loadScroll, saveScroll } from "./useArticle";

type ReadingView = "loading" | "ready" | "error" | "missing";

type Args = {
  id: string | undefined;
  /** Reader payload readiness; gates bottom detection and scroll restore. */
  view: ReadingView;
  /** Fired once per article when the reader reaches the bottom (toast UI). */
  onComplete: () => void;
};

/**
 * Reading-session bookkeeping, extracted from Reader: accumulated
 * visible+focused dwell time (feeds stats and interest ranking), read-to-the-
 * end detection, and scroll position memory. Owns its refs; the reader only
 * supplies the article id and a completion callback.
 */
export function useReadingSession({ id, view, onComplete }: Args): void {
  const readCompletedRef = useRef(false);
  const atBottomRef = useRef(false);
  const onCompleteRef = useRef(onComplete);
  useEffect(() => {
    onCompleteRef.current = onComplete;
  });

  /**
   * Finish = reached the bottom. Dwell still accrues for stats/ranking, but
   * it no longer gates the read flag — the old words/200-minutes threshold
   * meant fast readers never got marked 已读 at all.
   */
  const tryComplete = useCallback(() => {
    if (!id || readCompletedRef.current) return;
    if (!atBottomRef.current) return;
    readCompletedRef.current = true;
    // Read-state feeds the Home filters: bust the list window so back-nav
    // paints fresh instead of a stale row that the revalidate then yanks.
    // (Dwell-only flushes below skip this — churn, and they self-heal.)
    void api
      .markArticleProgress(id, 0, true)
      .then(() => invalidateQueries(["articles"]))
      .catch(() => undefined);
    onCompleteRef.current();
  }, [id]);

  // Reading-time tracking: flush while the window is visible AND focused,
  // on losing focus/hiding, and on unmount. Capped per flush so sleep/resume
  // can't inflate it.
  useEffect(() => {
    if (!id) return;
    const dwellMsRef = { current: 0 };
    atBottomRef.current = false;
    readCompletedRef.current = false;
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
      document.removeEventListener("blur", flushIfNotCounting);
      window.removeEventListener("focus", onFocus);
      if (!document.hidden && document.hasFocus()) flush();
    };
  }, [id, tryComplete]);

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

  // Read-to-the-end detection + scroll memory. Gated on the body being on
  // screen: while the article is still loading the document is short, so
  // `scrollHeight - 400` is already under the fold — an ungated check
  // self-completes on mount, latches, and marks every opened article read
  // before it is actually read. Re-runs when `view` flips to ready.
  useEffect(() => {
    if (!id) return;
    atBottomRef.current = false;
    readCompletedRef.current = false;
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
}
