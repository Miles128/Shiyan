use crate::error::AppError;
use super::{Article, ArticleListItem};
use rusqlite::{params, Connection, OptionalExtension};

/// Full-row projection. The list projection is derived from this (see
/// [`query_articles`]) so the column list cannot drift between the two.
const ARTICLE_COLS: &str =
    "id,url,title,source,category,published_at,content_text,fetched_at,origin,summary_zh,last_opened_at,open_count,word_count,quality,extraction_source,dwell_ms,read_completed,liked,tags_json";

/// Home list only needs an excerpt (known% + blurb). Full body stays on get_article.
pub const LIST_EXCERPT_CHARS: i32 = 6000;

pub fn list_articles(
    conn: &Connection,
    category: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ArticleListItem>, AppError> {
    query_articles(
        conn,
        &ArticleQuery {
            category,
            ..Default::default()
        },
        limit,
        offset,
    )
}

/// Limit clamped: SQLite treats a negative LIMIT as "no limit", so a bad
/// caller could request the whole table (× 6000-char excerpt per row).
const MAX_LIST_LIMIT: i64 = 500;

/// Read-state filter. "Finished" means scrolled to the end (the reader sets
/// the flag on reaching the bottom; dwell time only feeds stats/ranking and
/// no longer gates completion). Merely opening an article puts it in
/// `Reading`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadState {
    #[default]
    All,
    /// Never opened and not finished.
    Unread,
    /// Opened but not finished — stays in the list forever.
    Reading,
    /// Not finished (unread + reading): the home digest default.
    Unfinished,
    Read,
}

/// Library / list filters. Empty fields are ignored.
#[derive(Debug, Clone, Default)]
pub struct ArticleQuery<'a> {
    pub category: Option<&'a str>,
    /// OR across tags.
    pub tags: &'a [String],
    /// Exact source (publication) name.
    pub source: Option<&'a str>,
    pub read_state: ReadState,
    pub liked_only: bool,
    /// Newest first (library) instead of source-grouped (default).
    pub recent_first: bool,
}

/// Filtered article list. Tags are OR; everything else is AND.
pub fn query_articles(
    conn: &Connection,
    query: &ArticleQuery<'_>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ArticleListItem>, AppError> {
    // Same columns as ARTICLE_COLS, with the body truncated to an excerpt.
    let list_cols = ARTICLE_COLS.replace(
        "content_text",
        &format!("SUBSTR(content_text,1,{LIST_EXCERPT_CHARS})"),
    );
    let mut sql = format!("SELECT {list_cols} FROM articles");
    let mut params: Vec<rusqlite::types::Value> = vec![];
    let mut clauses: Vec<String> = vec![];
    if let Some(cat) = query.category.filter(|c| *c != "all") {
        clauses.push("category=?".into());
        params.push(rusqlite::types::Value::Text(cat.to_string()));
    }
    if !query.tags.is_empty() {
        // json_each() errors on an empty string — articles without tags can
        // never match a tag filter, so exclude them up front.
        clauses.push("tags_json <> ''".into());
        let ors: Vec<String> = query
            .tags
            .iter()
            .map(|_| "EXISTS (SELECT 1 FROM json_each(tags_json) WHERE value=?)".to_string())
            .collect();
        clauses.push(format!("({})", ors.join(" OR ")));
        for tag in query.tags {
            params.push(rusqlite::types::Value::Text(tag.clone()));
        }
    }
    if let Some(source) = query.source.filter(|s| !s.is_empty()) {
        clauses.push("source=?".into());
        params.push(rusqlite::types::Value::Text(source.to_string()));
    }
    match query.read_state {
        ReadState::All => {}
        ReadState::Unfinished => clauses.push("read_completed = 0".into()),
        ReadState::Unread => {
            clauses.push("read_completed = 0 AND last_opened_at IS NULL".into())
        }
        ReadState::Reading => {
            clauses.push("read_completed = 0 AND last_opened_at IS NOT NULL".into())
        }
        ReadState::Read => clauses.push("read_completed = 1".into()),
    }
    if query.liked_only {
        clauses.push("liked=1".into());
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    if query.recent_first {
        sql.push_str(" ORDER BY fetched_at DESC, published_at DESC, id ASC");
    } else {
        // For ranked scoring, prefer recent articles over alphabetical source
        // ordering so the window is not starved by source name.
        sql.push_str(" ORDER BY fetched_at DESC, published_at DESC, id ASC");
    }
    sql.push_str(" LIMIT ? OFFSET ?");
    params.push(rusqlite::types::Value::Integer(
        limit.unwrap_or(60).clamp(1, MAX_LIST_LIMIT),
    ));
    params.push(rusqlite::types::Value::Integer(offset.unwrap_or(0).max(0)));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), map_article_list_item)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Distinct sources with article counts, busiest first (library filter).
