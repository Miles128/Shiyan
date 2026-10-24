//! Public-domain books as subscriptions.
//!
//! Design: **one book = one `feed_sources` row** (`kind='book'`), **one chapter
//! = one ordinary article** (`origin='book'`, `chapter_no=n`). That is not a
//! shortcut — it is the only shape the reader can render: `Reader` maps over
//! every paragraph of an article with no virtualization, so a 200k-word book in
//! one row would be thousands of paragraphs of React spans. Chapters are
//! ordinary articles, so reading, lookup, translation, stats, the sidebar
//! tree and the source filter all work unchanged, and the refresh pipeline
//! skips books (`feeds::pipeline::select_enabled_feeds`).
//!
//! - [`catalog`] — the Gutenberg catalog mirror + local search
//! - [`text`] — plain-text cleanup: PG header/footer, hard line-wrap recovery
//! - [`chapters`] — heading detection and splitting
//! - [`import`] — download, split, write feed + chapters in one transaction

pub mod catalog;
pub mod chapters;
pub mod import;
pub mod text;

pub use catalog::{ensure_catalog, search, CatalogStatus};
pub use import::{import_book, ImportedBook};

#[cfg(test)]
mod e2e_tests {
    use crate::books::{ensure_catalog, import_book, search};
    use crate::db::{self, DbState};
    use uuid::Uuid;

    /// Real-network probe of the whole chain: download the 21 MB catalog,
    /// search it, import a book, import it again. Ignored by default (catalog
    /// plus a full book body is ~22 MB) — run it after touching any of the
    /// catalog / chapters / import code:
    ///
    /// ```text
    /// cargo test -p shiyan-core books::e2e_tests -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "下载 21 MB 目录 + 整本正文，只在人工核对导入链路时执行"]
    fn catalog_search_import_end_to_end() {
        let dir = std::env::temp_dir().join(format!("shiyan-books-e2e-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let state = DbState::open(db::db_path(dir.clone())).expect("open state");

        let status = ensure_catalog(&state).expect("ensure catalog");
        assert!(
            status.count > 50_000,
            "catalog too small: {} rows",
            status.count
        );

        let hits = search(&state, "moby dick", "", 20).expect("search");
        let moby = hits
            .iter()
            .find(|b| b.gid == 2701)
            .expect("Moby Dick missing from search");
        assert!(moby.title.contains("Moby Dick"));
        assert!(!moby.imported, "fresh catalog must not report imported");

        let first = import_book(&state, moby.gid).expect("import");
        assert!(first.imported_now);
        assert!(
            first.chapter_count >= 130,
            "expected ~135 chapters, got {}",
            first.chapter_count
        );

        // Re-import is a no-op that hands back the same entry point.
        let again = import_book(&state, moby.gid).expect("re-import");
        assert!(!again.imported_now, "second import must not re-download");
        assert_eq!(again.first_article_id, first.first_article_id);
        assert_eq!(again.chapter_count, first.chapter_count);

        // Search now shows the book as imported.
        let hits = search(&state, "moby dick", "", 20).expect("search again");
        assert!(hits.iter().any(|b| b.gid == 2701 && b.imported));

        let conn = state.lock_read().expect("read lock");
        let kind: String = conn
            .query_row(
                "SELECT kind FROM feed_sources WHERE url LIKE '%/ebooks/2701'",
                [],
                |row| row.get(0),
            )
            .expect("book feed row");
        assert_eq!(kind, "book");

        // Chapters land as ordinary articles, numbered 1..N with no gaps.
        let (rows, min_no, max_no): (i64, i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), MIN(chapter_no), MAX(chapter_no) FROM articles
                  WHERE origin='book' AND source=?1",
                rusqlite::params![first.feed.name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("chapter stats");
        assert_eq!(rows, first.chapter_count);
        assert_eq!(min_no, 1);
        assert_eq!(max_no, first.chapter_count);
        // No blurb queued for chapters: they must stay out of the LLM backfill.
        let blank: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles
                  WHERE origin='book' AND source=?1 AND summary_zh=''",
                rusqlite::params![first.feed.name],
                |row| row.get(0),
            )
            .expect("blank summaries");
        assert_eq!(blank, rows);

        let pos = db::book_chapter_position(&conn, &first.first_article_id)
            .expect("position query")
            .expect("first chapter has a position");
        assert_eq!(pos.chapter_no, 1);
        assert_eq!(pos.chapter_count, first.chapter_count);
        assert!(pos.prev_id.is_none());
        assert!(pos.next_id.is_some());

        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
