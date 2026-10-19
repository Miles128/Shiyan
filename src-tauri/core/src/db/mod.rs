//! SQLite storage: schema + migrations + per-domain repositories.
//!
//! Sub-modules own their tables and re-export the repository functions at the
//! `db::` root so callers (`feeds`, `import_file`, `commands`, tests) keep a
//! single facade entry point.

mod articles;
mod books_catalog;
mod curated_feeds;
mod feeds;
mod known;
mod memory;
mod translations;

pub use articles::*;
pub use books_catalog::*;
pub use curated_feeds::*;
pub use known::*;
pub use memory::*;
pub use feeds::*;
pub use translations::*;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use ts_rs::TS;

use crate::error::AppError;

/// Map one row onto a domain struct by column name: struct fields must match
/// the SELECT list (see the per-table `*_SELECT` constants). This replaces the
/// hand-written `map_*` row mappers — serde fills the struct and ignores any
/// extra column. Rows are `rusqlite::Result` so the value drops straight into
/// `query_map` / `query_row`.
pub fn row_as<T: serde::de::DeserializeOwned>(row: &rusqlite::Row<'_>) -> rusqlite::Result<T> {
    serde_rusqlite::from_row(row).map_err(|e| match e {
        // Real column type errors already are rusqlite errors; keep them intact.
        serde_rusqlite::Error::Rusqlite(inner) => inner,
        other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
    })
}

/// Two pooled connections to the same WAL database. Writes serialize on the
/// write connection; reads run concurrently on the read connection instead of
/// queueing behind every writer.
pub struct DbState {
    write: Mutex<Connection>,
    read: Mutex<Connection>,
}

impl DbState {
    /// Open both connections. Migrations + seeds run once on the write
    /// connection; the read connection only applies PRAGMAs.
    pub fn open(app_data: PathBuf) -> Result<Self, AppError> {
        let write = open_db(app_data.clone(), true)?;
        let read = open_db(app_data, false)?;
        Ok(Self {
            write: Mutex::new(write),
            read: Mutex::new(read),
        })
    }