pub fn list_article_sources(conn: &Connection) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT source, COUNT(*) FROM articles GROUP BY source ORDER BY COUNT(*) DESC, source ASC")
        ?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

pub fn map_article_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArticleListItem> {
    Ok(ArticleListItem {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        source: row.get(3)?,
        category: row.get(4)?,
        published_at: row.get(5)?,
        excerpt: row.get(6)?,
        fetched_at: row.get(7)?,
        origin: row.get(8)?,
        summary_zh: row.get(9)?,
        last_opened_at: row.get(10)?,
        open_count: row.get(11)?,
        word_count: row.get(12)?,
        rank_score: 0.0,
        dwell_ms: row.get(15)?,
        read_completed: row.get::<_, i64>(16)? != 0,
        liked: row.get::<_, i64>(17)? != 0,
        tags: parse_tags_json(&row.get::<_, String>(18)?),
    })
}

pub fn map_article(row: &rusqlite::Row<'_>) -> rusqlite::Result<Article> {
    Ok(Article {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        source: row.get(3)?,
        category: row.get(4)?,
        published_at: row.get(5)?,
        content_text: row.get(6)?,
        fetched_at: row.get(7)?,
        origin: row.get(8)?,
        summary_zh: row.get(9)?,
        last_opened_at: row.get(10)?,
        open_count: row.get(11)?,
        word_count: row.get(12)?,
        quality: row.get(13)?,
        extraction_source: row.get(14)?,
        dwell_ms: row.get(15)?,
        read_completed: row.get::<_, i64>(16)? != 0,
        liked: row.get::<_, i64>(17)? != 0,
        tags: parse_tags_json(&row.get::<_, String>(18)?),
    })
}

pub fn mark_article_opened(conn: &Connection, id: &str) -> Result<(), AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let changed = conn
        .execute(
            "UPDATE articles SET last_opened_at=?1, open_count=open_count+1 WHERE id=?2",
            params![now, id],
        )
        ?;
    if changed == 0 {
        return Err(AppError::msg("article not found"));
    }
    Ok(())
}

pub fn learning_stats(conn: &Connection) -> Result<super::LearningStats, AppError> {
    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let opened_total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM articles WHERE last_opened_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        ?;
    let opened_7d: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM articles WHERE last_opened_at >= ?1",
            params![since],
            |row| row.get(0),
        )
        ?;
    // "Today" means the machine-local day, matching `reading_stats`.
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let opened_today: i64 = {
        let mut stmt = conn.prepare(
            "SELECT last_opened_at FROM articles WHERE last_opened_at IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut count = 0i64;
        for row in rows {
            if local_day(&row?) == today {
                count += 1;
            }
        }
        count
    };
    let top_source = conn
        .query_row(
            "SELECT source FROM articles WHERE last_opened_at IS NOT NULL
             GROUP BY source ORDER BY SUM(open_count) DESC, source ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
    .map_err(AppError::from)
        ?;
    let top_category = conn
        .query_row(
            "SELECT category FROM articles WHERE last_opened_at IS NOT NULL
             GROUP BY category ORDER BY SUM(open_count) DESC, category ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
    .map_err(AppError::from)
        ?;
    let vocab_created_7d: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_items WHERE kind='word' AND created_at >= ?1",
            params![since],
            |row| row.get(0),
        )
        ?;
    let vocab_learning: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_items WHERE kind='word' AND status='learning'",
            [],
            |row| row.get(0),
        )
        ?;
    Ok(super::LearningStats {
        opened_total,
        opened_today,
        opened_7d,
        top_source,
        top_category,
        vocab_created_7d,
        vocab_learning,
    })
}

