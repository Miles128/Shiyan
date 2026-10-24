import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api, type RefreshProgress } from "../api";
import { useConfigForm } from "../useConfigForm";
import { placementSummaryText } from "../placement/engine";
import { useToast } from "../components/Toaster";
import PageBack from "../components/PageBack";
import { type PrefOption } from "../prefOptions";
import {
  CEFR_LEVELS,
  FREQ_BANDS,
  type CefrLevel,
  type FreqBand,
} from "../wordLevels";
import ManageFeeds from "../components/ManageFeeds";
import BooksSection from "../components/BooksSection";

type Tab = "reading" | "subscriptions" | "books" | "llm" | "data";

const TABS = [
  ["reading", "阅读"],
  ["subscriptions", "RSS"],
  ["books", "书籍"],
  ["llm", "大模型"],
  ["data", "数据"],
] as const;

const CEFR_OPTIONS: readonly PrefOption[] = CEFR_LEVELS.map((lv) => ({
  value: lv,
  label: lv,
}));
const FREQ_OPTIONS: readonly PrefOption[] = FREQ_BANDS.map((n) => ({
  value: n,
  label: String(n >= 1000 ? `${n / 1000}k` : n),
}));

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

export default function Settings() {
  const toast = useToast();
  const { cfg, updateNow, updateSoon } = useConfigForm((message) =>
    toast.err(message),
  );
  const [tab, setTab] = useState<Tab>("reading");
  const [backingUp, setBackingUp] = useState(false);
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
          <p className="muted">
            主题、字体、字号、行距、行宽与超难词释义都在阅读页右上「Aa」面板调整。
          </p>

          <section className="settings-section">
            <h2>阅读难度</h2>
            <p className="muted">
              难词 = 正文里加下划线的词（超出 CEFR 或超出词频上限，两套阈值同时生效）；
              超难词 = 比阈值再高一档的词，可在「Aa」面板开启后淡色挂中文释义。
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
            <div className="placement-settings">
              <p className="muted">
                {placementSummaryText(cfg) ?? "尚未完成词汇量测验。"}
              </p>
              <Link className="btn" to="/placement">
                {cfg.vocab_placement_done ? "重新测验" : "测一下词汇量"}
              </Link>
            </div>
          </section>

          <p className="muted settings-group-label">存档</p>
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
        </>
      )}

      {tab === "subscriptions" && (
        <section className="settings-section">
          <h2>RSS 订阅</h2>
          <ManageFeeds />
        </section>
      )}

      {tab === "books" && <BooksSection />}

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
          <h2>备份</h2>
          <p className="muted">
            备份生成整个数据库的一致快照（文章、订阅源、生词与短语）。
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
          </div>
        </section>
      )}
    </div>
  );
}
