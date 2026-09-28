//! One-shot coverage audit: for the feeds that are on, sample the entries the
//! library never got and report *why* the real gates rejected them.
//!
//! The refresh pipeline collapses every rejection into a handful of counters,
//! so a source that renders its text with JavaScript looks identical to a
//! source that simply publishes short posts. This module walks the same gates
//! in the same order as [`super::pipeline`] (trust-RSS → page fetch → paywall →
//! language → blocked) and keeps the reasons apart.
//!
//! It is read-only: nothing here writes articles, feed metadata, or counters.

use super::dedup::canonical_article_url;
use super::extract::extract_page;
use super::filters::{
    body_reject_reason, choose_article_body, is_blocked_content, is_english_article,
    looks_like_paywall, looks_truncated, rss_is_full_text, rss_trust_chars, PageFailure,
};
use super::extract::html_to_text;
use super::net::{ensure_public_http_url, http_client, read_limited_bytes};
use crate::db::{self, FeedSource};
use crate::error::AppError;
use chrono::Utc;
use feed_rs::parser;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

/// Entries the feed has that we did not attempt, because the sample budget for
/// that feed was already spent. Mirrors the pipeline's own page-fetch cap,
/// which drops entries for reasons that have nothing to do with the source.
const UNSAMPLED: &str = "未抽样（超出本次配额）";
/// The feed itself could not be read, so none of its entries could be judged.
const FEED_UNREADABLE: &str = "订阅源抓取失败";
const NOT_ENGLISH: &str = "非英文内容";
const BLOCKED_KIND: &str = "链接周报或播客稿";

/// Entries scanned per feed, matching the pipeline's own window.
const ENTRIES_PER_FEED: usize = 40;

#[derive(Debug, Default, Serialize)]
pub struct FeedCoverage {
    pub feed_id: String,
    pub feed_name: String,
    pub category: String,
    pub url: String,
    /// Full-text ratio the pipeline last observed for this feed (-1 = unknown).
    pub fulltext_ratio: f64,
    /// Entries published inside the retention window.
    pub entries_in_window: usize,
    /// Already in the library — not a loss.
    pub already_stored: usize,
    /// Would be ingested right now: full-text RSS, or a page that passes.
    pub recoverable: usize,
    /// Reason label → count, for the entries genuinely missing from the library.
    pub drops: BTreeMap<String, usize>,
    /// Article pages actually fetched (bounds request volume per source).
    pub pages_sampled: usize,
    /// Example URLs per reason, so a verdict can be checked by hand.
    pub examples: BTreeMap<String, Vec<String>>,
}

impl FeedCoverage {
    fn note(&mut self, label: &str, url: &str) {
        *self.drops.entry(label.to_string()).or_insert(0) += 1;
        let examples = self.examples.entry(label.to_string()).or_default();
        if examples.len() < 2 && !url.is_empty() {
            examples.push(url.to_string());
        }
    }
}

/// Audit every enabled feed, fetching at most `pages_per_feed` article pages
/// each. `retention_days` mirrors the setting the pipeline applies.
pub fn audit_coverage(
    conn: &Connection,
    pages_per_feed: usize,
    retention_days: u32,
) -> Result<Vec<FeedCoverage>, AppError> {
    let feeds: Vec<FeedSource> = db::list_feeds(conn)?
        .into_iter()
        .filter(|f| f.enabled)
        .collect();
    let known: HashSet<String> = db::list_article_urls(conn)?
        .into_iter()
        .map(|u| canonical_article_url(&u))
        .collect();

    let mut report = Vec::with_capacity(feeds.len());
    for feed in &feeds {
        report.push(audit_one_feed(feed, &known, pages_per_feed, retention_days));
    }
    Ok(report)
}

