//! RSS ingest pipeline for the reading library.
//!
//! Module layout (each concern gets one file):
//! - [`net`] — shared HTTP client, SSRF guard, feed-URL validation
//! - [`filters`] — readability / English-only / blocked-content / paywall gates
//! - [`extract`] — article-page fetch + HTML→text extraction
//! - [`dedup`] — URL canonicalization + cross-source title dedup
//! - [`pipeline`] — the parallel refresh flow (`refresh_feeds`)
//! - [`coverage`] — read-only audit of which sources lose articles, and why
//! - [`cleanup`] — one-time audits, retention purges, body repair
//! - [`enrich`] — LLM backfill of summaries
//! - [`import`] — import-a-single-article-by-URL path
//!
//! All public items are re-exported here so call sites keep using
//! `crate::feeds::…` unchanged.

pub mod cleanup;
/// Read-only coverage audit. Driven from tests rather than the UI, so nothing
/// in the shipped binary calls it yet.
#[allow(dead_code)]
pub mod coverage;
pub mod dedup;
pub mod enrich;
pub mod extract;
pub mod filters;
pub mod import;
pub mod net;
pub mod pipeline;

// The module's outward facade (`crate::feeds::X`): only what is used from
// outside this module. Sibling submodules reach each other by their own path
// (`super::filters::x`), so nothing else is re-exported here — that is what
// used to need a pile of `#[allow(unused_imports)]`. Items consumed by the
// command shell live in another crate now, so their re-exports are `pub`.
pub use cleanup::{
    clear_stale_paragraph_translations_once, purge_blocked_articles, repair_missing_paragraphs,
};
pub(crate) use enrich::fill_article_card_zh;
pub use enrich::{fill_missing_card_zh, CARDS_PER_REFRESH};
pub(crate) use filters::{is_english_article, MIN_IMPORTED_BODY_CHARS};
pub use import::import_article_from_url;
pub use net::{validate_feed_url, FeedValidation};
pub use pipeline::{refresh_feeds, request_refresh_cancel, RefreshProgress, RefreshResult};

/// Reached only from tests (`feeds/tests.rs` via `use super::*`, plus
/// `db_tests.rs` through `crate::feeds::`).
#[cfg(test)]
pub(crate) use cleanup::{
    audit_rss_bodies_once, collect_non_english_rss_ids, delete_articles,
    purge_expired_articles, purge_non_english_articles, purge_rss_below_word_threshold,
    purge_summary_only_articles,
};
#[cfg(test)]
pub(crate) use dedup::{
    canonical_article_url, is_near_duplicate_title, partition_new_urls, TitleIndex,
};
#[cfg(test)]
pub(crate) use extract::{html_to_text, title_from_html};
#[cfg(test)]
pub(crate) use filters::is_summary_only_body;
#[cfg(test)]
pub(crate) use import::source_from_url;
#[cfg(test)]
pub(crate) use net::ensure_public_http_url;
#[cfg(test)]
pub(crate) use pipeline::select_enabled_feeds;

/// The one auto-ingest length bar, in words: a real learning session needs
/// substance, not a blurb. Every gate that decides whether a fetched body
/// becomes a library entry uses this ([`filters::is_readable_article_body`]).
/// User imports have their own, lower floor — see
/// [`filters::MIN_IMPORTED_BODY_CHARS`].
pub(crate) const MIN_ARTICLE_WORDS: usize = 400;

pub fn split_paragraphs(text: &str) -> Vec<String> {
    crate::reflow::reflow(text)
}

#[cfg(test)]
mod tests;
