import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api } from "../api";
import {
  BOOK_CATALOG,
  BOOK_INTERESTS,
  BOOK_LENGTHS,
  gutenbergTextUrl,
  type BookInterestId,
  type BookLengthId,
} from "../books/catalog";
import {
  inferBookInterests,
  searchBooks,
  type DifficultyChoice,
} from "../books/search";
import BookCard from "../components/BookCard";
import PageBack from "../components/PageBack";
import { useToast } from "../components/Toaster";
import { useAppConfig } from "../store";
import { isCefrLevel, type CefrLevel } from "../wordLevels";

const DIFFICULTY_CHOICES: { id: DifficultyChoice; label: string }[] = [
  { id: "mine", label: "适合我" },
  { id: "easier", label: "更简单" },
  { id: "stretch", label: "挑战一点" },
  { id: "A2", label: "A2" },
  { id: "B1", label: "B1" },
  { id: "B2", label: "B2" },
  { id: "C1", label: "C1" },
  { id: "C2", label: "C2" },
  { id: "all", label: "全部难度" },
];

export default function Books() {
  const { cfg } = useAppConfig();
  const userCefr: CefrLevel = isCefrLevel(cfg.cefr_level) ? cfg.cefr_level : "B1";
  const navigate = useNavigate();
  const toast = useToast();
  const [text, setText] = useState("");
  const [difficulty, setDifficulty] = useState<DifficultyChoice>("mine");
  const [interests, setInterests] = useState<BookInterestId[]>([]);
  const [interestsReady, setInterestsReady] = useState(false);
  const interestsTouched = useRef(false);
  const [personal, setPersonal] = useState<BookInterestId[]>([]);
  const [length, setLength] = useState<BookLengthId | "all">("all");
  const [downloaded, setDownloaded] = useState<Map<string, string>>(new Map());
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [sample, stats] = await Promise.all([
          api.listArticlesRanked(undefined, undefined, undefined, false, 80, 0),
          api.getLearningStats().catch(() => null),
        ]);
        if (cancelled) return;
        const tags = sample.flatMap((article) => article.tags ?? []);
        const inferred = inferBookInterests({
          tags,
          topCategory: stats?.top_category,
        });
        setPersonal(inferred);
        if (!interestsTouched.current) setInterests(inferred);
      } catch {
        if (!cancelled) setInterests([]);
      } finally {
        if (!cancelled) setInterestsReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    void api
      .listBookArticles()
      .then((rows) => {
        setDownloaded(new Map(rows.map((row) => [row.url, row.id])));
      })
      .catch(() => undefined);
  }, []);

  const results = useMemo(() => {
    if (!interestsReady) return [];
    return searchBooks(BOOK_CATALOG, {
      text,
      difficulty,
      interests,
      length,
      userCefr,
    });
  }, [interestsReady, text, difficulty, interests, length, userCefr]);

  function selectInterests(next: BookInterestId[]) {
    interestsTouched.current = true;
    setInterests(next);
  }

  function toggleInterest(id: BookInterestId) {
    interestsTouched.current = true;
    setInterests((current) =>
      current.includes(id) ? current.filter((item) => item !== id) : [...current, id],
    );
  }

  async function openBook(bookId: string) {
    const book = BOOK_CATALOG.find((item) => item.id === bookId);
    if (!book || busyId) return;
    const url = gutenbergTextUrl(book.gutenbergId);
    const existing = downloaded.get(url);
    if (existing) {
      navigate(`/article/${existing}`);
      return;
    }
    setBusyId(book.id);
    try {
      const article = await api.importBook({
        url,
        title: book.title,
        author: book.author,
        category: "other",
        summaryZh: book.summaryZh,
        tags: book.interests,
      });
      setDownloaded((prev) => new Map(prev).set(article.url, article.id));
      navigate(`/article/${article.id}`);
    } catch (e) {
      toast.err(String(e));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="page books-page">
      <header className="page-header page-header-slim">
        <div>
          <h1>书库</h1>
          <p className="muted">
            按词汇量、兴趣和篇幅找一本公版英文书。当前水平 {userCefr}
            ，下载后留在本地。
          </p>
        </div>
        <PageBack />
      </header>

      <form
        role="search"
        className="book-search"
        onSubmit={(e) => e.preventDefault()}
      >
        <input
          type="search"
          className="search"
          value={text}
          placeholder="搜索书名、作者或书里的词"
          aria-label="搜索书名、作者或书里的词"
          onChange={(e) => setText(e.target.value)}
        />
      </form>

      <div className="book-filters">
        <div className="filter-block">
          <span className="filter-label">词汇量</span>
          <div className="filter-row" role="group" aria-label="词汇量">
            {DIFFICULTY_CHOICES.map((choice) => (
              <button
                key={choice.id}
                type="button"
                className={difficulty === choice.id ? "tag-chip active" : "tag-chip"}
                aria-pressed={difficulty === choice.id}
                onClick={() => setDifficulty(choice.id)}
              >
                {choice.label}
              </button>
            ))}
          </div>
        </div>

        <div className="filter-block">
          <span className="filter-label">兴趣</span>
          <div className="filter-row" role="group" aria-label="兴趣">
            {personal.length > 0 && (
              <button
                type="button"
                className={
                  sameSet(interests, personal) ? "tag-chip active" : "tag-chip"
                }
                aria-pressed={sameSet(interests, personal)}
                onClick={() => selectInterests(personal)}
              >
                我的兴趣
              </button>
            )}
            <button
              type="button"
              className={interests.length === 0 ? "tag-chip active" : "tag-chip"}
              aria-pressed={interests.length === 0}
              onClick={() => selectInterests([])}
            >
              全部
            </button>
            {BOOK_INTERESTS.map((item) => (
              <button
                key={item.id}
                type="button"
                className={interests.includes(item.id) ? "tag-chip active" : "tag-chip"}
                aria-pressed={interests.includes(item.id)}
                onClick={() => toggleInterest(item.id)}
              >
                {item.label}
              </button>
            ))}
          </div>
        </div>

        <div className="filter-block">
          <span className="filter-label">篇幅</span>
          <div className="filter-row" role="group" aria-label="篇幅">
            <button
              type="button"
              className={length === "all" ? "tag-chip active" : "tag-chip"}
              aria-pressed={length === "all"}
              onClick={() => setLength("all")}
            >
              全部
            </button>
            {BOOK_LENGTHS.map((band) => (
              <button
                key={band.id}
                type="button"
                className={length === band.id ? "tag-chip active" : "tag-chip"}
                aria-pressed={length === band.id}
                title={band.hint}
                onClick={() => setLength(band.id)}
              >
                {band.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      <p className="muted book-result-count">
        {interestsReady ? `${results.length} 本` : "正在按你的阅读兴趣筛选…"}
      </p>

      {results.length === 0 ? (
        <div className="empty">
          <p>没有同时符合词汇量、兴趣和篇幅的书。放宽一个条件再看看。</p>
        </div>
      ) : (
        <div className="book-grid">
          {results.map((book) => (
            <BookCard
              key={book.id}
              book={book}
              downloadedId={downloaded.get(gutenbergTextUrl(book.gutenbergId))}
              busy={busyId === book.id}
              onOpen={() => void openBook(book.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function sameSet(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  const left = [...a].sort();
  const right = [...b].sort();
  return left.every((item, i) => item === right[i]);
}
