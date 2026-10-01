//! The RSS refresh pipeline: parallel feed download, entry filtering,
//! DB insertion, and progress reporting.

use super::cleanup::{
    assess_unassessed_articles, audit_rss_bodies_once, purge_expired_articles,
    purge_rss_below_word_threshold,
};
use super::dedup::{canonical_article_url, TitleIndex};
use super::enrich::{fill_missing_card_zh, CARDS_PER_REFRESH};
use super::extract::{fetch_article_page, html_to_text};
use super::filters::{
    choose_article_body, is_blocked_content, is_english_article, looks_like_paywall,
    looks_truncated, rss_is_full_text, rss_trust_chars,
};
use super::net::{ensure_public_http_url, http_client, read_limited_bytes};
use crate::config::AppConfig;
use crate::db::{self, Article, DbState, FeedSource};
use crate::error::AppError;
use chrono::Utc;
use feed_rs::parser;
use reqwest::blocking::Client;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use ts_rs::TS;
use uuid::Uuid;

/// How many feeds download concurrently. Bounded to keep polite to servers
/// and to preserve per-feed progress ordering in the UI.
const PARALLEL_FEEDS: usize = 4;
/// Total enrich (tags + card-zh) budget per refresh. Downloads always run to
/// completion; when the budget is exhausted the enrich phases are skipped and
/// noted in `result.errors` so a slow LLM never holds a refresh hostage.
const ENRICH_BUDGET: std::time::Duration = std::time::Duration::from_secs(90);
/// 滚动窗口：启用中的源这么多天没有产出任何新文章，就在刷新末尾退场
/// （精选删除、用户停用，见 db::prune_stale_feeds）。
const STALE_FEED_DAYS: i64 = 30;

