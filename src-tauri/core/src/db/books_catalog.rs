//! Local mirror of the Project Gutenberg catalog (`book_catalog`).
//!
//! Downloading + parsing the CSV lives in `crate::books::catalog`; this module
//! only stores and queries the rows. Search is a plain LIKE scan ordered by
//! relevance — 62k English rows scan in single-digit milliseconds, and a full
//! text index would need tokenization this app never uses anywhere else.

use super::{row_as, CatalogBook};
use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};

/// app_meta key holding the RFC3339 timestamp of the last catalog download.
pub const CATALOG_STAMP_KEY: &str = "book_catalog_updated_at";

pub fn catalog_count(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row("SELECT COUNT(*) FROM book_catalog", [], |row| row.get(0))
        .map_err(AppError::from)
}

/// Empty the mirror before a refresh. Callers wrap this in a transaction
/// together with the row inserts, so a failed download never leaves a half
/// catalog behind.
pub fn clear_catalog(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM book_catalog", [])?;
    Ok(())
}

/// One catalog row. Streaming on purpose: the CSV holds ~62k English rows and
/// materializing them all first would cost tens of MB for no gain.
pub fn insert_catalog_row(conn: &Connection, b: &CatalogBook) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO book_catalog (gid,title,authors,issued,shelf,subjects)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![b.gid, b.title, b.authors, b.issued, b.shelf, b.subjects],
    )?;
    Ok(())
}

pub fn set_catalog_stamp(conn: &Connection, stamp: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![CATALOG_STAMP_KEY, stamp],
    )?;
    Ok(())
}

/// Timestamp of the last catalog refresh, `None` when never downloaded.
pub fn catalog_stamp(conn: &Connection) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT value FROM app_meta WHERE key=?1",
        params![CATALOG_STAMP_KEY],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

/// Title/author search. `shelf` filters on the coarse genre ('' = all).
/// Ordering: title prefix match first, then any title match, then author-only
/// matches; ties by shorter title (more precise), then alphabetically, so the
/// result set is stable across reruns.
pub fn search_catalog(
    conn: &Connection,
    query: &str,
    shelf: &str,
    limit: i64,
) -> Result<Vec<CatalogBook>, AppError> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{}%", super::articles::escape_like_literal(q));
    let prefix = format!("{}%", super::articles::escape_like_literal(q).to_lowercase());
    // `?4 = ''` stands in for "no shelf filter" so both modes share one
    // parameter list and one query_map call.
    let sql = format!(
        "SELECT gid,title,authors,issued,shelf,subjects FROM book_catalog
          WHERE (title LIKE ?1 ESCAPE '\\' OR authors LIKE ?1 ESCAPE '\\')
            AND (?4 = '' OR shelf = ?4)
          ORDER BY CASE
                     WHEN lower(title) LIKE ?2 ESCAPE '\\' THEN 0
                     WHEN title LIKE ?1 ESCAPE '\\' THEN 1
                     ELSE 2
                   END,
                   LENGTH(title) ASC,
                   title ASC
          LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![pattern, prefix, limit.clamp(1, 200), shelf.trim()],
            row_as::<CatalogBook>,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_catalog_book(conn: &Connection, gid: i64) -> Result<Option<CatalogBook>, AppError> {
    conn.query_row(
        "SELECT gid,title,authors,issued,shelf,subjects FROM book_catalog WHERE gid=?1",
        params![gid],
        row_as::<CatalogBook>,
    )
    .optional()
    .map_err(AppError::from)
}

/// Gutenberg ids already imported, resolved from book feed urls. Feed rows are
/// few, so parsing the id out of the url beats a second column to keep in sync.
pub fn imported_gids(conn: &Connection) -> Result<std::collections::HashSet<i64>, AppError> {
    let mut stmt = conn.prepare("SELECT url FROM feed_sources WHERE kind='book'")?;
    let urls = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(urls.iter().filter_map(|u| gid_from_feed_url(u)).collect())
}

/// `https://www.gutenberg.org/ebooks/2701` → 2701
pub fn gid_from_feed_url(url: &str) -> Option<i64> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(|tail| tail.parse::<i64>().ok())
}