/// Reading statistics for the stats page (last 14 days + totals).
/// Calendar day of an RFC3339 timestamp in the machine-local timezone. The
/// app runs on the reader's own Mac, so stats follow the wall clock they see.
/// Unparseable values fall back to the UTC date prefix (previous behavior).
pub(crate) fn local_day(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| rfc3339.get(..10).unwrap_or("").to_string())
}

pub fn reading_stats(conn: &Connection) -> Result<super::ReadingStats, AppError> {
    const DAILY_DAYS: i64 = 14;

    // Aggregate in Rust so days are machine-local, not UTC substrings.
    // (articles, dwell_ms sum, word_count sum) per local day.
    let mut by_date: std::collections::HashMap<String, (i64, i64, i64)> =
        std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT last_opened_at, IFNULL(dwell_ms,0), IFNULL(word_count,0)
             FROM articles WHERE last_opened_at IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (opened_at, dwell_ms, word_count) = row?;
            let bucket = by_date.entry(local_day(&opened_at)).or_insert((0, 0, 0));
            bucket.0 += 1;
            bucket.1 += dwell_ms;
            bucket.2 += word_count;
        }
    }

    let today = chrono::Local::now().date_naive();
    let mut days: Vec<super::ReadingDay> = Vec::new();
    for offset in (0..DAILY_DAYS).rev() {
        let date = (today - chrono::Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string();
        let (articles, dwell_sum, words) = by_date.get(&date).copied().unwrap_or((0, 0, 0));
        days.push(super::ReadingDay {
            date,
            articles,
            minutes: dwell_sum / 60000,
            words,
        });
    }

    // Streak: consecutive local days with activity, starting today or yesterday.
    let present: std::collections::HashSet<&str> =
        by_date.keys().map(|s| s.as_str()).collect();
    let day_key = |d: chrono::NaiveDate| d.format("%Y-%m-%d").to_string();
    let start = if present.contains(day_key(today).as_str()) {
        today
    } else if present.contains(day_key(today - chrono::Duration::days(1)).as_str()) {
        today - chrono::Duration::days(1)
    } else {
        return build_stats(conn, days, 0);
    };
    let mut streak_days = 0i64;
    let mut cursor = start;
    while present.contains(day_key(cursor).as_str()) {
        streak_days += 1;
        cursor -= chrono::Duration::days(1);
    }

    build_stats(conn, days, streak_days)
}

fn build_stats(
    conn: &Connection,
    days: Vec<super::ReadingDay>,
    streak_days: i64,
) -> Result<super::ReadingStats, AppError> {
    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let scalar = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Result<i64, AppError> {
        conn.query_row(sql, params, |row| row.get::<_, i64>(0))
            .map_err(AppError::from)
    };

    let read_filter = "last_opened_at IS NOT NULL";
    let articles_total = scalar(
        &format!("SELECT COUNT(*) FROM articles WHERE {read_filter}"),
        &[],
    )?;
    let articles_7d = scalar(
        &format!("SELECT COUNT(*) FROM articles WHERE {read_filter} AND last_opened_at >= ?1"),
        &[&since],
    )?;
    let completed_total = scalar(
        "SELECT COUNT(*) FROM articles WHERE read_completed = 1",
        &[],
    )?;
    let liked_total = scalar("SELECT COUNT(*) FROM articles WHERE liked = 1", &[])?;
    let minutes_total = scalar(
        &format!("SELECT IFNULL(SUM(dwell_ms),0)/60000 FROM articles WHERE {read_filter}"),
        &[],
    )?;
    let minutes_7d = scalar(
        &format!(
            "SELECT IFNULL(SUM(dwell_ms),0)/60000 FROM articles WHERE {read_filter} AND last_opened_at >= ?1"
        ),
        &[&since],
    )?;
    let words_total = scalar(
        &format!("SELECT IFNULL(SUM(word_count),0) FROM articles WHERE {read_filter}"),
        &[],
    )?;

    let by_status = |kind: &str, status: &str| -> Result<i64, AppError> {
        conn.query_row(
            "SELECT COUNT(*) FROM memory_items WHERE kind=?1 AND status=?2",
            rusqlite::params![kind, status],
            |row| row.get::<_, i64>(0),
        )
        .map_err(AppError::from)
    };
    let vocab_learning = by_status("word", "learning")?;
    let vocab_mastered = by_status("word", "mastered")?;
    let phrases_learning = by_status("phrase", "learning")?;
    let phrases_mastered = by_status("phrase", "mastered")?;

    let due_since = chrono::Utc::now().to_rfc3339();
    let due_today = scalar(
        "SELECT COUNT(*) FROM memory_items WHERE status='learning' AND next_review_at <= ?1",
        &[&due_since],
    )?;

    let top_sources = {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT source, COUNT(*), CAST(IFNULL(SUM(dwell_ms),0)/60000 AS INTEGER)
                 FROM articles WHERE {read_filter}
                 GROUP BY source ORDER BY SUM(dwell_ms) DESC, COUNT(*) DESC LIMIT 6"
            ))
            ?;
        let rows = stmt
            .query_map([], |row| {
                Ok(super::SourceStat {
                    name: row.get(0)?,
                    articles: row.get(1)?,
                    minutes: row.get(2)?,
                })
            })
            ?
            .collect::<Result<Vec<_>, _>>()
            ?;
        rows
    };

    Ok(super::ReadingStats {
        days,
        streak_days,
        articles_total,
        articles_7d,
        completed_total,
        liked_total,
        minutes_total,
        minutes_7d,
        words_total,
        vocab_learning,
        vocab_mastered,
        phrases_learning,
        phrases_mastered,
        due_today,
        top_sources,
    })
}

