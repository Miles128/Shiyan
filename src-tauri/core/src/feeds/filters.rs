//! Content gates: readability, English-only, blocked content, paywall,
//! and the RSS-vs-page body decision.

use super::MIN_ARTICLE_WORDS;
use std::sync::LazyLock;

/// RSS bodies at or above this length are treated as full-text feeds (no page required).
/// Shorter bodies are teasers/summaries — page fetch must succeed or the entry is skipped.
pub(crate) const TRUST_RSS_FULLTEXT_CHARS: usize = 2000;

/// Floor for a body a learner chose by hand (import by URL, or a local
/// file). Deliberately lower than [`MIN_ARTICLE_WORDS`]: a 90-word post they
/// pointed at is still theirs to read, while auto-ingest needs substance.
pub(crate) const MIN_IMPORTED_BODY_CHARS: usize = 400;

static RE_TAG_STRIP: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_FOOTNOTE_DEF: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?m)^\[\d+\]:\s+\S+\s*$").unwrap());
static RE_LIST_LINK_LINE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?m)^\s*(?:[-*]|\d+\.)\s+\[").unwrap());
static RE_KEYWORD_TAIL: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?is)(?:Keywords for this article|Filed under:|^\s*Tags:).*$").unwrap()
});
/// Titles that mark a link roundup / daily digest rather than an article.
/// Only fires on multi-word roundup patterns (never a bare "daily"/"links"),
/// so articles *about* those words are never falsely blocked.
static RE_ROUNDUP_TITLE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)\b(roundup|weekly review|weekly links|daily digest|daily briefing|link roundup|link dump)\b").unwrap()
});
/// Titles that mark a podcast / interview transcript.
static RE_TRANSCRIPT_TITLE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)\b(transcript|podcast|episode)\b").unwrap());
/// Timestamps like 12:34 or 1:02:03 — transcripts are dense with them.
static RE_TIMESTAMP: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\b\d{1,2}:\d{2}(?::\d{2})?\b").unwrap());
static RE_BARE_URL: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"https?://").unwrap());

/// Bodies ending in these markers were cut off — a teaser or a truncated
/// extract, never the whole story.
const TRUNCATION_TAIL_MARKERS: &[&str] = &[
    "continue reading",
    "read more",
    "read the rest",
    "read the full",
    "view the full",
    "full story at",
    "[…]",
    "…]",
];

/// True when the tail of a body looks cut off (feed teasers / truncated extracts).
pub(crate) fn looks_truncated(text: &str) -> bool {
    let char_count = text.chars().count();
    let skip = char_count.saturating_sub(100);
    let tail: String = text.chars().skip(skip).collect();
    let lower = tail.to_ascii_lowercase();
    TRUNCATION_TAIL_MARKERS.iter().any(|m| lower.contains(m))
}

/// Per-feed trust bar for RSS bodies, adapted by the feed's observed
/// full-text ratio from previous refreshes.
/// - `>= 0.7` (mostly full-text feeds): lower the bar, page fetches rarely pay off.
/// - `<= 0.2` (teaser-only feeds): raise the bar; short RSS bodies are junk.
/// - otherwise (or unknown, -1): the default bar.
pub(crate) fn rss_trust_chars(fulltext_ratio: f64) -> usize {
    if fulltext_ratio >= 0.7 {
        1200
    } else if (0.0..=0.2).contains(&fulltext_ratio) {
        3200
    } else {
        TRUST_RSS_FULLTEXT_CHARS
    }
}

/// Why a candidate page never became a library entry. The refresh pipeline
/// folds all of these into one "too short" counter, which makes a source that
/// needs a real browser look identical to one that is simply thin — so the
/// coverage audit needs them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageFailure {
    /// Request failed or the server refused us (non-2xx, timeout, oversized body).
    FetchFailed,
    /// Blocked behind a subscription prompt — deliberately never ingested.
    Paywall,
    /// Answered fine but yielded almost no text: a client-side-rendered shell.
    Shell,
    /// Real prose, under the minimum length.
    TooShort,
    /// Navigation, tag walls, or a link list.
    NavOrLinks,
    /// Ends on a "read more" style marker.
    Truncated,
}

impl PageFailure {
    /// Label used in the coverage report.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            PageFailure::FetchFailed => "抓取失败或被拒",
            PageFailure::Paywall => "疑似付费墙",
            PageFailure::Shell => "空壳页（正文靠脚本渲染）",
            PageFailure::TooShort => "正文过短",
            PageFailure::NavOrLinks => "导航或链接堆",
            PageFailure::Truncated => "正文被截断",
        }
    }
}

/// Visible text below this many characters is a shell, not a short article.
const SHELL_MAX_CHARS: usize = 200;

/// What is wrong with this text *apart from length*: scaffolding, a link dump,
/// a cut-off tail, or nothing that came out at all. Length is left to the
/// caller because auto-ingest and a hand-picked import use different bars.
///
/// Everything that decides "is this body usable" goes through here, so the
/// audit label and the real gate cannot disagree.
pub(crate) fn body_defect(text: &str) -> Option<PageFailure> {
    if prose_char_count(text) < SHELL_MAX_CHARS {
        return Some(PageFailure::Shell);
    }
    if looks_like_page_chrome(text) || is_link_or_nav_dump(text) {
        return Some(PageFailure::NavOrLinks);
    }
    if looks_truncated(text) {
        return Some(PageFailure::Truncated);
    }
    None
}

/// Auto-ingest verdict: `None` when the body is a real article of at least
/// [`MIN_ARTICLE_WORDS`] words.
pub(crate) fn body_reject_reason(text: &str) -> Option<PageFailure> {
    match body_defect(text) {
        Some(defect) => Some(defect),
        None if text.split_whitespace().count() < MIN_ARTICLE_WORDS => {
            Some(PageFailure::TooShort)
        }
        None => None,
    }
}

