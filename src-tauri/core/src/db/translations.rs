use crate::error::AppError;
use super::TranslationRow;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

pub fn get_translation(
    conn: &Connection,
    article_id: &str,
    scope: &str,
    scope_key: &str,
) -> Result<Option<TranslationRow>, AppError> {
    conn.query_row(
        "SELECT id,article_id,scope,scope_key,source_text,translated_text,model FROM translations
         WHERE article_id=?1 AND scope=?2 AND scope_key=?3",
        params![article_id, scope, scope_key],
        map_translation,
    )
    .optional()
    .map_err(AppError::from)
    
}

pub fn save_translation(
    conn: &Connection,
    article_id: &str,
    scope: &str,
    scope_key: &str,
    source_text: &str,
    translated_text: &str,
    model: &str,
) -> Result<TranslationRow, AppError> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO translations (article_id,scope,scope_key,source_text,translated_text,model,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(article_id,scope,scope_key) DO UPDATE SET
           source_text=excluded.source_text,
           translated_text=excluded.translated_text,
           model=excluded.model,
           created_at=excluded.created_at",
        params![
            article_id,
            scope,
            scope_key,
            source_text,
            translated_text,
            model,
            now
        ],
    )
    ?;
    get_translation(conn, article_id, scope, scope_key)?
        .ok_or_else(|| "failed to read saved translation".into())
}

pub fn list_paragraph_translations(
    conn: &Connection,
    article_id: &str,
) -> Result<Vec<TranslationRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id,article_id,scope,scope_key,source_text,translated_text,model FROM translations
             WHERE article_id=?1 AND scope='paragraph'",
        )
        ?;
    let rows = stmt
        .query_map(params![article_id], map_translation)
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// Drop every cached paragraph translation. Paragraph indices are only valid
/// for one exact paragraph split, so a re-paragraphing (reflow) change makes
/// the stored rows unmatchable — they are cleared and re-created on demand.
pub fn clear_paragraph_translations(conn: &Connection) -> Result<usize, AppError> {
    let removed = conn.execute("DELETE FROM translations WHERE scope='paragraph'", [])?;
    Ok(removed)
}

/// Drop one article's cached paragraph translations. A stored paragraph
/// `scope_key` is a paragraph index that only matches one exact body split,
/// so any body replacement must invalidate these rows in the same write.
pub fn delete_paragraph_translations(
    conn: &Connection,
    article_id: &str,
) -> Result<usize, AppError> {
    let removed = conn.execute(
        "DELETE FROM translations WHERE scope='paragraph' AND article_id=?1",
        params![article_id],
    )?;
    Ok(removed)
}

fn map_translation(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranslationRow> {
    Ok(TranslationRow {
        id: row.get(0)?,
        article_id: row.get(1)?,
        scope: row.get(2)?,
        scope_key: row.get(3)?,
        source_text: row.get(4)?,
        translated_text: row.get(5)?,
        model: row.get(6)?,
    })
}