    pub fn lock_write(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        // Recover from a poisoned lock instead of failing every future call:
        // the connection state is still usable after a panic in a caller.
        Ok(self.write.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn lock_read(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        Ok(self.read.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Article {
    pub id: String,
    pub url: String,
    pub title: String,
    pub source: String,
    pub category: String,
    pub published_at: Option<String>,
    pub content_text: String,
    pub fetched_at: String,
    /// rss = auto-ingested; url / file = user-imported (never purged by refresh).
    #[serde(default = "default_article_origin")]
    pub origin: String,
    /// LLM-generated Simplified Chinese blurb, at most 50 characters.
    #[serde(default)]
    pub summary_zh: String,
    /// Set when the reader is opened. Implicit; never asked.
    #[serde(default)]
    pub last_opened_at: Option<String>,
    #[serde(default)]
    #[ts(type = "number")]
    pub open_count: i64,
    /// Whitespace-delimited word count of the body, stamped at ingest/refresh.
    #[serde(default)]
    #[ts(type = "number")]
    pub word_count: i64,
    /// '' = not yet assessed; 'fulltext' once the body passed the readability gate.
    #[serde(default)]
    pub quality: String,
    /// Where the final body came from: rss | page | url | file.
    #[serde(default)]
    pub extraction_source: String,
    /// Accumulated visible reading time in milliseconds.
    #[serde(default)]
    #[ts(type = "number")]
    pub dwell_ms: i64,
    #[serde(default)]
    pub read_completed: bool,
    #[serde(default)]
    pub liked: bool,
}

/// Home-list row: excerpt only. Full body stays on `Article` / get_article.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArticleListItem {
    pub id: String,
    pub url: String,
    pub title: String,
    pub source: String,
    pub category: String,
    pub published_at: Option<String>,
    pub excerpt: String,
    pub fetched_at: String,
    #[serde(default = "default_article_origin")]
    pub origin: String,
    #[serde(default)]
    pub summary_zh: String,
    #[serde(default)]
    pub last_opened_at: Option<String>,
    #[serde(default)]
    #[ts(type = "number")]
    pub open_count: i64,
    #[serde(default)]
    #[ts(type = "number")]
    pub word_count: i64,
    /// Interest rank from the ranked list; 0 when unranked.
    #[serde(default)]
    #[ts(type = "number")]
    pub rank_score: f64,
    #[serde(default)]
    #[ts(type = "number")]
    pub dwell_ms: i64,
    #[serde(default)]
    pub read_completed: bool,
    #[serde(default)]
    pub liked: bool,
}

/// One read-lock snapshot for the home list: the candidate window plus every
/// affinity input the frontend needs to score it. Ranking lives in TS
/// (`src/rank.ts`); the backend only gathers rows, so a refresh landing
/// mid-score can't rank a page against a half-updated profile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RankWindow {
    pub items: Vec<ArticleListItem>,
    #[ts(type = "Record<string, number>")]
    pub source_opens: std::collections::HashMap<String, i64>,
    #[ts(type = "Record<string, number>")]
    pub category_opens: std::collections::HashMap<String, i64>,
    /// (title, weight) with liked = 2.0, read-to-end = 1.0 — the semantic
    /// profile input; TS turns it into term weights.
    pub engaged_titles: Vec<(String, f64)>,
    #[ts(type = "Record<string, number>")]
    pub source_priority: std::collections::HashMap<String, i64>,
    #[ts(type = "Record<string, number>")]
    pub category_priority_max: std::collections::HashMap<String, i64>,
}

/// One day of reading activity (UTC date, `YYYY-MM-DD`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReadingDay {
    pub date: String,
    #[ts(type = "number")]
    pub articles: i64,
    #[ts(type = "number")]
    pub minutes: i64,
    #[ts(type = "number")]
    pub words: i64,
}

/// A source's share of reading (by time).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SourceStat {
    pub name: String,
    #[ts(type = "number")]
    pub articles: i64,
    #[ts(type = "number")]
    pub minutes: i64,
}

/// Reading statistics for the stats page.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReadingStats {
    /// Last 14 days, oldest first (UTC dates).
    pub days: Vec<ReadingDay>,
    /// Consecutive days ending today/yesterday with at least one article opened.
    #[ts(type = "number")]
    pub streak_days: i64,
    #[ts(type = "number")]
    pub articles_total: i64,
    #[ts(type = "number")]
    pub articles_7d: i64,
    #[ts(type = "number")]
    pub completed_total: i64,
    #[ts(type = "number")]
    pub liked_total: i64,
    #[ts(type = "number")]
    pub minutes_total: i64,
    #[ts(type = "number")]
    pub minutes_7d: i64,
    #[ts(type = "number")]
    pub words_total: i64,
    #[ts(type = "number")]
    pub vocab_learning: i64,
    #[ts(type = "number")]
    pub vocab_mastered: i64,
    #[ts(type = "number")]
    pub phrases_learning: i64,
    #[ts(type = "number")]
    pub phrases_mastered: i64,
    #[ts(type = "number")]
    pub due_today: i64,
    pub top_sources: Vec<SourceStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LearningStats {
    #[ts(type = "number")]
    pub opened_total: i64,
    #[ts(type = "number")]
    pub opened_today: i64,
    #[ts(type = "number")]
    pub opened_7d: i64,
    pub top_source: Option<String>,
    pub top_category: Option<String>,
    #[ts(type = "number")]
    pub vocab_created_7d: i64,
    #[ts(type = "number")]
    pub vocab_learning: i64,
}

fn default_article_origin() -> String {
    "rss".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FeedSource {
    pub id: String,
    pub name: String,
    pub category: String,
    pub url: String,
    pub enabled: bool,
    /// curated = built-in seed; user = subscribed via manage UI
    #[serde(default = "default_feed_origin")]
    pub origin: String,
    #[serde(default)]
    pub description: String,
    /// HTTP ETag from the last successful fetch; sent back as If-None-Match.
    #[serde(default)]
    pub etag: String,
    #[serde(default)]
    pub last_fetched_at: Option<String>,
    /// Fraction of entries in the last refresh whose RSS body was trusted
    /// full-text. -1 = unknown (no data yet). Drives the per-feed trust bar.
    #[serde(default)]
    #[ts(type = "number")]
    pub fulltext_ratio: f64,
    /// User-assigned display priority for the sidebar source list. Higher =
    /// surfaced earlier; 0 = never ordered (new feeds default here). Drives the
    /// home ranking via a source-priority bonus (see `src/rank.ts`).
    #[serde(default)]
    #[ts(type = "number")]
    pub priority: i64,
    /// 最近一次为该源新增文章的时间（RFC3339）。滚动窗口判沉寂的依据，
    /// 见 feeds::prune_stale_feeds；重新启用订阅时重置。
    #[serde(default)]
    pub last_new_article_at: Option<String>,
    /// 'rss' = 抓 RSS 的订阅源；'book' = 书籍（一本书就是一个源，章节是文章）。
    /// 刷新流程与沉寂清理都跳过 book，见 feeds::pipeline::select_enabled_feeds。
    #[serde(default = "default_feed_kind")]
    pub kind: String,
}

fn default_feed_kind() -> String {
    "rss".into()
}

fn default_feed_origin() -> String {
    "curated".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FeedCategory {
    pub id: String,
    pub label: String,
    pub builtin: bool,
}

/// One row of the Project Gutenberg catalog mirror (`book_catalog`). Search
/// runs locally against this table, so the UI never depends on a third-party
/// API. Kept out of `FeedSource` / `Article` on purpose: a catalog row is not a
/// subscription and carries no reading state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CatalogBook {
    /// Project Gutenberg ebook number.
    #[ts(type = "number")]
    pub gid: i64,
    pub title: String,
    /// "Melville, Herman, 1819-1891" — CSV form, left as published.
    pub authors: String,
    /// First issue date (`1871-01-01`), '' when the catalog has none.
    pub issued: String,
    /// Coarse genre from the first `Category:` entry of the catalog's
    /// `Bookshelves` column; '' when the catalog has none.
    pub shelf: String,
    pub subjects: String,
    /// Already imported as a book feed? Filled by the search command.
    #[serde(default)]
    pub imported: bool,
}


#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TranslationRow {
    #[ts(type = "number")]
    pub id: i64,
    pub article_id: String,
    pub scope: String,
    pub scope_key: String,
    pub source_text: String,
    pub translated_text: String,
    pub model: String,
}

/// A saved word or phrase with spaced-repetition state. Both library lists
/// (生词 / 短语组合) live in one table, distinguished by `kind`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryItem {
    pub id: String,
    /// "word" | "phrase"
    pub kind: String,
    pub term: String,
    pub definition_zh: String,
    /// Part of speech for words; usage label (idiom / collocation …) for phrases.
    pub word_type: String,
    /// Stored as a JSON array string in `memory_items.collocations_json`
    /// (aliased back to `collocations` in the SELECT), so the row mapper
    /// parses it here instead of in every query.
    #[serde(deserialize_with = "de_collocations")]
    pub collocations: Vec<String>,
    pub context_sentence: String,
    pub article_id: Option<String>,
    pub status: String,
    pub interval_days: f64,
    #[ts(type = "number")]
    pub reps: i64,
    #[ts(type = "number")]
    pub consecutive_know: i64,
    pub next_review_at: String,
    pub created_at: String,
}

