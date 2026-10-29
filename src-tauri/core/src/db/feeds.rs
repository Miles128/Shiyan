use crate::error::AppError;
use super::{count_map, curated_feeds, row_as, CountMap, FeedCategory, FeedSource};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub(crate) fn seed_feed_categories(conn: &Connection) -> Result<(), AppError> {
    let builtins = [
        ("tech", "科技"),
        ("finance", "财经"),
        ("world", "国际"),
        ("book", "书籍"),
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
    // Insert newly curated feeds; IGNORE keeps existing enable/disable.
    for f in &seeds {
        // 新种子从入库起算 30 天观察期；已存在的行被 IGNORE，保留原计时。
        conn.execute(
            "INSERT OR IGNORE INTO feed_sources (id, name, category, url, enabled, origin, description, last_new_article_at) VALUES (?1,?2,?3,?4,1,'curated','',?5)",
            params![f.id, f.name, f.category, f.url, Utc::now().to_rfc3339()],
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
            "SELECT id,name,category,url,enabled,origin,description,etag,fulltext_ratio,priority,last_new_article_at,kind FROM feed_sources ORDER BY category, priority DESC, name",
        )
        ?;
    let rows = stmt
        .query_map([], row_as::<FeedSource>)
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
    if enabled {
        // 重新启用 = 重新观察：沉寂计时从当下起算，否则停用数月后刚唤醒
        // 的源会在下一次刷新末尾被「30 天无新文章」清退。
        conn.execute(
            "UPDATE feed_sources SET enabled=1, last_new_article_at=?2 WHERE id=?1",
            params![id, Utc::now().to_rfc3339()],
        )?;
    } else {
        conn.execute(
            "UPDATE feed_sources SET enabled=0 WHERE id=?1",
            params![id],
        )?;
    }
    Ok(())
}

/// Feed display name → user-assigned priority, for the home ranking bonus.
/// Articles carry a `source` name (not the feed id), so the map is keyed by name.
/// Only sources with priority > 0 are returned.
pub fn source_priority_map(conn: &Connection) -> Result<CountMap, AppError> {
    count_map(conn, "SELECT name, priority FROM feed_sources WHERE priority > 0")
}

/// Category → highest source priority in that category, the denominator that
/// normalises the per-category ranking bonus to [0, 1].
pub fn category_priority_max(conn: &Connection) -> Result<CountMap, AppError> {
    count_map(
        conn,
        "SELECT category, MAX(priority) FROM feed_sources WHERE priority > 0 GROUP BY category",
    )
}

pub fn list_feed_categories(conn: &Connection) -> Result<Vec<FeedCategory>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id,label,builtin FROM feed_categories ORDER BY builtin DESC, label")
        ?;
    let rows = stmt
        .query_map([], row_as::<FeedCategory>)
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
        rusqlite::Error::SqliteFailure(err, _) if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
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
        row_as::<FeedCategory>,
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
        // Re-subscribing a known URL re-enables it and restarts the stale
        // clock (same rationale as set_feed_enabled).
        conn.execute(
            "UPDATE feed_sources SET name=?1, category=?2, description=?3, enabled=1, last_new_article_at=?5 WHERE id=?4",
            params![name, category, description, existing.id, Utc::now().to_rfc3339()],
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
        "INSERT INTO feed_sources (id, name, category, url, enabled, origin, description, last_new_article_at) VALUES (?1,?2,?3,?4,1,'user',?5,?6)",
        params![id, name, category, url, description, Utc::now().to_rfc3339()],
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
    let (origin, kind, name): (String, String, String) = conn
        .query_row(
            "SELECT origin, kind, name FROM feed_sources WHERE id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::msg("订阅源不存在"))?;
    if origin != "user" {
        return Err(AppError::msg("精选订阅源不支持删除，可改为停用"));
    }
    if kind == "book" {
        // Deleting a book means deleting its chapters: they carry no meaning
        // without the book row, and a re-import would resurrect the feed but
        // keep the orphaned articles around (url UNIQUE would block reinsert).
        conn.execute(
            "DELETE FROM articles WHERE source=?1 AND origin='book'",
            params![name],
        )?;
    }
    conn.execute("DELETE FROM feed_sources WHERE id=?1", params![id])?;
    Ok(())
}

/// Create (or re-enable) a book feed. One book = one feed row, kind='book':
/// the sidebar tree, the source filter and the article list all work unchanged,
/// while the refresh pipeline skips it (a book never produces new articles).
pub fn insert_book_feed(
    conn: &Connection,
    name: &str,
    url: &str,
    description: &str,
) -> Result<FeedSource, AppError> {
    let name = name.trim();
    let url = url.trim();
    if name.is_empty() || url.is_empty() {
        return Err(AppError::msg("书名与链接不能为空"));
    }
    let id = format!("book-{}", slugify_id(name));
    if let Some(existing) = find_feed_by_url(conn, url)? {
        conn.execute(
            "UPDATE feed_sources SET name=?1, category='book', description=?2, enabled=1, kind='book' WHERE id=?3",
            params![name, description, existing.id],
        )?;
        return Ok(FeedSource {
            name: name.into(),
            category: "book".into(),
            description: description.into(),
            enabled: true,
            kind: "book".into(),
            ..existing
        });
    }
    let id = if feed_id_exists(conn, &id)? {
        format!("book-{}", Uuid::new_v4())
    } else {
        id
    };
    conn.execute(
        "INSERT INTO feed_sources (id, name, category, url, enabled, origin, description, last_new_article_at, kind)
         VALUES (?1,?2,'book',?3,1,'user',?4,?5,'book')",
        params![id, name, url, description, Utc::now().to_rfc3339()],
    )?;
    Ok(FeedSource {
        id,
        name: name.into(),
        category: "book".into(),
        url: url.into(),
        enabled: true,
        origin: "user".into(),
        description: description.into(),
        kind: "book".into(),
        ..Default::default()
    })
}

fn find_feed_by_url(conn: &Connection, url: &str) -> Result<Option<FeedSource>, AppError> {    conn.query_row(
        "SELECT id,name,category,url,enabled,origin,description,etag,fulltext_ratio,priority,last_new_article_at,kind FROM feed_sources WHERE url=?1",
        params![url],
        row_as::<FeedSource>,
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

/// Persist per-refresh metadata: HTTP ETag (for 304 reuse) and the observed
/// full-text ratio that adapts the next refresh's trust bar.
/// `fulltext_ratio` of `None` leaves the stored value untouched.
/// `new_article_at` of `Some` stamps the rolling "last new article" clock that
/// the stale-feed prune reads (None keeps it).
pub fn set_feed_refresh_meta(
    conn: &Connection,
    id: &str,
    etag: Option<&str>,
    fulltext_ratio: Option<f64>,
    new_article_at: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE feed_sources
         SET etag = COALESCE(?2, etag),
             fulltext_ratio = COALESCE(?3, fulltext_ratio),
             last_new_article_at = COALESCE(?4, last_new_article_at)
         WHERE id=?1",
        params![id, etag, fulltext_ratio, new_article_at],
    )?;
    Ok(())
}

/// 滚动 `days` 天没有任何新文章的启用源自动退场：只停用，不删源、不删
/// 文章。用户想删除订阅时自己删；库里的文章由保留策略和用户行为决定。
/// 返回被停用的源名，供刷新结果汇报。
pub fn prune_stale_feeds(conn: &Connection, days: i64) -> Result<Vec<String>, AppError> {
    if days <= 0 {
        return Ok(Vec::new());
    }
    let cutoff = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    let stale: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name FROM feed_sources
              WHERE enabled = 1
                AND kind <> 'book'
                AND last_new_article_at IS NOT NULL
                AND last_new_article_at < ?1",
        )?;
        let rows = stmt
            .query_map(params![cutoff], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut pruned = Vec::new();
    for (id, name) in stale {
        conn.execute(
            "UPDATE feed_sources SET enabled = 0 WHERE id = ?1",
            params![id],
        )?;
        pruned.push(name);
    }
    Ok(pruned)
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