//! Reader payload assembly: the full article plus its reflowed paragraphs and
//! cached paragraph translations, in one invoke result.

use crate::db::{self, Article, TranslationRow};
use crate::error::AppError;
use crate::feeds;
use rusqlite::Connection;
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

pub fn load_article_view(conn: &Connection, id: &str) -> Result<Option<ArticleView>, AppError> {
    let Some(article) = db::get_article(conn, id)? else {
        return Ok(None);
    };
    let paragraphs = feeds::split_paragraphs(&article.content_text);
    let translations = db::list_paragraph_translations(conn, id)?;
    let book = if article.origin == "book" {
        db::book_chapter_position(conn, id)?.map(|pos| BookContext {
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