/// Parse the JSON array string stored in `memory_items.collocations_json`.
/// Lenient on purpose: a corrupt row degrades to an empty list instead of
/// failing the whole query (same as the old row mapper).
fn de_collocations<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let raw = String::deserialize(deserializer)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

const DB_FILE_NAME: &str = "shiyan.db";

pub fn db_path(app_data: PathBuf) -> PathBuf {
    std::fs::create_dir_all(&app_data).ok();
    app_data.join(DB_FILE_NAME)
}

/// Staged backup file swapped in on the next launch (restore flow).
pub fn pending_restore_path(app_data: &std::path::Path) -> PathBuf {
    app_data.join(format!("{DB_FILE_NAME}.pending-restore"))
}

/// Validate a candidate backup file: it must be a readable SQLite database
/// with our key tables and not from a newer app version. Returns user_version.
pub fn validate_backup_file(path: &std::path::Path) -> Result<i64, AppError> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    // Compare against the highest version any shipped backup can carry, not
    // LATEST_VERSION: pre-collapse backups stamp up to 19 and stay importable
    // (migrate normalizes their counter on next launch).
    if version > MAX_IMPORTABLE_VERSION {
        return Err(AppError::msg(format!(
            "备份来自更新版本的 App（schema v{version}），当前最高支持 v{MAX_IMPORTABLE_VERSION}"
        )));
    }
    for table in ["articles", "feed_sources", "memory_items"] {
        let has: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![table],
                |row| row.get(0),
            )?;
        if has == 0 {
            return Err(AppError::msg(format!("备份缺少关键表：{table}")));
        }
    }
    Ok(version)
}

