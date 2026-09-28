use crate::error::AppError;
use rusqlite::{params, Connection};

/// Canonical known-word key. Mirrors the frontend `normalizeKey`
/// (wordLevels): trim, lowercase, straighten curly apostrophes, collapse
/// inner whitespace — so "it’s" marked in one article matches "it's" in
/// another. Both write paths and the one-time migration below must use this.
pub fn normalize_known_term(term: &str) -> String {
    term.trim()
        .to_lowercase()
        .replace(['\u{2018}', '\u{2019}'], "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Words the learner marked as already known (normalized, deduped).
pub fn list_known_words(conn: &Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare("SELECT term FROM known_words ORDER BY term")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn add_known_word(conn: &Connection, term: &str) -> Result<(), AppError> {
    let key = normalize_known_term(term);
    if key.is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO known_words (term, created_at) VALUES (?1, ?2)",
        params![key, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn remove_known_word(conn: &Connection, term: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM known_words WHERE term=?1",
        params![normalize_known_term(term)],
    )?;
    Ok(())
}

/// One-time migration: rewrite pre-normalization rows to the canonical key.
/// Collisions (two rows folding to one) keep a single row. Guarded by
/// `app_meta`; the reader re-checks membership on every load anyway.
const NORMALIZE_KNOWN_KEY: &str = "known_words_normalized_v1";

pub fn normalize_known_words_once(conn: &Connection) -> Result<usize, AppError> {
    if super::get_meta(conn, NORMALIZE_KNOWN_KEY)?.is_some() {
        return Ok(0);
    }
    let mut fixed = 0usize;
    for term in list_known_words(conn)? {
        let canonical = normalize_known_term(&term);
        if canonical == term {
            continue;
        }
        conn.execute("DELETE FROM known_words WHERE term=?1", params![term])?;
        if !canonical.is_empty() {
            conn.execute(
                "INSERT OR IGNORE INTO known_words (term, created_at) VALUES (?1, ?2)",
                params![canonical, chrono::Utc::now().to_rfc3339()],
            )?;
        }
        fixed += 1;
    }
    super::set_meta(conn, NORMALIZE_KNOWN_KEY, "done")?;
    Ok(fixed)
}
