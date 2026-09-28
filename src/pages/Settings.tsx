import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api, defaultAppConfig, type AppConfig, type RefreshProgress } from "../api";
import { normalizeConfig, useAppConfig } from "../store";
import { placementSummaryText } from "../placement/engine";
import { useToast } from "../components/Toaster";
import PageBack from "../components/PageBack";
import { isThemePref, THEME_LABELS, THEME_PREFS, type ThemePref } from "../theme";
import {
  GLOSS_MIN_LINE_HEIGHT,
  READER_FONTS,
  READER_FONT_SIZES,
  READER_LINE_HEIGHTS,
  READER_LINE_WIDTHS,
  readingCssVars,
  resolveReadingPrefs,
  type ReaderFontId,
  type ReaderFontSize,
  type ReaderLineHeight,
  type ReaderLineWidthId,
} from "../readingPrefs";
import {
  CEFR_LEVELS,
  FREQ_BANDS,
  type CefrLevel,
  type FreqBand,
} from "../wordLevels";
import ManageFeeds from "../components/ManageFeeds";

type Tab = "reading" | "subscriptions" | "llm" | "data";

const TABS = [
  ["reading", "阅读"],
  ["subscriptions", "订阅"],
  ["llm", "大模型"],
  ["data", "数据"],
] as const;

type PrefOption = { value: string | number; label: string };

