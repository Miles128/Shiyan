import { bookLengthLabel, formatWordCount, interestLabel, type CatalogBook } from "../books/catalog";

type Props = {
  book: CatalogBook;
  downloadedId?: string | null;
  busy?: boolean;
  matched?: string[];
  onOpen: () => void;
  compact?: boolean;
};

export default function BookCard({
  book,
  downloadedId,
  busy,
  matched,
  onOpen,
  compact,
}: Props) {
  const downloaded = Boolean(downloadedId);
  return (
    <article className={compact ? "book-card compact" : "book-card"}>
      <div className="book-card-meta">
        <span className="pill">{book.cefr}</span>
        <span className="pill">{bookLengthLabel(book.words)}</span>
        <span className="muted">{formatWordCount(book.words)}</span>
      </div>
      <h3 className="book-card-title">{book.title}</h3>
      <p className="book-card-author muted">{book.author}</p>
      {!compact && <p className="book-card-summary">{book.summaryZh}</p>}
      <p className="book-card-tags muted">
        {book.interests.map(interestLabel).join(" · ")}
      </p>
      {matched && matched.length > 0 && (
        <p className="book-card-match">含生词：{matched.join("、")}</p>
      )}
      <button
        type="button"
        className="btn small"
        onClick={onOpen}
        disabled={busy}
      >
        {busy ? "正在下载…" : downloaded ? "打开" : "下载并阅读"}
      </button>
    </article>
  );
}
