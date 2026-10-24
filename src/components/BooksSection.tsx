import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, FeedSource, ImportedBook } from "../api";
import { useFeeds, useFilters } from "../store";
import BookSearchDialog from "./BookSearchDialog";
import { useToast } from "./Toaster";

/**
 * Settings → 书籍. Book download/import is deliberately separate from the RSS
 * subscription manager: books reuse the feed/article tables, but mixing them
 * into the RSS list made two different workflows look like one.
 */
export default function BooksSection() {
  const toast = useToast();
  const navigate = useNavigate();
  const { feeds, reloadFeeds } = useFeeds();
  const { setFocusSource } = useFilters();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const books = useMemo(
    () => feeds.filter((f) => f.kind === "book"),
    [feeds],
  );

  async function onImported(book: ImportedBook) {
    setDialogOpen(false);
    toast.ok(`已下载《${book.title}》·${book.chapter_count} 章`);
    try {
      await reloadFeeds();
    } catch {
      // The reader landing is the real feedback; the tree refreshes later.
    }
    navigate(`/article/${book.first_article_id}`);
  }

  async function onDelete(book: FeedSource) {
    if (
      !window.confirm(
        `删除书籍「${book.name}」？它的全部章节会一并删除，且需要重新下载。`,
      )
    ) {
      return;
    }
    setDeleting(book.id);
    setError(null);
    try {
      await api.deleteFeedSource(book.id);
      await reloadFeeds();
      toast.ok(`已删除书籍：${book.name}`);
    } catch (e) {
      setError(`删除书籍失败：${String(e)}`);
    } finally {
      setDeleting(null);
    }
  }

  function onOpen(book: FeedSource) {
    setFocusSource(book.name);
    navigate("/");
  }

  return (
    <section className="settings-section">
      <h2>书籍</h2>
      <p className="muted">
        从 Project Gutenberg 下载公版英文书。下载后每章作为一篇文章，出现在侧边栏「书籍」分类下。
      </p>
      <div className="data-actions">
        <button
          className="btn primary"
          type="button"
          onClick={() => setDialogOpen(true)}
        >
          搜索并下载书籍
        </button>
      </div>

      {error && <p className="banner err">{error}</p>}

      <ul className="feed-list">
        {books.length === 0 && <li className="muted">还没有下载过书籍。</li>}
        {books.map((book) => (
          <li key={book.id} className="feed-row-item">
            <div className="feed-row-main">
              <div className="feed-row-title">
                <strong>{book.name}</strong>
                {book.description && (
                  <span className="muted feed-desc">{book.description}</span>
                )}
              </div>
            </div>
            <button
              type="button"
              className="btn small"
              onClick={() => onOpen(book)}
            >
              打开
            </button>
            <button
              type="button"
              className="icon-btn feed-delete"
              title="删除书籍"
              aria-label={`删除书籍 ${book.name}`}
              disabled={deleting === book.id}
              onClick={() => void onDelete(book)}
            >
              ✕
            </button>
          </li>
        ))}
      </ul>

      {dialogOpen && (
        <BookSearchDialog
          onClose={() => setDialogOpen(false)}
          onImported={(book) => void onImported(book)}
        />
      )}
    </section>
  );
}
