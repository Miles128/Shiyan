//! One-time cleanup and retention purges for stored RSS articles.

use super::filters::is_blocked_content;
use super::MIN_ARTICLE_WORDS;
use crate::db::{self, Article};
use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};

/// Startup cleanup marker: empty-body deletion, legacy word-count backfill,
/// and removal of already-stored roundups/transcripts all only ever need one
/// pass over the library.
const STARTUP_CLEANUP_KEY: &str = "startup_cleanup_v1";

/// One-time cleanup after a re-paragraphing (reflow) change: paragraph
/// translations are keyed by index, which only matches one exact split, so any
/// rows written before the change would render against the wrong paragraph.
/// Cleared once per `reflow::REFLOW_VERSION`; the reader re-creates them on
/// demand. When reflow rules change, bump `REFLOW_VERSION` and the cleanup
/// runs again automatically.
const REFLOW_TRANSLATIONS_KEY_PREFIX: &str = "reflow_translations_cleared_v";

pub fn clear_stale_paragraph_translations_once(conn: &Connection) -> Result<usize, AppError> {
    let key = format!(
        "{REFLOW_TRANSLATIONS_KEY_PREFIX}{}",
        crate::reflow::REFLOW_VERSION
    );
    if db::get_meta(conn, &key)?.is_some() {
        return Ok(0);
    }
    let removed = db::clear_paragraph_translations(conn)?;
    db::set_meta(conn, &key, "done")?;
    Ok(removed)
}

/// Retention: drop auto-ingested articles older than `retention_days`
/// (0 = keep forever). Liked articles and user imports are never touched.
pub(crate) fn purge_expired_articles(
    conn: &Connection,
    retention_days: u32,
) -> Result<usize, AppError> {
    if retention_days == 0 {
        return Ok(0);
    }
    let cutoff = (Utc::now() - chrono::Duration::days(retention_days as i64)).to_rfc3339();
    db::purge_old_rss_articles(conn, &cutoff)
}

/// Rows the learner has claimed: liked, or opened and not finished. Purges
/// never delete these, whatever the body looks like.
fn is_protected(article: &Article) -> bool {
    article.liked || (!article.read_completed && article.last_opened_at.is_some())
}

/// Enforce [`MIN_ARTICLE_WORDS`] on stored RSS bodies (idempotent, cheap).
/// Runs every refresh so a raised threshold backfills against stamped rows.
/// Articles the learner liked or is still reading are never deleted.
pub(crate) fn purge_rss_below_word_threshold(conn: &Connection) -> Result<usize, AppError> {
    let changed = conn
        .execute(
            "DELETE FROM articles
             WHERE origin='rss' AND word_count > 0 AND word_count < ?1
               AND liked = 0
               AND NOT (last_opened_at IS NOT NULL AND read_completed = 0)",
            params![MIN_ARTICLE_WORDS as i64],
        )
        ?;
    Ok(changed)
}

/// Startup maintenance that only needs one pass over the library: drop empty
/// bodies, stamp never-measured word counts, and remove already-stored
/// roundups / transcripts. Runs once (guarded by `app_meta`); a failed pass
/// leaves the marker unset so the next launch retries.
pub fn run_startup_cleanup_once(conn: &Connection) -> Result<usize, AppError> {
    if db::get_meta(conn, STARTUP_CLEANUP_KEY)?.is_some() {
        return Ok(0);
    }
    let stamped = db::backfill_word_counts(conn)?;
    let removed = purge_blocked_articles(conn)?;
    db::set_meta(conn, STARTUP_CLEANUP_KEY, "done")?;
    Ok(stamped + removed)
}

/// One-time cleanup: delete already-stored RSS articles that match the
/// roundup / transcript filters. Articles the learner liked or is still
/// reading are never touched.
fn purge_blocked_articles(conn: &Connection) -> Result<usize, AppError> {
    let mut removed = 0usize;
    for article in db::list_all_rss_articles(conn)? {
        if !is_protected(&article) && is_blocked_content(&article.title, &article.content_text) {
            db::delete_article(conn, &article.id)?;
            removed += 1;
        }
    }
    Ok(removed)
}