/// Auto-ingest acceptance bar, as a boolean.
pub(crate) fn is_readable_article_body(text: &str) -> bool {
    body_reject_reason(text).is_none()
}

/// An RSS body that stands as the article itself: at least `trust_chars` long
/// for this feed, and real prose. Teasers, chrome and cut-off extracts need
/// the article page instead.
pub(crate) fn rss_is_full_text(rss_text: &str, trust_chars: usize) -> bool {
    rss_text.chars().count() >= trust_chars && is_readable_article_body(rss_text)
}

fn looks_like_page_chrome(text: &str) -> bool {
    let head: String = text.chars().take(480).collect();
    let lower = head.to_ascii_lowercase();
    lower.contains("skip to main content")
        || lower.contains("skip to content")
        || lower.contains("open navigation menu")
        || lower.contains("googletagmanager.com")
}

fn is_link_or_nav_dump(text: &str) -> bool {
    let without_notes = RE_FOOTNOTE_DEF.replace_all(text, "");
    let lines: Vec<&str> = without_notes
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() < 8 {
        return false;
    }
    let link_lines = lines
        .iter()
        .filter(|line| RE_LIST_LINK_LINE.is_match(line))
        .count();
    (link_lines as f64) / (lines.len() as f64) >= 0.5
}

/// Bare links per 1000 words — pure link dumps score very high.
fn bare_link_density(text: &str) -> f64 {
    let words = text.split_whitespace().count().max(1);
    RE_BARE_URL.find_iter(text).count() as f64 / words as f64 * 1000.0
}

/// Link roundups / dailies: a title that says so, or a pure link dump.
pub(crate) fn looks_like_link_roundup(title: &str, text: &str) -> bool {
    RE_ROUNDUP_TITLE.is_match(title) || bare_link_density(text) > 80.0
}

/// Podcast / interview transcripts: titled as such, or timestamp-dense.
pub(crate) fn looks_like_transcript(title: &str, text: &str) -> bool {
    RE_TRANSCRIPT_TITLE.is_match(title) || RE_TIMESTAMP.find_iter(text).count() >= 20
}

/// Content we do not ingest: link roundups and podcast transcripts.
pub(crate) fn is_blocked_content(title: &str, text: &str) -> bool {
    looks_like_link_roundup(title, text) || looks_like_transcript(title, text)
}

fn prose_char_count(text: &str) -> usize {
    let mut s = RE_KEYWORD_TAIL.replace_all(text, "").into_owned();
    s = RE_FOOTNOTE_DEF.replace_all(&s, "").into_owned();
    s = RE_TAG_STRIP.replace_all(&s, " ").into_owned();
    s.split_whitespace().map(|w| w.chars().count()).sum()
}

pub(crate) fn looks_like_paywall(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "subscribe to continue",
        "subscription required",
        "create a free account to read",
        "sign in to read",
        "already a subscriber",
        "metered paywall",
        "for subscribers only",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// Keep English-only articles for learning. Prefer feed/entry language tags;
/// fall back to a Latin-vs-other-script heuristic when tags are missing.
pub(crate) fn is_english_article(language: Option<&str>, title: &str, content: &str) -> bool {
    if let Some(tag) = language {
        if !is_english_lang_tag(tag) {
            return false;
        }
    }
    looks_like_english(title, content)
}

pub(crate) fn is_english_lang_tag(tag: &str) -> bool {
    let t = tag.trim().to_ascii_lowercase();
    t == "en" || t.starts_with("en-") || t.starts_with("en_")
}

fn looks_like_english(title: &str, content: &str) -> bool {
    let sample: String = title
        .chars()
        .chain(std::iter::once(' '))
        .chain(content.chars().take(1200))
        .collect();

    let mut letters = 0usize;
    let mut latin = 0usize;
    let mut non_latin = 0usize;

    for ch in sample.chars() {
        if !ch.is_alphabetic() {
            continue;
        }
        letters += 1;
        if ch.is_ascii_alphabetic() {
            latin += 1;
        } else {
            non_latin += 1;
        }
    }

    // Too little signal — keep (length filter already applied).
    if letters < 40 {
        return true;
    }

    // Obvious non-English scripts (CJK, Cyrillic, Arabic, etc.).
    if (non_latin as f64) / (letters as f64) > 0.12 {
        return false;
    }

    (latin as f64) / (letters as f64) >= 0.85
}

/// Decide final article body from RSS text and an optional page extract.
///
/// - RSS that clears this feed's own trust bar (see [`rss_trust_chars`]) is
///   trusted as full-text and no page is needed.
/// - Otherwise only accept a page extract that is real prose. Never keep chrome
///   just because it is long.
///
/// The bar comes from the caller on purpose: a teaser-only feed is held to a
/// higher one, and a body that clears the generic 2000-character line but not
/// the feed's own would otherwise throw the page we just fetched away.
pub(crate) fn choose_article_body(rss_text: &str, page_text: Option<&str>) -> Option<String> {
    match page_text {
        Some(page) if is_readable_article_body(page) => Some(page.to_string()),
        // No usable page: a body that clears the ingest bar on its own is still
        // worth reading, so a blocked page must not throw away a long RSS body.
        _ if is_readable_article_body(rss_text) => Some(rss_text.to_string()),
        _ => None,
    }
}

/// Test-only: the shape of the refresh decision with the default trust bar.
#[cfg(test)]
pub(crate) fn is_summary_only_body(content: &str) -> bool {
    choose_article_body(content, None).is_none()
}
