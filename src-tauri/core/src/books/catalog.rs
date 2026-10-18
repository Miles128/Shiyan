//! Project Gutenberg catalog: download once, search locally.
//!
//! Source of truth is the official CSV (`pg_catalog.csv`, ~21 MB, ~62k English
//! text rows). It is downloaded on first use and mirrored into `book_catalog`;
//! every search afterwards is a local query. The third-party Gutendex API was
//! rejected during design: three probes timed out at 45–60s, so it cannot be a
//! dependency for a search box.
//!
//! Columns used: `Text#`, `Type`, `Issued`, `Title`, `Language`, `Authors`,
//! `Subjects`, `LoCC`, `Bookshelves`. Genre comes from the first
//! `Category: X` entry of `Bookshelves` (`Category: Novels` → `Novels`); the
//! raw first entry is often a shelf like `Best Books Ever Listings`, which is
//! useless as a filter.

use crate::db::{self, CatalogBook, DbState};
use crate::error::AppError;
use crate::feeds::net::{http_client, read_limited_bytes_capped};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const CATALOG_URL: &str = "https://www.gutenberg.org/cache/epub/feeds/pg_catalog.csv";
/// Sanity bound: the real file is ~21 MB. Anything near this is a wrong URL.
const CATALOG_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Stored per row; the longest real subject list is ~900 chars and only the
/// first few subjects are ever shown.
const SUBJECTS_MAX_CHARS: usize = 300;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CatalogStatus {
    /// Rows in the local mirror.
    #[ts(type = "number")]
    pub count: i64,
    /// RFC3339 of the last download, None when never fetched.
    pub updated_at: Option<String>,
    /// True when this call performed the download.
    pub downloaded: bool,
}

/// Ensure the local mirror exists. Downloads only when empty — a stale catalog
/// is not silently re-fetched behind a search box (21 MB), it waits for the
/// next explicit call after the mirror is dropped.
pub fn ensure_catalog(db: &DbState) -> Result<CatalogStatus, AppError> {
    let (existing, stamp) = {
        let conn = db.lock_read()?;
        (db::catalog_count(&conn)?, db::catalog_stamp(&conn)?)
    };
    if existing > 0 {
        return Ok(CatalogStatus {
            count: existing,
            updated_at: stamp,
            downloaded: false,
        });
    }

    let client = http_client()?;
    let resp = client
        .get(CATALOG_URL)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| AppError::msg(format!("下载 Gutenberg 目录失败：{e}")))?;
    let bytes = read_limited_bytes_capped(resp, CATALOG_MAX_BYTES)?;
    let text = String::from_utf8_lossy(&bytes);
    let (stamp, count) = ingest(db, &text)?;
    Ok(CatalogStatus {
        count,
        updated_at: Some(stamp),
        downloaded: true,
    })
}

/// Parse the CSV straight into the mirror inside one transaction.
/// Returns (stamp, row count).
fn ingest(db: &DbState, raw: &str) -> Result<(String, i64), AppError> {
    let stamp = chrono::Utc::now().to_rfc3339();
    let conn = db.lock_write()?;
    let tx = conn.unchecked_transaction()?;
    db::clear_catalog(&tx)?;
    let mut inserted = 0i64;
    {
        let mut reader = CsvRecords::new(raw.as_bytes());
        let mut fields: Vec<String> = Vec::with_capacity(9);
        // Header first; the column count is the only shape check worth doing.
        if !reader.next_into(&mut fields) || fields.len() < 9 {
            return Err(AppError::msg("Gutenberg 目录格式不符（表头缺失）"));
        }
        while reader.next_into(&mut fields) {
            if fields.len() < 9 {
                continue;
            }
            let Some(book) = to_catalog_book(&fields) else {
                continue;
            };
            db::insert_catalog_row(&tx, &book)?;
            inserted += 1;
        }
    }
    db::set_catalog_stamp(&tx, &stamp)?;
    tx.commit()?;
    if inserted == 0 {
        return Err(AppError::msg("Gutenberg 目录解析为空，已放弃写入"));
    }
    Ok((stamp, inserted))
}

fn to_catalog_book(fields: &[String]) -> Option<CatalogBook> {
    let gid: i64 = fields[0].trim().parse().ok()?;
    if !fields[1].trim().eq_ignore_ascii_case("Text") {
        return None;
    }
    // Language can be a `;`-separated set; keep anything with English in it.
    let language = &fields[4];
    if !language
        .split(';')
        .any(|l| l.trim().eq_ignore_ascii_case("en"))
    {
        return None;
    }
    let title = crate::books::text::collapse_spaces(&fields[3]);
    if title.is_empty() {
        return None;
    }
    Some(CatalogBook {
        gid,
        title,
        authors: crate::books::text::collapse_spaces(&fields[5]),
        issued: fields[2].trim().to_string(),
        shelf: shelf_from_bookshelves(&fields[8]),
        subjects: fields[6]
            .chars()
            .take(SUBJECTS_MAX_CHARS)
            .collect::<String>()
            .trim()
            .to_string(),
        imported: false,
    })
}

