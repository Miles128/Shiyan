use crate::db::{self, Article, ArticleListItem, DbState, LearningStats, TranslationRow};
use crate::error::AppError;
use crate::feeds;
use crate::import_file;
use crate::translate;
use crate::vocab;
use chrono::Datelike;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

#[derive(Clone, serde::Serialize, ts_rs::TS)]
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

#[tauri::command]
pub async fn list_articles(
    app: AppHandle,
    category: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ArticleListItem>, AppError> {
    crate::commands::spawn_db(app, move |state| {
        let conn = state.lock_read()?;
        db::list_articles(&conn, category.as_deref(), limit, offset)
    })
    .await
}

/// Ranked window size for interest scoring. Wide enough that cursor pages can
/// reach past the first few screens of a daily reading session.
const RANK_WINDOW: i64 = 800;

/// Cursor + paging params must stay flat for the `invoke` contract, hence the
/// arity.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn list_articles_ranked(
    app: AppHandle,
    category: Option<String>,
    tags: Option<Vec<String>>,
    source: Option<String>,
    unread_only: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
    cursor_score: Option<f64>,
    cursor_id: Option<String>,
) -> Result<Vec<ArticleListItem>, AppError> {
    let tags = tags.unwrap_or_default();
    crate::commands::spawn_db(app, move |state| {
        let items = {
            let conn = state.lock_read()?;
            db::query_articles(
                &conn,
                &db::ArticleQuery {
                    category: category.as_deref(),
                    tags: &tags,
                    source: source.as_deref(),
                    read_state: if unread_only.unwrap_or(false) {
                        db::ReadState::Unfinished
                    } else {
                        db::ReadState::All
                    },
                    liked_only: false,
                    recent_first: false,
                },
                Some(RANK_WINDOW),
                Some(0),
            )?
        };
        let (source_opens, category_opens) = {
            let conn = state.lock_read()?;
            db::affinity_open_counts(&conn)?
        };
        let (tag_weights, tag_doc_counts, tagged_docs) = {
            let conn = state.lock_read()?;
            db::tag_profile(&conn)?
        };
        let affinity = crate::rank::Affinity::from_maps(
            source_opens,
            category_opens,
            tag_weights,
            tag_doc_counts,
            tagged_docs,
        );
        let now = chrono::Utc::now();
        let day_key = i64::from(now.num_days_from_ce());
        let ranked = crate::rank::rank_articles(items, &affinity, now, day_key);
        let cursor = match (cursor_score, cursor_id) {
            (Some(score), Some(id)) => Some((score, id)),
            _ => None,
        };
        let start = offset.unwrap_or(0).max(0) as usize;
        let take = limit.unwrap_or(60).max(0) as usize;
        Ok(crate::rank::page_ranked(ranked, cursor, start, take))
    })
    .await
}

/// Library listing: every article with the full filter set, newest first.
#[tauri::command]
pub async fn list_library(
    app: AppHandle,
    category: Option<String>,
    tags: Option<Vec<String>>,
    source: Option<String>,
    read_state: Option<String>,
    liked_only: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ArticleListItem>, AppError> {
    let tags = tags.unwrap_or_default();
    let read_state = match read_state.as_deref() {
        Some("unread") => db::ReadState::Unread,
        Some("reading") => db::ReadState::Reading,
        Some("unfinished") => db::ReadState::Unfinished,
        Some("read") => db::ReadState::Read,
        _ => db::ReadState::All,
    };
    crate::commands::spawn_db(app, move |state| {
        let conn = state.lock_read()?;
        db::query_articles(
            &conn,
            &db::ArticleQuery {
                category: category.as_deref(),
                tags: &tags,
                source: source.as_deref(),
                read_state,
                liked_only: liked_only.unwrap_or(false),
                recent_first: true,
            },
            limit,
            offset,
        )
    })
    .await
}

/// Distinct sources with counts for the library source filter.
#[tauri::command]
pub fn list_article_sources(
    state: tauri::State<'_, DbState>,
) -> Result<Vec<(String, i64)>, AppError> {
    let conn = state.lock_read()?;
    db::list_article_sources(&conn)
}

/// One-off translation without article caching (used outside the reader,
/// e.g. selecting a word on the home list). Goes through the LLM, no DB write.
#[tauri::command]
pub async fn translate_plain_text(text: String) -> Result<String, AppError> {
    let cfg = crate::config::load_config()?;
    crate::commands::spawn_blocking_err(move || vocab::translate_text(&cfg, &text)).await
}

/// Backfill topic tags (bounded per call; refresh also runs a batch).
#[tauri::command]
pub async fn fill_missing_tags(app: AppHandle, limit: Option<usize>) -> Result<usize, AppError> {
    let cfg = crate::config::load_config()?;
    if cfg.api_key.trim().is_empty() {
        return Ok(0);
    }
    crate::commands::spawn_db(app, move |state| {
        feeds::fill_missing_tags(state, &cfg, limit.unwrap_or(200), |_, _| {})
    })
    .await
}

#[tauri::command]
pub fn mark_article_progress(
    state: tauri::State<'_, DbState>,
    id: String,
    dwell_ms_delta: i64,
    read_completed: bool,
) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::add_article_reading_progress(
        &conn,
        &id,
        dwell_ms_delta,
        read_completed,
    )
}

