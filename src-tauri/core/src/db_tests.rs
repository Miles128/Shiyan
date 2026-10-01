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
    db::upsert_article(&conn, &article).unwrap();
    let list = db::list_articles(&conn, Some("tech"), None, None).unwrap();
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
fn seed_feeds_skips_auto_removed_curated_ids() {
    let tmp = TmpDb::new("tombstone");
    let conn = tmp.conn();
    // Pick a real curated id so seed would normally re-insert it.
    let curated = db::list_feeds(&conn)
        .unwrap()
        .into_iter()
        .find(|f| f.origin == "curated")
        .expect("seeded curated feed");
    // Simulate auto-removal: row gone + tombstone written.
    conn.execute(
        "INSERT INTO removed_feeds (id, removed_at) VALUES (?1, ?2)",
        rusqlite::params![curated.id, "2026-01-01T00:00:00Z"],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM feed_sources WHERE id=?1",
        rusqlite::params![curated.id],
    )
    .unwrap();
    drop(conn);

    let conn = tmp.conn();
    assert!(
        !db::list_feeds(&conn)
            .unwrap()
            .iter()
            .any(|f| f.id == curated.id),
        "tombstoned curated feed must not be re-seeded"
    );
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

    let stored = db::get_article_by_url(&conn, "https://example.com/same")
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
fn filter_new_entries_skips_known_urls() {
    let known = std::collections::HashSet::from([
        "https://example.com/old".to_string(),
        "https://example.com/also".to_string(),
    ]);
    let candidates = vec![
        "https://example.com/old".to_string(),
        "https://example.com/new".to_string(),
        "https://example.com/also".to_string(),
        "https://example.com/fresh".to_string(),
    ];
    let (new_urls, skipped) = feeds::partition_new_urls(&candidates, &known);
    assert_eq!(skipped, 2);
    assert_eq!(
        new_urls,
        vec![
            "https://example.com/new".to_string(),
            "https://example.com/fresh".to_string()
        ]
    );
}

#[test]
fn purge_summary_only_removes_teasers_keeps_fulltext() {
    let tmp = TmpDb::new("purge-summary");
    let conn = tmp.conn();

    let mut chrome = String::from("Skip to main content\n\n");
    for i in 1..40 {
        chrome.push_str(&format!("* [ Home topic {i} ][{i}]\n"));
    }
    chrome.push_str("\nA one-line dek.\n");
    let teaser = db::Article {
        id: "teaser".into(),
        url: "https://example.com/teaser".into(),
        title: "Teaser".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: chrome,
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    let full = db::Article {
        id: "full".into(),
        url: "https://example.com/full".into(),
        title: "Full".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "word ".repeat(500), // ≥ 2000 chars
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &teaser).unwrap();
    db::insert_article_if_new(&conn, &full).unwrap();

    let removed = feeds::purge_summary_only_articles(&conn).unwrap();
    assert_eq!(removed, 1);
    assert!(db::get_article(&conn, "teaser").unwrap().is_none());
    assert!(db::get_article(&conn, "full").unwrap().is_some());

}

#[test]
fn purge_never_touches_user_imported_articles() {
    let tmp = TmpDb::new("purge-import");
    let conn = tmp.conn();

    for (id, url, origin) in [
        ("u1", "https://example.com/url-import", "url"),
        ("f1", "file://import/xyz", "file"),
    ] {
        let a = db::Article {
            id: id.into(),
            url: url.into(),
            title: "Imported".into(),
                source: "导入".into(),
            category: "other".into(),
            published_at: None,
            content_text: "a".repeat(500), // would be purged if origin were rss
            fetched_at: "2020-01-01T00:00:00Z".into(),
            origin: origin.into(),
            summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
        };
        db::insert_article_if_new(&conn, &a).unwrap();
    }

    let removed_short = feeds::purge_summary_only_articles(&conn).unwrap();
    assert_eq!(removed_short, 0, "imported short bodies must survive");
    let removed_lang = feeds::purge_non_english_articles(&conn).unwrap();
    assert_eq!(removed_lang, 0, "imported articles must survive language purge");
    assert_eq!(
        db::list_articles(&conn, None, Some(100), Some(0))
            .unwrap()
            .len(),
        2
    );

}

#[test]
fn collect_non_english_ids_does_not_delete() {
    let tmp = TmpDb::new("collect-zh");
    let conn = tmp.conn();
    let zh = db::Article {
        id: "zh1".into(),
        url: "https://example.com/zh".into(),
        title: "如何学习 Rust 编程语言入门指南".into(),
        source: "T".into(),
        category: "tech".into(),
        published_at: None,
        content_text: "今天我们来讨论如何高效学习一门新的编程语言。首先需要理解基本概念，然后通过大量练习巩固知识。"
            .repeat(5),
        fetched_at: "2020-01-01T00:00:00Z".into(),
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        ..Default::default()
    };
    db::insert_article_if_new(&conn, &zh).unwrap();

    let ids = feeds::collect_non_english_rss_ids(&conn).unwrap();
    assert_eq!(ids, vec!["zh1".to_string()]);
    assert!(db::get_article(&conn, "zh1").unwrap().is_some());

    let removed = feeds::delete_articles(&conn, &ids).unwrap();
    assert_eq!(removed, 1);
    assert!(db::get_article(&conn, "zh1").unwrap().is_none());

}

#[test]
fn purge_word_threshold_drops_short_rss_keeps_imports() {
    let tmp = TmpDb::new("word-threshold");
    let conn = tmp.conn();

    let mut short = sample_article("short");
    short.word_count = 100;
    short.quality = "fulltext".into();
    let mut long = sample_article("long");
    long.word_count = 800;
    long.quality = "fulltext".into();
    let mut unknown = sample_article("unknown");
    unknown.word_count = 0;
    let mut imported_short = sample_article("imported");
    imported_short.word_count = 50;
    imported_short.origin = "file".into();

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
        "user imports are never deleted by threshold purges"
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

    // Repair path invalidates as well.
    db::set_article_body(&conn, "inv1", &"repaired ".repeat(60), 120, "page").unwrap();
    assert!(db::get_translation(&conn, "inv1", "paragraph", "0")
        .unwrap()
        .is_none());

}

#[test]
fn refresh_article_content_skips_url_imports() {
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
        origin: "url".into(),
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
    assert_eq!(stored.origin, "url");

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

    let listed = db::list_articles(&conn, None, Some(1), Some(0)).unwrap();
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
    let page1 = db::list_articles(&conn, None, Some(2), Some(0)).unwrap();
    let page2 = db::list_articles(&conn, None, Some(2), Some(2)).unwrap();
    let page3 = db::list_articles(&conn, None, Some(2), Some(4)).unwrap();
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
fn content_audit_removes_synopsis_only_bodies_once() {
    let tmp = TmpDb::new("audit");
    let conn = tmp.conn();

    // A 500-char synopsis that ends with a read-more marker (teaser).
    let mut teaser = sample_article("teaser");
    teaser.content_text = format!("word {} Continue reading…", "word ".repeat(90));
    teaser.word_count = 90;
    // A real full-text article.
    let mut full = sample_article("full");
    full.content_text = "word ".repeat(600);
    full.word_count = 600;

    db::insert_article_if_new(&conn, &teaser).unwrap();
    db::insert_article_if_new(&conn, &full).unwrap();

    let removed = feeds::audit_rss_bodies_once(&conn).unwrap();
    assert_eq!(removed, 1, "synopsis-only body should be purged");
    assert!(db::get_article(&conn, "teaser").unwrap().is_none());
    assert!(db::get_article(&conn, "full").unwrap().is_some());

    // One-shot: a second run does nothing even if junk appears later.
    let mut junk = sample_article("junk");
    junk.content_text = "word ".repeat(10);
    junk.word_count = 10;
    db::insert_article_if_new(&conn, &junk).unwrap();
    assert_eq!(feeds::audit_rss_bodies_once(&conn).unwrap(), 0);
    assert!(db::get_article(&conn, "junk").unwrap().is_some());

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
    old_import.origin = "url".into();
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
fn phrase_library_dedup_review_and_listing() {
    let tmp = TmpDb::new("phrases");
    let conn = tmp.conn();

    db::insert_memory(&conn, &sample_phrase("p1", "on the house")).unwrap();
    assert!(db::get_memory_by_term(&conn, "phrase", "On The House")
        .unwrap()
        .is_some());

    // Case-insensitive unique index blocks a differently-cased duplicate.
    let dup = db::insert_memory(&conn, &sample_phrase("p2", "ON THE HOUSE"));
    assert!(dup.is_err(), "duplicate phrase must be rejected");

    // Due immediately (next_review_at in the past), listed once.
    let due = db::due_memory(&conn, Some("phrase")).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].term, "on the house");

    // SRS: easy schedule pushes it out of the due list.
    let mut item = db::get_memory(&conn, "p1").unwrap().unwrap();
    crate::srs::apply_rating(&mut item, crate::srs::Rating::Easy);
    assert_eq!(item.interval_days, 1.0);
    db::update_memory_review(&conn, &item).unwrap();
    assert!(db::due_memory(&conn, Some("phrase")).unwrap().is_empty());

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
    db::upsert_article(&conn, &sample_article("a1")).unwrap();
    db::upsert_article(&conn, &sample_article("a2")).unwrap();
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

/// v10 folds the legacy `vocab` and `phrases` tables into `memory_items`.
#[test]
fn v10_migration_merges_vocab_and_phrases_into_memory_items() {
    let tmp = TmpDb::new("v10-migrate");
    let conn = rusqlite::Connection::open(tmp.file()).unwrap();
    conn.execute_batch(
        "CREATE TABLE articles (id TEXT PRIMARY KEY);
         CREATE TABLE feed_sources (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT NOT NULL,
            url TEXT NOT NULL UNIQUE, enabled INTEGER NOT NULL DEFAULT 1,
            origin TEXT NOT NULL DEFAULT 'curated', description TEXT NOT NULL DEFAULT '',
            etag TEXT NOT NULL DEFAULT '', last_fetched_at TEXT,
            fulltext_ratio REAL NOT NULL DEFAULT -1
         );
         CREATE TABLE vocab (
            id TEXT PRIMARY KEY, term TEXT NOT NULL, definition_zh TEXT NOT NULL,
            word_type TEXT NOT NULL, collocations_json TEXT NOT NULL DEFAULT '[]',
            context_sentence TEXT NOT NULL DEFAULT '', article_id TEXT,
            status TEXT NOT NULL DEFAULT 'learning', interval_days REAL NOT NULL DEFAULT 0,
            reps INTEGER NOT NULL DEFAULT 0, consecutive_know INTEGER NOT NULL DEFAULT 0,
            next_review_at TEXT NOT NULL, created_at TEXT NOT NULL
         );
         CREATE TABLE phrases (
            id TEXT PRIMARY KEY, phrase TEXT NOT NULL, meaning_zh TEXT NOT NULL DEFAULT '',
            usage TEXT NOT NULL DEFAULT '', context_sentence TEXT NOT NULL DEFAULT '',
            article_id TEXT, status TEXT NOT NULL DEFAULT 'learning',
            interval_days REAL NOT NULL DEFAULT 0, reps INTEGER NOT NULL DEFAULT 0,
            consecutive_know INTEGER NOT NULL DEFAULT 0, next_review_at TEXT NOT NULL,
            created_at TEXT NOT NULL
         );
         INSERT INTO vocab VALUES
            ('w1','Ubiquitous','无处不在的','adjective','[\"ubiquitous in\"]','It is ubiquitous.',
             NULL,'learning',0,0,0,'2020-01-01T00:00:00Z','2020-01-01T00:00:00Z');
         INSERT INTO phrases VALUES
            ('p1','on the house','本店请客','collocation','It is on the house.',
             NULL,'learning',0,0,0,'2020-01-01T00:00:00Z','2020-01-01T00:00:00Z');
         PRAGMA user_version = 9;",
    )
    .unwrap();
    db::migrate(&conn).unwrap();

    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        17
    );
    let words = db::list_memory(&conn, Some("word"), None).unwrap();
    let phrases = db::list_memory(&conn, Some("phrase"), None).unwrap();
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].term, "Ubiquitous");
    assert_eq!(words[0].collocations, vec!["ubiquitous in".to_string()]);
    assert_eq!(phrases.len(), 1);
    assert_eq!(phrases[0].term, "on the house");
    assert_eq!(phrases[0].definition_zh, "本店请客");
    assert_eq!(phrases[0].word_type, "collocation");
    // Legacy tables are renamed, not dropped: rows stay recoverable.
    let renamed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('_legacy_vocab','_legacy_phrases')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(renamed, 2, "legacy tables should be renamed, not dropped");
    let legacy_phrases: i64 = conn
        .query_row("SELECT COUNT(*) FROM _legacy_phrases", [], |r| r.get(0))
        .unwrap();
    assert_eq!(legacy_phrases, 1, "legacy phrase rows preserved");

}