pub fn get_article(conn: &Connection, id: &str) -> Result<Option<Article>, AppError> {
    conn.query_row(
        &format!("SELECT {ARTICLE_COLS} FROM articles WHERE id=?1"),
        params![id],
        map_article,
    )
    .optional()
    .map_err(AppError::from)
    
}

pub fn get_article_by_url(conn: &Connection, url: &str) -> Result<Option<Article>, AppError> {
    conn.query_row(
        &format!("SELECT {ARTICLE_COLS} FROM articles WHERE url=?1"),
        params![url],
        map_article,
    )
    .optional()
    .map_err(AppError::from)
    
}

pub fn list_article_urls(conn: &Connection) -> Result<std::collections::HashSet<String>, AppError> {
    let mut stmt = conn
        .prepare("SELECT url FROM articles")
        ?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        ?
        .collect::<Result<std::collections::HashSet<_>, _>>()
        ?;
    Ok(rows)
}

/// url → stored body length in bytes (used to refresh stale RSS bodies).
pub fn list_article_content_lengths(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, usize>, AppError> {
    let mut stmt = conn
        .prepare("SELECT url, LENGTH(content_text) FROM articles")
        ?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize)))
        ?
        .collect::<Result<std::collections::HashMap<_, _>, _>>()
        ?;
    Ok(rows)
}

/// Insert only when `url` is new. Returns `true` if inserted, `false` if already present.
/// Idempotent: never overwrites existing content / translations.
pub fn insert_article_if_new(conn: &Connection, a: &Article) -> Result<bool, AppError> {
    let changed = conn
        .execute(
            "INSERT INTO articles (id,url,title,source,category,published_at,content_text,fetched_at,origin,summary_zh,word_count,quality,extraction_source)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(url) DO NOTHING",
            params![
                a.id,
                a.url,
                a.title,
                a.source,
                a.category,
                a.published_at,
                a.content_text,
                a.fetched_at,
                a.origin,
                a.summary_zh,
                a.word_count,
                a.quality,
                a.extraction_source,
            ],
        )
        ?;
    Ok(changed > 0)
}