/// If a restore file was staged (via restore_database), swap it in BEFORE any
/// connections open. The current database is kept as `*.pre-restore.bak`.
/// Returns the backup path when a swap happened.
///
/// A corrupt staged file must never brick the next launch: validation failure
/// quarantines the pending file (`*.pending-restore.invalid`) and startup
/// continues with the existing database.
pub fn apply_pending_restore(app_data: &std::path::Path) -> Result<Option<PathBuf>, AppError> {
    let pending = pending_restore_path(app_data);
    if !pending.exists() {
        return Ok(None);
    }
    if let Err(e) = validate_backup_file(&pending) {
        let invalid = pending.with_extension("pending-restore.invalid");
        let _ = std::fs::remove_file(&invalid);
        if std::fs::rename(&pending, &invalid).is_err() {
            let _ = std::fs::remove_file(&pending);
        }
        eprintln!("staged restore failed validation ({e}); quarantined, keeping current DB");
        return Ok(None);
    }
    let db = db_path(app_data.to_path_buf());
    // Checkpoint the WAL first so the backup captures *all* committed
    // transactions. Removing -wal/-shm before copying loses anything not yet
    // merged into the main database file.
    if db.exists() {
        {
            let conn = Connection::open(&db)?;
            let _ = conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
            let _ = conn.execute_batch("PRAGMA journal_mode = DELETE;");
        }
        // Now safe to remove side files — they are empty after checkpoint.
        for suffix in ["-wal", "-shm"] {
            let side = PathBuf::from(format!("{}{}", db.display(), suffix));
            if side.exists() {
                std::fs::remove_file(&side)?;
            }
        }
        let backup = PathBuf::from(format!("{}.pre-restore.bak", db.display()));
        std::fs::copy(&db, &backup)?;
        drop(backup);
    }
    std::fs::rename(&pending, &db)?;
    Ok(Some(PathBuf::from(format!(
        "{}.pre-restore.bak",
        db.display()
    ))))
}

/// Consistent whole-database snapshot via `VACUUM INTO`. The target path is
/// passed as a bound parameter (covered by test against the bundled SQLite),
/// so no filename quoting is needed. Must run outside a transaction, and the
/// destination must not already exist.
pub fn vacuum_into_file(conn: &Connection, dest: &std::path::Path) -> Result<(), AppError> {
    conn.execute(
        "VACUUM INTO ?1",
        rusqlite::params![dest.to_string_lossy()],
    )?;
    Ok(())
}

pub fn open_db(path: PathBuf, run_migrations: bool) -> Result<Connection, AppError> {
    let conn = Connection::open(&path)?;
    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
        eprintln!("WAL pragma failed for {}: {e}", path.display());
    } else {
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap_or_default();
        if !mode.eq_ignore_ascii_case("wal") {
            eprintln!(
                "journal_mode is {mode} (not WAL) for {}; concurrency falls back to rollback-journal semantics",
                path.display()
            );
        }
    }
    conn.execute_batch("PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;")
        ?;
    if run_migrations {
        backup_before_migration(&conn, &path);
        migrate(&conn)?;
        feeds::seed_feed_categories(&conn)?;
        feeds::seed_feeds(&conn)?;
    }
    Ok(conn)
}

