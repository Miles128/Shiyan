use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One recorded term lookup (Home/Reader selection popover), newest first.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LookupEntry {
    #[ts(type = "number")]
    pub id: i64,
    pub term: String,
    /// Selection/sentence the lookup came from (may be empty).
    pub context: String,
    pub article_id: Option<String>,
    pub created_at: String,
}

fn map_lookup(row: &rusqlite::Row<'_>) -> rusqlite::Result<LookupEntry> {
    Ok(LookupEntry {
        id: row.get(0)?,
        term: row.get(1)?,
        context: row.get(2)?,
        article_id: row.get(3)?,
        created_at: row.get(4)?,
    })
}

const LOOKUP_SELECT: &str =
    "SELECT id,term,context,article_id,created_at FROM lookup_history";

/// History cap: every lookup is one row, so an unbounded table is a slow
/// privacy leak. Oldest rows past the cap are trimmed on every insert.
pub const MAX_LOOKUP_ROWS: i64 = 2000;
/// Re-selecting the same word within minutes is one lookup, not many.
const LOOKUP_DEDUP_MINUTES: i64 = 10;

pub fn record_lookup(
    conn: &Connection,
    term: &str,
    context: &str,
    article_id: Option<&str>,
) -> Result<(), AppError> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(());
    }
    let now = Utc::now();
    let recent: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lookup_history
          WHERE lower(term)=lower(?1) AND created_at > ?2",
        params![
            term,
            (now - chrono::Duration::minutes(LOOKUP_DEDUP_MINUTES)).to_rfc3339()
        ],
        |row| row.get(0),
    )?;
    if recent > 0 {
        return Ok(());
    }
    // Bound the stored context so a full-page selection can't bloat the DB.
    const MAX_CONTEXT_CHARS: usize = 300;
    let context: String = context.chars().take(MAX_CONTEXT_CHARS).collect();
    conn.execute(
        "INSERT INTO lookup_history (term, context, article_id, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![term, context, article_id, now.to_rfc3339()],
    )?;
    conn.execute(
        "DELETE FROM lookup_history WHERE id NOT IN (
            SELECT id FROM lookup_history
            ORDER BY created_at DESC, id DESC LIMIT ?1
         )",
        params![MAX_LOOKUP_ROWS],
    )?;
    Ok(())
}

/// Newest-first page of lookups; optional case-insensitive term/substring search.
pub fn list_lookups(
    conn: &Connection,
    search: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<LookupEntry>, AppError> {
    let mut sql = String::from(LOOKUP_SELECT);
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(q) = search.map(str::trim).filter(|q| !q.is_empty()) {
        // Escape LIKE metacharacters so `%`/`_` in the query are literals.
        let escaped = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        sql.push_str(" WHERE lower(term) LIKE ?1 ESCAPE '\\' OR context LIKE ?1 ESCAPE '\\'");
        values.push(format!("%{}%", escaped.to_lowercase()).into());
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?");
    values.push((limit as i64).into());
    values.push((offset as i64).into());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values.iter()), map_lookup)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_lookup(conn: &Connection, id: i64) -> Result<(), AppError> {
    let changed = conn.execute("DELETE FROM lookup_history WHERE id=?1", params![id])?;
    if changed == 0 {
        return Err(AppError::msg("lookup entry not found"));
    }
    Ok(())
}

pub fn clear_lookups(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM lookup_history", [])?;
    Ok(())
}

#[cfg(test)]
pub fn count_lookups(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row("SELECT COUNT(*) FROM lookup_history", [], |r| {
        r.get(0)
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lookup_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                term TEXT NOT NULL,
                context TEXT NOT NULL DEFAULT '',
                article_id TEXT,
                created_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn record_list_delete_roundtrip() {
        let conn = setup();
        record_lookup(&conn, "fragile", "a fragile peace", None).unwrap();
        record_lookup(&conn, "treaty", "signed a treaty", Some("a1")).unwrap();
        assert_eq!(count_lookups(&conn).unwrap(), 2);

        let rows = list_lookups(&conn, None, 10, 0).unwrap();
        assert_eq!(rows.len(), 2);
        // newest first — same timestamps could reorder, so check as a set
        let terms: Vec<&str> = rows.iter().map(|r| r.term.as_str()).collect();
        assert!(terms.contains(&"fragile") && terms.contains(&"treaty"));

        let found = list_lookups(&conn, Some("frag"), 10, 0).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].term, "fragile");

        delete_lookup(&conn, rows[0].id).unwrap();
        assert_eq!(count_lookups(&conn).unwrap(), 1);
        clear_lookups(&conn).unwrap();
        assert_eq!(count_lookups(&conn).unwrap(), 0);
    }

    #[test]
    fn record_skips_blank_terms() {
        let conn = setup();
        record_lookup(&conn, "   ", "ctx", None).unwrap();
        assert_eq!(count_lookups(&conn).unwrap(), 0);
    }

    #[test]
    fn record_debounces_repeat_lookups() {
        let conn = setup();
        record_lookup(&conn, "Fragile", "ctx one", None).unwrap();
        record_lookup(&conn, "fragile", "ctx two", None).unwrap();
        assert_eq!(count_lookups(&conn).unwrap(), 1, "same term, case-insensitive");
        record_lookup(&conn, "treaty", "ctx", None).unwrap();
        assert_eq!(count_lookups(&conn).unwrap(), 2);
    }

    #[test]
    fn record_trims_history_to_cap() {
        let conn = setup();
        for i in 0..(MAX_LOOKUP_ROWS + 50) {
            // Distinct terms defeat the repeat-term debounce.
            record_lookup(&conn, &format!("term-{i:05}"), "ctx", None).unwrap();
        }
        assert_eq!(count_lookups(&conn).unwrap(), MAX_LOOKUP_ROWS);
        let newest = list_lookups(&conn, None, 1, 0).unwrap();
        assert_eq!(newest[0].term, format!("term-{:05}", MAX_LOOKUP_ROWS + 49));
    }
}