/// `Best Books Ever Listings; Category: Novels; …` → `Novels`.
pub fn shelf_from_bookshelves(raw: &str) -> String {
    let mut first = "";
    for part in raw.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(genre) = part.strip_prefix("Category: ") {
            return genre.trim().to_string();
        }
        if first.is_empty() {
            first = part;
        }
    }
    first.to_string()
}

/// Local search. `shelf` filters on the coarse genre, '' means no filter.
/// Results carry `imported` so the picker can show 已导入 instead of a button.
pub fn search(
    db: &DbState,
    query: &str,
    shelf: &str,
    limit: i64,
) -> Result<Vec<CatalogBook>, AppError> {
    let conn = db.lock_read()?;
    let mut books = db::search_catalog(&conn, query, shelf, limit)?;
    let imported = db::imported_gids(&conn)?;
    for book in &mut books {
        book.imported = imported.contains(&book.gid);
    }
    Ok(books)
}

/// Minimal RFC 4180 reader over bytes. Gutenberg titles do contain commas,
/// quotes and semicolons, so a `split(',')` would corrupt rows.
struct CsvRecords<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> CsvRecords<'a> {
    fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }

    /// Fill `out` with the next record's fields; false at end of input.
    fn next_into(&mut self, out: &mut Vec<String>) -> bool {
        out.clear();
        if self.pos >= self.src.len() {
            return false;
        }
        let mut field: Vec<u8> = Vec::new();
        let mut quoted = false;
        let mut any = false;
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            self.pos += 1;
            any = true;
            match b {
                b'"' if quoted => {
                    if self.src.get(self.pos) == Some(&b'"') {
                        field.push(b'"');
                        self.pos += 1;
                    } else {
                        quoted = false;
                    }
                }
                b'"' => quoted = true,
                b',' if !quoted => out.push(take_field(&mut field)),
                b'\r' if !quoted => {}
                b'\n' if !quoted => {
                    out.push(take_field(&mut field));
                    return true;
                }
                _ => field.push(b),
            }
        }
        if !field.is_empty() || !out.is_empty() {
            out.push(take_field(&mut field));
            return any;
        }
        false
    }
}

fn take_field(field: &mut Vec<u8>) -> String {
    let bytes = std::mem::take(field);
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_fields_with_commas_and_newlines() {
        let csv = "Text#,Type,Issued,Title,Language,Authors,Subjects,LoCC,Bookshelves\n\
                   2701,Text,2001-07-01,\"Moby Dick; Or, The Whale\",en,\"Melville, Herman, 1819-1891\",\"Whaling -- Fiction; Sea stories\",PS,\"Best Books Ever Listings; Category: Novels\"\n";
        let mut reader = CsvRecords::new(csv.as_bytes());
        let mut fields = Vec::new();
        assert!(reader.next_into(&mut fields));
        assert_eq!(fields[0], "Text#");
        assert!(reader.next_into(&mut fields));
        assert_eq!(fields.len(), 9);
        assert_eq!(fields[3], "Moby Dick; Or, The Whale");
        assert_eq!(fields[5], "Melville, Herman, 1819-1891");
        let book = to_catalog_book(&fields).expect("row accepted");
        assert_eq!(book.gid, 2701);
        assert_eq!(book.shelf, "Novels");
        assert!(!reader.next_into(&mut fields));
    }

    #[test]
    fn skips_non_text_and_non_english_rows() {
        let mut fields: Vec<String> = ["1", "Sound", "", "Title", "en", "", "", "", ""]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(to_catalog_book(&fields).is_none());
        fields[1] = "Text".into();
        fields[4] = "fr".into();
        assert!(to_catalog_book(&fields).is_none());
        fields[4] = "en;fr".into();
        assert!(to_catalog_book(&fields).is_some());
    }

    #[test]
    fn shelf_falls_back_to_first_entry() {
        assert_eq!(shelf_from_bookshelves("Category: Poetry"), "Poetry");
        assert_eq!(shelf_from_bookshelves("Best Books"), "Best Books");
        assert_eq!(shelf_from_bookshelves(""), "");
    }
}