/// Back-compat wrapper for tests and callers that open a single connection.
#[cfg(test)]
pub fn open_db_single(path: PathBuf) -> Result<Connection, AppError> {
    open_db(path, true)
}

/// Snapshot the database before a version bump, so a bad migration can be
/// rolled back by hand. Best-effort: never blocks startup.
pub(crate) fn backup_before_migration(conn: &Connection, path: &std::path::Path) {
    let Ok(stored) = conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)) else {
        return;
    };
    // 0 = fresh database (nothing to lose); >= latest = no migration will run.
    if stored == 0 || stored >= LATEST_VERSION {
        return;
    }
    let backup = path.with_file_name(format!("{DB_FILE_NAME}.premigrate-v{stored}.bak"));
    if backup.exists() {
        return;
    }
    if vacuum_into_file(conn, &backup).is_ok() {
        prune_migration_backups(path);
    }
}

/// Keep only the two newest `premigrate-vN.bak` snapshots; each migration used
/// to leave one behind forever (~107 MB of history on a single-user install).
fn prune_migration_backups(path: &std::path::Path) {
    const KEEP: usize = 2;
    let Ok(entries) = std::fs::read_dir(path.parent().unwrap_or(std::path::Path::new(".")))
    else {
        return;
    };
    let prefix = format!("{DB_FILE_NAME}.premigrate-");
    let mut backups: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
        })
        .filter_map(|p| {
            let mtime = p.metadata().and_then(|m| m.modified()).ok()?;
            Some((mtime, p))
        })
        .collect();
    // Newest first; mtime (not the version number in the name) is the truth,
    // because several historical snapshots share the same size and order is
    // only meaningful by when they were taken.
    backups.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in backups.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(p);
    }
}

