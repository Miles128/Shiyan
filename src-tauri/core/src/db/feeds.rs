use crate::error::AppError;
use super::{curated_feeds, FeedCategory, FeedSource};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub(crate) fn seed_feed_categories(conn: &Connection) -> Result<(), AppError> {
    let builtins = [
        ("tech", "科技"),
        ("finance", "财经"),
        ("world", "国际"),
        ("other", "其他"),
    ];
    for (id, label) in builtins {
        conn.execute(
            "INSERT OR IGNORE INTO feed_categories (id, label, builtin) VALUES (?1,?2,1)",
            params![id, label],
        )
        ?;
        conn.execute(
            "UPDATE feed_categories SET label=?1, builtin=1 WHERE id=?2",
            params![label, id],
        )
        ?;
    }
    Ok(())
}

pub(crate) fn seed_feeds(conn: &Connection) -> Result<(), AppError> {
    let seeds = match curated_feeds() {
        Ok(seeds) => seeds,
        Err(e) => {
            eprintln!("curated feeds seed skipped: {e}");
            return Ok(());
        }
    };
    let removed = removed_feed_ids(conn)?;
    // Insert newly curated feeds; IGNORE keeps existing enable/disable.
    // Tombstoned ids (removed by the one-shot dead-feed cleanup) stay gone.
    for f in &seeds {
        if removed.contains(f.id.as_str()) {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO feed_sources (id, name, category, url, enabled, origin, description) VALUES (?1,?2,?3,?4,1,'curated','')",
            params![f.id, f.name, f.category, f.url],
        )
        ?;
        // Keep name/category/url/origin in sync if we retarget a curated id.
        conn.execute(
            "UPDATE feed_sources SET name=?1, category=?2, url=?3, origin='curated' WHERE id=?4",
            params![f.name, f.category, f.url, f.id],
        )
        ?;
    }
    // Drop obsolete curated sources only — never delete user subscriptions.
    let keep: std::collections::HashSet<&str> = seeds.iter().map(|f| f.id.as_str()).collect();
    for existing in list_feeds(conn)? {
        if existing.origin != "user" && !keep.contains(existing.id.as_str()) {
            conn.execute("DELETE FROM feed_sources WHERE id=?1", params![existing.id])
                ?;
        }
    }
    Ok(())
}

pub fn list_feeds(conn: &Connection) -> Result<Vec<FeedSource>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id,name,category,url,enabled,origin,description,etag,last_fetched_at,fulltext_ratio,priority FROM feed_sources ORDER BY category, priority DESC, name",
        )
        ?;
    let rows = stmt
        .query_map([], map_feed)
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// Persist the sidebar drag order. `ordered_ids` is a flat top-to-bottom list of
/// feed ids as currently grouped in the tree; sources are ranked **within their
/// own category**, so dragging never couples unrelated categories. For each
/// category we assign priority descending by position (top = category size).
pub fn reorder_feeds(conn: &Connection, ordered_ids: &[String]) -> Result<(), AppError> {
    // Resolve each feed's category, preserving the caller's order.
    let mut resolved: Vec<(String, String)> = Vec::with_capacity(ordered_ids.len());
    for id in ordered_ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        let category: Option<String> = conn
            .query_row(
                "SELECT category FROM feed_sources WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(category) = category {
            resolved.push((id.to_string(), category));
        }
    }

    // Count per category to size each block's priority range.
    let mut cat_count: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for (_, cat) in &resolved {
        *cat_count.entry(cat.clone()).or_insert(0) += 1;
    }
    let mut cat_seen: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for (id, cat) in &resolved {
        let total = cat_count[cat];
        let index = cat_seen.entry(cat.clone()).or_insert(0);
        let priority = total - *index;
        *index += 1;
        conn.execute(
            "UPDATE feed_sources SET priority=?1 WHERE id=?2",
            params![priority, id],
        )?;
    }
    Ok(())
}

