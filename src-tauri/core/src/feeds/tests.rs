use super::*;
use super::filters::{
    body_defect, choose_article_body, is_blocked_content, is_readable_article_body,
    looks_truncated, rss_trust_chars, MIN_IMPORTED_BODY_CHARS, PageFailure,
    TRUST_RSS_FULLTEXT_CHARS,
};
use std::collections::HashSet;

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
    assert!(cfg!(test));
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
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        stream
            .write_all(
                b"HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .expect("write 302");
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
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1073741824\r\nConnection: close\r\n\r\n",
            )
            .expect("write headers");
        // Client must already have errored; the unread body is never sent.
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
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
            .expect("write headers");
        // One chunk just over the cap.
        let chunk = vec![b'x'; net::MAX_RESPONSE_BYTES + 1];
        let _ = stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes());
        let _ = stream.write_all(&chunk);
        let _ = stream.write_all(b"\r\n0\r\n\r\n");
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
    use std::io::{Read, Write};
    use std::net::TcpListener;

    // ~700 English words: clears both the RSS-trust and word-count gates.
    let prose: String = (0..50)
        .map(|_| "The quick brown fox jumps over the lazy dog near the quiet river bank. ")
        .collect();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let now = chrono::Utc::now().to_rfc2822();
    let feed_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <rss version=\"2.0\" xmlns:content=\"http://purl.org/rss/1.0/modules/content/\">\n\
         <channel><title>Test Feed</title><link>http://127.0.0.1:{port}/</link>\
         <description>test</description>\n\
         <item><title>Longform One</title><link>http://127.0.0.1:{port}/p1</link>\
         <guid isPermaLink=\"false\">test-p1</guid><pubDate>{now}</pubDate>\
         <description>A long story.</description>\
         <content:encoded><![CDATA[<p>{prose}</p>]]></content:encoded></item>\n\
         <item><title>Short Two</title><link>http://127.0.0.1:{port}/p2</link>\
         <guid isPermaLink=\"false\">test-p2</guid><pubDate>{now}</pubDate>\
         <description>A short teaser.</description></item>\n\
         </channel></rss>"
    );
    let article_html = format!(
        "<html><head><title>Short Two</title></head>\
         <body><article><h1>Short Two</h1><p>{prose}</p><p>{prose}</p></article></body></html>"
    );
    // Exactly 3 requests: /feed.xml + /p2 on refresh 1, /feed.xml (304) on
    // refresh 2. Extra requests would fail fast (connection refused) rather
    // than hang the test.
    let server = std::thread::spawn(move || {
        for stream in listener.incoming().take(3) {
            let mut stream = stream.expect("accept");
            let mut buf = vec![0u8; 8192];
            let mut head = Vec::new();
            loop {
                let n = stream.read(&mut buf).expect("read request");
                if n == 0 {
                    break;
                }
                head.extend_from_slice(&buf[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&head);
            let path = head
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            let etag_match = head
                .lines()
                .any(|l| l.to_ascii_lowercase().starts_with("if-none-match:") && l.contains("test-etag-1"));
            let (status, body, content_type) = if path == "/feed.xml" && etag_match {
                ("HTTP/1.1 304 Not Modified", String::new(), "application/rss+xml")
            } else if path == "/feed.xml" {
                ("HTTP/1.1 200 OK", feed_xml.clone(), "application/rss+xml")
            } else if path == "/p2" {
                ("HTTP/1.1 200 OK", article_html.clone(), "text/html")
            } else {
                ("HTTP/1.1 404 Not Found", String::new(), "text/plain")
            };
            let _ = stream.write_all(
                format!(
                    "{status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nETag: test-etag-1\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });

    let dir = std::env::temp_dir().join(format!("shiyan-refresh-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = crate::db::DbState::open(crate::db::db_path(dir.clone())).unwrap();
    {
        let conn = db.lock_write().unwrap();
        let test_feed = crate::db::subscribe_feed(
            &conn,
            "Test Feed",
            "tech",
            &format!("http://127.0.0.1:{port}/feed.xml"),
            "integration fixture",
        )
        .unwrap();
        // Opening the DB seeds ~80 curated feeds (all enabled). Refresh would
        // then crawl the real internet — disable everything but the fixture.
        for feed in crate::db::list_feeds(&conn).unwrap() {
            if feed.id != test_feed.id {
                crate::db::set_feed_enabled(&conn, &feed.id, false).unwrap();
            }
        }
    }
    // No API key in tests: card/tag enrichment must no-op instead of failing.
    let cfg = crate::config::AppConfig::default();
    assert!(cfg.api_key.trim().is_empty());

    let first = refresh_feeds(&db, &cfg, |_: RefreshProgress| {}).expect("refresh 1");
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
        let conn = db.lock_read().unwrap();
        assert_eq!(crate::db::list_article_urls(&conn).unwrap().len(), 2);
        let feeds = crate::db::list_feeds(&conn).unwrap();
        let enabled: Vec<_> = feeds.iter().filter(|f| f.enabled).collect();
        assert_eq!(enabled.len(), 1, "only the fixture feed stays enabled");
        assert_eq!(enabled[0].etag, "test-etag-1", "ETag persisted");
    }

    let second = refresh_feeds(&db, &cfg, |_: RefreshProgress| {}).expect("refresh 2");
    assert!(
        second.errors.iter().all(|e| e.contains("API Key")),
        "only no-key enrichment errors allowed: {:?}",
        second.errors
    );
    assert_eq!(second.feeds_unchanged, 1, "304 makes refresh a no-op");
    assert_eq!(second.added_or_updated, 0);

    server.join().expect("server thread");
    let _ = std::fs::remove_dir_all(&dir);
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
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let prose: String = (0..50)
        .map(|_| "The quick brown fox jumps over the lazy dog near the quiet river bank. ")
        .collect();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let now = chrono::Utc::now().to_rfc2822();
    let feed_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <rss version=\"2.0\" xmlns:content=\"http://purl.org/rss/1.0/modules/content/\">\n\
         <channel><title>Count Feed</title><link>http://127.0.0.1:{port}/</link>\
         <description>test</description>\n\
         <item><title>Counted Story</title><link>http://127.0.0.1:{port}/p1</link>\
         <guid isPermaLink=\"false\">count-p1</guid><pubDate>{now}</pubDate>\
         <description>A long story.</description>\
         <content:encoded><![CDATA[<p>{prose}</p>]]></content:encoded></item>\n\
         </channel></rss>"
    );
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 8192];
        let _ = stream.read(&mut buf);
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                feed_xml.len(),
                feed_xml
            )
            .as_bytes(),
        );
    });

    let dir = std::env::temp_dir().join(format!("shiyan-progress-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = crate::db::DbState::open(crate::db::db_path(dir.clone())).unwrap();
    {
        let conn = db.lock_write().unwrap();
        let test_feed = crate::db::subscribe_feed(
            &conn,
            "Count Feed",
            "tech",
            &format!("http://127.0.0.1:{port}/feed.xml"),
            "progress fixture",
        )
        .unwrap();
        for feed in crate::db::list_feeds(&conn).unwrap() {
            if feed.id != test_feed.id {
                crate::db::set_feed_enabled(&conn, &feed.id, false).unwrap();
            }
        }
    }
    let cfg = crate::config::AppConfig::default();

    let (tx, rx) = std::sync::mpsc::channel::<RefreshProgress>();
    refresh_feeds(&db, &cfg, move |p| {
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
    let _ = std::fs::remove_dir_all(&dir);
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
fn english_lang_tags() {
    assert!(super::filters::is_english_lang_tag("en"));
    assert!(super::filters::is_english_lang_tag("en-US"));
    assert!(super::filters::is_english_lang_tag("EN_GB"));
    assert!(!super::filters::is_english_lang_tag("zh-CN"));
    assert!(!super::filters::is_english_lang_tag("ja"));
    assert!(!super::filters::is_english_lang_tag("pt-BR"));
}

#[test]
fn rejects_chinese_content() {
    let title = "如何学习 Rust 编程语言入门指南";
    let content = "今天我们来讨论如何高效学习一门新的编程语言。首先需要理解基本概念，然后通过大量练习巩固知识。".repeat(5);
    assert!(!is_english_article(None, title, &content));
    assert!(!is_english_article(Some("zh-CN"), "Anything", &content));
}

#[test]
fn accepts_english_content() {
    let title = "How to learn Rust effectively";
    let content = "Today we discuss how to learn a new programming language effectively. First understand the fundamentals, then practice with real projects until the ideas stick.".repeat(3);
    assert!(is_english_article(None, title, &content));
    assert!(is_english_article(Some("en-US"), title, &content));
    assert!(!is_english_article(Some("fr"), title, &content));
}

#[test]
fn partition_skips_known() {
    let known = HashSet::from(["https://a".into()]);
    let (new_urls, skipped) =
        partition_new_urls(&["https://a".into(), "https://b".into()], &known);
    assert_eq!(skipped, 1);
    assert_eq!(new_urls, vec!["https://b".to_string()]);
}

#[test]
fn source_strips_www() {
    assert_eq!(
        source_from_url("https://www.theguardian.com/world/example"),
        "theguardian.com"
    );
    assert_eq!(source_from_url("not-a-url"), "导入");
}

#[test]
fn title_parses_html_title() {
    let html = "<html><head><title>  Hello World  | Site </title></head></html>";
    assert_eq!(title_from_html(html).as_deref(), Some("Hello World"));
}

#[test]
fn skips_rss_summary_when_page_fetch_fails() {
    // Mid-length teaser (≥ old 400 threshold) must not be kept if page is unavailable
    // (anti-crawl / paywall / short extract).
    let teaser = "a".repeat(500);
    assert!(teaser.chars().count() >= MIN_IMPORTED_BODY_CHARS);
    assert!(teaser.chars().count() < TRUST_RSS_FULLTEXT_CHARS);
    assert!(choose_article_body(&teaser, None).is_none());
    assert!(choose_article_body(&teaser, Some("too short")).is_none());
}

#[test]
fn accepts_page_fulltext_over_rss_teaser() {
    let teaser = "teaser ".repeat(80); // ~560 chars
    let full = "full article body here ".repeat(140); // 560 words
    assert!(full.split_whitespace().count() >= MIN_ARTICLE_WORDS);
    let chosen = choose_article_body(&teaser, Some(&full)).expect("page body");
    assert_eq!(chosen, full);
}

#[test]
fn trusts_long_rss_fulltext_without_page() {
    let full_rss = "word ".repeat(500); // 2500 chars
    assert!(full_rss.chars().count() >= TRUST_RSS_FULLTEXT_CHARS);
    let chosen = choose_article_body(&full_rss, None).expect("rss full text");
    assert_eq!(chosen, full_rss);
}

#[test]
fn summary_only_body_matches_choose_without_page() {
    let teaser = "a".repeat(500);
    assert!(is_summary_only_body(&teaser));
    assert!(!is_summary_only_body(&"word ".repeat(500)));
    assert!(is_summary_only_body("short"));
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

    let teaser = format!(
        "In Chad, the Chari River has been badly affected by years of intensive sand \
extraction along its banks, particularly around the capital. The ministry banned \
the practice to protect wildlife.                            Keywords for this article"
    );
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

/// The coverage audit must tell a client-rendered shell apart from a thin
/// source and a refusal — the whole reason it exists.
#[test]
fn coverage_audit_separates_shell_pages_from_refusals() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let prose: String = (0..50)
        .map(|_| "The quick brown fox jumps over the lazy dog near the quiet river bank. ")
        .collect();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("port").port();
    let now = chrono::Utc::now().to_rfc2822();
    // Every entry carries only a teaser, so each one needs a page fetch.
    let feed_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <rss version=\"2.0\">\n\
         <channel><title>Coverage Feed</title><link>http://127.0.0.1:{port}/</link>\
         <description>test</description>\n\
         <item><title>Real Article</title><link>http://127.0.0.1:{port}/p1</link>\
         <guid isPermaLink=\"false\">cov-p1</guid><pubDate>{now}</pubDate>\
         <description>A readable story.</description></item>\n\
         <item><title>Shell Post</title><link>http://127.0.0.1:{port}/p2</link>\
         <guid isPermaLink=\"false\">cov-p2</guid><pubDate>{now}</pubDate>\
         <description>A readable story.</description></item>\n\
         <item><title>Refused Post</title><link>http://127.0.0.1:{port}/p3</link>\
         <guid isPermaLink=\"false\">cov-p3</guid><pubDate>{now}</pubDate>\
         <description>A readable story.</description></item>\n\
         </channel></rss>"
    );
    let article_html = format!(
        "<html><head><title>Real Article</title></head>\
         <body><article><h1>Real Article</h1><p>{prose}</p><p>{prose}</p></article></body></html>"
    );
    // The client-rendered shape: a root node and a script, no text.
    let shell_html = "<html><head><title>Shell Post</title></head>\
         <body><div id=\"root\"></div><script>window.__data__={};</script></body></html>"
        .to_string();

    // 1 feed fetch + 3 page fetches.
    let server = std::thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            let mut stream = stream.expect("accept");
            let mut buf = vec![0u8; 8192];
            let mut head = Vec::new();
            loop {
                let n = match stream.read(&mut buf) {
                    Ok(n) if n > 0 => n,
                    _ => break,
                };
                head.extend_from_slice(&buf[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&head);
            let path = head
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            let (status, body, content_type) = match path {
                "/feed.xml" => ("HTTP/1.1 200 OK", feed_xml.clone(), "application/rss+xml"),
                "/p1" => ("HTTP/1.1 200 OK", article_html.clone(), "text/html"),
                "/p2" => ("HTTP/1.1 200 OK", shell_html.clone(), "text/html"),
                "/p3" => ("HTTP/1.1 403 Forbidden", String::new(), "text/plain"),
                _ => ("HTTP/1.1 404 Not Found", String::new(), "text/plain"),
            };
            let _ = stream.write_all(
                format!(
                    "{status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });

    let dir = std::env::temp_dir().join(format!("shiyan-coverage-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = crate::db::DbState::open(crate::db::db_path(dir.clone())).unwrap();
    {
        let conn = db.lock_write().unwrap();
        let feed = crate::db::subscribe_feed(
            &conn,
            "Coverage Feed",
            "tech",
            &format!("http://127.0.0.1:{port}/feed.xml"),
            "coverage fixture",
        )
        .unwrap();
        for other in crate::db::list_feeds(&conn).unwrap() {
            if other.id != feed.id {
                crate::db::set_feed_enabled(&conn, &other.id, false).unwrap();
            }
        }
        drop(conn);
        let conn = db.lock_read().unwrap();
        let report = coverage::audit_coverage(&conn, 5, 14).expect("audit");
        assert_eq!(report.len(), 1, "only the fixture feed is enabled");
        let cov = &report[0];
        assert_eq!(cov.entries_in_window, 3);
        assert_eq!(cov.pages_sampled, 3);
        assert_eq!(cov.already_stored, 0);
        assert_eq!(cov.recoverable, 1, "the real article is recoverable");
        assert_eq!(
            cov.drops.get(PageFailure::Shell.label()),
            Some(&1),
            "shell page must be attributed to rendering, not thinness: {:?}",
            cov.drops
        );
        assert_eq!(
            cov.drops.get(PageFailure::FetchFailed.label()),
            Some(&1),
            "403 must be a refusal: {:?}",
            cov.drops
        );
    }

    server.join().expect("server thread");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Real-world coverage report against the live library. Ignored by default:
/// it talks to every enabled news site. Run it with
/// `cargo test -- --ignored audit_live_coverage_report --nocapture`,
/// optionally `SHIYAN_AUDIT_SAMPLE=2` to fetch fewer pages per feed and
/// `SHIYAN_AUDIT_DB=/path/to/snapshot.db` to audit a copy instead of the
/// live database.
#[test]
#[ignore = "向全部启用源发真实请求，只在人工跑丢文统计时执行"]
fn audit_live_coverage_report() {
    let db_file = match std::env::var("SHIYAN_AUDIT_DB") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("HOME").expect("HOME");
            crate::db::db_path(
                std::path::PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("com.sihai.shiyan"),
            )
        }
    };
    let conn = rusqlite::Connection::open_with_flags(
        &db_file,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("open the database read-only");

    let sample: usize = std::env::var("SHIYAN_AUDIT_SAMPLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let report = coverage::audit_coverage(&conn, sample, 14).expect("audit");

    let json = serde_json::to_string_pretty(&report).expect("serialize");
    std::fs::write("/tmp/shiyan-coverage.json", json).expect("write report");

    let mut ordered: Vec<&coverage::FeedCoverage> = report.iter().collect();
    let lost = |c: &coverage::FeedCoverage| c.drops.values().sum::<usize>();
    ordered.sort_by_key(|c| std::cmp::Reverse(lost(c)));

    println!("\n=== 丢文原因汇总（抽样 {} 页/源）===", sample);
    for (reason, count) in coverage::rollup_drops(&report) {
        println!("{count:>6}  {reason}");
    }
    println!("\n=== 丢文最多的源 ===");
    println!("丢文\t可救\t已入库\t全文率\t源名\t主要成因");
    for c in ordered.iter().filter(|c| lost(c) > 0).take(30) {
        let top = c
            .drops
            .iter()
            .max_by_key(|(_, n)| *n)
            .map(|(r, n)| format!("{r}×{n}"))
            .unwrap_or_default();
        println!(
            "{}\t{}\t{}\t{:.2}\t{}\t{}",
            lost(c),
            c.recoverable,
            c.already_stored,
            c.fulltext_ratio,
            c.feed_name,
            top,
        );
    }
    println!("\n完整报告：/tmp/shiyan-coverage.json");
}

/// Print what the real extractor gets for a list of URLs, beside what the raw
/// page holds. Used to tell "the source has no article" apart from "we threw
/// most of it away". Reads URLs from `SHIYAN_PROBE_FILE`, one per line.
#[test]
#[ignore = "对指定 URL 跑真实抽取器，只在人工核对丢文原因时执行"]
fn probe_extractor_word_counts() {
    let path = std::env::var("SHIYAN_PROBE_FILE").expect("SHIYAN_PROBE_FILE");
    let urls = std::fs::read_to_string(&path).expect("read url list");
    println!("\n抽取正文词数\t整页可见词数\t判定\t链接");
    for url in urls.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let client = net::http_client().expect("http client");
        let whole = extract::extract_page(&client, url)
            .ok()
            .map(|p| p.text);
        let fetched = (|| {
            let resp = client
                .get(url)
                .send()
                .ok()?
                .error_for_status()
                .ok()?;
            Some(String::from_utf8_lossy(&net::read_limited_bytes(resp).ok()?).into_owned())
        })();
        let whole_page = fetched.as_deref().map(|h| html_to_text(h));
        let (gotten, page) = match (&whole, &whole_page) {
            (Some(t), Some(p)) => (
                t.split_whitespace().count(),
                p.split_whitespace().count(),
            ),
            _ => {
                println!("-\t-\t页面抓取失败\t{url}");
                continue;
            }
        };
        let verdict = if gotten >= crate::feeds::MIN_ARTICLE_WORDS {
            "可入库"
        } else if page >= crate::feeds::MIN_ARTICLE_WORDS {
            "被丢，但整页文本够长 → 抽取丢文"
        } else {
            "被丢，整页也确实短"
        };
        println!("{gotten}\t{page}\t{verdict}\t{url}");
    }
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
