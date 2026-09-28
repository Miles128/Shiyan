import {
  createElement,
  Fragment,
  memo,
  useMemo,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import {
  annotateText,
  type DifficultyPrefs,
  type AnnotatedSpan,
} from "./wordLevels";

export type HardWordClick = (info: {
  term: string;
  display: string;
  zh?: string;
  clientX: number;
  clientY: number;
}) => void;

/**
 * Memoized per-paragraph annotation so only paragraphs whose text, prefs or
 * vocab changed re-run the (expensive) word-level annotation pass.
 * Every English word is clickable. `onHardClick` must be referentially stable.
 * `showGloss` reveals a small Chinese annotation under super-hard words; the
 * click-through behaviour is unchanged (still opens the LLM-context popover).
 */
export const AnnotatedPara = memo(function AnnotatedPara({
  text,
  prefs,
  learningTerms,
  knownTerms,
  onHardClick,
  showGloss = false,
}: {
  text: string;
  prefs: DifficultyPrefs;
  learningTerms: string[];
  knownTerms?: string[];
  onHardClick?: HardWordClick;
  showGloss?: boolean;
}): ReactNode {
  // Cache the expensive annotation pass: unrelated parent renders (TTS,
  // translation toggles) re-render this memo only when props change, and even
  // then identical text+prefs+vocab hits the memoized spans.
  const spans = useMemo(
    () => annotateText(text, prefs, learningTerms, knownTerms),
    [text, prefs, learningTerms, knownTerms],
  );
  const rendered = useMemo(
    () => spans.map((s, i) => renderSpan(s, i, onHardClick, showGloss)),
    [spans, onHardClick, showGloss],
  );
  return createElement(Fragment, null, ...rendered);
});

function renderSpan(
  span: AnnotatedSpan,
  key: number,
  onHardClick?: HardWordClick,
  showGloss = false,
): ReactNode {
  if (span.type === "text") return span.text;

  const isChunk = span.kind === "chunk";
  const wordClass = isChunk
    ? [
        "chunk-word",
        span.chunkType ? `chunk-${span.chunkType}` : "",
        span.hard ? "hard-word" : "plain-word",
      ]
        .filter(Boolean)
        .join(" ")
    : span.hard
      ? "hard-word"
      : "plain-word";

  const classNames = [wordClass, span.learning ? "vocab-hit" : null]
    .filter(Boolean)
    .join(" ");

  const accessibleName = [
    isChunk ? "词块 · 单击翻译" : span.hard ? "超出当前难度，单击翻译" : "单击翻译",
    span.learning ? "生词库 · 学习中" : null,
  ]
    .filter(Boolean)
    .join(" · ");

  const onClick = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    window.getSelection()?.removeAllRanges();
    onHardClick?.({
      term: span.term,
      display: span.text,
      zh: span.zh,
      clientX: e.clientX,
      clientY: e.clientY,
    });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    onHardClick?.({
      term: span.term,
      display: span.text,
      zh: span.zh,
      clientX: r.left + r.width / 2,
      clientY: r.top,
    });
  };

  // 用 aria-label 而非 title：原生 tooltip 在鼠标扫过时反复弹出，是阅读区行抖动的来源之一。
  const interactive = {
    key: `t-${key}`,
    "aria-label": accessibleName,
    role: "button" as const,
    onClick,
    onKeyDown,
  };

  const wordEl =
    span.learning && !span.hard
      ? createElement(
          "mark",
          { ...interactive, className: classNames },
          span.text,
        )
      : createElement(
          "span",
          { ...interactive, className: classNames },
          span.text,
        );

  // 词块始终挂淡色中文（chunks 是学习核心目标）；单词只在「超级难词 + 词典有中文 + 用户开了开关」时挂。
  // 点词/键盘 Enter 仍走原 LLM 上下文翻译路径，不受这里影响。
  // 释义挂在词/短语正下方（绝对定位，只占行距留白），所以开启时行高有下限。
  const wantsGloss =
    showGloss &&
    !!span.zh &&
    (isChunk || (span.superHard && span.hard));
  if (wantsGloss) {
    return createElement(
      "span",
      { key: `g-${key}`, className: "gloss-wrap" },
      wordEl,
      createElement(
        "span",
        { className: "word-gloss", "aria-hidden": true },
        span.zh,
      ),
    );
  }
  return wordEl;
}
