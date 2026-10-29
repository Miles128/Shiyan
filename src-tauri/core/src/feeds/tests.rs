use super::*;
use super::filters::{
    body_defect, choose_article_body, is_blocked_content, is_readable_article_body,
    looks_truncated, rss_trust_chars, MIN_IMPORTED_BODY_CHARS, TRUST_RSS_FULLTEXT_CHARS,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

struct TestReq<'a> {
    port: u16,
    path: &'a str,
    head: &'a str,
    index: usize,
}

/// Loopback HTTP stub: accepts exactly `count` connections and writes whatever
/// `reply` returns. A request beyond `count` is refused, so an unexpected extra
/// fetch fails fast instead of hanging the test.
fn test_server<F>(count: usize, reply: F) -> (u16, std::thread::JoinHandle<()>)
where
    F: Fn(TestReq<'_>) -> Vec<u8> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let handle = std::thread::spawn(move || {
        for (index, stream) in listener.incoming().take(count).enumerate() {
            let mut stream = stream.expect("accept");
            let mut chunk = [0u8; 8192];
            let mut head = Vec::new();
            loop {
                let n = stream.read(&mut chunk).expect("read request");
                if n == 0 {
                    break;
                }
                head.extend_from_slice(&chunk[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let path = head
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            let req = TestReq {
                port,
                path: &path,
                head: &head,
                index,
            };
            let _ = stream.write_all(&reply(req));
        }
    });
    (port, handle)
}

fn http_200(content_type: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn english_prose(sentences: usize) -> String {
    "The quick brown fox jumps over the lazy dog near the quiet river bank. ".repeat(sentences)
}

/// Temp app-data folder holding one subscribed feed, with every curated seed
/// disabled so a refresh only touches the fixture.
struct FeedFixture {
    dir: PathBuf,
    db: Option<crate::db::DbState>,
}

impl FeedFixture {
    fn new(tag: &str, name: &str, port: u16) -> Self {
        let dir = std::env::temp_dir().join(format!("shiyan-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::DbState::open(crate::db::db_path(dir.clone())).unwrap();
        {
            let conn = db.lock_write().unwrap();
            let mine = crate::db::subscribe_feed(
                &conn,
                name,
                "tech",
                &format!("http://127.0.0.1:{port}/feed.xml"),
                "integration fixture",
            )
            .unwrap();
            for feed in crate::db::list_feeds(&conn).unwrap() {
                if feed.id != mine.id {
                    crate::db::set_feed_enabled(&conn, &feed.id, false).unwrap();
                }
            }
        }
        Self { dir, db: Some(db) }
    }

    fn state(&self) -> &crate::db::DbState {
        self.db.as_ref().unwrap()
    }
}

impl Drop for FeedFixture {
    fn drop(&mut self) {
        // Close the connections before unlinking the folder.
        self.db.take();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn blocks_link_roundups_and_transcripts() {
    assert!(is_blocked_content("Weekly Links, 09/04/2026", "body"));
    assert!(is_blocked_content("Link Roundup: Climate Edition", "body"));
    assert!(is_blocked_content("Transcript: Seth Bernstein", "body"));
    assert!(is_blocked_content("LWiAI Podcast #256", "body"));
    // Pure link dump (density, not title).
    let dump = "https://a.example/x ".repeat(60);
    assert!(is_blocked_content("Some post", &dump));
    // Timestamp-dense transcript.
    let ts = "00:01 00:02 talk ".repeat(30);
    assert!(is_blocked_content("Interview", &ts));
    // A normal essay is kept.
    let essay = "plain prose about the world ".repeat(200);
    assert!(!is_blocked_content("A normal essay", &essay));
    // Bare "daily"/"links" in a title must not trigger the roundup filter.
    assert!(!is_blocked_content("Daily Rituals of a Translator", &essay));
    assert!(!is_blocked_content("Links Between Poverty and Health", &essay));
    assert!(!is_blocked_content("A Review of Weekly Radio Dramas", &essay));
}

#[test]
fn public_http_url_accepts_normal_targets() {
    assert!(ensure_public_http_url("https://example.com/feed.xml").is_ok());
    assert!(ensure_public_http_url("http://93.184.216.34/a").is_ok());
}

#[test]
fn public_http_url_blocks_private_and_local_targets() {
    for bad in [
        "http://10.0.0.5/x",
        "http://192.168.1.1/x",
        "http://172.16.0.1/x",
        "http://169.254.169.254/latest/meta-data",
        "http://[fe80::1]/x",
        "http://[::ffff:127.0.0.1]/x",
        "ftp://example.com/x",
        "file:///etc/passwd",
    ] {
        assert!(ensure_public_http_url(bad).is_err(), "should block {bad}");
    }
}

/// Loopback is allowed in test builds only, so integration tests can run a
/// fake feed server. Production builds (`cfg!(test)` false) still block it.
#[test]
fn loopback_allowed_for_tests_only() {
    for ok in [
        "http://localhost/x",
        "http://127.0.0.1/x",
        "http://[::1]/x",
    ] {
        assert!(ensure_public_http_url(ok).is_ok(), "should allow {ok}");
    }
}

/// A malicious server that answers with a 302 to a blocked metadata endpoint.
/// The shared client must refuse the hop instead of following it.
#[test]
fn redirect_to_blocked_host_is_rejected() {
    let (port, server) = test_server(1, |_| {
        b"HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec()
    });
    let err = net::http_client()
        .expect("http client")
        .get(format!("http://127.0.0.1:{port}/feed"))
        .send()
        .expect_err("redirect to a blocked host must be rejected");
    // The custom policy error lives in the source chain (`{:?}` prints it).
    let chain = format!("{err:?}");
    assert!(
        chain.contains("重定向"),
        "unexpected error chain: {chain}"
    );
    server.join().expect("server thread");
}

/// A server advertising a gigabyte body is rejected before reading anything.
#[test]
fn oversized_content_length_is_rejected() {
    let (port, server) = test_server(1, |_| {
        b"HTTP/1.1 200 OK\r\nContent-Length: 1073741824\r\nConnection: close\r\n\r\n".to_vec()
    });
    let resp = net::http_client()
        .expect("http client")
        .get(format!("http://127.0.0.1:{port}/big"))
        .send()
        .expect("headers")
        .error_for_status()
        .expect("status");
    let err = net::read_limited_bytes(resp).expect_err("1GB advertisement must be rejected");
    assert!(err.to_string().contains("过大"), "unexpected error: {err}");
    server.join().expect("server thread");
}

/// A server lying about its length (chunked, endless body) is cut off mid-stream.
#[test]
fn lying_chunked_body_is_cut_off() {
    let (port, server) = test_server(1, |_| {
        let mut out =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        // One chunk just over the cap.
        let chunk = vec![b'x'; net::MAX_RESPONSE_BYTES + 1];
        out.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
        out.extend_from_slice(&chunk);
        out.extend_from_slice(b"\r\n0\r\n\r\n");
        out
    });
    let resp = net::http_client()
        .expect("http client")
        .get(format!("http://127.0.0.1:{port}/endless"))
        .send()
        .expect("headers")
        .error_for_status()
        .expect("status");
    let err = net::read_limited_bytes(resp).expect_err("endless body must be cut off");
    assert!(err.to_string().contains("过大"), "unexpected error: {err}");
    server.join().expect("server thread");
}

/// End-to-end refresh against a fake feed server: the RSS-trusted long item
/// is stored without a page fetch, the short teaser triggers exactly one
/// page fetch, and ETag/304 makes the second refresh a no-op.
#[test]
fn refresh_ingests_feed_and_honors_etag() {
    // ~700 English words: clears both the RSS-trust and word-count gates.
    let prose = english_prose(50);
    let article_html = format!(
        "<html><head><title>Short Two</title></head>\
         <body><article><h1>Short Two</h1><p>{prose}</p><p>{prose}</p></article></body></html>"
    );
    // Exactly 3 requests: /feed.xml + /p2 on refresh 1, /feed.xml (304) on
    // refresh 2. Extra requests would fail fast (connection refused) rather
    // than hang the test.
    let (port, server) = test_server(3, move |req| {
        let now = chrono::Utc::now().to_rfc2822();
        let feed_xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <rss version=\"2.0\" xmlns:content=\"http://purl.org/rss/1.0/modules/content/\">\n\
             <channel><title>Test Feed</title><link>http://127.0.0.1:{}/</link>\
             <description>test</description>\n\
             <item><title>Longform One</title><link>http://127.0.0.1:{}/p1</link>\
             <guid isPermaLink=\"false\">test-p1</guid><pubDate>{now}</pubDate>\
             <description>A long story.</description>\
             <content:encoded><![CDATA[<p>{prose}</p>]]></content:encoded></item>\n\
             <item><title>Short Two</title><link>http://127.0.0.1:{}/p2</link>\
             <guid isPermaLink=\"false\">test-p2</guid><pubDate>{now}</pubDate>\
             <description>A short teaser.</description></item>\n\
             </channel></rss>",
            req.port, req.port, req.port
        );
        let etag_match = req.head.lines().any(|l| {
            l.to_ascii_lowercase().starts_with("if-none-match:") && l.contains("test-etag-1")
        });
        if req.path == "/feed.xml" && etag_match {
            return b"HTTP/1.1 304 Not Modified\r\nContent-Type: application/rss+xml\r\nContent-Length: 0\r\nETag: test-etag-1\r\nConnection: close\r\n\r\n"
                .to_vec();
        }
        let (status, content_type, body) = match req.path {
            "/feed.xml" => ("HTTP/1.1 200 OK", "application/rss+xml", feed_xml),
            "/p2" => ("HTTP/1.1 200 OK", "text/html", article_html.clone()),
            _ => ("HTTP/1.1 404 Not Found", "text/plain", String::new()),
        };
        format!(
            "{status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nETag: test-etag-1\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    });

    // Opening the DB seeds ~80 curated feeds (all enabled); the fixture
    // disables everything but its own source.
    let fixture = FeedFixture::new("refresh-it", "Test Feed", port);
    // No API key in tests: card/tag enrichment must no-op instead of failing.
    let cfg = crate::config::AppConfig::default();
    assert!(cfg.api_key.trim().is_empty());

    let first = refresh_feeds(fixture.state(), &cfg, |_: RefreshProgress| {}).expect("refresh 1");
    // No API key in tests: card/tag enrichment reports errors but must not
    // block ingestion.
    assert!(
        first.errors.iter().all(|e| e.contains("API Key")),
        "only no-key enrichment errors allowed: {:?}",
        first.errors
    );
    assert_eq!(first.added_or_updated, 2, "both items ingested");
    assert_eq!(first.feeds_unchanged, 0);
    {
        let conn = fixture.state().lock_read().unwrap();
        assert_eq!(crate::db::list_article_urls(&conn).unwrap().len(), 2);
        let feeds = crate::db::list_feeds(&conn).unwrap();
        let enabled: Vec<_> = feeds.iter().filter(|f| f.enabled).collect();
        assert_eq!(enabled.len(), 1, "only the fixture feed stays enabled");
        assert_eq!(enabled[0].etag, "test-etag-1", "ETag persisted");
    }

    let second = refresh_feeds(fixture.state(), &cfg, |_: RefreshProgress| {}).expect("refresh 2");
    assert!(
        second.errors.iter().all(|e| e.contains("API Key")),
        "only no-key enrichment errors allowed: {:?}",
        second.errors
    );
    assert_eq!(second.feeds_unchanged, 1, "304 makes refresh a no-op");
    assert_eq!(second.added_or_updated, 0);
    server.join().expect("server thread");
}

/// A feed body that grows must still upgrade an article already in the library,
/// and one that shrinks must never downgrade it. This is the behaviour the
/// known-url length shortcut in `download_feed_articles` could in principle
/// break, so it is pinned here rather than left to the unit test of the bound.
#[test]
fn known_url_upgrade_survives_the_length_shortcut() {
    let short_body = english_prose(45);
    let long_body = english_prose(90);

    // Three feed fetches, no ETag: the second serves a longer body, the third
    // goes back to the shorter one. No page fetches are expected, because every
    // body clears the RSS-trust bar.
    let (port, server) = test_server(3, move |req| {
        let prose = if req.index == 1 {
            &long_body
        } else {
            &short_body
        };
        let now = chrono::Utc::now().to_rfc2822();
        let feed_xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <rss version=\"2.0\" xmlns:content=\"http://purl.org/rss/1.0/modules/content/\">\n\
             <channel><title>Growth Feed</title><link>http://127.0.0.1:{}/</link>\
             <description>test</description>\n\
             <item><title>Story That Grows</title><link>http://127.0.0.1:{}/p1</link>\
             <guid isPermaLink=\"false\">growth-p1</guid><pubDate>{now}</pubDate>\
             <description>A long story.</description>\
             <content:encoded><![CDATA[<p>{prose}</p>]]></content:encoded></item>\n\
             </channel></rss>",
            req.port, req.port
        );
        http_200("application/rss+xml", &feed_xml)
    });

    let fixture = FeedFixture::new("upgrade-it", "Growth Feed", port);
    let cfg = crate::config::AppConfig::default();
    let stored_len = |db: &crate::db::DbState| -> usize {
        let conn = db.lock_read().unwrap();
        crate::db::list_article_content_lengths(&conn)
            .unwrap()
            .values()
            .copied()
            .sum()
    };

    let first = refresh_feeds(fixture.state(), &cfg, |_: RefreshProgress| {}).expect("refresh 1");
    assert_eq!(first.added_or_updated, 1, "the story is ingested once");
    let baseline = stored_len(fixture.state());

    let second = refresh_feeds(fixture.state(), &cfg, |_: RefreshProgress| {}).expect("refresh 2");
    assert_eq!(second.updated, 1, "a longer feed body must upgrade the row");
    assert!(
        stored_len(fixture.state()) > baseline,
        "the upgrade has to actually land"
    );
    let grown = stored_len(fixture.state());

    let third = refresh_feeds(fixture.state(), &cfg, |_: RefreshProgress| {}).expect("refresh 3");
    assert_eq!(third.updated, 0, "a shorter body must not rewrite the row");
    assert_eq!(third.added_or_updated, 0);
    assert_eq!(
        stored_len(fixture.state()),
        grown,
        "the library never shrinks"
    );

    server.join().expect("server thread");
}

fn dummy_feed(id: &str, enabled: bool) -> crate::db::FeedSource {
    crate::db::FeedSource {
        id: id.into(),
        name: id.into(),
        category: "world".into(),
        url: format!("https://example.com/{id}"),
        enabled,
        origin: "curated".into(),
        description: String::new(),
        ..Default::default()
    }
}

#[test]
fn select_enabled_uses_db_flag_only() {
    let enabled = select_enabled_feeds(vec![
        dummy_feed("on", true),
        dummy_feed("off", false),
    ]);
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].id, "on");
}

/// Progress events must carry a live article count, not just source counts.
#[test]
fn refresh_progress_reports_article_count() {
    let prose = english_prose(50);
    let (port, server) = test_server(1, move |req| {
        let now = chrono::Utc::now().to_rfc2822();
        let feed_xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <rss version=\"2.0\" xmlns:content=\"http://purl.org/rss/1.0/modules/content/\">\n\
             <channel><title>Count Feed</title><link>http://127.0.0.1:{}/</link>\
             <description>test</description>\n\
             <item><title>Counted Story</title><link>http://127.0.0.1:{}/p1</link>\
             <guid isPermaLink=\"false\">count-p1</guid><pubDate>{now}</pubDate>\
             <description>A long story.</description>\
             <content:encoded><![CDATA[<p>{prose}</p>]]></content:encoded></item>\n\
             </channel></rss>",
            req.port, req.port
        );
        http_200("application/rss+xml", &feed_xml)
    });

    let fixture = FeedFixture::new("progress-it", "Count Feed", port);
    let cfg = crate::config::AppConfig::default();

    let (tx, rx) = std::sync::mpsc::channel::<RefreshProgress>();
    refresh_feeds(fixture.state(), &cfg, move |p| {
        let _ = tx.send(p);
    })
    .expect("refresh");
    let events: Vec<RefreshProgress> = rx.try_iter().collect();
    assert!(!events.is_empty(), "progress events emitted");
    let done = events
        .iter()
        .find(|p| p.phase == "done")
        .expect("done event");
    assert_eq!(done.articles, 1, "done event carries inserted article count");
    assert!(
        events
            .iter()
            .filter(|p| p.phase == "download")
            .any(|p| p.articles >= 1),
        "download phase reports the running article count"
    );

    server.join().expect("server thread");
}

#[test]
fn trust_bar_adapts_to_feed_fulltext_ratio() {
    assert_eq!(rss_trust_chars(0.9), 1200, "full-text feeds lower the bar");
    assert_eq!(rss_trust_chars(0.7), 1200);
    assert_eq!(
        rss_trust_chars(0.1),
        3200,
        "teaser-only feeds raise the bar"
    );
    assert_eq!(rss_trust_chars(0.0), 3200);
    assert_eq!(
        rss_trust_chars(-1.0),
        TRUST_RSS_FULLTEXT_CHARS,
        "unknown ratio keeps the default"
    );
    assert_eq!(rss_trust_chars(0.5), TRUST_RSS_FULLTEXT_CHARS);
}

#[test]
fn canonical_url_strips_tracking_and_noise() {
    assert_eq!(
        canonical_article_url(
            "https://example.com/story?utm_source=rss&utm_medium=feed&id=7#more"
        ),
        "https://example.com/story?id=7"
    );
    assert_eq!(
        canonical_article_url("https://Example.com/story/?fbclid=abc"),
        "https://example.com/story"
    );
    assert_eq!(
        canonical_article_url("https://example.com/a?gclid=x&fbclid=y"),
        "https://example.com/a"
    );
    // Non-http schemes and unparseable input pass through untouched.
    assert_eq!(canonical_article_url("mailto:a@b.c"), "mailto:a@b.c");
    assert_eq!(canonical_article_url("not a url"), "not a url");
}

#[test]
fn near_duplicate_title_rules() {
    assert!(is_near_duplicate_title(
        "Fed Signals Open Door to Rate Cuts",
        "fed signals open door to rate cuts"
    ));
    assert!(is_near_duplicate_title(
        "Fed Signals Open Door to Rate Cuts in September Meeting Minutes",
        "Fed Signals Open Door to Rate Cuts in September Meeting Minutes"
    ));
    // Long headlines differing by a couple of words.
    let a = "The Federal Reserve Signaled It Could Cut Interest Rates at Its September Policy Meeting";
    let b = "The Federal Reserve Signaled It Might Cut Interest Rates at Its September Policy Meeting";
    assert!(is_near_duplicate_title(a, b));
    // Short headlines need exact matches.
    assert!(!is_near_duplicate_title(
        "Markets slide again",
        "Markets slide today"
    ));
    // Unrelated long headlines stay apart.
    assert!(!is_near_duplicate_title(
        "How remote work reshaped suburban housing markets across America",
        "A deep dive into the history of jazz piano in New Orleans"
    ));
    assert!(!is_near_duplicate_title("", "anything"));
}

#[test]
fn title_index_dedups_across_sources() {
    let mut index = TitleIndex::new(vec![]);
    index.insert("Fed Signals Open Door to Rate Cuts in September Meeting Minutes");
    assert!(index.is_dup("fed signals open door to rate cuts in september meeting minutes"));
    assert!(!index.is_dup("Earnings Season Begins With a Whimper"));
    index.insert("Earnings Season Begins With a Whimper Amid Rate Anxiety This Quarter");
    assert!(index.is_dup("Earnings Season Begins With a Whimper Amid Rate Anxiety This Quarter"));
}

/// The index is a bulk implementation of the pairwise rule, so the two must
/// never disagree — including on the titles too short to be fuzzy-matched,
/// which the index deliberately does not store.
#[test]
fn title_index_agrees_with_pairwise_rule() {
    let stored = [
        "Fed Signals Open Door to Rate Cuts in September Meeting Minutes",
        "The Federal Reserve Signaled It Could Cut Interest Rates at Meeting",
        "Markets slide again",
        "Earnings Season Begins With a Whimper",
        "How remote work reshaped suburban housing markets across America",
    ];
    let probes = [
        // Exact after normalization.
        "fed signals, open door to rate cuts in september meeting minutes!",
        // One word different in a long headline: near-duplicate.
        "The Federal Reserve Signaled It Might Cut Interest Rates at Meeting",
        // Same words, different order beyond the threshold.
        "Meeting Rates Interest Cut It Signaled Reserve Federal The Also Today",
        // Too short to fuzzy-match; only an exact hit counts.
        "Markets slide today",
        // Unrelated.
        "A deep dive into the history of jazz piano in New Orleans",
    ];
    let index = TitleIndex::new(stored.iter().map(|t| (t.to_string(), String::new())).collect());
    for probe in probes {
        let expected = stored
            .iter()
            .any(|d| is_near_duplicate_title(probe, d));
        assert_eq!(
            index.is_dup(probe),
            expected,
            "index disagrees with the pairwise rule for {probe:?}"
        );
    }
}

/// The known-url shortcut must only fire when the feed body cannot possibly
/// beat what is already stored, and must stay out of the way otherwise.
#[test]
fn rss_shortcut_only_skips_bodies_that_cannot_win() {
    // A teaser body against a stored full article: skip.
    assert!(rss_cannot_beat_stored(1_500, 10_000));
    // A full-text feed body around the stored size: still needs the real check.
    assert!(!rss_cannot_beat_stored(11_000, 10_000));
    // Tag-free content, exactly as long as what we stored: not an upgrade,
    // but the margin is small enough that we refuse to guess.
    assert!(!rss_cannot_beat_stored(10_000, 10_000));
    assert!(!rss_cannot_beat_stored(10_000, 10_001));
    assert!(rss_cannot_beat_stored(10_000, 10_000 + 1_024 + 625));
}

#[test]
fn truncated_tails_force_page_fetch() {
    let body = "word ".repeat(500) + "Continue reading…";
    assert!(looks_truncated(&body));
    // Long readable body ending with a read-more marker is NOT trusted.
    assert!(choose_article_body(&body, None).is_none());
    // A clean page extract is still accepted.
    let full = "word ".repeat(500);
    assert!(choose_article_body(&body, Some(&full)).is_some());
    // Clean fulltext is unaffected.
    let clean = "word ".repeat(500);
    assert!(!looks_truncated(&clean));
    assert!(choose_article_body(&clean, None).is_some());
    assert!(!looks_truncated("short"));
}

#[test]
fn english_gate_checks_lang_tag_and_content() {
    for tag in ["en", "en-US", "EN_GB"] {
        assert!(
            super::filters::is_english_lang_tag(tag),
            "{tag} should pass the tag gate"
        );
    }
    for tag in ["zh-CN", "ja", "pt-BR"] {
        assert!(
            !super::filters::is_english_lang_tag(tag),
            "{tag} should fail the tag gate"
        );
    }

    let zh_title = "如何学习 Rust 编程语言入门指南";
    let zh = "今天我们来讨论如何高效学习一门新的编程语言。首先需要理解基本概念，然后通过大量练习巩固知识。".repeat(5);
    assert!(!is_english_article(None, zh_title, &zh));
    assert!(!is_english_article(Some("zh-CN"), "Anything", &zh));

    let en_title = "How to learn Rust effectively";
    let en = "Today we discuss how to learn a new programming language effectively. First understand the fundamentals, then practice with real projects until the ideas stick.".repeat(3);
    assert!(is_english_article(None, en_title, &en));
    assert!(is_english_article(Some("en-US"), en_title, &en));
    assert!(!is_english_article(Some("fr"), en_title, &en));
}

#[test]
fn title_parses_html_title() {
    let html = "<html><head><title>  Hello World  | Site </title></head></html>";
    assert_eq!(title_from_html(html).as_deref(), Some("Hello World"));
}

#[test]
fn body_choice_prefers_page_fulltext_and_trusts_long_rss() {
    // Mid-length teaser (≥ old 400 threshold) must not be kept if the page is
    // unavailable (anti-crawl / paywall / short extract).
    let teaser = "a".repeat(500);
    assert!(teaser.chars().count() >= MIN_IMPORTED_BODY_CHARS);
    assert!(teaser.chars().count() < TRUST_RSS_FULLTEXT_CHARS);
    assert!(choose_article_body(&teaser, None).is_none());
    assert!(choose_article_body(&teaser, Some("too short")).is_none());
    assert!(is_summary_only_body(&teaser));
    assert!(is_summary_only_body("short"));

    // A long RSS body is trusted without a page fetch.
    let full_rss = "word ".repeat(500); // 2500 chars
    assert!(full_rss.chars().count() >= TRUST_RSS_FULLTEXT_CHARS);
    assert_eq!(
        choose_article_body(&full_rss, None).as_deref(),
        Some(full_rss.as_str())
    );
    assert!(!is_summary_only_body(&full_rss));

    // A page body wins over an RSS teaser.
    let page = "full article body here ".repeat(140); // 560 words
    assert!(page.split_whitespace().count() >= MIN_ARTICLE_WORDS);
    assert_eq!(
        choose_article_body(&teaser, Some(&page)).as_deref(),
        Some(page.as_str())
    );
}

fn chrome_nav_soup() -> String {
    let mut s = String::from("Skip to main content\n\n");
    for i in 1..40 {
        s.push_str(&format!("* [ Home topic {i} ][{i}]\n"));
    }
    s.push_str("\nA one-line dek about the story.\n");
    for i in 1..40 {
        s.push_str(&format!("[{i}]: https://example.com/{i}\n"));
    }
    s
}

fn link_dump_body() -> String {
    let mut s = String::from("#### Markets\n\n");
    for i in 1..20 {
        s.push_str(&format!("* [A market headline number {i} for readers][{i}]\n"));
    }
    s
}

fn short_real_post() -> String {
    "Culture provides scaffolding, and learning happens over time. \
The result is that we are each capable of extraordinary feats. \
People can fly planes, ski down mountains, or solve a crossword. \
Most people only exhibit this skill when there are months of exposure.\n\n\
That is the whole post."
        .repeat(2)
}

#[test]
fn rejects_page_chrome_and_keyword_teasers() {
    let chrome = chrome_nav_soup();
    assert!(chrome.chars().count() > TRUST_RSS_FULLTEXT_CHARS);
    assert!(!is_readable_article_body(&chrome));
    assert!(choose_article_body(&chrome, None).is_none());

    let teaser = "In Chad, the Chari River has been badly affected by years of intensive sand \
extraction along its banks, particularly around the capital. The ministry banned \
the practice to protect wildlife.                            Keywords for this article"
        .to_string();
    assert!(!is_readable_article_body(&teaser));
}

#[test]
fn rejects_link_dump_even_when_long() {
    let dump = link_dump_body();
    assert!(dump.chars().count() > 400);
    assert!(!is_readable_article_body(&dump));
    assert!(choose_article_body(&dump, Some(&dump)).is_none());
}

/// A short post is a real article — it just is not auto-ingest material. The
/// learner can still import it by hand, so the structural verdict and the
/// ingest bar are deliberately two different questions.
#[test]
fn short_real_prose_is_importable_but_not_auto_ingested() {
    let post = short_real_post();
    assert!(post.chars().count() >= MIN_IMPORTED_BODY_CHARS);
    assert!(post.chars().count() < TRUST_RSS_FULLTEXT_CHARS);
    assert!(body_defect(&post).is_none(), "nothing wrong with its shape");
    assert!(!is_readable_article_body(&post), "under the word bar");
    assert!(choose_article_body("teaser", Some(&post)).is_none());
}

/// The body choice must keep a real article from readability, and must stop
/// trusting a readability fragment that grabbed only the opening.
#[test]
fn pick_body_keeps_the_article_and_rejects_a_fragment() {
    let page = "chrome and prose words ".repeat(500);
    let whole_article = "real article words ".repeat(450);
    assert_eq!(
        extract::pick_body(whole_article.clone(), page.clone()),
        whole_article,
        "an extraction that reached the ingest bar wins"
    );
    let fragment = "opening only ".repeat(100);
    assert_eq!(
        extract::pick_body(fragment, page.clone()),
        page,
        "a 200-word fragment must lose to the full page text"
    );
}