fn audit_one_feed(
    feed: &FeedSource,
    known: &HashSet<String>,
    pages_per_feed: usize,
    retention_days: u32,
) -> FeedCoverage {
    let mut cov = FeedCoverage {
        feed_id: feed.id.clone(),
        feed_name: feed.name.clone(),
        category: feed.category.clone(),
        url: feed.url.clone(),
        fulltext_ratio: feed.fulltext_ratio,
        ..Default::default()
    };

    let parsed = match feed_document(&feed.url) {
        Ok(parsed) => parsed,
        Err(_) => {
            cov.note(FEED_UNREADABLE, "");
            return cov;
        }
    };

    let trust_chars = rss_trust_chars(feed.fulltext_ratio);
    let cutoff = if retention_days > 0 {
        Some(Utc::now() - chrono::Duration::days(retention_days as i64))
    } else {
        None
    };
    let mut sampled = 0usize;
    let http = match http_client() {
        Ok(http) => http,
        Err(_) => {
            cov.note(FEED_UNREADABLE, "");
            return cov;
        }
    };

    for entry in parsed.entries.into_iter().take(ENTRIES_PER_FEED) {
        let raw_url = entry_url(&entry);
        if raw_url.is_empty() {
            continue;
        }
        let url = canonical_article_url(&raw_url);
        let title = entry.title.map(|t| t.content).unwrap_or_else(|| "Untitled".into());

        // Outside the retention window the pipeline would not ingest it either,
        // so it is not a coverage loss.
        if let Some(cutoff) = cutoff {
            if let Some(published) = entry.published.or(entry.updated) {
                if published < cutoff {
                    continue;
                }
            }
        }
        cov.entries_in_window += 1;

        if known.contains(&url) {
            cov.already_stored += 1;
            continue;
        }

        let raw_html = entry
            .content
            .and_then(|c| c.body)
            .or_else(|| entry.summary.map(|s| s.content))
            .unwrap_or_default();
        // Mirrors the refresh: same clean, same truncation check on the raw body.
        let raw_rss = html_to_text(&raw_html);
        let rss_text = crate::reflow::clean_body(&raw_rss);
        let rss_trusted =
            !looks_truncated(&raw_rss) && rss_is_full_text(&rss_text, trust_chars);

        // Same bar as the refresh: a full-text feed needs no page fetch. Both
        // routes converge on the gates below, exactly as the pipeline does —
        // the language and blocked-content bars apply to a trusted RSS body too.
        let body = if rss_trusted {
            rss_text
        } else {
            if sampled >= pages_per_feed {
                cov.note(UNSAMPLED, &url);
                continue;
            }
            sampled += 1;
            cov.pages_sampled += 1;

            let page_text = match extract_page(&http, &url) {
                Ok(page) => Some(page.text),
                Err(err) => {
                    cov.note(err.failure.label(), &url);
                    continue;
                }
            };

            let Some(body) = choose_article_body(&rss_text, page_text.as_deref()) else {
                // Neither the RSS body nor the page gave us a usable article.
                // Report against the page when we have one — that is the
                // difference between a client-rendered shell and a source with
                // nothing to read.
                let rejected = page_text.as_deref().unwrap_or(&rss_text);
                let failure = body_reject_reason(rejected).unwrap_or(PageFailure::TooShort);
                cov.note(failure.label(), &url);
                continue;
            };
            body
        };

        if looks_like_paywall(&body) {
            cov.note(PageFailure::Paywall.label(), &url);
            continue;
        }
        let language = entry.language.as_deref().or(parsed.language.as_deref());
        if !is_english_article(language, &title, &body) {
            cov.note(NOT_ENGLISH, &url);
            continue;
        }
        if is_blocked_content(&title, &body) {
            cov.note(BLOCKED_KIND, &url);
            continue;
        }
        cov.recoverable += 1;
    }
    cov
}

fn feed_document(url: &str) -> Result<feed_rs::model::Feed, AppError> {
    ensure_public_http_url(url)?;
    let http = http_client()?;
    let resp = http.get(url).send()?.error_for_status()?;
    let bytes = read_limited_bytes(resp)?;
    parser::parse(&bytes[..]).map_err(|e| AppError::msg(e.to_string()))
}

/// The pipeline's own link pick: an alternate/html rel, else the first link.
fn entry_url(entry: &feed_rs::model::Entry) -> String {
    entry
        .links
        .iter()
        .find(|l| {
            l.rel.as_deref() == Some("alternate") || l.media_type.as_deref() == Some("text/html")
        })
        .or_else(|| entry.links.first())
        .map(|l| l.href.clone())
        .unwrap_or_else(|| entry.id.clone())
}

/// Entries missing from the library, totalled by reason across all feeds.
pub fn rollup_drops(report: &[FeedCoverage]) -> BTreeMap<String, usize> {
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    for feed in report {
        for (label, count) in &feed.drops {
            *totals.entry(label.clone()).or_insert(0) += count;
        }
    }
    totals
}
