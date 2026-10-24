use crate::db;
use crate::feeds;
use std::env::temp_dir;
use std::path::PathBuf;
use uuid::Uuid;

/// A throwaway database folder under the OS temp dir, deleted when the test
/// ends. Every test used to hand-write `let _ = remove_file(path)` as its last
/// line, which never runs when an assertion above it fails — and never touched
/// the `-wal`/`-shm` siblings or the premigrate snapshots the open path writes
/// beside the file, so each run left a few dozen databases behind.
///
/// Locals drop in reverse order of declaration, so a `conn`/`state` bound after
/// `TmpDb` is closed before the folder is removed.
struct TmpDb {
    dir: PathBuf,
}

impl TmpDb {
    /// `tag` names the folder so a run aborted before `Drop` stays diagnosable.
    fn new(tag: &str) -> Self {
        let dir = temp_dir().join(format!("shiyan-test-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self { dir }
    }

    /// The app-data folder itself, for tests that stage extra files beside the
    /// database (legacy-bundle renames, `VACUUM INTO` targets).
    fn dir(&self) -> PathBuf {
        self.dir.clone()
    }

    /// The database path, using the production filename.
    fn file(&self) -> PathBuf {
        db::db_path(self.dir.clone())
    }

    /// Open the database: create, migrate, seed.
    fn conn(&self) -> rusqlite::Connection {
        db::open_db(self.file(), true).expect("open test db")
    }

    /// Open the read/write connection pair the app uses.
    fn state(&self) -> db::DbState {
        db::DbState::open(self.file()).expect("open test db state")
    }
}

impl Drop for TmpDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn sample_article(id: &str) -> db::Article {
    db::Article {
        id: id.into(),
        url: format!("https://example.com/{id}"),
        title: id.into(),
        source: "Test".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "word ".repeat(100),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    }
}

fn list_articles(
    conn: &rusqlite::Connection,
    category: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<db::ArticleListItem>, crate::error::AppError> {
    db::query_articles(
        conn,
        &db::ArticleQuery {
            category,
            ..Default::default()
        },
        limit,
        offset,
    )
}

#[test]
fn db_seeds_feeds_and_stores_article() {
    let tmp = TmpDb::new("test");
    let conn = tmp.conn();
    let feeds = db::list_feeds(&conn).expect("feeds");
    assert!(
        feeds.len() >= 80,
        "expected curated news/blog feeds, got {}",
        feeds.len()
    );
    assert!(
        feeds.iter().any(|f| f.id == "mit-tr-ai"),
        "expected AI-focused tech feed"
    );
    assert!(
        feeds.iter().any(|f| f.id == "vox" || f.id == "the-atlantic"),
        "expected classic explainer/magazine feed"
    );
    assert!(
        !feeds.iter().any(|f| f.name.contains("Podcast") || f.id == "planet-money"),
        "podcasts should not be curated"
    );
    assert!(
        !feeds.iter().any(|f| f.id == "freecodecamp" || f.id == "rust-blog"),
        "programming blogs should be removed"
    );

    let article = db::Article {
        id: Uuid::new_v4().to_string(),
        url: "https://example.com/a".into(),
        title: "Hello".into(),
        source: "Test".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "word ".repeat(100),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &article).unwrap();
    let list = list_articles(&conn, Some("tech"), None, None).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn seed_feeds_adds_new_curated_sources() {
    let tmp = TmpDb::new("test-seed");
    let conn = tmp.conn();
    let before = db::list_feeds(&conn).unwrap().len();
    // Simulate older DB missing a curated feed; re-open triggers seed INSERT OR IGNORE.
    conn.execute("DELETE FROM feed_sources WHERE id='propublica'", [])
        .unwrap();
    let mid = db::list_feeds(&conn).unwrap().len();
    assert_eq!(mid, before - 1);
    drop(conn);
    let conn = tmp.conn();
    let after = db::list_feeds(&conn).unwrap().len();
    assert_eq!(after, before);
    assert!(db::list_feeds(&conn)
        .unwrap()
        .iter()
        .any(|f| f.id == "propublica"));
}

#[test]
fn seed_feeds_removes_obsolete_sources() {
    let tmp = TmpDb::new("test-obsolete");
    let conn = tmp.conn();
    conn.execute(
        "INSERT INTO feed_sources (id, name, category, url, enabled, origin, description) VALUES ('rust-blog','Rust Blog','tech','https://example.com/rust',1,'curated','')",
        [],
    )
    .unwrap();
    assert!(db::list_feeds(&conn)
        .unwrap()
        .iter()
        .any(|f| f.id == "rust-blog"));
    drop(conn);
    let conn = tmp.conn();
    assert!(
        !db::list_feeds(&conn)
            .unwrap()
            .iter()
            .any(|f| f.id == "rust-blog"),
        "obsolete curated feeds should be deleted on open"
    );
}

#[test]
fn seed_feeds_preserves_user_subscriptions() {
    let tmp = TmpDb::new("test-user-feed");
    let conn = tmp.conn();
    let feed = db::subscribe_feed(
        &conn,
        "My Climate Blog",
        "tech",
        "https://example.com/climate/rss.xml",
        "user pick",
    )
    .unwrap();
    assert_eq!(feed.origin, "user");
    drop(conn);
    let conn = tmp.conn();
    let feeds = db::list_feeds(&conn).unwrap();
    assert!(
        feeds.iter().any(|f| f.id == feed.id && f.origin == "user"),
        "user subscriptions must survive curated seed"
    );
    let cats = db::list_feed_categories(&conn).unwrap();
    assert!(cats.iter().any(|c| c.id == "tech" && c.builtin));
    let custom = db::add_feed_category(&conn, "气候").unwrap();
    assert!(!custom.builtin);
    assert!(!custom.id.is_empty());
}

#[test]
fn insert_article_if_new_is_idempotent() {
    let tmp = TmpDb::new("idempotent");
    let conn = tmp.conn();

    let first = db::Article {
        id: "id-1".into(),
        url: "https://example.com/same".into(),
        title: "Original Title".into(),
        source: "Test".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "original content that should stay".into(),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    assert!(db::insert_article_if_new(&conn, &first).unwrap());

    let second = db::Article {
        id: "id-2".into(),
        url: "https://example.com/same".into(),
        title: "Changed Title".into(),
        source: "Other".into(),
        category: "world".into(),
        published_at: Some("2024-01-01T00:00:00Z".into()),
        content_text: "should not overwrite".into(),
        fetched_at: "2024-06-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    assert!(!db::insert_article_if_new(&conn, &second).unwrap());

    let stored = db::get_article(&conn, "id-1")
        .unwrap()
        .expect("exists");
    assert_eq!(stored.id, "id-1");
    assert_eq!(stored.title, "Original Title");
    assert_eq!(stored.content_text, "original content that should stay");
    assert_eq!(stored.fetched_at, "2020-01-01T00:00:00Z");

}

#[test]
fn article_counts_by_source_groups_by_name() {
    let tmp = TmpDb::new("counts");
    let conn = tmp.conn();

    for i in 0..3 {
        let mut a = sample_article(&format!("c1-{i}"));
        a.source = "Alpha".into();
        assert!(db::insert_article_if_new(&conn, &a).unwrap());
    }
    let mut b = sample_article("c2-0");
    b.source = "Beta".into();
    assert!(db::insert_article_if_new(&conn, &b).unwrap());

    let counts = db::article_counts_by_source(&conn).unwrap();
    assert_eq!(counts.get("Alpha"), Some(&3));
    assert_eq!(counts.get("Beta"), Some(&1));
    assert!(!counts.contains_key("Gamma"));
}

#[test]
fn list_article_urls_supports_incremental_skip() {
    let tmp = TmpDb::new("urls");
    let conn = tmp.conn();
    let a = db::Article {
        id: "a1".into(),
        url: "https://example.com/one".into(),
        title: "One".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "x".repeat(50),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &a).unwrap();
    let urls = db::list_article_urls(&conn).unwrap();
    assert!(urls.contains("https://example.com/one"));
}

#[test]
fn split_paragraphs_works() {
    let parts = feeds::split_paragraphs("A\n\nB\n\n\nC");
    assert_eq!(parts, vec!["A", "B", "C"]);
}

#[test]
fn purge_word_threshold_drops_short_rss_keeps_imports() {
    let tmp = TmpDb::new("word-threshold");
    let conn = tmp.conn();

    let mut short = sample_article("short");
    short.word_count = 100;
    let mut long = sample_article("long");
    long.word_count = 800;
    let mut unknown = sample_article("unknown");
    unknown.word_count = 0;
    let mut imported_short = sample_article("imported");
    imported_short.word_count = 50;
    imported_short.origin = "book".into();

    for a in [&short, &long, &unknown, &imported_short] {
        db::insert_article_if_new(&conn, a).unwrap();
    }

    let removed = feeds::purge_rss_below_word_threshold(&conn).unwrap();
    assert_eq!(removed, 1, "only the stamped 100-word RSS article goes");
    assert!(db::get_article(&conn, "short").unwrap().is_none());
    assert!(db::get_article(&conn, "long").unwrap().is_some());
    assert!(
        db::get_article(&conn, "unknown").unwrap().is_some(),
        "unassessed rows must be handled by assessment, not blind word purge"
    );
    assert!(
        db::get_article(&conn, "imported").unwrap().is_some(),
        "book chapters are never deleted by threshold purges"
    );

}

#[test]
fn refresh_article_content_updates_longer_body() {
    let tmp = TmpDb::new("refresh");
    let conn = tmp.conn();
    let a = db::Article {
        id: "r1".into(),
        url: "https://example.com/refresh".into(),
        title: "Old".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "short body".into(),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &a).unwrap();
    db::set_article_summary_zh(&conn, "r1", "旧简介").unwrap();

    let longer = "word ".repeat(500);
    // The refresh path builds the update struct with an empty id; matching must
    // be by url, so a bogus id still upgrades the stored row.
    let update = db::Article {
        id: String::new(),
        url: "https://example.com/refresh".into(),
        title: "New Title".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: longer.clone(),
        fetched_at: "2024-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    let changed = db::refresh_article_content(&conn, &update).unwrap();
    assert!(changed);
    let stored = db::get_article(&conn, "r1").unwrap().expect("exists");
    assert_eq!(stored.title, "New Title");
    assert_eq!(stored.content_text, longer);
    assert_eq!(stored.summary_zh, "", "stale summary cleared on body refresh");
    assert_eq!(stored.origin, "rss");

    // Idempotent: same body is a no-op.
    let changed_again = db::refresh_article_content(&conn, &update).unwrap();
    assert!(!changed_again);

}

#[test]
fn body_replacement_invalidates_paragraph_translations() {
    let tmp = TmpDb::new("invalidate");
    let conn = tmp.conn();
    let a = db::Article {
        id: "inv1".into(),
        url: "https://example.com/invalidate".into(),
        title: "Old".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "old body ".repeat(60),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &a).unwrap();
    db::save_translation(&conn, "inv1", "paragraph", "0", "old", "旧", "test").unwrap();
    db::save_translation(&conn, "inv1", "selection", "abc", "hi", "嗨", "test").unwrap();

    // Refresh upgrade drops paragraph rows but keeps selections.
    let mut update = a.clone();
    update.content_text = "new body ".repeat(60);
    assert!(db::refresh_article_content(&conn, &update).unwrap());
    assert!(db::get_translation(&conn, "inv1", "paragraph", "0")
        .unwrap()
        .is_none());
    assert!(db::get_translation(&conn, "inv1", "selection", "abc")
        .unwrap()
        .is_some());

    // No-op refresh (same body) leaves fresh rows alone.
    db::save_translation(&conn, "inv1", "paragraph", "0", "new", "新", "test").unwrap();
    assert!(!db::refresh_article_content(&conn, &update).unwrap());
    assert!(db::get_translation(&conn, "inv1", "paragraph", "0")
        .unwrap()
        .is_some());

}

#[test]
fn refresh_article_content_skips_book_chapters() {
    let tmp = TmpDb::new("refresh-url");
    let conn = tmp.conn();
    let original = "imported body ".repeat(40);
    let a = db::Article {
        id: "imp1".into(),
        url: "https://example.com/same-url".into(),
        title: "Imported".into(),
        source: "导入".into(),
        category: "other".into(),
        published_at: None,
        content_text: original.clone(),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "book".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &a).unwrap();

    let update = db::Article {
        id: String::new(),
        url: "https://example.com/same-url".into(),
        title: "RSS overwrite".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "word ".repeat(500),
        fetched_at: "2024-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    let changed = db::refresh_article_content(&conn, &update).unwrap();
    assert!(!changed);
    let stored = db::get_article(&conn, "imp1").unwrap().expect("exists");
    assert_eq!(stored.title, "Imported");
    assert_eq!(stored.content_text, original);
    assert_eq!(stored.origin, "book");

}

#[test]
fn list_articles_returns_excerpt_not_full_body() {
    let tmp = TmpDb::new("list-excerpt");
    let conn = tmp.conn();
    let body = "x".repeat(9000);
    let a = db::Article {
        id: "long1".into(),
        url: "https://example.com/long".into(),
        title: "Long".into(),
        source: "S".into(),
        category: "tech".into(),
        published_at: None,
        content_text: body.clone(),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &a).unwrap();

    let listed = list_articles(&conn, None, Some(1), Some(0)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].excerpt.len(),
        db::LIST_EXCERPT_CHARS as usize,
        "home list should not ship the full body"
    );
    let stored = db::get_article(&conn, "long1").unwrap().expect("exists");
    assert_eq!(stored.content_text.len(), 9000);

}

#[test]
fn list_articles_paginates() {
    let tmp = TmpDb::new("page");
    let conn = tmp.conn();
    for i in 0..5 {
        let a = db::Article {
            id: format!("p{i}"),
            url: format!("https://example.com/{i}"),
            title: format!("T{i}"),
                source: "S".into(),
            category: "tech".into(),
            published_at: None,
            content_text: "x".repeat(50),
            fetched_at: format!("2020-01-0{}T00:00:00Z", i + 1),
            origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
        };
        db::insert_article_if_new(&conn, &a).unwrap();
    }
    let page1 = list_articles(&conn, None, Some(2), Some(0)).unwrap();
    let page2 = list_articles(&conn, None, Some(2), Some(2)).unwrap();
    let page3 = list_articles(&conn, None, Some(2), Some(4)).unwrap();
    assert_eq!(page1.len(), 2);
    assert_eq!(page2.len(), 2);
    assert_eq!(page3.len(), 1);
    let ids: Vec<String> = page1
        .iter()
        .chain(page2.iter())
        .chain(page3.iter())
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(ids.len(), 5);
    assert!(ids.iter().all(|id| id.starts_with('p')));
}

#[test]
fn reading_stats_counts_days_streak_and_time() {
    let tmp = TmpDb::new("stats");
    let conn = tmp.conn();

    let mut a = sample_article("today");
    a.source = "NPR".into();
    a.word_count = 900;
    let mut b = sample_article("yesterday");
    b.source = "NPR".into();
    b.word_count = 500;
    let mut c = sample_article("old");
    c.source = "BBC".into();
    c.word_count = 700;
    for article in [&a, &b, &c] {
        db::insert_article_if_new(&conn, article).unwrap();
    }

    let fmt = |offset: i64| {
        (chrono::Utc::now() - chrono::Duration::days(offset))
            .to_rfc3339()
    };
    let set_opened = |id: &str, at: &str, dwell: i64, completed: i64| {
        conn.execute(
            "UPDATE articles SET last_opened_at=?1, dwell_ms=?2, read_completed=?3, open_count=1 WHERE id=?4",
            rusqlite::params![at, dwell, completed, id],
        )
        .unwrap();
    };
    set_opened("today", &fmt(0), 6 * 60_000 * 2, 1); // 12 min
    set_opened("yesterday", &fmt(1), 3 * 60_000 * 2, 0); // 6 min
    set_opened("old", &fmt(4), 2 * 60_000 * 2, 0); // 4 min, breaks the streak

    let stats = db::reading_stats(&conn).unwrap();
    assert_eq!(stats.articles_total, 3);
    assert_eq!(stats.completed_total, 1);
    assert_eq!(stats.streak_days, 2, "today + yesterday");
    assert_eq!(stats.minutes_total, 22);
    assert!(stats.minutes_7d >= 22);
    assert_eq!(stats.words_total, 900 + 500 + 700);
    assert_eq!(stats.days.len(), 14);
    assert_eq!(stats.days.last().unwrap().articles, 1, "today");
    assert_eq!(stats.days.last().unwrap().minutes, 12);
    assert_eq!(stats.days[13 - 1].articles, 1, "yesterday row");
    assert_eq!(stats.top_sources.first().unwrap().name, "NPR");
    assert_eq!(stats.top_sources.first().unwrap().minutes, 18);

}

#[test]
fn list_article_titles_dedup_window() {
    let tmp = TmpDb::new("dedup-window");
    let conn = tmp.conn();

    let mut recurring_old = sample_article("old-briefing");
    recurring_old.fetched_at = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let mut recurring_new = sample_article("new-briefing");
    recurring_new.fetched_at = chrono::Utc::now().to_rfc3339();

    db::insert_article_if_new(&conn, &recurring_old).unwrap();
    db::insert_article_if_new(&conn, &recurring_new).unwrap();

    let since = (chrono::Utc::now() - chrono::Duration::days(14)).to_rfc3339();
    let recent = db::list_article_titles(&conn, Some(&since)).unwrap();
    assert_eq!(
        recent.len(),
        1,
        "windowed dedup seed must exclude stale rows"
    );
    assert_eq!(recent[0].0, "new-briefing");
    let all = db::list_article_titles(&conn, None).unwrap();
    assert_eq!(all.len(), 2);

}

#[test]
fn query_articles_filters_read_state_source_and_liked() {
    let tmp = TmpDb::new("query");
    let conn = tmp.conn();

    let mut unread = sample_article("unread");
    unread.source = "NPR".into();
    let mut read = sample_article("read");
    read.source = "NPR".into();
    let mut liked = sample_article("liked");
    liked.source = "404 Media".into();
    let mut other = sample_article("other");
    other.source = "404 Media".into();

    for a in [&unread, &read, &liked, &other] {
        db::insert_article_if_new(&conn, a).unwrap();
    }
    // "Read" now means finished, not merely opened.
    db::add_article_reading_progress(&conn, "read", 0, true).unwrap();
    db::set_article_liked(&conn, "liked", true).unwrap();

    let q = |query: db::ArticleQuery<'_>, limit: i64| {
        db::query_articles(&conn, &query, Some(limit), Some(0))
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect::<Vec<_>>()
    };

    let unread_only = q(
        db::ArticleQuery {
            read_state: db::ReadState::Unread,
            ..Default::default()
        },
        10,
    );
    assert_eq!(unread_only.len(), 3);
    assert!(!unread_only.contains(&"read".to_string()));

    let read_only = q(
        db::ArticleQuery {
            read_state: db::ReadState::Read,
            ..Default::default()
        },
        10,
    );
    assert_eq!(read_only, vec!["read".to_string()]);

    let by_source = q(
        db::ArticleQuery {
            source: Some("404 Media"),
            ..Default::default()
        },
        10,
    );
    assert_eq!(by_source.len(), 2);

    let liked_only = q(
        db::ArticleQuery {
            liked_only: true,
            ..Default::default()
        },
        10,
    );
    assert_eq!(liked_only, vec!["liked".to_string()]);

    let combined = q(
        db::ArticleQuery {
            source: Some("404 Media"),
            read_state: db::ReadState::Unread,
            ..Default::default()
        },
        10,
    );
    assert_eq!(combined.len(), 2);

}

#[test]
fn query_articles_search_matches_title_blurb_source_and_escapes_like() {
    let tmp = TmpDb::new("search");
    let conn = tmp.conn();

    let mut fed = sample_article("fed");
    fed.title = "The Fed holds rates".into();
    fed.source = "Reuters".into();
    let mut chip = sample_article("chip");
    chip.title = "Chip stocks rally".into();
    chip.summary_zh = "半导体行情".into();
    chip.source = "404 Media".into();
    let mut pct = sample_article("pct");
    pct.title = "100% organic growth".into();
    pct.source = "Blog".into();

    for a in [&fed, &chip, &pct] {
        db::insert_article_if_new(&conn, a).unwrap();
    }

    let q = |search: &str| {
        db::query_articles(
            &conn,
            &db::ArticleQuery {
                search: Some(search),
                ..Default::default()
            },
            Some(10),
            Some(0),
        )
        .unwrap()
        .into_iter()
        .map(|a| a.id)
        .collect::<Vec<_>>()
    };

    assert_eq!(q("fed"), vec!["fed".to_string()]);
    assert_eq!(q("半导体"), vec!["chip".to_string()]);
    assert_eq!(q("404"), vec!["chip".to_string()]);
    // LIKE metacharacters are literal: "100%" matches only the pct row.
    assert_eq!(q("100%"), vec!["pct".to_string()]);
    // Empty / whitespace-only means no filter.
    assert_eq!(q("   ").len(), 3);

}

#[test]
fn retention_purge_drops_old_rss_but_keeps_liked_and_imports() {
    let tmp = TmpDb::new("retention");
    let conn = tmp.conn();

    let recent = chrono::Utc::now().to_rfc3339();
    let old = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();

    let mut old_rss = sample_article("old-rss");
    old_rss.published_at = Some(old.clone());
    let mut old_liked = sample_article("old-liked");
    old_liked.published_at = Some(old.clone());
    old_liked.liked = true;
    let mut old_import = sample_article("old-import");
    old_import.published_at = Some(old.clone());
    old_import.origin = "book".into();
    let mut new_rss = sample_article("new-rss");
    new_rss.published_at = Some(recent);

    for a in [&old_rss, &old_liked, &old_import, &new_rss] {
        db::insert_article_if_new(&conn, a).unwrap();
    }
    // `liked` is a signal column written via its own setter, not on insert.
    db::set_article_liked(&conn, "old-liked", true).unwrap();

    let removed = feeds::purge_expired_articles(&conn, 14).unwrap();
    assert_eq!(removed, 1, "only the old unliked RSS article goes");
    assert!(db::get_article(&conn, "old-rss").unwrap().is_none());
    assert!(db::get_article(&conn, "old-liked").unwrap().is_some());
    assert!(db::get_article(&conn, "old-import").unwrap().is_some());
    assert!(db::get_article(&conn, "new-rss").unwrap().is_some());

    // 0 = keep forever.
    assert_eq!(feeds::purge_expired_articles(&conn, 0).unwrap(), 0);

}

fn sample_phrase(id: &str, text: &str) -> db::MemoryItem {
    db::MemoryItem {
        id: id.into(),
        kind: "phrase".into(),
        term: text.into(),
        definition_zh: "测试释义".into(),
        word_type: "collocation".into(),
        collocations: vec![],
        context_sentence: "It is on the house.".into(),
        article_id: None,
        status: "learning".into(),
        interval_days: 0.0,
        reps: 0,
        consecutive_know: 0,
        next_review_at: "2020-01-01T00:00:00Z".into(),
        created_at: "2020-01-01T00:00:00Z".into(),
    }
}

#[test]
fn phrase_library_dedup_and_listing() {
    let tmp = TmpDb::new("phrases");
    let conn = tmp.conn();

    db::insert_memory(&conn, &sample_phrase("p1", "on the house")).unwrap();
    assert!(db::get_memory_by_term(&conn, "phrase", "On The House")
        .unwrap()
        .is_some());

    // Case-insensitive unique index blocks a differently-cased duplicate.
    let dup = db::insert_memory(&conn, &sample_phrase("p2", "ON THE HOUSE"));
    assert!(dup.is_err(), "duplicate phrase must be rejected");

    // Status transitions + delete.
    db::set_memory_status(&conn, "p1", "mastered").unwrap();
    assert_eq!(
        db::list_memory(&conn, Some("phrase"), Some("mastered"))
            .unwrap()
            .len(),
        1
    );
    db::delete_memory(&conn, "p1").unwrap();
    assert!(db::list_memory(&conn, Some("phrase"), None).unwrap().is_empty());

}

#[test]
fn set_memory_status_rejects_unknown_status_and_missing_id() {
    let tmp = TmpDb::new("memory-status-guard");
    let conn = tmp.conn();
    db::insert_memory(&conn, &sample_phrase("s1", "guard term")).unwrap();

    let bad = db::set_memory_status(&conn, "s1", "banana");
    assert!(bad.is_err(), "unknown status must be rejected");
    // The rejected write leaves the row untouched (no silent orphan).
    assert_eq!(
        db::get_memory(&conn, "s1").unwrap().unwrap().status,
        "learning"
    );

    let missing = db::set_memory_status(&conn, "nope", "mastered");
    assert!(missing.is_err(), "missing id must be rejected");

    // Legit transitions still work both ways.
    db::set_memory_status(&conn, "s1", "mastered").unwrap();
    assert_eq!(
        db::get_memory(&conn, "s1").unwrap().unwrap().status,
        "mastered"
    );
    db::set_memory_status(&conn, "s1", "learning").unwrap();
}

#[test]
fn word_and_phrase_libraries_are_independent() {
    let tmp = TmpDb::new("memory-kind");
    let conn = tmp.conn();

    // Same text in both libraries is allowed (uniqueness is per kind).
    db::insert_memory(&conn, &sample_vocab("w1", "make", "2020-01-01T00:00:00Z")).unwrap();
    db::insert_memory(&conn, &sample_phrase("p1", "make")).unwrap();

    assert_eq!(db::list_memory(&conn, Some("word"), None).unwrap().len(), 1);
    assert_eq!(db::list_memory(&conn, Some("phrase"), None).unwrap().len(), 1);
    assert_eq!(db::list_memory(&conn, None, None).unwrap().len(), 2);

}

#[test]
fn vocab_dedup_by_term_and_delete_article_detaches() {
    let tmp = TmpDb::new("vocab");
    let conn = tmp.conn();

    let item = db::MemoryItem {
        term: "Ubiquitous".into(),
        definition_zh: "无处不在的".into(),
        word_type: "adjective".into(),
        collocations: vec!["ubiquitous in".into()],
        context_sentence: "It is ubiquitous.".into(),
        article_id: Some("a1".into()),
        ..sample_vocab("v1", "Ubiquitous", "2020-01-01T00:00:00Z")
    };
    db::insert_article_if_new(&conn, &sample_article("a1")).unwrap();
    db::insert_article_if_new(&conn, &sample_article("a2")).unwrap();
    db::insert_memory(&conn, &item).unwrap();

    // Case-insensitive lookup re-adding the same term returns the same row.
    let found = db::get_memory_by_term(&conn, "word", "ubiquitous")
        .unwrap()
        .expect("exists");
    assert_eq!(found.id, "v1");

    // Merge meta into existing entry.
    let mut merged = found;
    merged.definition_zh = String::new(); // existing keeps its def
    merged.collocations = vec!["ubiquitous in".into(), "ubiquitous across".into()];
    merged.article_id = Some("a2".into());
    db::update_memory_meta(&conn, &merged).unwrap();
    let after = db::get_memory(&conn, "v1").unwrap().expect("exists");
    assert_eq!(after.collocations.len(), 2);
    assert_eq!(after.article_id.as_deref(), Some("a2"));

    // Deleting an article detaches memory rows instead of deleting them.
    db::delete_article(&conn, "a2").unwrap();
    let detached = db::get_memory(&conn, "v1").unwrap().expect("still exists");
    assert_eq!(detached.article_id, None);

}

fn sample_vocab(id: &str, term: &str, created_at: &str) -> db::MemoryItem {
    db::MemoryItem {
        id: id.into(),
        kind: "word".into(),
        term: term.into(),
        definition_zh: String::new(),
        word_type: "noun".into(),
        collocations: vec![],
        context_sentence: String::new(),
        article_id: None,
        status: "learning".into(),
        interval_days: 0.0,
        reps: 0,
        consecutive_know: 0,
        next_review_at: created_at.into(),
        created_at: created_at.into(),
    }
}

/// Pre-collapse databases (user_version up to 19) must normalize on launch:
/// the deprecated title_zh column is dropped and the counter lands on
/// LATEST_VERSION — otherwise every future `stored < N` gate would be skipped
/// forever behind the stale high counter.
#[test]
fn pre_collapse_database_is_normalized() {
    let tmp = TmpDb::new("collapse-normalize");
    let conn = rusqlite::Connection::open(tmp.file()).unwrap();
    // Minimal v19-shape database: the counter is what drives the gate, and
    // title_zh is the one column v2 removes.
    conn.execute_batch(
        "CREATE TABLE articles (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            title_zh TEXT NOT NULL DEFAULT ''
         );
         PRAGMA user_version = 19;",
    )
    .unwrap();
    db::migrate(&conn).unwrap();

    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        db::LATEST_VERSION,
        "pre-collapse counter must normalize to the current version"
    );
    let has: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='title_zh'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(has, 0, "deprecated title_zh should be dropped by v2");
}

#[test]
fn prune_stale_feeds_disables_and_keeps_articles() {
    let tmp = TmpDb::new("prune-stale");
    let conn = tmp.conn();
    let user_feed = db::subscribe_feed(
        &conn,
        "User Stale",
        "tech",
        "https://user-stale.example/rss",
        "",
    )
    .unwrap();
    let curated_feed = db::list_feeds(&conn)
        .unwrap()
        .into_iter()
        .find(|f| f.origin == "curated" && f.id != user_feed.id)
        .expect("seeded curated feed");
    let fresh_feed = db::subscribe_feed(
        &conn,
        "Fresh Source",
        "tech",
        "https://fresh.example/rss",
        "",
    )
    .unwrap();

    // Both targets: enabled but no new article for 40 days.
    let old = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
    conn.execute(
        "UPDATE feed_sources SET last_new_article_at=?1 WHERE id IN (?2, ?3)",
        rusqlite::params![old, user_feed.id, curated_feed.id],
    )
    .unwrap();

    let mut unread = sample_article("stale-unread");
    unread.source = user_feed.name.clone();
    let mut liked = sample_article("stale-liked");
    liked.source = user_feed.name.clone();
    for a in [&unread, &liked] {
        db::insert_article_if_new(&conn, a).unwrap();
    }

    let pruned = db::prune_stale_feeds(&conn, 30).unwrap();
    assert_eq!(pruned.len(), 2, "only the two aged feeds retire");

    let feeds = db::list_feeds(&conn).unwrap();
    // Both stale feeds are disabled but still present.
    for id in [&user_feed.id, &curated_feed.id] {
        assert_eq!(
            feeds.iter().find(|f| &f.id == id).map(|f| f.enabled),
            Some(false),
            "stale feed stays in the library, disabled"
        );
    }
    assert_eq!(
        feeds
            .iter()
            .find(|f| f.id == fresh_feed.id)
            .map(|f| f.enabled),
        Some(true)
    );

    // Retirement never touches stored articles.
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM articles WHERE source=?1",
            rusqlite::params![user_feed.name],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 2, "stale-feed retirement must not delete articles");
}

#[test]
fn re_enabling_a_feed_restarts_the_stale_clock() {
    let tmp = TmpDb::new("prune-reenable");
    let conn = tmp.conn();
    let feed = db::subscribe_feed(
        &conn,
        "Sleeper",
        "tech",
        "https://sleeper.example/rss",
        "",
    )
    .unwrap();
    conn.execute(
        "UPDATE feed_sources SET last_new_article_at='2020-01-01T00:00:00Z' WHERE id=?1",
        rusqlite::params![feed.id],
    )
    .unwrap();

    // Disabling keeps the old clock (it is not an observation restart).
    db::set_feed_enabled(&conn, &feed.id, false).unwrap();
    let clock: Option<String> = conn
        .query_row(
            "SELECT last_new_article_at FROM feed_sources WHERE id=?1",
            rusqlite::params![feed.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(clock.as_deref(), Some("2020-01-01T00:00:00Z"));

    // Re-enabling restarts it; the next prune must not retire the feed.
    db::set_feed_enabled(&conn, &feed.id, true).unwrap();
    let pruned = db::prune_stale_feeds(&conn, 30).unwrap();
    assert!(!pruned.contains(&feed.name));
}

#[test]
fn refresh_meta_moves_stale_clock_only_on_new_articles() {
    let tmp = TmpDb::new("prune-clock");
    let conn = tmp.conn();
    let feed = db::subscribe_feed(
        &conn,
        "Clock Test",
        "tech",
        "https://clock.example/rss",
        "",
    )
    .unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    // Fetch without new articles: clock keeps ticking down (stays old).
    db::set_feed_refresh_meta(&conn, &feed.id, None, None, None).unwrap();
    let after_fetch: Option<String> = conn
        .query_row(
            "SELECT last_new_article_at FROM feed_sources WHERE id=?1",
            rusqlite::params![feed.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        after_fetch.unwrap() < now,
        "fetch without new articles must not refresh the stale clock"
    );
    // A new article moves it.
    db::set_feed_refresh_meta(&conn, &feed.id, None, None, Some(&now)).unwrap();
    let after_new: String = conn
        .query_row(
            "SELECT last_new_article_at FROM feed_sources WHERE id=?1",
            rusqlite::params![feed.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after_new, now);
}

#[test]
fn vocab_term_unique_index_rejects_case_insensitive_dup() {
    let tmp = TmpDb::new("vocab-uniq");
    let conn = tmp.conn();
    db::insert_memory(&conn, &sample_vocab("v1", "Focus", "2020-01-01T00:00:00Z")).unwrap();
    let err = db::insert_memory(&conn, &sample_vocab("v2", "focus", "2020-01-02T00:00:00Z"))
        .expect_err("duplicate term");
    assert!(
        err.to_string().to_lowercase().contains("unique"),
        "expected unique violation, got {err}"
    );
}

#[test]
fn add_or_merge_memory_reuses_existing_term() {
    let tmp = TmpDb::new("vocab-merge");
    let state = tmp.state();
    let first = crate::vocab::add_or_merge_memory(
        &state,
        crate::vocab::AddMemoryInput {
            kind: "word".into(),
            term: "serendipity".into(),
            context_sentence: "A happy serendipity.".into(),
            article_id: None,
            definition_zh: Some("意外发现".into()),
            word_type: Some("noun".into()),
            collocations: Some(vec![]),
        },
    )
    .unwrap();
    let second = crate::vocab::add_or_merge_memory(
        &state,
        crate::vocab::AddMemoryInput {
            kind: "word".into(),
            term: "Serendipity".into(),
            context_sentence: String::new(),
            article_id: None,
            definition_zh: Some("意外发现".into()),
            word_type: Some("noun".into()),
            collocations: Some(vec!["pure serendipity".into()]),
        },
    )
    .unwrap();
    assert_eq!(first.id, second.id);
    assert!(second.collocations.contains(&"pure serendipity".to_string()));
}

#[test]
fn add_or_merge_memory_unifies_word_and_phrase_kinds() {
    let tmp = TmpDb::new("memory-kinds");
    let state = tmp.state();
    let phrase = crate::vocab::add_or_merge_memory(
        &state,
        crate::vocab::AddMemoryInput {
            kind: "phrase".into(),
            term: "  on   the house ".into(),
            context_sentence: "It is on the house.".into(),
            article_id: None,
            definition_zh: Some("本店请客".into()),
            word_type: Some("collocation".into()),
            collocations: None,
        },
    )
    .unwrap();
    assert_eq!(phrase.kind, "phrase");
    assert_eq!(phrase.term, "on the house", "whitespace normalized");
    assert_eq!(phrase.definition_zh, "本店请客");
}

#[test]
fn read_state_reading_and_unfinished_filters() {
    let tmp = TmpDb::new("reading-state");
    let conn = tmp.conn();

    let mut untouched = sample_article("untouched");
    untouched.source = "NPR".into();
    let mut in_progress = sample_article("in-progress");
    in_progress.source = "NPR".into();
    for a in [&untouched, &in_progress] {
        db::insert_article_if_new(&conn, a).unwrap();
    }
    // Merely opening (without completion) puts it in `Reading`.
    db::mark_article_opened(&conn, "in-progress").unwrap();

    let ids = |state: db::ReadState| {
        db::query_articles(
            &conn,
            &db::ArticleQuery {
                read_state: state,
                ..Default::default()
            },
            Some(10),
            Some(0),
        )
        .unwrap()
        .into_iter()
        .map(|a| a.id)
        .collect::<Vec<_>>()
    };

    assert_eq!(
        ids(db::ReadState::Reading),
        vec!["in-progress".to_string()],
        "opened-but-unfinished is Reading"
    );
    let unfinished = ids(db::ReadState::Unfinished);
    assert_eq!(unfinished.len(), 2, "Unfinished covers both unread and reading");
    assert_eq!(
        ids(db::ReadState::Unread),
        vec!["untouched".to_string()],
        "Unread excludes opened articles"
    );

    // Completing moves it out of Reading into Read.
    db::add_article_reading_progress(&conn, "in-progress", 0, true).unwrap();
    assert!(ids(db::ReadState::Reading).is_empty());
    assert_eq!(
        ids(db::ReadState::Read),
        vec!["in-progress".to_string()]
    );

}

#[test]
fn article_foreign_keys_cascade_and_reject_orphans() {
    let tmp = TmpDb::new("fk");
    let conn = tmp.conn();

    db::insert_article_if_new(&conn, &sample_article("a1")).unwrap();
    db::save_translation(&conn, "a1", "paragraph", "0", "Hello", "你好", "test").unwrap();
    db::insert_memory(
        &conn,
        &db::MemoryItem {
            article_id: Some("a1".into()),
            ..sample_vocab("v1", "hello", "2020-01-01T00:00:00Z")
        },
    )
    .unwrap();

    let trans_fks: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_foreign_key_list('translations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let vocab_fks: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_foreign_key_list('memory_items')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(trans_fks >= 1, "translations should reference articles");
    assert!(vocab_fks >= 1, "memory_items should reference articles");

    let err = db::save_translation(&conn, "missing", "paragraph", "0", "x", "y", "test")
        .expect_err("orphan translation");
    assert!(
        err.to_string().to_lowercase().contains("foreign key"),
        "expected FK failure, got {err}"
    );

    conn.execute("DELETE FROM articles WHERE id='a1'", [])
        .unwrap();
    let remaining: i64 = conn
        .query_row(
            "SELECT count(*) FROM translations WHERE article_id='a1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0, "translations should cascade-delete");
    let detached = db::get_memory(&conn, "v1").unwrap().expect("vocab kept");
    assert_eq!(detached.article_id, None);

}

#[test]
fn reflow_cleanup_clears_paragraph_translations_once() {
    let tmp = TmpDb::new("reflow");
    let conn = tmp.conn();
    db::insert_article_if_new(&conn, &sample_article("a1")).unwrap();
    db::save_translation(&conn, "a1", "paragraph", "0", "Hello", "你好", "test").unwrap();
    db::save_translation(&conn, "a1", "selection", "abc", "hi", "嗨", "test").unwrap();

    let removed = feeds::clear_stale_paragraph_translations_once(&conn).unwrap();
    assert_eq!(removed, 1, "only paragraph rows are cleared");
    assert!(db::get_translation(&conn, "a1", "paragraph", "0")
        .unwrap()
        .is_none());
    assert!(db::get_translation(&conn, "a1", "selection", "abc")
        .unwrap()
        .is_some());

    // The one-time marker is versioned off REFLOW_VERSION: bumping the
    // version is what re-arms the cleanup after a reflow rule change.
    let marker = db::get_meta(
        &conn,
        &format!("reflow_translations_cleared_v{}", crate::reflow::REFLOW_VERSION),
    )
    .unwrap();
    assert_eq!(marker.as_deref(), Some("done"));

    // Guarded by app_meta: a second run is a no-op even with new rows.
    db::save_translation(&conn, "a1", "paragraph", "0", "Hello", "你好", "test").unwrap();
    assert_eq!(feeds::clear_stale_paragraph_translations_once(&conn).unwrap(), 0);
    assert!(db::get_translation(&conn, "a1", "paragraph", "0")
        .unwrap()
        .is_some());

}

#[test]
fn stats_day_is_offset_aware_not_a_substring() {
    // Same instant written with different offsets lands on the same local
    // day, whatever the machine timezone is.
    assert_eq!(
        db::local_day("2024-06-15T12:00:00Z"),
        db::local_day("2024-06-15T14:00:00+02:00")
    );
    let day = db::local_day("2024-06-15T12:00:00Z");
    assert_eq!(day.len(), 10, "YYYY-MM-DD shape");
    // Unparseable values keep the old UTC-prefix behavior instead of panicking.
    assert_eq!(db::local_day("2024-06-15 garbage"), "2024-06-15");
    assert_eq!(db::local_day("garbage"), "");
}

#[test]
fn vacuum_into_file_produces_valid_backup() {
    let tmp = TmpDb::new("vacuum");
    let conn = tmp.conn();
    db::insert_article_if_new(&conn, &sample_article("a1")).unwrap();
    // VACUUM INTO refuses an existing file; a fresh guarded folder guarantees
    // the destination is absent, which the old pre-clear remove_file only hoped for.
    let dest = tmp.dir().join("out.db");

    db::vacuum_into_file(&conn, &dest).unwrap();
    assert!(dest.exists(), "VACUUM INTO must write the snapshot");
    // The snapshot passes the same validation as a user-supplied backup file.
    let version = db::validate_backup_file(&dest).unwrap();
    assert!(version >= 1);
    let check = rusqlite::Connection::open(&dest).unwrap();
    let count: i64 = check
        .query_row("SELECT COUNT(*) FROM articles WHERE id='a1'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn article_view_loads_paragraphs_and_translations() {
    let tmp = TmpDb::new("view");
    let state = tmp.state();
    {
        let conn = state.lock_write().unwrap();
        db::insert_article_if_new(&conn, &sample_article("a1")).unwrap();
        conn.execute(
            "UPDATE articles SET content_text = ?1 WHERE id = 'a1'",
            ["First para.\n\nSecond para."],
        )
        .unwrap();
        db::save_translation(&conn, "a1", "paragraph", "0", "First para.", "第一段", "test")
            .unwrap();
    }

    let view = crate::article_view::load_article_view(&state, "a1")
        .unwrap()
        .expect("present");
    assert_eq!(view.article.id, "a1");
    assert_eq!(view.paragraphs, vec!["First para.", "Second para."]);
    assert_eq!(view.translations.len(), 1);
    assert_eq!(view.translations[0].translated_text, "第一段");
    assert!(crate::article_view::load_article_view(&state, "missing")
        .unwrap()
        .is_none());
}

#[test]
fn schema_adds_summary_zh_column() {
    let tmp = TmpDb::new("summary-col");
    let conn = tmp.conn();
    let has: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='summary_zh'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(has, 1, "articles.summary_zh should exist after migrate");
    let opened_col: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='last_opened_at'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(opened_col, 1, "articles.last_opened_at should exist after migrate");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, db::LATEST_VERSION);
    let memory_table: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memory_items'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(memory_table, 1, "memory_items should exist after migrate");
    let meta_table: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='app_meta'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(meta_table, 1, "app_meta should exist after migrate");
    for (table, column) in [
        ("articles", "tags_json"),
        ("book_catalog", "subjects"),
        ("feed_sources", "fail_count"),
        ("feed_sources", "last_fetched_at"),
    ] {
        let n: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name=?1"
                ),
                [column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "{table}.{column} must be gone after migrate");
    }
    for table in ["known_words", "lookup_history", "removed_feeds"] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "{table} must be gone after migrate");
    }
    let quality_col: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='quality'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(quality_col, 0, "articles.quality must be gone after migrate");
    let ratio_col: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('feed_sources') WHERE name='fulltext_ratio'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ratio_col, 1, "feed_sources.fulltext_ratio should exist after migrate");
}

#[test]
fn stale_backup_prune_keeps_two_and_drops_legacy() {
    let tmp = TmpDb::new("backup-prune");
    let db_path = tmp.file();
    let dir = tmp.dir();
    let db_name = db_path.file_name().unwrap().to_string_lossy().to_string();
    let names = [
        format!("{db_name}.premigrate-v1.bak"),
        format!("{db_name}.premigrate-v2.bak"),
        format!("{db_name}.premigrate-v3.bak"),
        format!("{db_name}.premigrate-v4.bak"),
        format!("{db_name}.prereadreset-20261002.bak"),
        format!("{db_name}-wal.prereadreset-20261002.bak"),
    ];
    let base = std::time::SystemTime::now();
    for (i, name) in names.iter().enumerate() {
        let file = std::fs::File::create(dir.join(name)).unwrap();
        file.set_times(
            std::fs::FileTimes::new()
                .set_modified(base + std::time::Duration::from_secs(i as u64)),
        )
        .unwrap();
    }

    db::prune_stale_backups(&db_path);

    let mut remaining: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    remaining.sort();
    assert_eq!(
        remaining,
        vec![
            format!("{db_name}.premigrate-v3.bak"),
            format!("{db_name}.premigrate-v4.bak"),
        ],
        "keep the two newest snapshots, drop legacy prereadreset files"
    );
}

#[test]
fn summary_zh_roundtrips_and_missing_query() {
    let tmp = TmpDb::new("summary-zh");
    let conn = tmp.conn();
    let mut a = sample_article("s1");
    a.summary_zh = String::new();
    db::insert_article_if_new(&conn, &a).unwrap();

    let missing = db::articles_missing_card_zh(&conn, 40).unwrap();
    assert_eq!(missing.len(), 1, "empty summary still needs a card fill");
    assert_eq!(missing[0].id, "s1");

    db::set_article_summary_zh(&conn, "s1", "这是一条不超过五十字的中文简介").unwrap();
    let stored = db::get_article(&conn, "s1").unwrap().expect("exists");
    assert_eq!(stored.summary_zh, "这是一条不超过五十字的中文简介");
    assert!(db::articles_missing_card_zh(&conn, 40).unwrap().is_empty());

}

#[test]
fn mark_article_opened_is_implicit_and_repeatable() {
    let tmp = TmpDb::new("opened");
    let conn = tmp.conn();
    db::insert_article_if_new(&conn, &sample_article("r1")).unwrap();

    let before = db::get_article(&conn, "r1").unwrap().expect("exists");
    assert!(before.last_opened_at.is_none());
    assert_eq!(before.open_count, 0);

    db::mark_article_opened(&conn, "r1").unwrap();
    let once = db::get_article(&conn, "r1").unwrap().expect("exists");
    assert!(once.last_opened_at.as_deref().unwrap().starts_with("20"));
    assert_eq!(once.open_count, 1);

    db::mark_article_opened(&conn, "r1").unwrap();
    let twice = db::get_article(&conn, "r1").unwrap().expect("exists");
    assert_eq!(twice.open_count, 2);
    assert!(twice.last_opened_at >= once.last_opened_at);

    assert!(db::mark_article_opened(&conn, "missing").is_err());
}

#[test]
fn learning_stats_uses_opens_and_new_vocab() {
    let tmp = TmpDb::new("learn-stats");
    let conn = tmp.conn();

    let mut bbc = sample_article("bbc1");
    bbc.source = "BBC".into();
    bbc.category = "world".into();
    let mut npr = sample_article("npr1");
    npr.source = "NPR".into();
    npr.category = "world".into();
    db::insert_article_if_new(&conn, &bbc).unwrap();
    db::insert_article_if_new(&conn, &npr).unwrap();
    db::mark_article_opened(&conn, "bbc1").unwrap();
    db::mark_article_opened(&conn, "bbc1").unwrap();
    db::mark_article_opened(&conn, "npr1").unwrap();

    db::insert_memory(
        &conn,
        &sample_vocab("v-new", "fresh", &chrono::Utc::now().to_rfc3339()),
    )
    .unwrap();
    db::insert_memory(
        &conn,
        &sample_vocab("v-old", "stale", "2020-01-01T00:00:00Z"),
    )
    .unwrap();

    let stats = db::learning_stats(&conn).unwrap();
    assert_eq!(stats.opened_total, 2);
    assert_eq!(stats.opened_7d, 2);
    assert_eq!(stats.top_source.as_deref(), Some("BBC"));
    assert_eq!(stats.top_category.as_deref(), Some("world"));
    assert_eq!(stats.vocab_created_7d, 1);
    assert_eq!(stats.vocab_learning, 2);

    let other = TmpDb::new("learn-empty");
    let empty = other.conn();
    let zero = db::learning_stats(&empty).unwrap();
    assert_eq!(zero.opened_total, 0);
    assert_eq!(zero.opened_7d, 0);
    assert!(zero.top_source.is_none());
    assert_eq!(zero.vocab_created_7d, 0);
}

#[test]
fn export_memory_csv_dumps_all_libraries_with_escaping() {
    let tmp = TmpDb::new("vocab-export");
    let conn = tmp.conn();

    // A word whose definition contains a comma+quote exercises RFC 4180 escaping.
    let mut word = sample_vocab("w1", "ubiquitous", "2020-01-01T00:00:00Z");
    word.definition_zh = "无处不在的，\"普遍\"的".into();
    word.collocations = vec!["ubiquitous in".into(), "ubiquitous across".into()];
    word.status = "mastered".into();
    db::insert_memory(&conn, &word).unwrap();
    db::insert_memory(&conn, &sample_phrase("p1", "on the house")).unwrap();

    let csv = db::export_memory_csv(&conn).unwrap();
    assert!(csv.starts_with('\u{FEFF}'), "BOM prepended for Excel/Numbers");
    let lines: Vec<&str> = csv.trim_end_matches('\n').lines().collect();
    assert_eq!(lines.len(), 3, "header + 2 items");
    assert!(
        lines[0]
            .trim_start_matches('\u{FEFF}')
            .starts_with("term,kind,status,definition_zh"),
        "first line is the header (after BOM)"
    );
    // The word row keeps its commas/quotes wrapped; collocations joined by "; ".
    assert!(lines[1].contains("\"无处不在的，\"\"普遍\"\"的\""));
    assert!(lines[1].contains("ubiquitous in; ubiquitous across"));
    assert!(lines[1].contains(",mastered,"));
    // The phrase row is present with its kind and term.
    assert!(lines[2].starts_with("on the house,phrase,learning"));

    // Empty library still yields a valid (header-only) file.
    db::delete_memory(&conn, "w1").unwrap();
    db::delete_memory(&conn, "p1").unwrap();
    let csv = db::export_memory_csv(&conn).unwrap();
    assert_eq!(
        csv.trim_start_matches('\u{FEFF}'),
        "term,kind,status,definition_zh,word_type,collocations,context_sentence,created_at,next_review_at,reps,interval_days\n"
    );

}

#[test]
fn catalog_search_ranks_prefix_first_and_filters_by_shelf() {
    let tmp = TmpDb::new("catalog-search");
    let conn = tmp.conn();
    let book = |gid: i64, title: &str, authors: &str, shelf: &str| db::CatalogBook {
        gid,
        title: title.into(),
        authors: authors.into(),
        issued: "1871-01-01".into(),
        shelf: shelf.into(),
        imported: false,
    };
    for b in [
        book(1, "Moby-Dick", "Melville, Herman, 1819-1891", "Fiction"),
        book(2, "The Moby-Dick Companion", "Smith, John", "Reference"),
        book(3, "Typee", "Melville, Herman, 1819-1891", "Reference"),
        book(4, "Moby-Dick Study Guide", "Lee, Amy", "Reference"),
    ] {
        db::insert_catalog_row(&conn, &b).unwrap();
    }

    // Prefix matches rank before mere substring matches; ties by shorter title.
    let hits = db::search_catalog(&conn, "moby-dick", "", 10).unwrap();
    let titles: Vec<&str> = hits.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Moby-Dick", "Moby-Dick Study Guide", "The Moby-Dick Companion"]
    );

    // A non-empty shelf restricts to that shelf…
    let fiction = db::search_catalog(&conn, "melville", "Fiction", 10).unwrap();
    assert_eq!(fiction.len(), 1);
    assert_eq!(fiction[0].title, "Moby-Dick");

    // …while an empty shelf keeps every shelf (matches on author names too).
    let all = db::search_catalog(&conn, "melville", "", 10).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn reorder_feeds_assigns_priority_within_category() {
    let tmp = TmpDb::new("reorder");
    let conn = tmp.conn();
    // Start from a controlled feed set: two tech, two finance.
    conn.execute("DELETE FROM feed_sources", []).unwrap();
    for (id, cat) in [
        ("t1", "tech"),
        ("t2", "tech"),
        ("f1", "finance"),
        ("f2", "finance"),
    ] {
        conn.execute(
            "INSERT INTO feed_sources (id, name, category, url, enabled, origin) VALUES (?1,?1,?2,?3,1,'user')",
            rusqlite::params![id, cat, format!("https://x/{id}")],
        )
        .unwrap();
    }

    // Flat top-to-bottom order across the tree; priority is per-category, so
    // the first source of each category shares the same numeric top.
    db::reorder_feeds(&conn, &["t1".into(), "t2".into(), "f1".into(), "f2".into()])
        .unwrap();

    let prio = |id: &str| -> i64 {
        conn.query_row(
            "SELECT priority FROM feed_sources WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(prio("t1"), 2, "first in tech block gets the category size");
    assert_eq!(prio("t2"), 1);
    assert_eq!(prio("f1"), 2, "first in finance normalises to the same top");
    assert_eq!(prio("f2"), 1);

    let cat_max = db::category_priority_max(&conn).unwrap();
    assert_eq!(cat_max.get("tech"), Some(&2));
    assert_eq!(cat_max.get("finance"), Some(&2));

    let name_prio = db::source_priority_map(&conn).unwrap();
    assert_eq!(name_prio.get("t1"), Some(&2));
    assert_eq!(name_prio.get("f1"), Some(&2));

}

/// v19 contract: a book is a feed with kind='book'; its chapters are ordinary
/// articles carrying `chapter_no`. Everything the rest of the app relies on is
/// asserted here — chapter ordering, chapter navigation, the card backfill
/// exclusion, stale-feed pruning and delete-cascades.
#[test]
fn v19_book_feed_and_chapters_behave_like_subscriptions() {
    let tmp = TmpDb::new("v19-books");
    let conn = tmp.conn();

    let chapter_col: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='chapter_no'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(chapter_col, 1, "articles.chapter_no should exist");
    let catalog_table: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='book_catalog'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(catalog_table, 1, "book_catalog should exist");

    let feed = db::insert_book_feed(
        &conn,
        "Moby Dick; Or, The Whale",
        "https://www.gutenberg.org/ebooks/2701",
        "Herman Melville · 1851",
    )
    .unwrap();
    assert_eq!(feed.kind, "book");
    assert_eq!(feed.category, "book");
    assert_eq!(feed.origin, "user");
    // Idempotent by url: importing twice must not create a second feed.
    let again = db::insert_book_feed(
        &conn,
        "Moby Dick; Or, The Whale",
        "https://www.gutenberg.org/ebooks/2701",
        "Herman Melville · 1851",
    )
    .unwrap();
    assert_eq!(again.id, feed.id);

    // Reversed insertion order (3, 1, 2): chapter order must come from
    // chapter_no, not from the insert order or the uuid. `ids` is therefore
    // not in chapter order — index by chapter number below.
    let mut ids = Vec::new();
    for no in [3, 1, 2] {
        let mut article = sample_article(&format!("ch{no}"));
        article.url = format!("https://www.gutenberg.org/ebooks/2701#ch-{no}");
        article.source = feed.name.clone();
        article.category = "book".into();
        article.origin = "book".into();
        article.title = format!("Chapter {no}");
        article.fetched_at = "2026-01-01T00:00:00Z".into();
        article.summary_zh = String::new();
        db::insert_book_chapter(&conn, &article, no).unwrap();
        ids.push(article.id);
    }
    let (ch1, ch2, ch3) = (ids[1].as_str(), ids[2].as_str(), ids[0].as_str());

    let listed = db::query_articles(
        &conn,
        &db::ArticleQuery {
            source: Some(feed.name.as_str()),
            ..Default::default()
        },
        Some(10),
        Some(0),
    )
    .unwrap();
    let titles: Vec<String> = listed.iter().map(|a| a.title.clone()).collect();
    assert_eq!(titles, vec!["Chapter 1", "Chapter 2", "Chapter 3"]);

    let pos = db::book_chapter_position(&conn, ch3)
        .expect("position query")
        .expect("ch3 is a chapter");
    assert_eq!(pos.chapter_no, 3);
    assert_eq!(pos.chapter_count, 3);
    assert_eq!(pos.next_id, None, "last chapter has no next");
    assert_eq!(pos.prev_id.as_deref(), Some(ch2), "ch3 follows ch2");
    let mid = db::book_chapter_position(&conn, ch2)
        .expect("position query")
        .expect("ch2 is a chapter");
    assert_eq!(mid.prev_id.as_deref(), Some(ch1));
    assert_eq!(mid.next_id.as_deref(), Some(ch3));
    let plain = sample_article("plain");
    db::insert_article_if_new(&conn, &plain).unwrap();
    assert!(
        db::book_chapter_position(&conn, "plain").unwrap().is_none(),
        "non-chapter rows have no book position"
    );

    // A book must never queue LLM blurbs: 135 chapters would eat the whole
    // backfill budget for months.
    let missing = db::articles_missing_card_zh(&conn, 50).unwrap();
    assert!(
        missing.iter().all(|a| a.origin != "book"),
        "book chapters must be excluded from the card backfill"
    );

    // The rolling stale-feed prune must not disable a book: it never produces
    // new articles after import.
    conn.execute(
        "UPDATE feed_sources SET last_new_article_at='2020-01-01T00:00:00Z' WHERE id=?1",
        rusqlite::params![feed.id],
    )
    .unwrap();
    let pruned = db::prune_stale_feeds(&conn, 30).unwrap();
    assert!(
        !pruned.iter().any(|n| n == &feed.name),
        "book feeds are exempt from stale pruning, pruned: {pruned:?}"
    );
    let still_enabled: i64 = conn
        .query_row(
            "SELECT enabled FROM feed_sources WHERE id=?1",
            rusqlite::params![feed.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still_enabled, 1);

    // Deleting the book removes its chapters instead of orphaning them.
    db::delete_user_feed(&conn, &feed.id).unwrap();
    let leftovers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM articles WHERE origin='book'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(leftovers, 0);

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, db::LATEST_VERSION);
}