/// Cooperative cancel flag for an in-flight refresh. Set by the shell's
/// `cancel_refresh` command; the download workers and the enrich phases poll
/// it. Everything committed so far stays (1C: 取消保留已入库).
static REFRESH_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Signal an in-flight [`refresh_feeds`] to stop after the current unit of
/// work. Committed articles are kept.
pub fn request_refresh_cancel() {
    REFRESH_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn refresh_cancelled() -> bool {
    REFRESH_CANCEL.load(std::sync::atomic::Ordering::SeqCst)
}

fn clear_refresh_cancel() {
    REFRESH_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
}
/// Articles written per write-lock hold. Releasing the lock between batches
/// keeps one feed's download from starving readers and other workers.
const WRITE_BATCH_SIZE: usize = 10;

#[derive(Debug, Default, Serialize, TS)]
#[ts(export)]
pub struct RefreshResult {
    pub fetched_feeds: usize,
    pub added_or_updated: usize,
    pub updated: usize,
    pub skipped_existing: usize,
    pub skipped_short: usize,
    pub skipped_non_english: usize,
    pub skipped_duplicate: usize,
    /// One-time backfill removals of synopsis-only / truncated bodies.
    pub purged_teasers: usize,
    /// Retention removals (articles older than the configured window).
    pub purged_old: usize,
    /// Stale feeds retired this refresh (30-day rolling window, no new articles).
    pub pruned_stale_feeds: usize,
    pub feeds_unchanged: usize,
    pub titles_translated: usize,
    pub errors: Vec<String>,
}

#[derive(Default)]
pub(crate) struct DownloadStats {
    skipped_existing: usize,
    skipped_short: usize,
    skipped_non_english: usize,
    skipped_duplicate: usize,
    /// Link roundups / podcast transcripts — never ingested.
    skipped_blocked: usize,
    /// Entries older than the retention window — never ingested.
    skipped_old: usize,
    /// Entries whose RSS body was trusted full-text without a page fetch.
    rss_fulltext_hits: usize,
    /// Entries considered for new content (denominator of fulltext_ratio).
    evaluated: usize,
    /// Entries skipped because the page-fetch budget was exhausted.
    skipped_budget: usize,
}

pub(crate) struct FeedDownload {
    articles: Vec<Article>,
    updates: Vec<Article>,
    stats: DownloadStats,
    /// ETag from this response; None leaves the stored value untouched.
    etag: Option<String>,
    /// Server answered 304 Not-Modified — nothing to parse or insert.
    unchanged: bool,
}

/// The refresh result every worker reports into.
type Shared<'a> = std::sync::Mutex<&'a mut RefreshResult>;

/// Record one feed's failure and carry on: a dead or misbehaving source must
/// not take the other feeds' downloads down with it.
/// Never panics on a poisoned mutex — a panicking worker must not take the
/// rest of the refresh down with it (same poison-recover as `DbState`).
fn note_failure(shared: &Shared<'_>, feed_name: &str, error: impl std::fmt::Display) {
    shared
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .errors
        .push(format!("{feed_name}: {error}"));
}

/// Insert a feed's new articles, one transaction per [`WRITE_BATCH_SIZE`] chunk.
/// The write lock is released between chunks so one feed cannot starve readers
/// or the other workers; inside a chunk SQLite fsyncs once instead of once per
/// article, and a mid-chunk failure rolls that whole chunk back — the next
/// refresh re-fetches it.
///
/// `Err` means stop writing this feed; everything committed so far stays.
/// Returns the number of rows newly inserted this call.
fn persist_articles(
    db: &DbState,
    shared: &Shared<'_>,
    articles: &[Article],
    known_urls: &std::sync::Mutex<HashSet<String>>,
    title_index: &std::sync::Mutex<TitleIndex>,
    stats: &mut DownloadStats,
) -> Result<usize, AppError> {
    let mut inserted = 0usize;
    for chunk in articles.chunks(WRITE_BATCH_SIZE) {
        let conn = db.lock_write()?;
        let tx = conn.unchecked_transaction()?;
        for article in chunk {
            if title_index
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_dup(&article.title)
            {
                stats.skipped_duplicate += 1;
                continue;
            }
            match db::insert_article_if_new(&tx, article) {
                Ok(true) => {
                    known_urls
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(article.url.clone());
                    title_index
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(&article.title);
                    shared.lock().unwrap_or_else(|e| e.into_inner()).added_or_updated += 1;
                    inserted += 1;
                }
                Ok(false) => stats.skipped_existing += 1,
                Err(e) => return Err(e),
            }
        }
        tx.commit()?;
    }
    Ok(inserted)
}

/// Upgrade the stored body of articles already in the library, again as a
/// single transaction. A per-update error used to be swallowed while the loop
/// carried on, which is not something a transaction can report honestly.
fn persist_updates(
    db: &DbState,
    shared: &Shared<'_>,
    updates: &[Article],
) -> Result<(), AppError> {
    let conn = db.lock_write()?;
    let tx = conn.unchecked_transaction()?;
    for update in updates {
        if db::refresh_article_content(&tx, update)? {
            shared.lock().unwrap_or_else(|e| e.into_inner()).updated += 1;
        }
    }
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RefreshProgress {
    /// download | translate | done
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub label: String,
    /// 0–100 overall progress across download + translate
    pub percent: u8,
    /// Articles newly inserted into the library so far this refresh.
    pub articles: usize,
}

pub(crate) fn select_enabled_feeds(feeds: Vec<FeedSource>) -> Vec<FeedSource> {
    feeds.into_iter().filter(|f| f.enabled).collect()
}

pub fn refresh_feeds(
    db: &DbState,
    cfg: &AppConfig,
    mut on_progress: impl FnMut(RefreshProgress) + Send + 'static,
) -> Result<RefreshResult, AppError> {
    let started_at = std::time::Instant::now();
    clear_refresh_cancel();
    let feeds = {
        let conn = db.lock_read()?;
        db::list_feeds(&conn)?
    };

    let enabled: Vec<FeedSource> = select_enabled_feeds(feeds);

    let download_total = enabled.len();
    // Reserve ~80% of the bar for downloads, ~20% for title translation.
    let translate_weight = 20u8;
    let download_weight = 80u8;

    let mut result = RefreshResult::default();

    if download_total == 0 {
        on_progress(RefreshProgress {
            phase: "done".into(),
            current: 0,
            total: 0,
            label: "没有启用的订阅源".into(),
            percent: 100,
            articles: 0,
        });
        return Ok(result);
    }

    // Progress flows through a channel: workers (and this thread) send
    // events, one pump thread owns the `FnMut` callback.
    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<RefreshProgress>();
    let pump = std::thread::spawn(move || {
        for event in progress_rx {
            on_progress(event);
        }
    });
    let report = {
        let progress_tx = &progress_tx;
        move |phase: &str, current: usize, total: usize, label: String, percent: u8, articles: usize| {
            let _ = progress_tx.send(RefreshProgress {
                phase: phase.into(),
                current,
                total,
                label,
                percent,
                articles,
            });
        }
    };

    // One-time assessment of rows that never got a quality stamp.
    {
        let conn = db.lock_write()?;
        let (del_non_english, del_short) = assess_unassessed_articles(&conn)?;
        result.skipped_non_english += del_non_english;
        result.skipped_short += del_short;
        result.skipped_short += purge_rss_below_word_threshold(&conn)?;
        // One-time body audit: synopsis-only / truncated bodies out.
        result.purged_teasers += audit_rss_bodies_once(&conn)?;
        // Retention window from settings.
        result.purged_old += purge_expired_articles(&conn, cfg.article_retention_days)?;
    }
    let known_urls = std::sync::Mutex::new({
        let conn = db.lock_read()?;
        db::list_article_urls(&conn)?
            .into_iter()
            .map(|u| canonical_article_url(&u))
            .collect::<HashSet<String>>()
    });
    // Dedup window: only recent titles, so recurring same-name features
    // (daily briefings, link roundups) are never swallowed forever.
    let dedup_since = (Utc::now() - chrono::Duration::days(14)).to_rfc3339();
    let title_index = std::sync::Mutex::new(TitleIndex::new({
        let conn = db.lock_read()?;
        db::list_article_titles(&conn, Some(&dedup_since))?
    }));
    let known_lengths = {
        let conn = db.lock_read()?;
        db::list_article_content_lengths(&conn)?
    };
    let upgraded = std::sync::Mutex::<HashSet<String>>::new(HashSet::new());

    let next_index = std::sync::atomic::AtomicUsize::new(0);
    let done_feeds = std::sync::atomic::AtomicUsize::new(0);
    let shared = std::sync::Mutex::new(&mut result);
    let now = Utc::now().to_rfc3339();
    let http = http_client()?;

    std::thread::scope(|scope| {
        let workers = PARALLEL_FEEDS.min(download_total);
        for _ in 0..workers {
            scope.spawn(|| loop {
                if refresh_cancelled() {
                    break;
                }
                let index = next_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if index >= download_total {
                    break;
                }
                let feed = &enabled[index];
                let done = done_feeds.load(std::sync::atomic::Ordering::SeqCst);
                let started_articles = shared.lock().unwrap_or_else(|e| e.into_inner()).added_or_updated;
                report(
                    "download",
                    done + 1,
                    download_total,
                    format!(
                        "增量下载 {}/{} 源 · 新增 {} 篇：{}",
                        done + 1,
                        download_total,
                        started_articles,
                        feed.name
                    ),
                    ((done as u16 * download_weight as u16) / download_total.max(1) as u16) as u8,
                    started_articles,
                );

                let trust_chars = rss_trust_chars(feed.fulltext_ratio);
                let outcome =
                    download_feed_articles(
                        &http,
                        feed,
                        trust_chars,
                        &known_urls,
                        &known_lengths,
                        &upgraded,
                        cfg.article_retention_days,
                    );
                done_feeds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                let mut ok = true;
                let mut ratio: Option<f64> = None;
                match outcome {
                    Ok(download) => {
                        if download.unchanged {
                            (shared.lock().unwrap_or_else(|e| e.into_inner())).feeds_unchanged += 1;
                        }
                        let mut stats = download.stats;
                        let mut inserted = 0usize;
                        let insert_ok = match persist_articles(
                            db,
                            &shared,
                            &download.articles,
                            &known_urls,
                            &title_index,
                            &mut stats,
                        ) {
                            Ok(n) => {
                                inserted = n;
                                true
                            }
                            Err(e) => {
                                ok = false;
                                note_failure(&shared, &feed.name, e);
                                false
                            }
                        };
                        // Body upgrades only rewrite rows the insert phase left
                        // alone, so a failed insert means a stale picture.
                        if insert_ok && !download.updates.is_empty() {
                            if let Err(e) = persist_updates(db, &shared, &download.updates) {
                                ok = false;
                                note_failure(&shared, &feed.name, e);
                            }
                        }
                        if stats.evaluated > 0 {
                            ratio =
                                Some(stats.rss_fulltext_hits as f64 / stats.evaluated as f64);
                        }
                        {
                            let mut result = shared.lock().unwrap_or_else(|e| e.into_inner());
                            result.skipped_existing += stats.skipped_existing + stats.skipped_old;
                            result.skipped_short += stats.skipped_short;
                            result.skipped_non_english += stats.skipped_non_english;
                            result.skipped_duplicate += stats.skipped_duplicate;
                        }
                        match db.lock_write() {
                            Err(e) => {
                                ok = false;
                                note_failure(&shared, &feed.name, e);
                            }
                            Ok(conn) => {
                                if let Err(e) = db::set_feed_refresh_meta(
                                    &conn,
                                    &feed.id,
                                    download.etag.as_deref(),
                                    &now,
                                    ratio,
                                    // 有新文章才走动沉寂计时钟；升级旧文不算。
                                    (inserted > 0).then(|| now.as_str()),
                                ) {
                                    ok = false;
                                    note_failure(&shared, &feed.name, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        ok = false;
                        note_failure(&shared, &feed.name, e);
                    }
                }
                if ok {
                    (shared.lock().unwrap_or_else(|e| e.into_inner())).fetched_feeds += 1;
                }
                let done = done_feeds.load(std::sync::atomic::Ordering::SeqCst);
                let articles_so_far = shared.lock().unwrap_or_else(|e| e.into_inner()).added_or_updated;
                report(
                    "download",
                    done,
                    download_total,
                    format!(
                        "已完成 {done}/{download_total} 源 · 新增 {articles_so_far} 篇：{}",
                        feed.name
                    ),
                    ((done as u16 * download_weight as u16) / download_total.max(1) as u16) as u8,
                    articles_so_far,
                );
            });
        }
    });
    drop(shared);

    let cancelled = refresh_cancelled();
    clear_refresh_cancel();
    if cancelled {
        result.errors.push("已取消刷新，已保留新增内容".into());
    }

    // 沉寂源退场：滚动 30 天无新文章的启用源。放在下载全部完成后，
    // 让本轮有产出的源先把计时钟走掉；取消的刷新不判沉寂——没跑完的
    // 源会被冤枉。
    if !cancelled {
        match db.lock_write() {
            Ok(conn) => match db::prune_stale_feeds(&conn, STALE_FEED_DAYS) {
                Ok(names) if !names.is_empty() => {
                    result.pruned_stale_feeds = names.len();
                    result.errors.push(format!(
                        "已移除 {} 个超 30 天无新文章的源：{}",
                        names.len(),
                        names.join("、")
                    ));
                }
                Ok(_) => {}
                Err(e) => result.errors.push(format!("沉寂源清理: {e}")),
            },
            Err(e) => result.errors.push(format!("沉寂源清理: {e}")),
        }
    }

    let articles_downloaded = result.added_or_updated;
    // Enrich runs only when there is budget left and no cancel: downloads are
    // the point of a refresh, blurbs catch up over later runs.
    let enrich_allowed = !cancelled && started_at.elapsed() < ENRICH_BUDGET;
    if !enrich_allowed && !cancelled {
        result.errors.push("补简介跳过：刷新耗时超预算，下次自动补".into());
    }
    if enrich_allowed {
    match fill_missing_card_zh(db, cfg, CARDS_PER_REFRESH, |done, total| {
        let translate_pct = if total == 0 {
            translate_weight
        } else {
            ((done as u16 * translate_weight as u16) / total.max(1) as u16) as u8
        };
        report(
            "translate",
            done,
            total,
            if total == 0 {
                "标题与简介完成".into()
            } else {
                format!("正在补简介 {done}/{total}")
            },
            download_weight.saturating_add(translate_pct).min(99),
            articles_downloaded,
        );
    }) {
        Ok(n) => result.titles_translated = n,
        Err(e) => result.errors.push(format!("标题/简介: {e}")),
    }
    }

    report(
        "done",
        download_total,
        download_total,
        if cancelled {
            format!("已取消 · 已保留新增 {} 篇", result.added_or_updated)
        } else {
            format!("刷新完成 · 新增 {} 篇", result.added_or_updated)
        },
        100,
        result.added_or_updated,
    );
    drop(progress_tx);
    let _ = pump.join();

    Ok(result)
}

/// Download + parse one feed without holding the DB lock.
/// Skips entries whose URL is already in `known_urls` (incremental / idempotent).
/// `trust_chars` is the per-feed RSS full-text trust bar (see [`rss_trust_chars`]).
fn download_feed_articles(
    client: &Client,
    feed: &FeedSource,
    trust_chars: usize,
    known_urls: &std::sync::Mutex<HashSet<String>>,
    known_lengths: &HashMap<String, usize>,
    upgraded: &std::sync::Mutex<HashSet<String>>,
    retention_days: u32,
) -> Result<FeedDownload, AppError> {
    let mut stats = DownloadStats::default();
    ensure_public_http_url(&feed.url)?;
    let request = client.get(&feed.url);
    let request = if feed.etag.is_empty() {
        request
    } else {
        request.header(reqwest::header::IF_NONE_MATCH, feed.etag.as_str())
    };
    let resp = request.send()?;
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FeedDownload {
            articles: vec![],
            updates: vec![],
            stats,
            etag: None,
            unchanged: true,
        });
    }
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = read_limited_bytes(resp.error_for_status()?)?;

    let parsed = parser::parse(&bytes[..]).map_err(|e| AppError::msg(e.to_string()))?;
    let feed_language = parsed.language.clone();
    let mut articles = Vec::new();
    let mut updates = Vec::new();
    let now = Utc::now().to_rfc3339();

    let mut page_fetches = 0usize;
    const MAX_PAGE_FETCHES: usize = 12;

    for entry in parsed.entries.into_iter().take(40) {
        let url = entry
            .links
            .iter()
            .find(|l| {
                l.rel.as_deref() == Some("alternate")
                    || l.media_type.as_deref() == Some("text/html")
            })
            .or_else(|| entry.links.first())
            .map(|l| l.href.clone())
            .unwrap_or_else(|| entry.id.clone());
        if url.is_empty() {
            continue;
        }
        let url = canonical_article_url(&url);

        let title = entry
            .title
            .map(|t| t.content)
            .unwrap_or_else(|| "Untitled".into());

        // Retention: never ingest entries older than the configured window
        // (otherwise the purge/reseed loop re-adds them every refresh).
        let published = entry.published.or(entry.updated);
        if retention_days > 0 {
            if let Some(published_at) = published {
                let cutoff =
                    Utc::now() - chrono::Duration::days(retention_days as i64);
                if published_at < cutoff {
                    stats.skipped_old += 1;
                    continue;
                }
            }
        }

        let raw_html = entry
            .content
            .and_then(|c| c.body)
            .or_else(|| entry.summary.map(|s| s.content))
            .unwrap_or_default();

        // Clean before anything measures it: the appended link-reference block
        // is page furniture, and counting it as prose let a 250-word story with a
        // 150-word definition wall pass the ingest bar. reflow() cleans again on
        // display, which is idempotent.
        let raw_rss = html_to_text(&raw_html);
        let rss_text = crate::reflow::clean_body(&raw_rss);
        // Cleaning removes a standalone "Continue reading" line, which is the
        // very evidence the truncation gate reads — so that one check runs on
        // the body as it came from the feed.
        let rss_trusted =
            !looks_truncated(&raw_rss) && rss_is_full_text(&rss_text, trust_chars);

        // Already downloaded — only upgrade when the RSS body itself is now
        // trusted full-text AND meaningfully longer than what we stored.
        // Never page-fetch known URLs again (budget preserved for new ones).
        // The `known_urls` set is read under a short lock; the guard is
        // dropped before any network I/O in the page-fetch branch below.
        let is_known = known_urls
            .lock()
            .map_err(|_| "known urls poisoned")?
            .contains(&url);
        if is_known {
            let stored_len = known_lengths.get(&url).copied().unwrap_or(0);
            stats.evaluated += 1;
            if rss_trusted && rss_text.chars().count() > stored_len {
                // Two parallel workers can see the same URL from different
                // feeds; only the first upgrade wins.
                let mut upgraded = upgraded.lock().map_err(|_| "upgraded set poisoned")?;
                if !upgraded.insert(url.clone()) {
                    stats.skipped_existing += 1;
                    continue;
                }
                drop(upgraded);
                stats.rss_fulltext_hits += 1;
                updates.push(fulltext_article(
                    String::new(), // not used by refresh_article_content
                    url.clone(),
                    title,
                    feed.name.clone(),
                    feed.category.clone(),
                    published.map(|d| d.to_rfc3339()),
                    rss_text,
                    now.clone(),
                    "rss",
                ));
            } else {
                stats.skipped_existing += 1;
            }
            continue;
        }

        // Trusted RSS needs no page fetch; teaser / chrome / tag-wall /
        // truncated / too-thin bodies do. If the page is also junk, skip. Both
        // branches are already past [`MIN_ARTICLE_WORDS`] — that bar lives
        // inside the gate, so there is no second copy here.
        let content_text = if rss_trusted {
            stats.evaluated += 1;
            stats.rss_fulltext_hits += 1;
            rss_text
        } else {
            stats.evaluated += 1;
            if page_fetches >= MAX_PAGE_FETCHES {
                stats.skipped_budget += 1;
                continue;
            }
            page_fetches += 1;
            let page_text = fetch_article_page(client, &url).ok();
            match choose_article_body(&rss_text, page_text.as_deref()) {
                Some(body) => body,
                None => {
                    stats.skipped_short += 1;
                    continue;
                }
            }
        };

        if looks_like_paywall(&content_text) {
            stats.skipped_short += 1;
            continue;
        }

        let language = entry.language.as_deref().or(feed_language.as_deref());
        if !is_english_article(language, &title, &content_text) {
            stats.skipped_non_english += 1;
            continue;
        }

        // Link roundups and podcast transcripts are not reading material.
        if is_blocked_content(&title, &content_text) {
            stats.skipped_blocked += 1;
            continue;
        }

        articles.push(fulltext_article(
            Uuid::new_v4().to_string(),
            url,
            title,
            feed.name.clone(),
            feed.category.clone(),
            published.map(|d| d.to_rfc3339()),
            content_text,
            now.clone(),
            "page",
        ));
    }
    Ok(FeedDownload {
        articles,
        updates,
        stats,
        etag,
        unchanged: false,
    })
}

/// Build an article whose body already passed the readability gate.
/// Stamps word count + quality so refresh never re-derives them.
#[allow(clippy::too_many_arguments)]
fn fulltext_article(
    id: String,
    url: String,
    title: String,
    source: String,
    category: String,
    published_at: Option<String>,
    content_text: String,
    fetched_at: String,
    extraction_source: &str,
) -> Article {
    let word_count = content_text.split_whitespace().count() as i64;
    Article {
        id,
        url,
        title,
        source,
        category,
        published_at,
        content_text,
        fetched_at,
        origin: "rss".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        word_count,
        quality: "fulltext".into(),
        extraction_source: extraction_source.into(),
        dwell_ms: 0,
        read_completed: false,
        liked: false,
    }
}
