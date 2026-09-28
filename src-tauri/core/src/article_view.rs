//! Reader payload assembly: the full article plus its reflowed paragraphs and
//! cached paragraph translations, in one invoke result.

use crate::db::{self, Article, TranslationRow};
use crate::error::AppError;
use crate::feeds;
use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct ArticleView {
    pub article: Article,
    pub paragraphs: Vec<String>,
    pub translations: Vec<TranslationRow>,
}

pub fn load_article_view(conn: &Connection, id: &str) -> Result<Option<ArticleView>, AppError> {
    let Some(article) = db::get_article(conn, id)? else {
        return Ok(None);
    };
    let paragraphs = feeds::split_paragraphs(&article.content_text);
    let translations = db::list_paragraph_translations(conn, id)?;
    Ok(Some(ArticleView {
        article,
        paragraphs,
        translations,
    }))
}