#[tauri::command]
pub fn set_article_liked(
    state: tauri::State<'_, DbState>,
    id: String,
    liked: bool,
) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::set_article_liked(&conn, &id, liked)
}

#[tauri::command]
pub fn get_article_view(
    state: tauri::State<'_, DbState>,
    id: String,
) -> Result<Option<ArticleView>, AppError> {
    let conn = state.lock_read()?;
    load_article_view(&conn, &id)
}

#[tauri::command]
pub fn mark_article_opened(
    state: tauri::State<'_, DbState>,
    id: String,
) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::mark_article_opened(&conn, &id)
}

/// Reading statistics for the stats page.
#[tauri::command]
pub fn get_reading_stats(
    state: tauri::State<'_, DbState>,
) -> Result<crate::db::ReadingStats, AppError> {
    let conn = state.lock_read()?;
    db::reading_stats(&conn)
}

#[tauri::command]
pub fn get_learning_stats(
    state: tauri::State<'_, DbState>,
) -> Result<LearningStats, AppError> {
    let conn = state.lock_read()?;
    db::learning_stats(&conn)
}

#[tauri::command]
pub async fn translate_paragraph(
    app: AppHandle,
    article_id: String,
    paragraph_index: usize,
    text: String,
) -> Result<TranslationRow, AppError> {
    let cfg = crate::config::load_config()?;
    crate::commands::spawn_db(app, move |state| {
        translate::translate_and_cache(
            state,
            &cfg,
            &article_id,
            "paragraph",
            &paragraph_index.to_string(),
            &text,
        )
    })
    .await
}

#[tauri::command]
pub async fn translate_selection(
    app: AppHandle,
    article_id: String,
    text: String,
) -> Result<TranslationRow, AppError> {
    let cfg = crate::config::load_config()?;
    crate::commands::spawn_db(app, move |state| {
        let scope_key = translate::stable_scope_key(&text);
        translate::translate_and_cache(
            state,
            &cfg,
            &article_id,
            "selection",
            &scope_key,
            &text,
        )
    })
    .await
}

#[tauri::command]
pub async fn translate_full_article(
    app: AppHandle,
    article_id: String,
) -> Result<translate::FullTranslateResult, AppError> {
    let cfg = crate::config::load_config()?;
    let emit_app = app.clone();
    crate::commands::spawn_db(app, move |state| {
        translate::translate_full_article(
            state,
            &cfg,
            &article_id,
            |p| {
                let _ = emit_app.emit("translate-progress", p);
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn fill_missing_card_zh(app: AppHandle) -> Result<usize, AppError> {
    let cfg = crate::config::load_config()?;
    if cfg.api_key.trim().is_empty() {
        return Ok(0);
    }
    crate::commands::spawn_db(app, move |state| {
        feeds::fill_missing_card_zh(state, &cfg, 80, |_, _| {})
    })
    .await
}

#[tauri::command]
pub async fn import_article_url(app: AppHandle, url: String) -> Result<Article, AppError> {
    crate::commands::spawn_db(app, move |state| {
        feeds::import_article_from_url(state, &url)
    })
    .await
}

/// Re-fetch articles whose stored body lost paragraph breaks.
#[tauri::command]
pub async fn repair_paragraphs(
    app: AppHandle,
    limit: Option<usize>,
) -> Result<usize, AppError> {
    crate::commands::spawn_db(app, move |state| {
        feeds::repair_missing_paragraphs(state, limit.unwrap_or(30))
    })
    .await
}

#[tauri::command]
pub async fn import_article_file(app: AppHandle, path: String) -> Result<Article, AppError> {
    crate::commands::spawn_db(app, move |state| {
        import_file::import_article_from_file(state, &path)
    })
    .await
}

#[tauri::command]
pub async fn import_book(
    app: AppHandle,
    input: crate::books::ImportBookRequest,
) -> Result<Article, AppError> {
    crate::commands::spawn_db(app, move |state| {
        crate::books::import_gutenberg_book(state, &input)
    })
    .await
}

#[tauri::command]
pub fn list_book_articles(state: tauri::State<'_, DbState>) -> Result<Vec<ArticleListItem>, AppError> {
    let conn = state.lock_read()?;
    db::list_articles_by_origin(&conn, "book", 200)
}

#[tauri::command]
pub fn articles_matching_terms(
    state: tauri::State<'_, DbState>,
    terms: Vec<String>,
    recent: Vec<String>,
    limit: Option<usize>,
) -> Result<Vec<ArticleListItem>, AppError> {
    let conn = state.lock_read()?;
    db::articles_matching_terms(&conn, &terms, &recent, limit.unwrap_or(12))
}