/// Refresh an existing RSS article when a longer full-text body is available.
/// Matches by URL: callers may build the update struct with an empty/fresh id
/// (the RSS refresh path does exactly that).
/// Keeps id / url / title / source / category / published_at / origin intact.
/// Clears `summary_zh` so the refresh pipeline regenerates it for the new body.
/// Also clears `tags_json` so stale tags (describing the old body) are replaced.
pub fn refresh_article_content(conn: &Connection, a: &Article) -> Result<bool, AppError> {
    let changed = conn
        .execute(
            "UPDATE articles SET title=?1, content_text=?2, fetched_at=?3, summary_zh='',
                    word_count=?4, quality='fulltext', extraction_source=?5
             WHERE url=?6 AND origin='rss' AND content_text <> ?2",
            params![a.title, a.content_text, a.fetched_at, a.word_count, a.extraction_source, a.url],
        )
        ?;
    if changed > 0 {
        // Paragraph translations are keyed by index: a new body makes every
        // stored row point at the wrong paragraph. Invalidate them here, on
        // the same connection the caller locked, so the body swap and the
        // invalidation are never observed separately by another writer.
        let id: Option<String> = conn
            .query_row("SELECT id FROM articles WHERE url=?1", params![a.url], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(id) = id {
            super::delete_paragraph_translations(conn, &id)?;
            // Tags describe the old body; clear so `fill_missing_tags` refills.
            conn.execute(
                "UPDATE articles SET tags_json='' WHERE id=?1",
                params![id],
            )?;
        }
    }
    Ok(changed > 0)
}

/// RSS articles whose body quality has not been assessed yet (`quality=''`).
/// Legacy rows get assessed once during refresh; new rows are stamped on insert.
pub fn list_unassessed_rss_articles(conn: &Connection) -> Result<Vec<Article>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ARTICLE_COLS} FROM articles WHERE origin='rss' AND quality=''"
        ))
        ?;
    let rows = stmt
        .query_map([], map_article)
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// Store topic tags (lowercase English) produced by card translation.
/// Empty string = no tags yet; overwritten wholesale on refresh.
pub fn set_article_tags(conn: &Connection, id: &str, tags: &[String]) -> Result<(), AppError> {
    let json = if tags.is_empty() {
        String::new()
    } else {
        serde_json::to_string(tags)?
    };
    conn.execute(
        "UPDATE articles SET tags_json=?1 WHERE id=?2",
        params![json, id],
    )
    ?;
    Ok(())
}

/// Stamp the body-quality verdict on an article (once; never re-derived later).
pub fn set_article_quality(
    conn: &Connection,
    id: &str,
    quality: &str,
    extraction_source: &str,
    word_count: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE articles SET quality=?1, extraction_source=?2, word_count=?3 WHERE id=?4",
        params![quality, extraction_source, word_count, id],
    )
    ?;
    Ok(())
}

/// Accumulate visible reading time and optionally mark the article as read to the end.
/// Delta is clamped so a buggy client can't inflate a session in one call.
pub fn add_article_reading_progress(
    conn: &Connection,
    id: &str,
    dwell_ms_delta: i64,
    read_completed: bool,
) -> Result<(), AppError> {
    const MAX_DWELL_DELTA_MS: i64 = 120_000;
    let delta = dwell_ms_delta.clamp(-MAX_DWELL_DELTA_MS, MAX_DWELL_DELTA_MS);
    let changed = conn
        .execute(
            "UPDATE articles
             SET dwell_ms = MAX(0, dwell_ms + ?1),
                 read_completed = CASE WHEN ?2 THEN 1 ELSE read_completed END
             WHERE id=?3",
            params![delta, read_completed, id],
        )
        ?;
    if changed == 0 {
        return Err(AppError::msg("article not found"));
    }
    Ok(())
}

pub fn set_article_liked(conn: &Connection, id: &str, liked: bool) -> Result<(), AppError> {
    let changed = conn
        .execute(
            "UPDATE articles SET liked=?1 WHERE id=?2",
            params![liked, id],
        )
        ?;
    if changed == 0 {
        return Err(AppError::msg("article not found"));
    }
    Ok(())
}

/// `(title, url)` of articles ingested within the dedup window — the seed for
/// the refresh-time title-dedup index. Windowed so recurring same-name
/// features (daily briefings, link roundups) are never swallowed forever.
pub fn list_article_titles(
    conn: &Connection,
    since: Option<&str>,
) -> Result<Vec<(String, String)>, AppError> {
    let (sql, args): (&str, Vec<&str>) = match since {
        Some(s) => ("SELECT title, url FROM articles WHERE fetched_at >= ?1", vec![s]),
        None => ("SELECT title, url FROM articles", vec![]),
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// Every stored RSS article (full body) — used by one-time content audits.
pub fn list_all_rss_articles(conn: &Connection) -> Result<Vec<Article>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ARTICLE_COLS} FROM articles WHERE origin='rss'"
        ))
        ?;
    let rows = stmt
        .query_map([], map_article)
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// Retention purge: drop auto-ingested articles whose publication (falling
/// back to fetch) time is older than `cutoff`. Liked articles and user
/// imports are kept.
pub fn purge_old_rss_articles(conn: &Connection, cutoff_rfc3339: &str) -> Result<usize, AppError> {
    let changed = conn
        .execute(
            // 在读 (opened but unfinished) never disappears, even past retention.
            "DELETE FROM articles
             WHERE origin='rss' AND liked=0
               AND NOT (last_opened_at IS NOT NULL AND read_completed = 0)
               AND julianday(COALESCE(published_at, fetched_at)) < julianday(?1)",
            params![cutoff_rfc3339],
        )
        ?;
    Ok(changed)
}

