import { useCallback, useEffect, useRef, useState } from "react";
import { clipContext } from "../readerUtils";
import { api, type MemoryItem, type MemoryKind } from "../api";
import { fetchQuery, invalidateQueries, useQuery } from "../query";
import { useTts } from "../useTts";

type Tab = "learning" | "review" | "mastered";

const USAGE_LABELS: Record<string, string> = {
  idiom: "习语",
  "phrasal-verb": "短语动词",
  collocation: "搭配",
  "fixed-expression": "固定表达",
  slang: "俚语",
  phrase: "短语",
};

/** Words show their part of speech verbatim; phrases map usage to a CN label. */
function typeLabel(kind: MemoryKind, wordType: string): string {
  if (kind !== "phrase") return wordType;
  return USAGE_LABELS[wordType] ?? wordType;
}

/** Fisher-Yates; returns a fresh array. */
function shuffled<T>(list: T[]): T[] {
  const out = [...list];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

type Props = {
  kind: MemoryKind;
  searchPlaceholder: string;
  emptyText: string;
  /** Called after any add/review/status/delete so the host can refresh highlights. */
  onChanged?: () => void;
};

/** Shared library view for words and phrases (tabs / search / flashcard review). */
export default function MemoryLibrary({
  kind,
  searchPlaceholder,
  emptyText,
  onChanged,
}: Props) {
  const [tab, setTab] = useState<Tab>("learning");
  const [current, setCurrent] = useState<MemoryItem | null>(null);
  const [flipped, setFlipped] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  /** Free-practice deck (shuffled learning list); non-null = practice mode, no SRS writes. */
  const [practiceDeck, setPracticeDeck] = useState<MemoryItem[] | null>(null);
  /** Shown inline when a practice deck is exhausted (replaces the old toast). */
  const [practiceDone, setPracticeDone] = useState(false);
  /** Card id with a review write in flight (see rate). */
  const ratingRef = useRef<string | null>(null);
  const tts = useTts();

  // Replace-model reads: tab switches paint the cached list instantly and
  // revalidate; status/delete mutations below invalidate so the mounted list
  // reloads itself — no manual load() threading.
  const listTab = tab === "review" ? null : tab;
  const listQuery = useQuery(
    listTab ? ["memory", kind, listTab] : null,
    () => api.listMemory(kind, listTab ?? "learning"),
  );
  const dueQuery = useQuery(
    tab === "review" ? ["memory", kind, "due"] : null,
    () => api.dueMemory(kind),
  );
  const items = listQuery.data ?? [];
  // Review cursor advances locally (see rate): the shadow masks the query
  // until the next genuine reload (tab enter / mutation invalidation), which
  // clears it and restarts at the first card — the old load() did the same.
  const [localDue, setLocalDue] = useState<MemoryItem[] | null>(null);
  const due = localDue ?? dueQuery.data ?? [];
  const queryError = listQuery.error ?? dueQuery.error;
  const error =
    actionError ?? (queryError == null ? null : String(queryError));

  // A fresh due list (tab enter, kind switch, post-mutation reload) clears
  // the local advance shadow and restarts the review cursor at the first
  // card — the old load() did the same.
  useEffect(() => {
    if (dueQuery.data) {
      setLocalDue(null);
      setCurrent(dueQuery.data[0] ?? null);
      setFlipped(false);
    }
  }, [dueQuery.data]);

  // Leaving the review tab ends practice mode.
  useEffect(() => {
    if (tab !== "review") setPracticeDeck(null);
  }, [tab]);

  const speakTerm = useCallback((text: string) => {
    if (!text.trim()) return;
    tts.startSpeak({ kind: "word" }, [text]);
  }, [tts]);

  const filtered = items.filter((v) => {
    if (!q.trim()) return true;
    const s = q.toLowerCase();
    return (
      v.term.toLowerCase().includes(s) ||
      v.definition_zh.includes(q) ||
      v.word_type.toLowerCase().includes(s)
    );
  });

  async function rate(rating: string) {
    // Practice mode never writes to the SRS schedule — just advance.
    if (practiceDeck) {
      advancePractice();
      return;
    }
    if (!current) return;
    // Drop repeats while a write for this card is in flight: the keyboard
    // effect holds a stale `current` until re-render, so a held key (or a
    // fast double press) would record the same card twice and over-promote
    // it. Retries after a failure are unaffected (ref cleared in finally).
    if (ratingRef.current === current.id) return;
    ratingRef.current = current.id;
    try {
      await api.reviewMemory(current.id, rating);
      onChanged?.();
      // Local advance only: the rated card drops out of view immediately.
      // The query cache keeps the pre-rate list (refreshed on next mount);
      // invalidating here would yank the cursor back to the first card.
      const rest = due.filter((d) => d.id !== current.id);
      setLocalDue(rest);
      setCurrent(rest[0] ?? null);
      setFlipped(false);
    } catch (e) {
      setActionError(String(e));
    } finally {
      ratingRef.current = null;
    }
  }

  function advancePractice() {
    setPracticeDeck((prev) => (prev ? prev.slice(1) : prev));
    setFlipped(false);
  }

  async function startPractice() {
    try {
      // Read through the shared cache so the list tab paints instantly next.
      const list = await fetchQuery(["memory", kind, "learning"], () =>
        api.listMemory(kind, "learning"),
      );
      if (list.length === 0) {
        setActionError("学习库还是空的，先去积累几个生词吧。");
        return;
      }
      setPracticeDone(false);
      setPracticeDeck(shuffled(list));
      setFlipped(false);
    } catch (e) {
      setActionError(String(e));
    }
  }

  // Practice deck exhausted → inline completion state (stays visible).
  useEffect(() => {
    if (practiceDeck && practiceDeck.length === 0) {
      setPracticeDeck(null);
      setFlipped(false);
      setPracticeDone(true);
    }
  }, [practiceDeck]);

  // Keyboard: Space/Enter flips, 1/2/3 rate (or advance in practice mode).
  useEffect(() => {
    if (tab !== "review") return;
    const activeCard = practiceDeck ? practiceDeck[0] ?? null : current;
    const onKey = (e: KeyboardEvent) => {
      // Held keys auto-repeat: without this, holding Space/1 flips or rates
      // the same card on every repeat (see the ratingRef guard in rate).
      if (e.repeat) return;
      const el = document.activeElement;
      // Let buttons, links, and form controls handle their own Enter/Space.
      if (
        el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.tagName === "BUTTON" ||
          el.tagName === "A" ||
          el.tagName === "SELECT" ||
          el.getAttribute("role") === "button")
      )
        return;
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        setFlipped((f) => !f);
      } else if (e.key === "1" || e.key === "2" || e.key === "3") {
        if (!activeCard) return;
        e.preventDefault();
        void rate(e.key === "1" ? "again" : e.key === "2" ? "hard" : "easy");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, practiceDeck, current]);

  async function setStatus(id: string, status: string) {
    try {
      await api.setMemoryStatus(id, status);
      // The mounted list/due queries reload themselves on invalidation.
      setActionError(null);
      invalidateQueries(["memory", kind]);
      onChanged?.();
    } catch (e) {
      setActionError(String(e));
    }
  }

  async function remove(id: string) {
    try {
      await api.deleteMemory(id);
      setActionError(null);
      invalidateQueries(["memory", kind]);
      onChanged?.();
    } catch (e) {
      setActionError(String(e));
    }
  }

  return (
    <>
      <div className="vocab-toolbar">
        <div className="tabs">
          {(
            [
              ["learning", "学习中"],
              ["review", "复习"],
              ["mastered", "已掌握"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              className={tab === id ? "tab active" : "tab"}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </div>
        {tab !== "review" && (
          <input
            className="search vocab-toolbar-search"
            placeholder={searchPlaceholder}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        )}
      </div>

      {error && <p className="banner err">{error}</p>}

      {tab !== "review" && (
        <>
          <ul className="vocab-list">
            {filtered.map((v) => (
              <li key={v.id} className="vocab-row">
                <button
                  type="button"
                  className="vocab-term"
                  onClick={() => speakTerm(v.term)}
                  title="点击朗读"
                >
                  {v.term}
                </button>
                <span className="vocab-zh" title={v.definition_zh || undefined}>
                  {v.definition_zh || <span className="muted">暂无释义</span>}
                </span>
                <span className="vocab-row-actions">
                  {tab === "learning" ? (
                    <button
                      type="button"
                      className="vocab-icon-btn"
                      title="标为已掌握"
                      aria-label={`标为已掌握：${v.term}`}
                      onClick={() => void setStatus(v.id, "mastered")}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                        <path d="M20 6 9 17l-5-5" />
                      </svg>
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="vocab-icon-btn"
                      title="恢复学习"
                      aria-label={`恢复学习：${v.term}`}
                      onClick={() => void setStatus(v.id, "learning")}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                        <path d="M3 12a9 9 0 1 0 2.64-6.36" />
                        <path d="M3 4v5h5" />
                      </svg>
                    </button>
                  )}
                  <button
                    type="button"
                    className="vocab-icon-btn danger"
                    title="删除"
                    aria-label={`删除：${v.term}`}
                    onClick={() => void remove(v.id)}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                      <path d="M3 6h18" />
                      <path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2" />
                      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
                    </svg>
                  </button>
                </span>
              </li>
            ))}
            {filtered.length === 0 && <p className="muted">{emptyText}</p>}
          </ul>
        </>
      )}

      {tab === "review" && (
        <div className="review-panel">
          {(() => {
            const activeCard = practiceDeck ? practiceDeck[0] ?? null : current;
            const deckDone = practiceDeck !== null && practiceDeck.length === 0;
            if (deckDone) return null;
            if (practiceDone && !activeCard) {
              return (
                <>
                  <p className="muted">练习完成！</p>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void startPractice()}
                  >
                    再练一组
                  </button>
                </>
              );
            }
            if (!activeCard) {
              return (
                <>
                  <p className="muted">今日没有到期复习的词条。</p>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void startPractice()}
                  >
                    自由练习（学习中的词 · 不计入复习计划）
                  </button>
                </>
              );
            }
            return (
              <>
                <p className="muted">
                  {practiceDeck ? (
                    <>
                      练习模式 · 剩余 {practiceDeck.length} 张 · 不计入复习计划{" "}
                      <button
                        type="button"
                        className="linklike"
                        onClick={() => setPracticeDeck(null)}
                      >
                        退出练习
                      </button>
                    </>
                  ) : (
                    <>剩余 {due.length} 张</>
                  )}
                  {" · Space 翻面 · 1/2/3 评分 "}
                  <button
                    type="button"
                    className="linklike"
                    onClick={() => speakTerm(activeCard.term)}
                  >
                    🔊 发音
                  </button>
                </p>
                <button
                  type="button"
                  className="flashcard"
                  aria-pressed={flipped}
                  onClick={() => setFlipped((f) => !f)}
                >
                  <p className="flash-context">
                    “{clipContext(activeCard.context_sentence || activeCard.term)}”
                  </p>
                  {flipped ? (
                    <div className="flash-back">
                      <div className="flash-term">{activeCard.term}</div>
                      <p>{activeCard.definition_zh}</p>
                      {activeCard.word_type && (
                        <p className="pill inline">
                          {typeLabel(kind, activeCard.word_type)}
                        </p>
                      )}
                      {activeCard.collocations?.length > 0 && (
                        <p className="muted">
                          常见搭配：{activeCard.collocations.join(" · ")}
                        </p>
                      )}
                    </div>
                  ) : (
                    <p className="muted tip">点击或按 Enter 查看释义</p>
                  )}
                </button>
                <div className="rate-row">
                  <button className="btn" onClick={() => void rate("again")}>
                    不认识
                  </button>
                  <button className="btn" onClick={() => void rate("hard")}>
                    模糊
                  </button>
                  <button
                    className="btn primary"
                    onClick={() => void rate("easy")}
                  >
                    认识
                  </button>
                </div>
              </>
            );
          })()}
        </div>
      )}
    </>
  );
}
