import { type AppConfig } from "../api";
import { useAppConfig } from "../store";
import { useToast } from "./Toaster";
import { type ThemePref } from "../theme";
import {
  type ReaderFontId,
  type ReaderFontSize,
  type ReaderLineHeight,
  type ReaderLineWidthId,
} from "../readingPrefs";
import {
  FONT_OPTIONS,
  FONT_SIZE_OPTIONS,
  LINE_HEIGHT_OPTIONS,
  LINE_WIDTH_OPTIONS,
  THEME_OPTIONS,
  type PrefOption,
} from "../prefOptions";

function ChipRow({
  label,
  value,
  options,
  onPick,
}: {
  label: string;
  value: string | number;
  options: readonly PrefOption[];
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
            {o.short ?? o.label}
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

    // Values come from fixed pick lists, so the merged config is already
    // normalized; the store's `save` normalizes again as the safety net.
    const next = { ...cfg, ...patch };
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
        options={THEME_OPTIONS}
        onPick={(v) => update({ theme: v as ThemePref })}
      />
      <ChipRow
        label="字体"
        value={cfg.reader_font}
        options={FONT_OPTIONS}
        onPick={(v) => update({ reader_font: v as ReaderFontId })}
      />
      <ChipRow
        label="字号"
        value={cfg.reader_font_size}
        options={FONT_SIZE_OPTIONS}
        onPick={(v) => update({ reader_font_size: v as ReaderFontSize })}
      />
      <ChipRow
        label="行距"
        value={cfg.reader_line_height}
        options={LINE_HEIGHT_OPTIONS}
        onPick={(v) => update({ reader_line_height: v as ReaderLineHeight })}
      />
      <ChipRow
        label="行宽"
        value={cfg.reader_line_width}
        options={LINE_WIDTH_OPTIONS}
        onPick={(v) => update({ reader_line_width: v as ReaderLineWidthId })}
      />
      <ChipRow
        label="超难词释义"
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
