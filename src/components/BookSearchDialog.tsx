import { useCallback, useEffect, useRef, useState } from "react";
import { api, type CatalogBook, type ImportedBook } from "../api";
import { BOOK_SHELVES, bookMeta } from "../bookDerived";
import { useEscapeKey } from "../useEscapeKey";
import IconBtn from "./IconBtn";

type Props = {
  onClose: () => void;
  /** Called after a successful import: reload feeds, then open the first chapter. */
  onImported: (book: ImportedBook) => void;
};

/**
 * Book download entry (Settings → 书籍). A book lands as a feed row with one
 * article per chapter (see `books::import`), so there is no separate bookshelf
 * screen — imported titles show up in the sidebar under 书籍 and their chapters
 * in the normal article list.
 */
export default function BookSearchDialog({ onClose, onImported }: Props) {
  const [query, setQuery] = useState("");
  const [shelf, setShelf] = useState("");
  const [rows, setRows] = useState<CatalogBook[]>([]);
  const [searching, setSearching] = useState(false);
  const [importing, setImporting] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Null while the catalog mirror is being prepared on first open. */
  const [catalogCount, setCatalogCount] = useState<number | null>(null);
  const [loadingCatalog, setLoadingCatalog] = useState(true);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEscapeKey(true, onClose);

  // The catalog is a 21 MB download on first use; do it when the dialog opens
  // rather than on the first keystroke, so the search box is usable by the time
  // the user has typed something.
  useEffect(() => {
    let alive = true;
    void api
      .ensureBookCatalog()
      .then((status) => {
        if (alive) setCatalogCount(status.count);
      })
      .catch((e) => {
        if (alive) setError(`书籍目录准备失败：${String(e)}`);
      })
      .finally(() => {
        if (alive) setLoadingCatalog(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const runSearch = useCallback(
    async (nextShelf: string) => {
      const q = query.trim();
      if (!q) {
        setRows([]);
        return;
      }
      setSearching(true);
      setError(null);
      try {
        setRows(await api.searchBooks(q, nextShelf));
      } catch (e) {
        setError(`搜索失败：${String(e)}`);
      } finally {
        setSearching(false);
      }
    },
    [query],
  );

  async function onImport(book: CatalogBook) {
    if (book.imported) return;
    setImporting(book.gid);
    setError(null);
    try {
      const imported = await api.importBook(book.gid);
      setRows((prev) =>
        prev.map((r) => (r.gid === book.gid ? { ...r, imported: true } : r)),
      );
      onImported(imported);
    } catch (e) {
      setError(`导入失败：${String(e)}`);
    } finally {
      setImporting(null);
    }
  }

  return (
    <div className="book-dialog-scrim" onClick={onClose} role="presentation">
      <div
        className="book-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="搜索并导入英文书籍"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="book-dialog-head">
          <h2>搜索 Project Gutenberg</h2>
          <IconBtn icon="✕" title="关闭" onClick={onClose} />
        </div>

        <p className="muted book-dialog-hint">
          {loadingCatalog
            ? "首次使用需下载官方目录（21 MB），之后搜索在本地完成…"
            : catalogCount != null
              ? `本地目录 ${catalogCount.toLocaleString()} 本英文书 · 公版 · 无账号`
              : "目录未就绪"}
        </p>

        <form
          className="feeds-paste book-search-form"
          onSubmit={(e) => {
            e.preventDefault();
            void runSearch(shelf);
          }}
        >
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="书名或作者，例如 Middlemarch / George Eliot"
            disabled={loadingCatalog || searching}
          />
          <button
            type="submit"
            className="btn primary"
            disabled={loadingCatalog || searching || !query.trim()}
          >
            {searching ? "搜索中…" : "搜索"}
          </button>
        </form>

        <div className="book-shelf-row">
          <button
            type="button"
            className={shelf === "" ? "tag-chip active" : "tag-chip"}
            onClick={() => {
              setShelf("");
              if (rows.length > 0) void runSearch("");
            }}
          >
            全部
          </button>
          {BOOK_SHELVES.map((s) => (
            <button
              key={s.value}
              type="button"
              className={shelf === s.value ? "tag-chip active" : "tag-chip"}
              onClick={() => {
                setShelf(s.value);
                if (rows.length > 0) void runSearch(s.value);
              }}
            >
              {s.label}
            </button>
          ))}
        </div>

        {error && <p className="banner err">{error}</p>}

        {rows.length === 0 && !searching && !error && (
          <p className="muted book-empty">
            搜一本公版英文书，导入后每章会作为一篇文章出现在「书籍」分类下。
          </p>
        )}

        <ul className="book-result-list">
          {rows.map((b) => (
            <li key={b.gid} className="book-result-row">
              <div className="book-result-main">
                <strong>{b.title}</strong>
                <p className="muted">
                  {bookMeta(b.authors, b.issued)}
                  {b.shelf ? ` · ${b.shelf}` : ""}
                </p>
              </div>
              <button
                type="button"
                className={b.imported ? "btn small" : "btn primary small"}
                disabled={b.imported || importing === b.gid}
                onClick={() => void onImport(b)}
              >
                {b.imported
                  ? "已导入"
                  : importing === b.gid
                    ? "导入中…"
                    : "导入"}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
