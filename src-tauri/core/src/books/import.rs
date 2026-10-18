//! Import one Gutenberg book: download, split into chapters, write the feed row
//! plus one article per chapter, all in a single transaction.
//!
//! Idempotent by book url — importing a book that is already in the library
//! returns the existing state without touching the network, so the picker can
//! offer 导入 on any row without a pre-check on the UI side.

use crate::books::{chapters, text};
use crate::db::{self, Article, DbState, FeedSource};
use crate::error::AppError;
use crate::feeds::net::{http_client, read_limited_bytes};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Metadata lives on gutenberg.org; the body is the cached plain text, which is
/// what the reader needs (no HTML extraction, no epub unzip).
pub fn book_page_url(gid: i64) -> String {
    format!("https://www.gutenberg.org/ebooks/{gid}")
}

pub fn book_text_url(gid: i64) -> String {
    format!("https://www.gutenberg.org/cache/epub/{gid}/pg{gid}.txt")
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportedBook {
    pub feed: FeedSource,
    pub title: String,
    pub author: String,
    #[ts(type = "number")]
    pub chapter_count: i64,
    /// Reader lands here after import.
    pub first_article_id: String,
    /// False when the book was already in the library (no work done).
    pub imported_now: bool,
}

pub fn import_book(db: &DbState, gid: i64) -> Result<ImportedBook, AppError> {
    let book = {
        let conn = db.lock_read()?;
        db::get_catalog_book(&conn, gid)?
    }
    .ok_or_else(|| AppError::msg("目录里没有这本书，请重新搜索"))?;

    let url = book_page_url(gid);
    let author = display_author(&book.authors);
    let description = book_description(&author, &book.issued);

    // Creating the feed first makes a repeat import a no-op below: the row is
    // idempotent by url, and an existing row with chapters means we are done.
    let feed = {
        let conn = db.lock_write()?;
        db::insert_book_feed(&conn, &book.title, &url, &description)?
    };

    if let Some(existing) = existing_chapters(db, &url)? {
        return Ok(ImportedBook {
            first_article_id: existing.0,
            chapter_count: existing.1,
            title: feed.name.clone(),
            author,
            feed,
            imported_now: false,
        });
    }

    let client = http_client()?;
    let resp = client
        .get(book_text_url(gid))
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| AppError::msg(format!("下载正文失败：{e}")))?;
    let bytes = read_limited_bytes(resp)?;
    let raw = String::from_utf8_lossy(&bytes);

    let body = text::strip_markers(&raw);
    let blocks = text::blocks(body);
    let parsed = chapters::split(&blocks);
    if parsed.is_empty() {
        return Err(AppError::msg("没能从这本书里提取到正文"));
    }

    let fetched_at = Utc::now().to_rfc3339();
    let mut first_article_id = String::new();
    let chapter_count;
    {
        let conn = db.lock_write()?;
        let tx = conn.unchecked_transaction()?;
        for (index, chapter) in parsed.iter().enumerate() {
            let no = index as i64 + 1;
            let article = Article {
                id: Uuid::new_v4().to_string(),
                // The fragment doubles as the chapter key: neighbour lookups
                // match on `<book url>#ch-<n>` (see db::book_chapter_position).
                url: format!("{url}#ch-{no}"),
                title: chapter.title.clone(),
                source: feed.name.clone(),
                category: "book".into(),
                published_at: None,
                content_text: chapter.text.clone(),
                fetched_at: fetched_at.clone(),
                // 'book' is what keeps retention purges away from chapters:
                // every purge predicate in the DB filters on origin='rss'.
                origin: "book".into(),
                // No blurb: a book of 135 chapters must never queue 135 LLM
                // calls. Chapters are excluded from the card backfill too.
                summary_zh: String::new(),
                last_opened_at: None,
                open_count: 0,
                word_count: chapter.words() as i64,
                quality: "fulltext".into(),
                extraction_source: "gutenberg".into(),
                dwell_ms: 0,
                read_completed: false,
                liked: false,
            };
            db::insert_book_chapter(&tx, &article, no)?;
            if no == 1 {
                first_article_id = article.id;
            }
        }
        tx.commit()?;
        chapter_count = parsed.len() as i64;
    }
    if first_article_id.is_empty() {
        return Err(AppError::msg("导入失败：章节未写入"));
    }

    Ok(ImportedBook {
        feed,
        title: book.title,
        author,
        chapter_count,
        first_article_id,
        imported_now: true,
    })
}

/// (first chapter id, chapter count) when the book is already imported.
fn existing_chapters(db: &DbState, book_url: &str) -> Result<Option<(String, i64)>, AppError> {
    let conn = db.lock_read()?;
    let prefix = format!("{book_url}#ch-");
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM articles WHERE url LIKE ?1 || '%'",
        rusqlite::params![prefix],
        |row| row.get(0),
    )?;
    if count == 0 {
        return Ok(None);
    }
    let first: String = conn.query_row(
        "SELECT id FROM articles WHERE url = ?1 || '1'",
        rusqlite::params![prefix],
        |row| row.get(0),
    )?;
    Ok(Some((first, count)))
}

/// `Melville, Herman, 1819-1891` → `Herman Melville`. Best effort: any shape we
/// do not recognise is returned as published rather than guessed at.
pub fn display_author(raw: &str) -> String {
    let first = raw.split(';').next().unwrap_or("").trim();
    if first.is_empty() {
        return String::new();
    }
    let mut parts: Vec<&str> = first.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 && parts.last().is_some_and(|p| p.chars().any(|c| c.is_ascii_digit())) {
        parts.pop();
    }
    match parts.len() {
        2 => format!("{} {}", parts[1], parts[0]),
        1 => parts[0].to_string(),
        _ => first.to_string(),
    }
}

/// `Herman Melville · 1851` — the subtitle under the book in the sidebar/list.
pub fn book_description(author: &str, issued: &str) -> String {
    let year: String = issued.chars().take(4).collect();
    match (author.is_empty(), year.trim().is_empty()) {
        (false, false) => format!("{author} · {year}"),
        (false, true) => author.to_string(),
        (true, false) => year,
        (true, true) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_is_reordered_and_datedropped() {
        assert_eq!(display_author("Melville, Herman, 1819-1891"), "Herman Melville");
        assert_eq!(display_author("Austen, Jane"), "Jane Austen");
        assert_eq!(display_author("Anonymous"), "Anonymous");
        assert_eq!(display_author("Eliot, George; Lewes, G. H."), "George Eliot");
        assert_eq!(display_author(""), "");
    }

    #[test]
    fn description_pairs_author_and_year() {
        assert_eq!(book_description("Herman Melville", "1851-11-14"), "Herman Melville · 1851");
        assert_eq!(book_description("Herman Melville", ""), "Herman Melville");
        assert_eq!(book_description("", "1851-11-14"), "1851");
    }

    #[test]
    fn urls_are_stable() {
        assert_eq!(book_page_url(2701), "https://www.gutenberg.org/ebooks/2701");
        assert_eq!(
            book_text_url(2701),
            "https://www.gutenberg.org/cache/epub/2701/pg2701.txt"
        );
    }
}
