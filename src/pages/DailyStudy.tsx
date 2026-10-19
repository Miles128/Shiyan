import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, type MemoryItem } from "../api";
import { clipContext } from "../readerUtils";
import {
  buildDailyQueue,
  localDateKey,
  markDailyStudyDone,
  readDailyStudyDate,
} from "../dailyStudy";
import { useVocab } from "../store";
import { useTts } from "../useTts";

export default function DailyStudy() {
  const navigate = useNavigate();
  const { refreshLearningTerms } = useVocab();
  const tts = useTts();
  const today = localDateKey(new Date());
  const [queue, setQueue] = useState<MemoryItem[] | null>(null);
  const [index, setIndex] = useState(0);
  const [flipped, setFlipped] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(() => readDailyStudyDate() === today);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [due, learning] = await Promise.all([
        api.dueMemory("word"),
        api.listMemory("word", "learning"),
      ]);
      const next = buildDailyQueue({ due, learning });
      setQueue(next);
      setIndex(0);
      setFlipped(false);
      if (next.length === 0) {
        markDailyStudyDone(today);
        setDone(true);
      }
    } catch (e) {
      setError(String(e));
      setQueue([]);
    }
  }, [today]);

  useEffect(() => {
    if (done) return;
    void load();
  }, [done, load]);

  const card = queue?.[index] ?? null;
  const total = queue?.length ?? 0;

  const finish = useCallback(() => {
    markDailyStudyDone(today);
    setDone(true);
    void refreshLearningTerms();
  }, [refreshLearningTerms, today]);

  const rate = useCallback(
    async (rating: string) => {
      if (!card || !queue) return;
      try {
        await api.reviewMemory(card.id, rating);
        const next = index + 1;
        if (next >= queue.length) {
          finish();
          return;
        }
        setIndex(next);
        setFlipped(false);
      } catch (e) {
        setError(String(e));
      }
    },
    [card, finish, index, queue],
  );

  useEffect(() => {
    if (!card) return;
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement;
      if (
        el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.tagName === "BUTTON" ||
          el.tagName === "A" ||
          el.tagName === "SELECT")
      ) {
        return;
      }
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        setFlipped((value) => !value);
      } else if (e.key === "1" || e.key === "2" || e.key === "3") {
        e.preventDefault();
        void rate(e.key === "1" ? "again" : e.key === "2" ? "hard" : "easy");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [card, rate]);

  if (done) {
    return (
      <div className="page daily-study">
        <header className="page-header">
          <h1>今天的单词学完了</h1>
          <p className="muted">可以去读了。想再练可以进生词库。</p>
        </header>
        <div className="row-actions">
          <button type="button" className="btn primary" onClick={() => navigate("/")}>
            开始阅读
          </button>
          <Link className="btn" to="/vocab">
            生词库
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div className="page daily-study">
      <header className="page-header">
        <h1>今日单词</h1>
        <p className="muted">
          每天第一次打开先过一遍生词。先看到期的，再补最近刚存的。
          {total > 0 ? ` ${index + 1} / ${total}` : ""}
        </p>
      </header>
      {error && <p className="banner err">{error}</p>}
      {queue === null && <p className="muted">加载中…</p>}
      {queue && queue.length === 0 && !error && (
        <p className="muted">生词库还是空的。</p>
      )}
      {error && (
        <button type="button" className="btn" onClick={() => void load()}>
          重试
        </button>
      )}
      {card && (
        <div className="review-panel">
          <p className="muted review-meta">
            Space 翻面 · 1/2/3 评分{" "}
            <button
              type="button"
              className="linklike"
              onClick={() => tts.startSpeak({ kind: "word" }, [card.term])}
            >
              发音
            </button>
          </p>
          <button
            type="button"
            className="flashcard"
            aria-pressed={flipped}
            onClick={() => setFlipped((value) => !value)}
          >
            <p className="flash-context">
              “{clipContext(card.context_sentence || card.term)}”
            </p>
            {flipped ? (
              <div className="flash-back">
                <div className="flash-term">{card.term}</div>
                <p>{card.definition_zh || "还没有释义"}</p>
                {card.word_type && <p className="pill inline">{card.word_type}</p>}
              </div>
            ) : (
              <p className="muted tip">点击卡片看释义</p>
            )}
          </button>
          <div className="rate-row">
            <button type="button" className="btn" onClick={() => void rate("again")}>
              不认识
            </button>
            <button type="button" className="btn" onClick={() => void rate("hard")}>
              模糊
            </button>
            <button type="button" className="btn primary" onClick={() => void rate("easy")}>
              认识
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
