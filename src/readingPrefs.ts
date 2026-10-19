import type { CSSProperties } from "react";

export const READER_FONTS = [
  {
    id: "serif",
    label: "衬线（霞鹜文楷）",
    family:
      '"Iowan Old Style", "Palatino Linotype", Palatino, "LXGW WenKai", serif',
  },
  {
    id: "palatino",
    label: "Palatino",
    family: 'Palatino, "Palatino Linotype", "LXGW WenKai", serif',
  },
  {
    id: "georgia",
    label: "Georgia",
    family: 'Georgia, "LXGW WenKai", serif',
  },
  {
    id: "newyork",
    label: "New York",
    family: '"New York", "Iowan Old Style", "LXGW WenKai", serif',
  },
  {
    id: "songti",
    label: "宋体",
    family: '"Songti SC", "Source Han Serif SC", Palatino, serif',
  },
  {
    id: "sans",
    label: "无衬线",
    family:
      '-apple-system, BlinkMacSystemFont, "PingFang SC", "Hiragino Sans GB", sans-serif',
  },
] as const;

export type ReaderFontId = (typeof READER_FONTS)[number]["id"];

export const READER_FONT_SIZES = [
  { value: 16, label: "16 · 小" },
  { value: 18, label: "18 · 标准" },
  { value: 20, label: "20 · 大" },
  { value: 22, label: "22 · 较大" },
  { value: 24, label: "24 · 特大" },
] as const;

export type ReaderFontSize = (typeof READER_FONT_SIZES)[number]["value"];

export const READER_LINE_HEIGHTS = [
  { value: 1.5, label: "紧凑 1.5" },
  { value: 1.65, label: "适中 1.65" },
  { value: 1.75, label: "标准 1.75" },
  { value: 1.9, label: "宽松 1.9" },
  { value: 2.1, label: "很松 2.1" },
] as const;

export type ReaderLineHeight = (typeof READER_LINE_HEIGHTS)[number]["value"];

export const READER_LINE_WIDTHS = [
  { id: "narrow", label: "窄", measure: "42rem" },
  { id: "medium", label: "适中", measure: "52rem" },
  { id: "wide", label: "宽", measure: "64rem" },
  { id: "full", label: "全宽", measure: "100%" },
] as const;

export type ReaderLineWidthId = (typeof READER_LINE_WIDTHS)[number]["id"];

export type ReadingPrefs = {
  reader_font: ReaderFontId;
  reader_font_size: ReaderFontSize;
  reader_line_height: ReaderLineHeight;
  reader_line_width: ReaderLineWidthId;
};

export type ResolvedReading = {
  fontId: ReaderFontId;
  fontFamily: string;
  fontSizePx: ReaderFontSize;
  lineHeight: ReaderLineHeight;
  lineWidthId: ReaderLineWidthId;
  measure: string;
  fullWidth: boolean;
};

/**
 * Loose input shape for persisted values: ts-rs-generated `AppConfig` carries
 * these as plain `string` / `number`; the guards below do the narrowing.
 */
export type RawReadingInput = {
  reader_font?: string;
  reader_font_size?: number;
  reader_line_height?: number;
  reader_line_width?: string;
};

const FONT_IDS = new Set<string>(READER_FONTS.map((f) => f.id));
const FONT_SIZES = new Set<number>(READER_FONT_SIZES.map((s) => s.value));
const LINE_HEIGHTS = new Set<number>(READER_LINE_HEIGHTS.map((h) => h.value));
const LINE_WIDTHS = new Set<string>(READER_LINE_WIDTHS.map((w) => w.id));

export function defaultReadingPrefs(): ReadingPrefs {
  return {
    reader_font: "serif",
    reader_font_size: 18,
    reader_line_height: 1.75,
    reader_line_width: "full",
  };
}

function isReaderFontId(v: unknown): v is ReaderFontId {
  return typeof v === "string" && FONT_IDS.has(v);
}

function isReaderFontSize(v: unknown): v is ReaderFontSize {
  return typeof v === "number" && FONT_SIZES.has(v);
}

function isReaderLineHeight(v: unknown): v is ReaderLineHeight {
  return typeof v === "number" && LINE_HEIGHTS.has(v);
}

function isReaderLineWidthId(v: unknown): v is ReaderLineWidthId {
  return typeof v === "string" && LINE_WIDTHS.has(v);
}

export function normalizeReadingPrefs(
  raw?: Partial<RawReadingInput> | null,
): ReadingPrefs {
  const fallback = defaultReadingPrefs();
  return {
    reader_font: isReaderFontId(raw?.reader_font)
      ? raw.reader_font
      : fallback.reader_font,
    reader_font_size: isReaderFontSize(raw?.reader_font_size)
      ? raw.reader_font_size
      : fallback.reader_font_size,
    reader_line_height: isReaderLineHeight(raw?.reader_line_height)
      ? raw.reader_line_height
      : fallback.reader_line_height,
    reader_line_width: isReaderLineWidthId(raw?.reader_line_width)
      ? raw.reader_line_width
      : fallback.reader_line_width,
  };
}

export function resolveReadingPrefs(
  raw?: Partial<RawReadingInput> | null,
): ResolvedReading {
  const prefs = normalizeReadingPrefs(raw);
  const font = READER_FONTS.find((f) => f.id === prefs.reader_font) ?? READER_FONTS[0];
  const width =
    READER_LINE_WIDTHS.find((w) => w.id === prefs.reader_line_width) ??
    READER_LINE_WIDTHS[1];
  return {
    fontId: font.id,
    fontFamily: font.family,
    fontSizePx: prefs.reader_font_size,
    lineHeight: prefs.reader_line_height,
    lineWidthId: width.id,
    measure: width.measure,
    fullWidth: width.id === "full",
  };
}

/** 释义下挂后住在行距留白里：低于这个行高会压到下一行，开启释义时抬到它。
 *  释义字号 = 0.55em + 2pt，16px 正文时最吃行距，故取 2.1 再叠下面的 1pt。 */
export const GLOSS_MIN_LINE_HEIGHT = 2.1;

/** 开释义时在档位/下限之上再给的绝对留白：1pt（CSS 1pt = 4/3px）。 */
const GLOSS_LINE_HEIGHT_BONUS_PT = 4 / 3;

export function readingCssVars(
  resolved: ResolvedReading,
  glossBelow = false,
): CSSProperties {
  const base = glossBelow
    ? Math.max(resolved.lineHeight, GLOSS_MIN_LINE_HEIGHT)
    : resolved.lineHeight;
  // 折算成倍数而非 calc 长度：--reader-lh 保持无单位，继承到更小字号的元素
  // （如 markdown 行内 code）仍按自身字号缩放，和改之前一致。
  const lineHeight = base + (glossBelow ? GLOSS_LINE_HEIGHT_BONUS_PT / resolved.fontSizePx : 0);
  return {
    "--reader-font": resolved.fontFamily,
    "--reader-size": `${resolved.fontSizePx}px`,
    "--reader-lh": String(Math.round(lineHeight * 1000) / 1000),
    "--reader-measure": resolved.measure,
  } as CSSProperties;
}
