import { type AppConfig } from "../api";
import { normalizeConfig, useAppConfig } from "../store";
import { useToast } from "./Toaster";
import { THEME_LABELS, THEME_PREFS, type ThemePref } from "../theme";
import {
  READER_FONTS,
  READER_FONT_SIZES,
  READER_LINE_HEIGHTS,
  READER_LINE_WIDTHS,
  type ReaderFontId,
  type ReaderFontSize,
  type ReaderLineHeight,
  type ReaderLineWidthId,
} from "../readingPrefs";

type ChipOption = { value: string | number; label: string };

const THEME_CHIPS: ChipOption[] = THEME_PREFS.map((p) => ({
  value: p,
  label: THEME_LABELS[p],
}));
const FONT_CHIPS: ChipOption[] = READER_FONTS.map((f) => ({
  value: f.id,
  label: f.label,
}));
const SIZE_CHIPS: ChipOption[] = READER_FONT_SIZES.map((s) => ({
  value: s.value,
  label: String(s.value),
}));
const HEIGHT_CHIPS: ChipOption[] = READER_LINE_HEIGHTS.map((h) => ({
  value: h.value,
  label: String(h.value),
}));
const WIDTH_CHIPS: ChipOption[] = READER_LINE_WIDTHS.map((w) => ({
  value: w.id,
  label: w.label,
}));

function ChipRow({
  label,
  value,
  options,
  onPick,
}: {
  label: string;
  value: string | number;
  options: readonly ChipOption[];
  onPick: (v: string | number) => void;
}) {
  return (
    <div className="type-panel-row">
      <span className="type-panel-label">{label}</span>
      <div className="type-panel-chips">
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            className={o.value === value ? "tag-chip active" : "tag-chip"}
            onClick={() => onPick(o.value)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * In-reader typography quick panel (Aa). Every pick persists via the config
 * store and takes effect immediately; layout-changing picks restore the
 * scroll position by ratio so the reader doesn't lose their place.
 */
export default function ReaderTypePanel() {
  const { cfg, save } = useAppConfig();
  const toast = useToast();

  const update = (patch: Partial<AppConfig>) => {
    const touchesLayout =
      "reader_font" in patch ||
      "reader_font_size" in patch ||
      "reader_line_height" in patch ||
      "reader_line_width" in patch;
    const doc = document.documentElement;
    const maxBefore = doc.scrollHeight - window.innerHeight;
    const ratio = maxBefore > 0 ? window.scrollY / maxBefore : 0;

    const next = normalizeConfig({ ...cfg, ...patch });
    save(next)
      .then(() => {
        if (!touchesLayout) return;
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            const maxAfter =
              document.documentElement.scrollHeight - window.innerHeight;
            window.scrollTo({
              top: Math.max(0, Math.round(ratio * maxAfter)),
              behavior: "auto",
            });
          }),
        );
      })
      .catch((e) => toast.err(String(e)));
  };

  return (
    <div className="type-panel" role="dialog" aria-label="排版设置">
      <ChipRow
        label="主题"
        value={cfg.theme}
        options={THEME_CHIPS}
        onPick={(v) => update({ theme: v as ThemePref })}
      />
      <ChipRow
        label="字体"
        value={cfg.reader_font}
        options={FONT_CHIPS}
        onPick={(v) => update({ reader_font: v as ReaderFontId })}
      />
      <ChipRow
        label="字号"
        value={cfg.reader_font_size}
        options={SIZE_CHIPS}
        onPick={(v) => update({ reader_font_size: v as ReaderFontSize })}
      />
      <ChipRow
        label="行距"
        value={cfg.reader_line_height}
        options={HEIGHT_CHIPS}
        onPick={(v) => update({ reader_line_height: v as ReaderLineHeight })}
      />
      <ChipRow
        label="行宽"
        value={cfg.reader_line_width}
        options={WIDTH_CHIPS}
        onPick={(v) => update({ reader_line_width: v as ReaderLineWidthId })}
      />
      <ChipRow
        label="难词注释"
        value={cfg.show_hard_word_gloss ? "on" : "off"}
        options={[
          { value: "on", label: "显示" },
          { value: "off", label: "隐藏" },
        ]}
        onPick={(v) => update({ show_hard_word_gloss: v === "on" })}
      />
    </div>
  );
}
