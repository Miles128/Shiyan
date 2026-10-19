/** 设置表单选项域：设置页（<select>）与阅读页 Aa 面板（chips）共用的选项表。
 *  两边只是控件形态不同，选项数据只在这里派生一次。 */

import { THEME_LABELS, THEME_PREFS } from "./theme";
import {
  READER_FONTS,
  READER_FONT_SIZES,
  READER_LINE_HEIGHTS,
  READER_LINE_WIDTHS,
} from "./readingPrefs";

/** One choice in a settings control. */
export type PrefOption = {
  value: string | number;
  /** Full label for selects. */
  label: string;
  /** Chip text when it should be shorter than `label`; falls back to it. */
  short?: string;
};

export const THEME_OPTIONS: readonly PrefOption[] = THEME_PREFS.map((p) => ({
  value: p,
  label: THEME_LABELS[p],
}));
export const FONT_OPTIONS: readonly PrefOption[] = READER_FONTS.map((f) => ({
  value: f.id,
  label: f.label,
}));
export const FONT_SIZE_OPTIONS: readonly PrefOption[] = READER_FONT_SIZES.map(
  (s) => ({ value: s.value, label: s.label, short: String(s.value) }),
);
export const LINE_HEIGHT_OPTIONS: readonly PrefOption[] = READER_LINE_HEIGHTS.map(
  (h) => ({ value: h.value, label: h.label, short: String(h.value) }),
);
export const LINE_WIDTH_OPTIONS: readonly PrefOption[] = READER_LINE_WIDTHS.map(
  (w) => ({ value: w.id, label: w.label }),
);
