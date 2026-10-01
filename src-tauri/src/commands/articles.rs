use crate::article_view;
use crate::db::{self, Article, ArticleListItem, DbState, LearningStats, RankWindow, TranslationRow};
use crate::error::AppError;
use crate::feeds;
use crate::import_file;
use crate::translate;
use crate::vocab;
use tauri::{AppHandle, Emitter};

/// Candidate window size for interest scoring. Wide enough that cursor pages can
/// reach past the first few screens of a daily reading session.
const RANK_WINDOW: i64 = 800;

/// Gather one read-lock snapshot of the candidate window plus every affinity
/// input, and hand it to the frontend for scoring (`src/rank.ts`). The backend
/// only reads rows — no ranking, no paging.
#[tauri::command]
pub async fn list_rank_window(
    app: AppHandle,
    category: Option<String>,
    source: Option<String>,
    unread_only: Option<bool>,
    search: Option<String>,
) -> Result<RankWindow, AppError> {
    crate::commands::spawn_db(app, move |state| {
        // One snapshot for the page. These queries used to take the read lock
        // separately, so a refresh landing in between could score a page
        // against a half-updated profile.
        let conn = state.lock_read()?;
        let items = db::query_articles(
            &conn,
            &db::ArticleQuery {
                category: category.as_deref(),
                source: source.as_deref(),
                read_state: if unread_only.unwrap_or(false) {
                    db::ReadState::Unfinished
                } else {
                    db::ReadState::All
                },
                liked_only: false,
                search: search.as_deref(),
            },
            Some(RANK_WINDOW),
            Some(0),
        )?;
        let (source_opens, category_opens) = db::affinity_open_counts(&conn)?;
        let engaged_titles = db::engaged_titles(&conn)?;
        let source_priority = db::source_priority_map(&conn)?;
        let category_priority_max = db::category_priority_max(&conn)?;
        drop(conn);
        Ok(RankWindow {
            items,
            source_opens,
            category_opens,
            engaged_titles,
            source_priority,
            category_priority_max,
        })
    })
    .await
}

/// Library listing: every article with the full filter set, newest first.
#[tauri::command]
pub async fn list_library(
    app: AppHandle,
    category: Option<String>,
    source: Option<String>,
    read_state: Option<String>,
    liked_only: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
    search: Option<String>,
) -> Result<Vec<ArticleListItem>, AppError> {
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
                source: source.as_deref(),
                read_state,
                liked_only: liked_only.unwrap_or(false),
                search: search.as_deref(),
            },
            limit,
            offset,
        )
    })
    .await
}

/// One-off translation without article caching (used outside the reader,
/// e.g. selecting a word on the home list). Goes through the LLM, no DB write.
#[tauri::command]
pub async fn translate_plain_text(text: String) -> Result<String, AppError> {
    let cfg = crate::config::load_config()?;
    crate::commands::spawn_blocking_err(move || vocab::translate_text(&cfg, &text)).await
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
) -> Result<Option<article_view::ArticleView>, AppError> {
    // Fetch the row under a short read lock, reflow (CPU work) without holding
    // the lock, then fetch cached translations. Keeps long articles from
    // blocking other readers.
    let article = {
        let conn = state.lock_read()?;
        db::get_article(&conn, &id)?
    };
    let Some(article) = article else {
        return Ok(None);
    };
    let paragraphs = crate::feeds::split_paragraphs(&article.content_text);
    let translations = {
        let conn = state.lock_read()?;
        db::list_paragraph_translations(&conn, &id)?
    };
    Ok(Some(article_view::ArticleView {
        article,
        paragraphs,
        translations,
    }))
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
        feeds::fill_missing_card_zh(state, &cfg, feeds::CARDS_PER_REFRESH, |_, _| {})
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