/// The current schema, collapsed from the historical v1–v19 evolution
/// (2026-10). Each migration's DDL was merged into one idempotent baseline, so
/// a fresh database is born at the shape v19 used to leave behind.
///
/// One deliberate difference: the dead `articles.title_zh` column (deprecated,
/// zero code references) is dropped by v2 below instead of carried forever.
const BASELINE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS articles (
    id TEXT PRIMARY KEY,
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    category TEXT NOT NULL,
    published_at TEXT,
    content_text TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    origin TEXT NOT NULL DEFAULT 'rss',
    summary_zh TEXT NOT NULL DEFAULT '',
    last_opened_at TEXT,
    open_count INTEGER NOT NULL DEFAULT 0,
    word_count INTEGER NOT NULL DEFAULT 0,
    quality TEXT NOT NULL DEFAULT '',
    extraction_source TEXT NOT NULL DEFAULT '',
    dwell_ms INTEGER NOT NULL DEFAULT 0,
    read_completed INTEGER NOT NULL DEFAULT 0,
    liked INTEGER NOT NULL DEFAULT 0,
    chapter_no INTEGER
);
CREATE TABLE IF NOT EXISTS feed_sources (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    enabled INTEGER NOT NULL DEFAULT 1,
    origin TEXT NOT NULL DEFAULT 'curated',
    description TEXT NOT NULL DEFAULT '',
    etag TEXT NOT NULL DEFAULT '',
    last_fetched_at TEXT,
    fulltext_ratio REAL NOT NULL DEFAULT -1,
    priority INTEGER NOT NULL DEFAULT 0,
    fail_count INTEGER NOT NULL DEFAULT 0,
    last_new_article_at TEXT,
    kind TEXT NOT NULL DEFAULT 'rss'
);
CREATE TABLE IF NOT EXISTS feed_categories (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    builtin INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS translations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    article_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    scope_key TEXT NOT NULL,
    source_text TEXT NOT NULL,
    translated_text TEXT NOT NULL,
    model TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(article_id, scope, scope_key),
    FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS memory_items (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'word',
    term TEXT NOT NULL,
    definition_zh TEXT NOT NULL DEFAULT '',
    word_type TEXT NOT NULL DEFAULT '',
    collocations_json TEXT NOT NULL DEFAULT '[]',
    context_sentence TEXT NOT NULL DEFAULT '',
    article_id TEXT,
    status TEXT NOT NULL DEFAULT 'learning',
    interval_days REAL NOT NULL DEFAULT 0,
    reps INTEGER NOT NULL DEFAULT 0,
    consecutive_know INTEGER NOT NULL DEFAULT 0,
    next_review_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE SET NULL
);
CREATE TABLE IF NOT EXISTS known_words (
    term TEXT PRIMARY KEY,
    created_at TEXT NOT NULL
);
-- 查词历史功能已移除，表与索引零引用，仅保留 schema 免迁移。
CREATE TABLE IF NOT EXISTS lookup_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    term TEXT NOT NULL,
    context TEXT NOT NULL DEFAULT '',
    article_id TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS app_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS removed_feeds (
    id TEXT PRIMARY KEY,
    removed_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS book_catalog (
    gid INTEGER PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    authors TEXT NOT NULL DEFAULT '',
    issued TEXT NOT NULL DEFAULT '',
    shelf TEXT NOT NULL DEFAULT '',
    subjects TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_articles_category ON articles(category);
CREATE INDEX IF NOT EXISTS idx_articles_fetched ON articles(fetched_at DESC);
CREATE INDEX IF NOT EXISTS idx_articles_origin_quality ON articles(origin, quality);
CREATE INDEX IF NOT EXISTS idx_translations_article ON translations(article_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_kind_term ON memory_items(kind, lower(term));
CREATE INDEX IF NOT EXISTS idx_memory_kind_status ON memory_items(kind, status);
CREATE INDEX IF NOT EXISTS idx_memory_next ON memory_items(next_review_at);
CREATE INDEX IF NOT EXISTS idx_memory_article ON memory_items(article_id);
CREATE INDEX IF NOT EXISTS idx_lookup_created ON lookup_history(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_lookup_term ON lookup_history(lower(term));
"#;

/// Version-gated migrations. To add one: raise `LATEST_VERSION` and apply its
/// DDL inside `migrate` when `stored < N`. Stamp each version with its own
/// number (never `LATEST_VERSION`) so later steps are not skipped.
pub(crate) const LATEST_VERSION: i64 = 2;

/// The highest user_version any pre-collapse database can carry. Backup files
/// written before the collapse (`.premigrate-v*.bak`) stamp up to 19; imports
/// must accept them even though the counter now tops out lower.
const MAX_IMPORTABLE_VERSION: i64 = 19;

/// Drop the deprecated `articles.title_zh` column (v2). Tolerates "no such
/// column": fresh baselines never had it.
fn drop_title_zh_column(conn: &Connection) -> Result<(), AppError> {
    if let Err(e) = conn.execute("ALTER TABLE articles DROP COLUMN title_zh", []) {
        let msg = e.to_string();
        if !msg.contains("no such column") {
            return Err(AppError::from(e));
        }
    }
    Ok(())
}

pub(crate) fn migrate(conn: &Connection) -> Result<(), AppError> {
    let mut stored: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        ?;

    // Pre-collapse databases stamp 1..=19. Their schema is exactly what
    // BASELINE_SCHEMA reproduces (plus the dead title_zh column), so instead
    // of replaying history, normalize the counter to 1 and let v2 below drop
    // the dead column. Normalizing matters: leaving stored=19 would make every
    // future `stored < N` gate skip its migration forever.
    if stored > LATEST_VERSION {
        stored = 1;
    }

    if stored < 1 {
        conn.execute_batch(BASELINE_SCHEMA)?;
        conn.pragma_update(None, "user_version", 1)?;
        stored = 1;
    }

    if stored < 2 {
        drop_title_zh_column(conn)?;
        conn.pragma_update(None, "user_version", 2)?;
        stored = 2;
    }

    if stored < LATEST_VERSION {
        return Err(AppError::msg(format!(
            "incomplete schema migration: user_version={stored}, expected {LATEST_VERSION}"
        )));
    }
    Ok(())
}