#[test]
fn migration_snapshot_is_written_before_version_bump() {
    let tmp = TmpDb::new("premigrate");
    let conn = rusqlite::Connection::open(tmp.file()).unwrap();
    conn.pragma_update(None, "user_version", 5).unwrap();

    db::backup_before_migration(&conn, &tmp.file());
    let backup = tmp
        .file()
        .with_file_name("shiyan.db.premigrate-v5.bak");
    assert!(backup.exists(), "pre-migration snapshot should exist");

    // Already up to date → no new snapshot. A sibling folder keeps this file's
    // own name, so the assertion below targets the real snapshot path.
    let up_to_date = tmp.dir().join("uptodate");
    std::fs::create_dir_all(&up_to_date).unwrap();
    let current = up_to_date.join("shiyan.db");
    let conn2 = rusqlite::Connection::open(&current).unwrap();
    conn2.pragma_update(None, "user_version", 17).unwrap();
    db::backup_before_migration(&conn2, &current);
    assert!(!current
        .with_file_name("shiyan.db.premigrate-v17.bak")
        .exists());
}

#[test]
fn v17_drops_tags_json_column() {
    let tmp = TmpDb::new("v17");
    let conn = tmp.conn();
    // Fresh DBs migrate straight to v17: the v7 column must be gone.
    let has: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='tags_json'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(has, 0, "tags_json should be dropped by v17");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 17);
}

