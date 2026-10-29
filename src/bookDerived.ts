/**
 * Book-domain presentation helpers (Project Gutenberg import).
 *
 * Kept out of the dialog component so the string rules are testable in the
 * node-only vitest setup, same as `homeDerived` / `wordResolve`.
 */

/**
 * Catalog authors are published as `Melville, Herman, 1819-1891`. Flip to
 * `Herman Melville` and drop the life dates; anything unrecognised is returned
 * unchanged rather than guessed at (mirrors the Rust `display_author`).
 */
export function formatAuthor(raw: string): string {
  const first = raw.split(";")[0]?.trim() ?? "";
  if (!first) return "";
  const parts = first
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length >= 2 && /\d/.test(parts[parts.length - 1])) {
    parts.pop();
  }
  if (parts.length === 2) return `${parts[1]} ${parts[0]}`;
  if (parts.length === 1) return parts[0];
  return first;
}

/** `Herman Melville · 1851`, or whichever half exists, or ''. */
export function bookMeta(authors: string, issued: string): string {
  const author = formatAuthor(authors);
  const year = issued.slice(0, 4).trim();
  if (author && year) return `${author} · ${year}`;
  return author || year;
}

/**
 * Genre filters offered by the search dialog. Values are the catalog's own
 * `Category:` names (verified against the CSV distribution: each of these has
 * 2,900+ English titles), labels are what the UI shows.
 */
export const BOOK_SHELVES: ReadonlyArray<{ value: string; label: string }> = [
  { value: "Novels", label: "小说" },
  { value: "Poetry", label: "诗歌" },
  { value: "Short Stories", label: "短篇" },
  { value: "Historical Novels", label: "历史小说" },
  { value: "Essays, Letters & Speeches", label: "随笔" },
  { value: "Science-Fiction & Fantasy", label: "科幻奇幻" },
];
