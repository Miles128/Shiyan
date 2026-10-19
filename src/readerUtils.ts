import type { TranslateProgress } from "./api";

// 阅读器共享工具：分类标签、上下文截取、翻译进度合并 + Markdown 判定
// （原 markdown.ts，仅 Reader 消费）。

export type CategoryRef = { id: string; label: string };

/** Prefer the live feed-category list; fall back to the raw id. */
export function categoryLabel(
  id: string,
  categories: readonly CategoryRef[],
): string {
  return categories.find((c) => c.id === id)?.label ?? id;
}

/** A short reading-window around `term`: sentence-bounded when possible,
 * hard-clipped to ~`max` chars otherwise. Examples stay scannable. */
export function clipContext(sentence: string, max: number = 140): string {
  const s = sentence.replace(/\s+/g, " ").trim();
  if (s.length <= max) return s;
  const slice = s.slice(0, max);
  // Cut at the last sentence end inside the window instead of mid-word.
  const cut = Math.max(
    slice.lastIndexOf(". "),
    slice.lastIndexOf("! "),
    slice.lastIndexOf("? "),
  );
  return (cut > max * 0.5 ? slice.slice(0, cut + 1) : slice.trimEnd()) + "…";
}

/** Short context window containing `term`, or the term itself. */
export function findContext(paragraphs: string[], term: string): string {
  const lower = term.toLowerCase();
  const hit = paragraphs.find((p) => p.toLowerCase().includes(lower));
  if (!hit) return term;
  const at = hit.toLowerCase().indexOf(lower);
  if (at < 0) return clipContext(hit);
  const start = Math.max(0, at - 60);
  const end = Math.min(hit.length, at + term.length + 60);
  const window = hit.slice(start, end);
  const clipped = clipContext(window, 140);
  // Trailing ellipsis when the window stops mid-paragraph, unless clipping
  // already added one.
  const tail =
    end < hit.length && !clipped.endsWith("…") ? "…" : "";
  return (start > 0 ? "…" : "") + clipped + tail;
}

/** Merge one streamed paragraph; ignore the terminal `done` event. */
export function applyTranslateProgress(
  map: Record<string, string>,
  progress: TranslateProgress,
): Record<string, string> {
  if (progress.done || !progress.scope_key) return map;
  return { ...map, [progress.scope_key]: progress.translated_text };
}

export function translateProgressLabel(
  progress: TranslateProgress | null,
): string | null {
  if (!progress || progress.done) return null;
  return `翻译中 ${progress.current}/${progress.total}`;
}

// --- Markdown 判定（原 markdown.ts）---

const MD_EXT = /\.(md|markdown)(?:$|[?#])/i;

const MD_HEURISTICS: RegExp[] = [
  /^#{1,6}\s+\S/m, // ATX headings
  /^(\s{0,3}[-*+]|\s{0,3}\d+\.)\s+\S/m, // lists
  /^```/m, // fenced code
  /\[[^\]]+\]\([^)]+\)/, // links
  /(\*\*|__).+?\1/, // bold
  /(?:^|[^*])\*[^*\n]+\*(?:[^*]|$)/, // italic *...*
];

export function urlLooksLikeMarkdown(url: string): boolean {
  try {
    const path = new URL(url).pathname;
    return MD_EXT.test(path);
  } catch {
    return MD_EXT.test(url);
  }
}

/** True if at least two distinct Markdown signals appear in the text. */
function contentLooksLikeMarkdown(text: string): boolean {
  let hits = 0;
  for (const re of MD_HEURISTICS) {
    if (re.test(text)) {
      hits += 1;
      if (hits >= 2) return true;
    }
  }
  // Strong single signals: fenced code or multiple headings
  if (/^```/m.test(text)) return true;
  const headings = text.match(/^#{1,6}\s+\S/gm);
  if (headings && headings.length >= 2) return true;
  return false;
}

export function shouldRenderMarkdown(url: string, content: string): boolean {
  return urlLooksLikeMarkdown(url) || contentLooksLikeMarkdown(content);
}
