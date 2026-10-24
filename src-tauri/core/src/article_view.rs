//! Reader payload assembly: the full article plus its reflowed paragraphs and
//! cached paragraph translations, in one invoke result.

use crate::db::{self, Article, DbState, TranslationRow};
use crate::error::AppError;
use crate::feeds;
use serde::Serialize;
use ts_rs::TS;

/// Chapter position for a book chapter, so the reader can show "12 / 135" and
/// offer previous/next without a second round trip. `None` for every article
/// that is not a chapter.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct BookContext {
    /// Book title — the chapter's `source`, so no extra lookup is needed.
    pub title: String,
    #[ts(type = "number")]
    pub chapter_no: i64,
    #[ts(type = "number")]
    pub chapter_count: i64,
    pub prev_id: Option<String>,
    pub next_id: Option<String>,
}

#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct ArticleView {
    pub article: Article,
    pub paragraphs: Vec<String>,
    pub translations: Vec<TranslationRow>,
    #[serde(default)]
    pub book: Option<BookContext>,
}

/// Assemble the reader payload. Locks are held only while rows are fetched;
/// reflow runs on the borrowed article after the read lock is released, so a
/// long article never blocks other readers.
pub fn load_article_view(state: &DbState, id: &str) -> Result<Option<ArticleView>, AppError> {
    let article = {
        let conn = state.lock_read()?;
        db::get_article(&conn, id)?
    };
    let Some(article) = article else {
        return Ok(None);
    };
    let paragraphs = feeds::split_paragraphs(&article.content_text);
    let translations = {
        let conn = state.lock_read()?;
        db::list_paragraph_translations(&conn, id)?
    };
    // Only queried for chapters so ordinary articles pay nothing.
    let book = if article.origin == "book" {
        let conn = state.lock_read()?;
        db::book_chapter_position(&conn, id)?.map(|pos| BookContext {
            title: article.source.clone(),
            chapter_no: pos.chapter_no,
            chapter_count: pos.chapter_count,
            prev_id: pos.prev_id,
            next_id: pos.next_id,
        })
    } else {
        None
    };
    Ok(Some(ArticleView {
        article,
        paragraphs,
        translations,
        book,
    }))
}
