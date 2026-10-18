import { typedInvoke } from "./invoke";

/**
 * Project Gutenberg import. A book becomes a feed row and each chapter an
 * ordinary article, so nothing else in the app needs a book-specific path.
 */
export const apiBooks = {
  /** Download the catalog mirror on first use (21 MB); no-op afterwards. */
  ensureBookCatalog: () => typedInvoke("ensure_book_catalog"),
  /** Local title/author search over the cached catalog. */
  searchBooks: (query: string, shelf?: string) =>
    typedInvoke("search_books", {
      query,
      shelf: shelf && shelf.trim() ? shelf : null,
    }),
  /** Download + split + insert. Idempotent: re-importing returns the old state. */
  importBook: (gid: number) => typedInvoke("import_book", { gid }),
};