pub fn set_feed_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<(), AppError> {
    conn.execute(
        "UPDATE feed_sources SET enabled=?1 WHERE id=?2",
        params![if enabled { 1 } else { 0 }, id],
    )
    ?;
    Ok(())
}

/// Feed display name → user-assigned priority, for the home ranking bonus.
/// Articles carry a `source` name (not the feed id), so the map is keyed by name.
/// Only sources with priority > 0 are returned.
pub fn source_priority_map(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, i64>, AppError> {
    let mut stmt = conn.prepare("SELECT name, priority FROM feed_sources WHERE priority > 0")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
    Ok(rows)
}

/// Category → highest source priority in that category, the denominator that
/// normalises the per-category ranking bonus to [0, 1].
pub fn category_priority_max(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, i64>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT category, MAX(priority) FROM feed_sources WHERE priority > 0 GROUP BY category",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
    Ok(rows)
}

/// One-shot import of the legacy `config.disabled_feeds` list onto DB `enabled`.
pub fn apply_legacy_disabled_feeds(conn: &Connection, ids: &[String]) -> Result<(), AppError> {
    for id in ids {
        let id = id.trim();
        if !id.is_empty() {
            set_feed_enabled(conn, id, false)?;
        }
    }
    Ok(())
}

pub fn list_feed_categories(conn: &Connection) -> Result<Vec<FeedCategory>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id,label,builtin FROM feed_categories ORDER BY builtin DESC, label")
        ?;
    let rows = stmt
        .query_map([], map_category)
        ?
        .collect::<Result<Vec<_>, _>>()
        ?;
    Ok(rows)
}

/// Add a user category. `id` is slugified from label if empty.
pub fn add_feed_category(conn: &Connection, label: &str) -> Result<FeedCategory, AppError> {
    let label = label.trim();
    if label.is_empty() {
        return Err(AppError::msg("分类名不能为空"));
    }
    let id = slugify_id(label);
    if id.is_empty() {
        return Err(AppError::msg("无法生成分类 id"));
    }
    conn.execute(
        "INSERT INTO feed_categories (id, label, builtin) VALUES (?1,?2,0)",
        params![id, label],
    )
    .map_err(|e| match &e {
        rusqlite::Error::SqliteFailure(err, _) if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE as i32
            || err.code == rusqlite::ErrorCode::ConstraintViolation => {
            AppError::msg("分类已存在")
        }
        _ if e.to_string().contains("UNIQUE") => AppError::msg("分类已存在"),
        _ => AppError::from(e),
    })?;
    Ok(FeedCategory {
        id,
        label: label.into(),
        builtin: false,
    })
}

pub fn get_feed_category(conn: &Connection, id: &str) -> Result<Option<FeedCategory>, AppError> {
    conn.query_row(
        "SELECT id,label,builtin FROM feed_categories WHERE id=?1",
        params![id],
        map_category,
    )
    .optional()
    .map_err(AppError::from)
    
}

/// Subscribe or re-enable a user feed. If URL exists, enable and refresh metadata.
pub fn subscribe_feed(
    conn: &Connection,
    name: &str,
    category: &str,
    url: &str,
    description: &str,
) -> Result<FeedSource, AppError> {
    let name = name.trim();
    let url = url.trim();
    let category = category.trim();
    if name.is_empty() || url.is_empty() || category.is_empty() {
        return Err(AppError::msg("名称、分类与 URL 不能为空"));
    }
    // Same SSRF guard as refresh/validate: a direct `subscribe_feed` call must
    // not be able to store localhost / LAN / non-http targets that only blow
    // up later at refresh time.
    crate::feeds::net::ensure_public_http_url(url)?;
    if get_feed_category(conn, category)?.is_none() {
        return Err(AppError::msg(format!("未知分类：{category}")));
    }

    // Existing by URL?
    if let Some(existing) = find_feed_by_url(conn, url)? {
        conn.execute(
            "UPDATE feed_sources SET name=?1, category=?2, description=?3, enabled=1 WHERE id=?4",
            params![name, category, description, existing.id],
        )
        ?;
        return Ok(FeedSource {
            id: existing.id,
            name: name.into(),
            category: category.into(),
            url: url.into(),
            enabled: true,
            origin: existing.origin,
            description: description.into(),
            ..Default::default()
        });
    }

    let id = format!("user-{}", slugify_id(name));
    let id = if feed_id_exists(conn, &id)? {
        format!("user-{}", Uuid::new_v4())
    } else {
        id
    };

    conn.execute(
        "INSERT INTO feed_sources (id, name, category, url, enabled, origin, description) VALUES (?1,?2,?3,?4,1,'user',?5)",
        params![id, name, category, url, description],
    )
    ?;

    Ok(FeedSource {
        id,
        name: name.into(),
        category: category.into(),
        url: url.into(),
        enabled: true,
        origin: "user".into(),
        description: description.into(),
        ..Default::default()
    })
}