/// One-time cleanup for rows with no word count: truly empty bodies are
/// deleted, and bodies that were never measured (pre-v6 rows) are counted now.
/// Returns the number of rows whose count was stamped.
pub fn backfill_word_counts(conn: &Connection) -> Result<usize, AppError> {
    conn.execute("DELETE FROM articles WHERE TRIM(content_text) = ''", [])?;
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, content_text FROM articles WHERE word_count = 0",
        )?;
        let mapped = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let mut stamped = 0;
    for (id, text) in rows {
        let wc = text.split_whitespace().count() as i64;
        if wc == 0 {
            conn.execute("DELETE FROM articles WHERE id=?1", params![id])?;
        } else {
            conn.execute(
                "UPDATE articles SET word_count=?1 WHERE id=?2",
                params![wc, id],
            )?;
            stamped += 1;
        }
    }
    Ok(stamped)
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT value FROM app_meta WHERE key=?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )
    ?;
    Ok(())
}

/// Parse the stored tags JSON; malformed/empty → no tags.
pub fn parse_tags_json(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return vec![];
    }
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

/// Articles that still lack topic tags (for the one-time / rolling backfill).
pub fn articles_missing_tags(conn: &Connection, limit: usize) -> Result<Vec<Article>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ARTICLE_COLS} FROM articles
             WHERE IFNULL(tags_json,'') = '' AND quality='fulltext'
             ORDER BY fetched_at DESC
             LIMIT ?1"
        ))
        ?;
    let rows = stmt
        .query_map(params![limit as i64], map_article)
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// Interest profile inputs for tag-based ranking:
/// - `user_weights`: tag → engagement weight (liked 2.0, read-to-end 1.0)
/// - `doc_counts`: tag → number of articles carrying it
/// - `docs`: number of tagged articles (IDF denominator)
pub fn tag_profile(
    conn: &Connection,
) -> Result<
    (
        std::collections::HashMap<String, f64>,
        std::collections::HashMap<String, i64>,
        i64,
    ),
    AppError,
> {
    let mut stmt = conn
        .prepare("SELECT tags_json, liked, read_completed FROM articles")
        ?;
    let mut rows = stmt.query([])?;

    let mut user_weights: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    let mut doc_counts: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    let mut docs = 0i64;

    while let Some(row) = rows.next()? {
        let tags = parse_tags_json(&row.get::<_, String>(0)?);
        if tags.is_empty() {
            continue;
        }
        docs += 1;
        let liked: i64 = row.get(1)?;
        let completed: i64 = row.get(2)?;
        let weight = if liked != 0 {
            2.0
        } else if completed != 0 {
            1.0
        } else {
            0.0
        };
        for tag in tags {
            *doc_counts.entry(tag.clone()).or_insert(0) += 1;
            if weight > 0.0 {
                *user_weights.entry(tag).or_insert(0.0) += weight;
            }
        }
    }
    Ok((user_weights, doc_counts, docs))
}

/// All-time open counts grouped by source and by category — the affinity
/// signal for article ranking.
pub fn affinity_open_counts(
    conn: &Connection,
) -> Result<(std::collections::HashMap<String, i64>, std::collections::HashMap<String, i64>), AppError>
{
    let read = |sql: &str| -> Result<std::collections::HashMap<String, i64>, AppError> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            ?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()
            ?;
        Ok(rows)
    };
    let by_source = read(
        "SELECT source, SUM(open_count) FROM articles WHERE open_count > 0 GROUP BY source",
    )?;
    let by_category = read(
        "SELECT category, SUM(open_count) FROM articles WHERE open_count > 0 GROUP BY category",
    )?;
    Ok((by_source, by_category))
}

#[cfg(test)]
pub fn upsert_article(conn: &Connection, a: &Article) -> Result<(), AppError> {
    // Legacy alias used by older tests; refresh path uses insert_article_if_new.
    let _ = insert_article_if_new(conn, a)?;
    Ok(())
}

