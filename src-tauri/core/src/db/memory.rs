use super::MemoryItem;
use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

fn map_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryItem> {
    let collocations_json: String = row.get(5)?;
    let collocations: Vec<String> =
        serde_json::from_str(&collocations_json).unwrap_or_default();
    Ok(MemoryItem {
        id: row.get(0)?,
        kind: row.get(1)?,
        term: row.get(2)?,
        definition_zh: row.get(3)?,
        word_type: row.get(4)?,
        collocations,
        context_sentence: row.get(6)?,
        article_id: row.get(7)?,
        status: row.get(8)?,
        interval_days: row.get(9)?,
        reps: row.get(10)?,
        consecutive_know: row.get(11)?,
        next_review_at: row.get(12)?,
        created_at: row.get(13)?,
    })
}

const MEMORY_SELECT: &str = "SELECT id,kind,term,definition_zh,word_type,collocations_json,context_sentence,article_id,status,interval_days,reps,consecutive_know,next_review_at,created_at FROM memory_items";

pub fn insert_memory(conn: &Connection, item: &MemoryItem) -> Result<(), AppError> {
    let collocations_json = serde_json::to_string(&item.collocations)?;
    conn.execute(
        "INSERT INTO memory_items
            (id,kind,term,definition_zh,word_type,collocations_json,context_sentence,
             article_id,status,interval_days,reps,consecutive_know,next_review_at,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            item.id,
            item.kind,
            item.term,
            item.definition_zh,
            item.word_type,
            collocations_json,
            item.context_sentence,
            item.article_id,
            item.status,
            item.interval_days,
            item.reps,
            item.consecutive_know,
            item.next_review_at,
            item.created_at
        ],
    )?;
    Ok(())
}

/// List a library (`kind` = "word" | "phrase") optionally filtered by status.
/// Both filters are optional; empty/None means "all".
pub fn list_memory(
    conn: &Connection,
    kind: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<MemoryItem>, AppError> {
    let mut sql = String::from(MEMORY_SELECT);
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(k) = kind.filter(|k| !k.is_empty()) {
        clauses.push("kind=?".into());
        values.push(k.to_string().into());
    }
    if let Some(s) = status.filter(|s| !s.is_empty()) {
        clauses.push("status=?".into());
        values.push(s.to_string().into());
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values.iter()), map_memory)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_memory(conn: &Connection, id: &str) -> Result<Option<MemoryItem>, AppError> {
    let sql = format!("{MEMORY_SELECT} WHERE id=?1");
    conn.query_row(&sql, params![id], map_memory)
        .optional()
        .map_err(AppError::from)
}

/// Case-insensitive lookup by term within one library (oldest row wins).
pub fn get_memory_by_term(
    conn: &Connection,
    kind: &str,
    term: &str,
) -> Result<Option<MemoryItem>, AppError> {
    let sql = format!(
        "{MEMORY_SELECT} WHERE kind=?1 AND lower(term)=lower(?2) ORDER BY created_at ASC LIMIT 1"
    );
    conn.query_row(&sql, params![kind, term.trim()], map_memory)
        .optional()
        .map_err(AppError::from)
}

/// Update learner-facing fields when an existing term is re-added.
pub fn update_memory_meta(conn: &Connection, item: &MemoryItem) -> Result<(), AppError> {
    let collocations_json = serde_json::to_string(&item.collocations)?;
    conn.execute(
        "UPDATE memory_items SET definition_zh=?1, word_type=?2, collocations_json=?3, context_sentence=?4, article_id=?5 WHERE id=?6",
        params![
            item.definition_zh,
            item.word_type,
            collocations_json,
            item.context_sentence,
            item.article_id,
            item.id
        ],
    )?;
    Ok(())
}

pub fn update_memory_review(conn: &Connection, item: &MemoryItem) -> Result<(), AppError> {
    conn.execute(
        "UPDATE memory_items SET status=?1, interval_days=?2, reps=?3, consecutive_know=?4, next_review_at=?5 WHERE id=?6",
        params![
            item.status,
            item.interval_days,
            item.reps,
            item.consecutive_know,
            item.next_review_at,
            item.id
        ],
    )?;
    Ok(())
}

/// Items due for review (oldest due first), optionally scoped to one library.
pub fn due_memory(conn: &Connection, kind: Option<&str>) -> Result<Vec<MemoryItem>, AppError> {
    let now = Utc::now().to_rfc3339();
    let mut sql = format!(
        "{MEMORY_SELECT} WHERE status='learning' AND next_review_at<=?1"
    );
    let mut values: Vec<rusqlite::types::Value> = vec![now.into()];
    if let Some(k) = kind.filter(|k| !k.is_empty()) {
        sql.push_str(" AND kind=?2");
        values.push(k.to_string().into());
    }
    sql.push_str(" ORDER BY next_review_at ASC LIMIT 50");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values.iter()), map_memory)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Statuses the UI can move a row between. Anything else would silently
/// orphan the row: every list query filters on these two values, so an
/// unknown status disappears from all tabs with no error.
const MEMORY_STATUSES: &[&str] = &["learning", "mastered"];

pub fn set_memory_status(conn: &Connection, id: &str, status: &str) -> Result<(), AppError> {
    if !MEMORY_STATUSES.contains(&status) {
        return Err(AppError::msg(format!("unknown memory status: {status}")));
    }
    let changed = conn.execute(
        "UPDATE memory_items SET status=?1 WHERE id=?2",
        params![status, id],
    )?;
    if changed == 0 {
        return Err(AppError::msg(format!("memory item not found: {id}")));
    }
    Ok(())
}

pub fn delete_memory(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM memory_items WHERE id=?1", params![id])?;
    Ok(())
}

/// Escape one CSV field: wrap in quotes when it contains a comma, quote,
/// or line break; double embedded quotes (RFC 4180). Prefix a single quote
/// for fields starting with `=`, `+`, `-`, or `@` to prevent CSV formula
/// injection when opened in spreadsheet applications.
pub fn csv_field(s: &str) -> String {
    let s = if s.starts_with('=') || s.starts_with('+') || s.starts_with('-') || s.starts_with('@')
    {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// Dump every memory item (words + phrases, all statuses) as one CSV string.
/// A UTF-8 BOM is prepended so Excel/Numbers auto-detect the encoding;
/// columns are Anki-import friendly (term first, meaning second).
pub fn export_memory_csv(conn: &Connection) -> Result<String, AppError> {
    let items = list_memory(conn, None, None)?;
    let mut out =
        String::from("\u{FEFF}term,kind,status,definition_zh,word_type,collocations,context_sentence,created_at,next_review_at,reps,interval_days\n");
    for it in items {
        let collocations = it.collocations.join("; ");
        let next_review = it.next_review_at.as_str();
        let reps = it.reps.to_string();
        let interval = it.interval_days.to_string();
        let fields = [
            it.term.as_str(),
            it.kind.as_str(),
            it.status.as_str(),
            it.definition_zh.as_str(),
            it.word_type.as_str(),
            collocations.as_str(),
            it.context_sentence.as_str(),
            it.created_at.as_str(),
            next_review,
            reps.as_str(),
            interval.as_str(),
        ];
        out.push_str(
            &fields
                .iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    Ok(out)
}
