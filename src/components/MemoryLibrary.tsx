import { useCallback, useState } from "react";
import { api, type MemoryKind } from "../api";
import { invalidateQueries, useQuery } from "../query";
import { useTts } from "../useTts";
import IconBtn from "./IconBtn";
import Tabs from "./Tabs";

type Tab = "learning" | "mastered";

type Props = {
  kind: MemoryKind;
  searchPlaceholder: string;
  emptyText: string;
  /** Called after any status change or delete so the host can refresh highlights. */
  onChanged?: () => void;
};

/** Shared library view for words and phrases (tabs / search / row actions). */
export default function MemoryLibrary({
  kind,
  searchPlaceholder,
  emptyText,
  onChanged,
}: Props) {
  const [tab, setTab] = useState<Tab>("learning");
  const [actionError, setActionError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const tts = useTts();

  // Replace-model reads: tab switches paint the cached list instantly and
  // revalidate; status/delete mutations below invalidate so the mounted list
  // reloads itself — no manual load() threading.
  const listQuery = useQuery(
    ["memory", kind, tab],
    () => api.listMemory(kind, tab),
  );
  const items = listQuery.data ?? [];
  const queryError = listQuery.error;
  const error =
    actionError ?? (queryError == null ? null : String(queryError));

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

  async function mutate(action: () => Promise<unknown>) {
    try {
      await action();
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
        <Tabs
          items={[
            ["learning", "学习中"],
            ["mastered", "已掌握"],
          ] as const}
          value={tab}
          onChange={setTab}
        />
        <input
          className="search vocab-toolbar-search"
          placeholder={searchPlaceholder}
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
      </div>

      {error && <p className="banner err">{error}</p>}

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
              <IconBtn
                className="square"
                icon={
                  tab === "learning" ? (
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                      <path d="M20 6 9 17l-5-5" />
                    </svg>
                  ) : (
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                      <path d="M3 12a9 9 0 1 0 2.64-6.36" />
                      <path d="M3 4v5h5" />
                    </svg>
                  )
                }
                title={tab === "learning" ? "标为已掌握" : "恢复学习"}
                label={`${tab === "learning" ? "标为已掌握" : "恢复学习"}：${v.term}`}
                onClick={() =>
                  void mutate(() =>
                    api.setMemoryStatus(
                      v.id,
                      tab === "learning" ? "mastered" : "learning",
                    ),
                  )
                }
              />
              <IconBtn
                className="square danger"
                icon={
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                    <path d="M3 6h18" />
                    <path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2" />
                    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
                  </svg>
                }
                title="删除"
                label={`删除：${v.term}`}
                onClick={() => void mutate(() => api.deleteMemory(v.id))}
              />
            </span>
          </li>
        ))}
        {filtered.length === 0 && <p className="muted">{emptyText}</p>}
      </ul>
    </>
  );
}