/// Hard-delete a user-subscribed feed. Curated feeds are rejected: they are
/// re-seeded with INSERT OR IGNORE at startup, so deleting one would silently
/// resurrect on the next launch — disable them instead.
pub fn delete_user_feed(conn: &Connection, id: &str) -> Result<(), AppError> {
    let origin: String = conn
        .query_row(
            "SELECT origin FROM feed_sources WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::msg("订阅源不存在"))?;
    if origin != "user" {
        return Err(AppError::msg("精选订阅源不支持删除，可改为停用"));
    }
    conn.execute("DELETE FROM feed_sources WHERE id=?1", params![id])?;
    Ok(())
}

/// Feed ids a one-time cleanup removed; the startup seed must not re-insert them.
pub(crate) fn removed_feed_ids(conn: &Connection) -> Result<std::collections::HashSet<String>, AppError> {
    let mut stmt = conn.prepare("SELECT id FROM removed_feeds")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    Ok(rows)
}

fn find_feed_by_url(conn: &Connection, url: &str) -> Result<Option<FeedSource>, AppError> {    conn.query_row(
        "SELECT id,name,category,url,enabled,origin,description,etag,last_fetched_at,fulltext_ratio,priority FROM feed_sources WHERE url=?1",
        params![url],
        map_feed,
    )
    .optional()
    .map_err(AppError::from)
    
}

fn feed_id_exists(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM feed_sources WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        ?;
    Ok(n > 0)
}

fn map_feed(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedSource> {
    Ok(FeedSource {
        id: row.get(0)?,
        name: row.get(1)?,
        category: row.get(2)?,
        url: row.get(3)?,
        enabled: row.get::<_, i64>(4)? == 1,
        origin: row.get(5)?,
        description: row.get(6)?,
        etag: row.get(7)?,
        last_fetched_at: row.get(8)?,
        fulltext_ratio: row.get(9)?,
        priority: row.get(10)?,
    })
}

/// Persist per-refresh metadata: HTTP ETag (for 304 reuse), fetch timestamp,
/// and the observed full-text ratio that adapts the next refresh's trust bar.
/// `fulltext_ratio` of `None` leaves the stored value untouched.
pub fn set_feed_refresh_meta(
    conn: &Connection,
    id: &str,
    etag: Option<&str>,
    last_fetched_at: &str,
    fulltext_ratio: Option<f64>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE feed_sources
         SET etag = COALESCE(?2, etag),
             last_fetched_at = ?3,
             fulltext_ratio = COALESCE(?4, fulltext_ratio)
         WHERE id=?1",
        params![id, etag, last_fetched_at, fulltext_ratio],
    )?;
    Ok(())
}

fn map_category(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedCategory> {
    Ok(FeedCategory {
        id: row.get(0)?,
        label: row.get(1)?,
        builtin: row.get::<_, i64>(2)? == 1,
    })
}

pub fn slugify_id(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        } else {
            // keep CJK / other letters as hex codepoints for stable ids
            out.push_str(&format!("u{:x}", ch as u32));
        }
    }
    out.trim_matches('-').chars().take(48).collect()
}