pub fn delete_article(conn: &Connection, id: &str) -> Result<(), AppError> {
    // translations CASCADE; vocab.article_id SET NULL (schema v3).
    conn.execute("DELETE FROM articles WHERE id=?1", params![id])
        ?;
    Ok(())
}

pub fn articles_missing_card_zh(conn: &Connection, limit: usize) -> Result<Vec<Article>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ARTICLE_COLS} FROM articles
             WHERE IFNULL(summary_zh,'') = ''
             ORDER BY fetched_at DESC
             LIMIT ?1"
        ))
        ?;
    let rows = stmt
        .query_map(params![limit as i64], map_article)
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// RSS articles whose stored body has no paragraph breaks (extraction loss),
/// newest first — candidates for a re-fetch repair.
pub fn articles_without_paragraphs(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<Article>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ARTICLE_COLS} FROM articles
         WHERE origin='rss' AND url LIKE 'http%'
           AND instr(content_text, char(10)) = 0
         ORDER BY fetched_at DESC
         LIMIT ?1"
    ))?;
    let rows = stmt
        .query_map(params![limit as i64], map_article)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Replace an article body after a successful re-extraction.
pub fn set_article_body(
    conn: &Connection,
    id: &str,
    content_text: &str,
    word_count: i64,
    extraction_source: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE articles SET content_text=?1, word_count=?2, quality='fulltext', extraction_source=?3 WHERE id=?4",
        params![content_text, word_count, extraction_source, id],
    )?;
    // Same index-key invalidation as `refresh_article_content`: the repaired
    // body re-splits into different paragraphs.
    super::delete_paragraph_translations(conn, id)?;
    Ok(())
}

pub fn set_article_summary_zh(conn: &Connection, id: &str, summary_zh: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE articles SET summary_zh=?1 WHERE id=?2",
        params![summary_zh, id],
    )
    ?;
    Ok(())
}

/// Articles the reader downloaded from the public-domain shelf.
pub fn list_articles_by_origin(
    conn: &Connection,
    origin: &str,
    limit: i64,
) -> Result<Vec<ArticleListItem>, AppError> {
    let list_cols = ARTICLE_COLS.replace(
        "content_text",
        &format!("SUBSTR(content_text,1,{LIST_EXCERPT_CHARS})"),
    );
    let mut stmt = conn.prepare(&format!(
        "SELECT {list_cols} FROM articles WHERE origin=?1 ORDER BY fetched_at DESC LIMIT ?2"
    ))?;
    let rows = stmt
        .query_map(params![origin, limit.clamp(1, MAX_LIST_LIMIT)], map_article_list_item)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn list_item_by_id(conn: &Connection, id: &str) -> Result<Option<ArticleListItem>, AppError> {
    let list_cols = ARTICLE_COLS.replace(
        "content_text",
        &format!("SUBSTR(content_text,1,{LIST_EXCERPT_CHARS})"),
    );
    conn.query_row(
        &format!("SELECT {list_cols} FROM articles WHERE id=?1"),
        params![id],
        map_article_list_item,
    )
    .optional()
    .map_err(AppError::from)
}

/// Articles whose title or body contains the learner's words.
/// `rank_score` is the weighted hit count (recent terms weigh more).
pub fn articles_matching_terms(
    conn: &Connection,
    terms: &[String],
    recent: &[String],
    limit: usize,
) -> Result<Vec<ArticleListItem>, AppError> {
    let weighted = crate::term_match::weight_terms(terms, recent);
    if weighted.is_empty() {
        return Ok(vec![]);
    }
    let limit = limit.clamp(1, 30);
    let mut stmt = conn.prepare("SELECT id, title, content_text FROM articles")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut scored: Vec<(String, f64)> = Vec::new();
    for row in rows {
        let (id, title, body) = row?;
        let hay = format!("{} {}", title.to_ascii_lowercase(), body.to_ascii_lowercase());
        let score = crate::term_match::score_text(&hay, &weighted);
        if score > 0 {
            scored.push((id, score as f64));
        }
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
    scored.truncate(limit);
    let mut out = Vec::new();
    for (id, score) in scored {
        if let Some(mut item) = list_item_by_id(conn, &id)? {
            item.rank_score = score;
            out.push(item);
        }
    }
    Ok(out)
}
