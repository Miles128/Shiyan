import { useLayoutEffect, useRef, useState } from "react";
import type { SpeakTarget } from "../useTts";
import type { WordDetail } from "../wordResolve";

export type Popover = {
  x: number;
  y: number;
  text: string;
  /** Raw selection (before lemma reduction) — used for context sentences. */
  source?: string;
  /** Rich local dictionary entry (senses, phonetics, examples). */
  detail?: WordDetail | null;
  /** Where the shown translation came from, for transparency. */
  origin?: "local" | "ai";
  translation?: string;
  loading?: boolean;
  error?: string;
};

type Props = {
  popover: Popover;
  speaking: boolean;
  speakTarget: SpeakTarget | null;
  onSpeakWord: (text: string) => void;
  onAddVocab: () => void;
  /** Present for multi-word selections: save into the phrase library. */
  onAddPhrase?: () => void;
  /** Present for single words: mark/unmark as already known. */
  onToggleKnown?: () => void;
  known?: boolean;
  onClose: () => void;
};

export function clampPopoverPosition(input: {
  x: number;
  y: number;
  viewW: number;
  viewH: number;
  popW?: number;
  popH?: number;
  gap?: number;
  margin?: number;
  shiftX?: number;
}): { x: number; y: number } {
  const popW = input.popW ?? 280;
  const popH = input.popH ?? 160;
  const gap = input.gap ?? 12;
  const margin = input.margin ?? 8;
  const shiftX = input.shiftX ?? 0.3;

  let left = input.x - popW * shiftX;
  let top = input.y + gap;
  const maxLeft = input.viewW - margin - popW;
  left = Math.min(Math.max(left, margin), Math.max(margin, maxLeft));
  if (top + popH > input.viewH - margin) {
    top = input.y - gap - popH;
  }
  const maxTop = input.viewH - margin - popH;
  top = Math.min(Math.max(top, margin), Math.max(margin, maxTop));
  return { x: left + popW * shiftX, y: top - gap };
}

/** Split a stored POS string ("v./n.") for display. */
export function posLabel(pos: string): string {
  return pos
    .split("/")
    .map((p) => p.trim())
    .filter(Boolean)
    .join(" · ");
}

/** Floating panel shown after selecting / clicking a word. */
export default function SelectionPopover({
  popover,
  speaking,
  speakTarget,
  onSpeakWord,
  onAddVocab,
  onAddPhrase,
  onToggleKnown,
  known,
  onClose,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 280, h: 160 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.focus();
  }, []);
  useLayoutEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const el = ref.current;
      if (!el) return;
      const focusables = el.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === first || active === el)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setBox((prev) =>
      Math.abs(prev.w - width) < 1 && Math.abs(prev.h - height) < 1
        ? prev
        : { w: width, h: height },
    );
  }, [popover.text, popover.translation, popover.loading, popover.error]);
  const pos = clampPopoverPosition({
    x: popover.x,
    y: popover.y,
    viewW: typeof window === "undefined" ? 800 : window.innerWidth,
    viewH: typeof window === "undefined" ? 600 : window.innerHeight,
    popW: box.w,
    popH: box.h,
  });
  return (
    <div
      ref={ref}
      className="selection-pop"
      role="dialog"
      aria-modal="true"
      aria-label={popover.text}
      tabIndex={-1}
      style={{ left: pos.x, top: pos.y + 12 }}
    >
      <div className="pop-head">
        <div className="pop-term">
          {popover.text}
          {popover.detail?.phonetic ? (
            <span className="pop-phonetic">/ {popover.detail.phonetic} /</span>
          ) : null}
          {popover.detail?.pos ? (
            <span className="pop-pos">{posLabel(popover.detail.pos)}</span>
          ) : null}
        </div>
        <div className="pop-icons">
          <button
            className={`pop-icon-btn${speaking && speakTarget?.kind === "word" ? " active" : ""}`}
            type="button"
            title={speaking && speakTarget?.kind === "word" ? "停止朗读" : "朗读"}
            aria-label={speaking && speakTarget?.kind === "word" ? "停止朗读" : "朗读"}
            onClick={() => onSpeakWord(popover.text)}
          >
            {speaking && speakTarget?.kind === "word" ? (
              <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true">
                <rect x="5" y="5" width="14" height="14" rx="2" fill="currentColor" />
              </svg>
            ) : (
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <polygon points="11 5 6 9 3 9 3 15 6 15 11 19 11 5" fill="currentColor" stroke="none" />
                <path d="M15.5 8.5a5 5 0 0 1 0 7" />
                <path d="M18.5 5.5a9.5 9.5 0 0 1 0 13" />
              </svg>
            )}
          </button>
          <button
            className="pop-icon-btn"
            type="button"
            title="关闭"
            aria-label="关闭"
            onClick={onClose}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <line x1="6" y1="6" x2="18" y2="18" />
              <line x1="18" y1="6" x2="6" y2="18" />
            </svg>
          </button>
        </div>
      </div>
      {popover.loading && <div className="muted">翻译中…</div>}
      {popover.error && <div className="err-inline">{popover.error}</div>}

      {popover.detail && popover.detail.senses.length > 0 ? (
        <>
          <ol className="pop-senses">
            <li>{popover.detail.senses[0]}</li>
          </ol>
          {popover.detail.senses.length > 1 && (
            <details className="pop-more">
              <summary>更多释义（{popover.detail.senses.length - 1}）</summary>
              <ol className="pop-senses">
                {popover.detail.senses.slice(1, 4).map((sense, i) => (
                  <li key={i}>{sense}</li>
                ))}
              </ol>
            </details>
          )}
        </>
      ) : popover.translation ? (
        <div className="pop-zh">{popover.translation}</div>
      ) : null}

      {popover.detail && popover.detail.examples.length > 0 && (
        <details className="pop-more">
          <summary>例句（{popover.detail.examples.length}）</summary>
          <div className="pop-examples">
            {popover.detail.examples.map((ex, i) => (
              <div className="pop-example" key={i}>
                <p className="pop-ex-en">{ex.en}</p>
                {ex.zh ? <p className="pop-ex-zh">{ex.zh}</p> : null}
              </div>
            ))}
          </div>
        </details>
      )}

      {popover.origin && (
        <div
          className="pop-origin muted"
          title={popover.origin === "local" ? "内置词典 ECDICT / Tatoeba" : "AI 翻译"}
        >
          {popover.origin === "local" ? "词典" : "AI"}
        </div>
      )}
      <div className="pop-actions">
        <button className="pop-link primary" onClick={onAddVocab}>
          加入生词库
        </button>
        {onAddPhrase ? (
          <button className="pop-link primary" onClick={onAddPhrase}>
            加入短语
          </button>
        ) : null}
        {onToggleKnown ? (
          <button className="pop-link" onClick={onToggleKnown}>
            {known ? "取消已认识" : "标为已认识"}
          </button>
        ) : null}
      </div>
    </div>
  );
}