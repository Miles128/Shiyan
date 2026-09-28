import { useMemo, useState } from "react";
import { useKnown } from "../store";
import { filterKnownWords } from "../knownWords";

/**
 * Management view for words the learner marked as already known.
 * They stop being highlighted in the reader; this list can search,
 * add (manual entry), and un-mark them.
 */
export default function KnownWords() {
  const { knownTerms, markKnown, unmarkKnown } = useKnown();
  const [q, setQ] = useState("");
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);

  const filtered = useMemo(
    () => filterKnownWords(knownTerms, q),
    [knownTerms, q],
  );

  async function add() {
    const term = input.trim();
    if (!term) return;
    try {
      await markKnown(term);
      setInput("");
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function remove(term: string) {
    try {
      await unmarkKnown(term);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <>
      <div className="known-add">
        <input
          placeholder="手动添加一个已认识的词"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void add();
          }}
        />
        <button className="btn small" onClick={() => void add()}>
          添加
        </button>
        <span className="muted">共 {knownTerms.length} 个</span>
      </div>

      {error && <p className="banner err">{error}</p>}

      <input
        className="search"
        placeholder="搜索已认识的词"
        value={q}
        onChange={(e) => setQ(e.target.value)}
      />
      <ul className="vocab-list">
        {filtered.map((term) => (
          <li key={term} className="vocab-card">
            <div className="vocab-head">
              <strong>{term}</strong>
            </div>
            <div className="row-actions">
              <button
                className="btn small danger"
                onClick={() => void remove(term)}
              >
                取消已认识
              </button>
            </div>
          </li>
        ))}
        {filtered.length === 0 && (
          <p className="muted">
            {knownTerms.length === 0
              ? "还没有标记的已认识单词。阅读时选中单词点「已认识」，或在上方手动添加。"
              : "没有匹配的词。"}
          </p>
        )}
      </ul>
    </>
  );
}