#[test]
fn v16_drops_saved_sentences_table() {
    let tmp = TmpDb::new("v16");
    let conn = tmp.conn();
    // Fresh DBs migrate straight to v16: the v15 table must be gone.
    let has: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='saved_sentences'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(has, 0, "saved_sentences should be dropped by v16");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 17);
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
fn add_or_merge_memory_preserves_srs_progress() {
    let tmp = TmpDb::new("vocab-srs");
    let state = tmp.state();
    let input = |ctx: &str| crate::vocab::AddMemoryInput {
        kind: "word".into(),
        term: "persevere".into(),
        context_sentence: ctx.into(),
        article_id: None,
        definition_zh: Some("坚持".into()),
        word_type: Some("verb".into()),
        collocations: Some(vec![]),
    };
    let first = crate::vocab::add_or_merge_memory(&state, input("Keep going.")).unwrap();

    // Simulate two successful reviews.
    {
        let conn = state.lock_write().unwrap();
        let mut item = db::get_memory_by_term(&conn, "word", "persevere")
            .unwrap()
            .expect("exists");
        item.reps = 2;
        item.consecutive_know = 2;
        item.interval_days = 3.0;
        db::update_memory_review(&conn, &item).unwrap();
    }

    // Re-adding the same word merges metadata but never resets SRS state.
    let merged = crate::vocab::add_or_merge_memory(&state, input("Still going.")).unwrap();
    assert_eq!(merged.id, first.id);
    assert_eq!(merged.reps, 2);
    assert_eq!(merged.consecutive_know, 2);
    assert_eq!(merged.interval_days, 3.0);
    assert_eq!(merged.status, "learning");
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
fn known_words_add_list_remove_roundtrip() {
    let tmp = TmpDb::new("known");
    let conn = tmp.conn();

    // Insert normalizes case/whitespace; a differently-cased re-add is ignored.
    db::add_known_word(&conn, "  Serendipity ").unwrap();
    db::add_known_word(&conn, "serendipity").unwrap();
    // Empty / whitespace-only terms are silently ignored.
    db::add_known_word(&conn, "   ").unwrap();

    let words = db::list_known_words(&conn).unwrap();
    assert_eq!(words, vec!["serendipity".to_string()]);

    db::add_known_word(&conn, "Ubiquitous").unwrap();
    let words = db::list_known_words(&conn).unwrap();
    assert_eq!(words.len(), 2, "sorted list keeps both terms");

    db::remove_known_word(&conn, " SERENDIPITY ").unwrap();
    let words = db::list_known_words(&conn).unwrap();
    assert_eq!(words, vec!["ubiquitous".to_string()]);

    // Removing a missing term is a no-op, not an error.
    db::remove_known_word(&conn, "ghost").unwrap();

}

#[test]
fn known_words_normalize_apostrophes_and_legacy_rows() {
    let tmp = TmpDb::new("known-norm");
    let conn = tmp.conn();

    // Curly apostrophes fold to straight ones on write.
    db::add_known_word(&conn, "it’s").unwrap();
    db::add_known_word(&conn, "it's").unwrap();
    assert_eq!(db::list_known_words(&conn).unwrap(), vec!["it's".to_string()]);
    db::remove_known_word(&conn, "IT’S").unwrap();
    assert!(db::list_known_words(&conn).unwrap().is_empty());

    // Legacy rows written before normalization are canonicalized once,
    // with collisions collapsed ('don’t' and 'DON’T' fold to one row,
    // while plain 'dont' is a different word and stays).
    conn.execute(
        "INSERT INTO known_words (term, created_at) VALUES ('don’t', 'x'), ('DON’T', 'x'), ('dont', 'x')",
        [],
    )
    .unwrap();
    let fixed = db::normalize_known_words_once(&conn).unwrap();
    assert_eq!(fixed, 2, "both non-canonical rows are rewritten");
    assert_eq!(
        db::list_known_words(&conn).unwrap(),
        vec!["don't".to_string(), "dont".to_string()]
    );
    // Second run is a no-op even with fresh legacy-shaped rows.
    conn.execute(
        "INSERT INTO known_words (term, created_at) VALUES ('can’t', 'x')",
        [],
    )
    .unwrap();
    assert_eq!(db::normalize_known_words_once(&conn).unwrap(), 0);

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
fn repair_target_selection_and_body_replacement() {
    let tmp = TmpDb::new("repair");
    let conn = tmp.conn();

    // Target: RSS article whose body lost all paragraph breaks.
    let mut flat = sample_article("flat");
    flat.content_text = "one long line without any newlines ".repeat(30);
    // Not a target: body already has paragraph breaks.
    let mut healthy = sample_article("healthy");
    healthy.content_text = "First paragraph.\n\nSecond paragraph.".into();
    // Not a target: user import (origin != rss), even with a flat body.
    let mut imported = sample_article("imported");
    imported.origin = "url".into();
    imported.content_text = "another flat body ".repeat(30);

    for a in [&flat, &healthy, &imported] {
        db::insert_article_if_new(&conn, a).unwrap();
    }

    let targets = db::articles_without_paragraphs(&conn, 50).unwrap();
    assert_eq!(targets.len(), 1, "only the flat RSS body is a repair target");
    assert_eq!(targets[0].id, "flat");

    // Replace the body as the repair path would.
    let fixed = "First para.\n\nSecond para.\n\nThird para.";
    db::set_article_body(&conn, "flat", fixed, 6, "page").unwrap();
    let stored = db::get_article(&conn, "flat").unwrap().expect("exists");
    assert_eq!(stored.content_text, fixed);
    assert_eq!(stored.word_count, 6);
    assert_eq!(stored.quality, "fulltext");
    assert_eq!(stored.extraction_source, "page");

    // Repaired rows leave the candidate set.
    assert!(db::articles_without_paragraphs(&conn, 50).unwrap().is_empty());

}

#[test]
fn apply_legacy_disabled_feeds_sets_enabled_false() {
    let tmp = TmpDb::new("disabled");
    let conn = tmp.conn();
    let some = db::list_feeds(&conn).unwrap().into_iter().next().expect("seed");
    assert!(some.enabled);
    db::apply_legacy_disabled_feeds(&conn, &[some.id.clone()]).unwrap();
    let after = db::list_feeds(&conn)
        .unwrap()
        .into_iter()
        .find(|f| f.id == some.id)
        .unwrap();
    assert!(!after.enabled);
}

#[test]
fn article_foreign_keys_cascade_and_reject_orphans() {
    let tmp = TmpDb::new("fk");
    let conn = tmp.conn();

    db::upsert_article(&conn, &sample_article("a1")).unwrap();
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
    db::upsert_article(&conn, &sample_article("a1")).unwrap();
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
    db::upsert_article(&conn, &sample_article("a1")).unwrap();
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
    let conn = tmp.conn();
    db::upsert_article(&conn, &sample_article("a1")).unwrap();
    conn.execute(
        "UPDATE articles SET content_text = ?1 WHERE id = 'a1'",
        ["First para.\n\nSecond para."],
    )
    .unwrap();
    db::save_translation(&conn, "a1", "paragraph", "0", "First para.", "第一段", "test")
        .unwrap();

    let view = crate::article_view::load_article_view(&conn, "a1")
        .unwrap()
        .expect("present");
    assert_eq!(view.article.id, "a1");
    assert_eq!(view.paragraphs, vec!["First para.", "Second para."]);
    assert_eq!(view.translations.len(), 1);
    assert_eq!(view.translations[0].translated_text, "第一段");
    assert!(crate::article_view::load_article_view(&conn, "missing")
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
    assert_eq!(version, 17);
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
    let tags_col: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='tags_json'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tags_col, 0, "articles.tags_json should be dropped by v17");
    let quality_col: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='quality'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(quality_col, 1, "articles.quality should exist after migrate");
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
fn bundle_id_rename_adopts_legacy_data_dir_once() {
    let tmp = TmpDb::new("bundle");
    let parent = tmp.dir();
    let legacy = parent.join("com.sihai.learnenglish");
    let current = parent.join("com.sihai.shiyan");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("learnenglish.db"), b"fake-db").unwrap();
    std::fs::write(legacy.join("learnenglish.db-wal"), b"fake-wal").unwrap();
    std::fs::write(legacy.join("config.local.json"), b"{}").unwrap();

    let copied = db::migrate_legacy_app_dir(&current).unwrap();
    assert_eq!(copied, 3, "db + wal + config move over");
    // DB files are adopted under the new name; config keeps its own name.
    assert_eq!(
        std::fs::read(current.join("shiyan.db")).unwrap(),
        b"fake-db"
    );
    assert_eq!(
        std::fs::read(current.join("shiyan.db-wal")).unwrap(),
        b"fake-wal"
    );
    assert!(current.join("config.local.json").exists());
    // Copy, never move: the old dir stays as a backup.
    assert!(legacy.join("learnenglish.db").exists());

    // Second run is a no-op and never overwrites new data.
    std::fs::write(current.join("shiyan.db"), b"new-db").unwrap();
    assert_eq!(db::migrate_legacy_app_dir(&current).unwrap(), 0);
    assert_eq!(std::fs::read(current.join("shiyan.db")).unwrap(), b"new-db");

    // No legacy dir at all is also a no-op.
    let bare_parent = tmp.dir().join("bare");
    let fresh = bare_parent.join("com.sihai.shiyan");
    assert_eq!(db::migrate_legacy_app_dir(&fresh).unwrap(), 0);
}

#[test]
fn db_file_rename_happens_in_place_and_is_idempotent() {
    // Already on the new bundle id, but the DB still carries the legacy name.
    let tmp = TmpDb::new("dbfile");
    let dir = tmp.dir();
    std::fs::write(dir.join("learnenglish.db"), b"fake-db").unwrap();
    std::fs::write(dir.join("learnenglish.db-wal"), b"fake-wal").unwrap();
    std::fs::write(dir.join("learnenglish.db.premigrate-v10.bak"), b"old").unwrap();
    std::fs::write(dir.join("config.local.json"), b"{}").unwrap();

    assert_eq!(db::migrate_legacy_app_dir(&dir).unwrap(), 3);
    assert_eq!(std::fs::read(dir.join("shiyan.db")).unwrap(), b"fake-db");
    assert_eq!(std::fs::read(dir.join("shiyan.db-wal")).unwrap(), b"fake-wal");
    assert!(dir.join("shiyan.db.premigrate-v10.bak").exists());
    // Legacy names are gone and unrelated files are untouched.
    assert!(!dir.join("learnenglish.db").exists());
    assert!(!dir.join("learnenglish.db-wal").exists());
    assert!(dir.join("config.local.json").exists());

    // Rerun is a no-op once the new-named DB exists.
    assert_eq!(db::migrate_legacy_app_dir(&dir).unwrap(), 0);

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