/** Labelled <select> bound to one config field; options are data-driven. */
function PrefSelect(props: {
  label: string;
  value: string | number;
  onChange: (raw: string) => void;
  options: readonly PrefOption[];
}) {
  return (
    <label>
      {props.label}
      <select value={props.value} onChange={(e) => props.onChange(e.target.value)}>
        {props.options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

const THEME_OPTIONS: readonly PrefOption[] = THEME_PREFS.map((p) => ({
  value: p,
  label: THEME_LABELS[p],
}));
const CEFR_OPTIONS: readonly PrefOption[] = CEFR_LEVELS.map((lv) => ({
  value: lv,
  label: lv,
}));
const FREQ_OPTIONS: readonly PrefOption[] = FREQ_BANDS.map((n) => ({
  value: n,
  label: String(n >= 1000 ? `${n / 1000}k` : n),
}));
const FONT_OPTIONS: readonly PrefOption[] = READER_FONTS.map((f) => ({
  value: f.id,
  label: f.label,
}));
const FONT_SIZE_OPTIONS: readonly PrefOption[] = READER_FONT_SIZES.map(
  (s) => ({ value: s.value, label: s.label }),
);
const LINE_HEIGHT_OPTIONS: readonly PrefOption[] = READER_LINE_HEIGHTS.map(
  (h) => ({ value: h.value, label: h.label }),
);
const LINE_WIDTH_OPTIONS: readonly PrefOption[] = READER_LINE_WIDTHS.map(
  (w) => ({ value: w.id, label: w.label }),
);

export default function Settings() {
  const { cfg: savedCfg, ready, save: saveCfg } = useAppConfig();
  const toast = useToast();
  const [cfg, setCfg] = useState<AppConfig>(() =>
    normalizeConfig(defaultAppConfig()),
  );
  const cfgRef = useRef(cfg);
  useEffect(() => {
    cfgRef.current = cfg;
  }, [cfg]);
  const [tab, setTab] = useState<Tab>("reading");
  const [repairing, setRepairing] = useState(false);
  const [repairMsg, setRepairMsg] = useState<string | null>(null);
  const [backingUp, setBackingUp] = useState(false);
  const [restoring, setRestoring] = useState(false);
  // 5B: 刷新进行中禁用备份（后端同样拒绝，双保险）。
  const [refreshing, setRefreshing] = useState(false);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<RefreshProgress>("refresh-progress", (event) => {
      if (cancelled) return;
      setRefreshing(event.payload.phase !== "done");
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  const saveTimer = useRef<number | undefined>(undefined);
  const pendingSaveRef = useRef(false);
  const seededRef = useRef(false);

  async function backupDb() {
    setBackingUp(true);
    try {
      const path = await api.backupDatabase();
      if (path) toast.ok(`已备份到 ${path}`);
    } catch (e) {
      toast.err(String(e));
    } finally {
      setBackingUp(false);
    }
  }

  async function restoreDb() {
    const confirmed = window.confirm(
      "恢复将覆盖当前全部数据（文章、生词、复习进度、已认识词）。\n" +
        "重启前当前数据不会丢失；恢复在重启拾言后生效，且会自动留一份 .pre-restore.bak。\n\n确定继续？",
    );
    if (!confirmed) return;
    setRestoring(true);
    try {
      const msg = await api.restoreDatabase();
      toast.ok(msg);
    } catch (e) {
      if (!String(e).includes("未选择备份文件")) toast.err(String(e));
    } finally {
      setRestoring(false);
    }
  }

  async function repairParagraphs() {
    setRepairing(true);
    setRepairMsg(null);
    try {
      const n = await api.repairParagraphs(30);
      setRepairMsg(n > 0 ? `已重抓修复 ${n} 篇的段落。` : "没有需要修复的文章。");
    } catch (e) {
      setRepairMsg(String(e));
    } finally {
      setRepairing(false);
    }
  }
  const readingPreview = useMemo(() => resolveReadingPrefs(cfg), [cfg]);

  useEffect(() => {
    // Only seed local state on first ready; subsequent `savedCfg` changes
    // (from our own debounced saves) must not overwrite in-progress edits.
    if (ready && !seededRef.current) {
      seededRef.current = true;
      setCfg(normalizeConfig(savedCfg));
    }
  }, [ready, savedCfg]);

  const persist = useCallback(
    async (next: AppConfig) => {
      try {
        await saveCfg(next);
      } catch (e) {
        toast.err(String(e));
      }
    },
    [saveCfg, toast],
  );

  /** Apply a patch to local state; selects persist right away. */
  const updateNow = useCallback(
    (patch: Partial<AppConfig>) => {
      const next = normalizeConfig({ ...cfgRef.current, ...patch });
      cfgRef.current = next;
      setCfg(next);
      void persist(next);
    },
    [persist],
  );

  /** Text/number inputs debounce so typing doesn't hit disk per keystroke. */
  const updateSoon = useCallback(
    (patch: Partial<AppConfig>) => {
      const next = normalizeConfig({ ...cfgRef.current, ...patch });
      cfgRef.current = next;
      setCfg(next);
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      pendingSaveRef.current = true;
      saveTimer.current = window.setTimeout(() => {
        saveTimer.current = undefined;
        pendingSaveRef.current = false;
        void persist(cfgRef.current);
      }, 800);
    },
    [persist],
  );

  // Flush a pending debounced save when leaving the page.
  useEffect(() => {
    return () => {
      if (saveTimer.current) {
        window.clearTimeout(saveTimer.current);
        saveTimer.current = undefined;
        if (pendingSaveRef.current) {
          void saveCfg(cfgRef.current).catch(() => undefined);
          pendingSaveRef.current = false;
        }
      }
    };
  }, [saveCfg]);

  return (
    <div className="page">
      <div className="tabs header-tabs">
        {TABS.map(([id, label]) => (
          <button
            key={id}
            type="button"
            className={tab === id ? "tab active" : "tab"}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
        <PageBack />
      </div>

      {tab === "reading" && (
        <>
          <p className="muted settings-group-label">读与标</p>
          <section className="settings-section">
            <h2>外观</h2>
            <PrefSelect
              label="主题"
              value={isThemePref(cfg.theme) ? cfg.theme : "system"}
              onChange={(raw) => updateNow({ theme: raw as ThemePref })}
              options={THEME_OPTIONS}
            />
          </section>

          <section className="settings-section">
            <h2>阅读难度</h2>
            <p className="muted">
              正文会给「超出 CEFR」或「超出词频上限」的词/短语加下划线。两套阈值同时生效。
            </p>
            <PrefSelect
              label="我的 CEFR 水平"
              value={cfg.cefr_level}
              onChange={(raw) => updateNow({ cefr_level: raw as CefrLevel })}
              options={CEFR_OPTIONS}
            />
            <PrefSelect
              label="词频上限（大约认识多少词）"
              value={cfg.freq_band}
              onChange={(raw) => updateNow({ freq_band: Number(raw) as FreqBand })}
              options={FREQ_OPTIONS}
            />
            <label className="toggle-row">
              <input
                type="checkbox"
                checked={cfg.show_hard_word_gloss}
                onChange={(e) =>
                  updateNow({ show_hard_word_gloss: e.target.checked })
                }
              />
              <span>
                <strong>超级难词自动挂中文</strong>
                <span className="muted toggle-hint">
                  在「比下划线再高一档」的词下方淡色显示词典首义（比设定高 2 个 CEFR 档或词频档翻倍），
                  hover 加深。释义只占行距留白，开启后行高至少 {GLOSS_MIN_LINE_HEIGHT}。关闭后阅读区仅保留下划线，点击/划词翻译不变。
                </span>
              </span>
            </label>
            <div className="placement-settings">
              <p className="muted">
                {placementSummaryText(cfg) ?? "尚未完成词汇量测验。"}
              </p>
              <Link className="btn" to="/placement">
                {cfg.vocab_placement_done ? "重新测验" : "测一下词汇量"}
              </Link>
            </div>
          </section>

          <p className="muted settings-group-label">版式与存档</p>
          <section className="settings-section">
            <h2>阅读排版</h2>
            <p className="muted">只作用于阅读页正文。改动即时生效，阅读页内也可用「Aa」按钮调整。</p>
            <div className="settings-type-grid">
              <PrefSelect
                label="字体"
                value={cfg.reader_font}
                onChange={(raw) => updateNow({ reader_font: raw as ReaderFontId })}
                options={FONT_OPTIONS}
              />
              <PrefSelect
                label="字号"
                value={cfg.reader_font_size}
                onChange={(raw) =>
                  updateNow({ reader_font_size: Number(raw) as ReaderFontSize })
                }
                options={FONT_SIZE_OPTIONS}
              />
              <PrefSelect
                label="行距"
                value={cfg.reader_line_height}
                onChange={(raw) =>
                  updateNow({ reader_line_height: Number(raw) as ReaderLineHeight })
                }
                options={LINE_HEIGHT_OPTIONS}
              />
              <PrefSelect
                label="行宽"
                value={cfg.reader_line_width}
                onChange={(raw) =>
                  updateNow({ reader_line_width: raw as ReaderLineWidthId })
                }
                options={LINE_WIDTH_OPTIONS}
              />
            </div>
            <div
              className={`reader-preview${readingPreview.fullWidth ? " reader-full" : ""}`}
              style={readingCssVars(readingPreview)}
            >
              <p>
                The best time to plant a tree was twenty years ago. The second
                best time is now. Reading English news works the same way: a
                little every day, in a column that does not tire the eye.
              </p>
            </div>
          </section>

          <section className="settings-section">
            <h2>文章缓存</h2>
            <label>
              自动收录文章保留天数
              <input
                type="number"
                min={0}
                max={365}
                value={cfg.article_retention_days}
                onChange={(e) =>
                  updateSoon({
                    article_retention_days: Math.max(
                      0,
                      Math.min(365, Number(e.target.value) || 0),
                    ),
                  })
                }
              />
            </label>
            <p className="muted">
              刷新时自动删除发布时间早于该天数的收录文章（0 =
              永久保留）。你收藏的文章和手动导入的文件/链接不会被清理。默认 14 天。
            </p>
          </section>

          <details className="settings-section">
            <summary>维护（低频运维）</summary>
            <p className="muted">
              重新抓取「正文丢失段落换行」的文章并替换正文（每次最多 30 篇）。
            </p>
            <button
              className="btn"
              onClick={() => void repairParagraphs()}
              disabled={repairing}
            >
              {repairing ? "修复中…" : "重抓修复段落"}
            </button>
            {repairMsg && <p className="banner ok">{repairMsg}</p>}
          </details>
        </>
      )}

      {tab === "subscriptions" && (
        <section className="settings-section">
          <h2>RSS 订阅</h2>
          <ManageFeeds />
        </section>
      )}

      {tab === "llm" && (
        <section className="settings-section">
          <h2>大模型（OpenAI 兼容）</h2>
          <label>
            Base URL
            <input
              value={cfg.base_url}
              onChange={(e) => updateSoon({ base_url: e.target.value })}
              placeholder="https://api.openai.com/v1"
            />
          </label>
          <label>
            API Key
            <input
              type="password"
              value={cfg.api_key}
              onChange={(e) => updateSoon({ api_key: e.target.value })}
              placeholder="sk-..."
            />
          </label>
          <label>
            Model
            <input
              value={cfg.model}
              onChange={(e) => updateSoon({ model: e.target.value })}
              placeholder="gpt-4o-mini"
            />
          </label>
          <p className="muted">
            可复制 <code>config.local.json.example</code> 为{" "}
            <code>config.local.json</code> 后编辑；该文件已 gitignore。
          </p>
        </section>
      )}
      {tab === "data" && (
        <section className="settings-section">
          <h2>备份与恢复</h2>
          <p className="muted">
            备份生成整个数据库的一致快照（文章、生词、复习进度、已认识词、查词历史）。
            恢复时选择备份文件，重启拾言后生效；当前数据会自动备份为 .pre-restore.bak。
          </p>
          <div className="data-actions">
            <button
              className="btn primary"
              onClick={() => void backupDb()}
              disabled={backingUp || refreshing}
              title={refreshing ? "刷新进行中，请刷新结束后再备份" : undefined}
            >
              {backingUp ? "备份中…" : refreshing ? "刷新中，稍后备份" : "备份数据库"}
            </button>
            <button
              className="btn"
              onClick={() => void restoreDb()}
              disabled={restoring}
            >
              {restoring ? "校验中…" : "恢复数据库…"}
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
