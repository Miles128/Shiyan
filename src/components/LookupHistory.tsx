import { useCallback, useEffect, useRef, useState } from "react";
import { api, type LookupEntry } from "../api";
import { fetchQuery, invalidateQueries } from "../query";
import { useVocab } from "../store";
import { normalizeKey } from "../wordLevels";
import { useToast } from "./Toaster";
/**
 * Lookup history: every term resolved through the selection popover, with
 * quick paths into the vocab/known libraries.
 */
export default function LookupHistory() {
  const [entries, setEntries] = useState<LookupEntry[]>([]);
  const [q, setQ] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [added, setAdded] = useState<Set<number>>(new Set());
  /** Terms already in the word/phrase libraries (so rows render 已在库). */
  const [inLibrary, setInLibrary] = useState<Set<string>>(new Set());
  const { refreshLearningTerms, markKnown: storeMarkKnown, refreshKnownTerms } = useVocab();
  const toast = useToast();

  // Monotonic guard: fast typing fires overlapping loads for different
  // queries ("a" vs "ab"); only the newest may paint. (The effect's `alive`
  // flag only cancels the debounce timer, not an in-flight request.)
  const loadSeq = useRef(0);
  const load = useCallback(async (search: string) => {
    const seq = ++loadSeq.current;
    setError(null);
    try {
      // Read through the shared cache: repeat searches dedup, and the
      // learning-list reads populate the MemoryLibrary list-tab entries.
      const [rows, words, phrases] = await Promise.all([
        fetchQuery(["lookups", search, 200, 0], () =>
          api.listLookups(search || undefined, 200, 0),
        ),
        fetchQuery(["memory", "word", "learning"], () =>
          api.listMemory("word", "learning"),
        ).catch(() => []),
        fetchQuery(["memory", "phrase", "learning"], () =>
          api.listMemory("phrase", "learning"),
        ).catch(() => []),
      ]);
      if (seq !== loadSeq.current) return;
      setEntries(rows);
      setInLibrary(
        new Set(
          [...words, ...phrases].map((v) => normalizeKey(v.term)),
        ),
      );
    } catch (e) {
      if (seq !== loadSeq.current) return;
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    let alive = true;
    const timer = window.setTimeout(() => {
      void load(q).then(() => {
        if (!alive) return;
      });
    }, 250);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [load, q]);

  async function addToVocab(entry: LookupEntry, kind: "word" | "phrase") {
    try {
      await api.addMemory({
        kind,
        term: entry.term,
        contextSentence: entry.context,
        articleId: entry.article_id,
        definitionZh: null,
      });
      setAdded((prev) => new Set(prev).add(entry.id));
      setInLibrary((prev) => new Set(prev).add(normalizeKey(entry.term)));
      toast.ok(kind === "phrase" ? `已加入短语：${entry.term}` : `已加入生词库：${entry.term}`);
      // Bust the learning-list entries so the library tabs paint fresh.
      invalidateQueries(["memory"]);
      void refreshLearningTerms();
    } catch (e) {
      setError(`加入${kind === "phrase" ? "短语" : "生词库"}失败：${String(e)}`);
    }
  }

  async function markKnown(entry: LookupEntry) {
    try {
      await storeMarkKnown(normalizeKey(entry.term));
      setAdded((prev) => new Set(prev).add(entry.id));
      void refreshKnownTerms();
      toast.ok(`已标为认识：${entry.term}`);
    } catch (e) {
      toast.err(`标为已认识失败：${String(e)}`);
    }
  }

  async function remove(entry: LookupEntry) {
    try {
      await api.deleteLookup(entry.id);
      setEntries((prev) => prev.filter((e) => e.id !== entry.id));
      // Keep the cache from re-serving the deleted row on next mount.
      invalidateQueries(["lookups"]);
    } catch (e) {
      setError(String(e));
    }
  }

  async function clearAll() {
    if (!window.confirm("确定清空全部查词历史？此操作不可恢复。")) return;
    try {
      await api.clearLookups();
      setEntries([]);
      invalidateQueries(["lookups"]);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div>
      <div>
        <input
          className="search"
          placeholder="搜索查过的词…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        {entries.length > 0 && (
          <button type="button" className="btn small" onClick={() => void clearAll()}>
            清空
          </button>
        )}
      </div>
      {error && <p className="banner err">{error}</p>}
      <ul className="vocab-list">
        {entries.map((entry) => (
          <li key={entry.id} className="vocab-card">
            <div className="vocab-head">
              <strong>{entry.term}</strong>
              <span className="pill">
                {new Date(entry.created_at).toLocaleString()}
              </span>
            </div>
            {entry.context && <p className="context">“{entry.context}”</p>}
            <div className="row-actions">
              {added.has(entry.id) || inLibrary.has(normalizeKey(entry.term)) ? (
                <span className="muted">
                  {inLibrary.has(normalizeKey(entry.term)) ? "已在库" : "已处理"}
                </span>
              ) : (
                <>
                  <button
                    className="btn small"
                    onClick={() =>
                      void addToVocab(
                        entry,
                        /\s/.test(entry.term.trim()) ? "phrase" : "word",
                      )
                    }
                  >
                    {/\s/.test(entry.term.trim()) ? "加入短语" : "加入生词库"}
                  </button>
                  <button
                    className="btn small"
                    onClick={() => void markKnown(entry)}
                  >
                    标为已认识
                  </button>
                </>
              )}
              <button
                className="btn small danger"
                onClick={() => void remove(entry)}
              >
                删除
              </button>
            </div>
          </li>
        ))}
        {entries.length === 0 && !error && (
          <p className="muted">还没有查词记录。阅读或浏览列表时划词查询会自动留档。</p>
        )}
      </ul>
    </div>
  );
}
