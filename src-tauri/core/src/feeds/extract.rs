//! Fetching a public article page and extracting title + main text.

use super::filters::{body_defect, looks_like_paywall, PageFailure};
use super::net::{ensure_public_http_url, read_limited_bytes};
use crate::error::AppError;
use reqwest::blocking::Client;
use std::sync::LazyLock;

static RE_TAG_STRIP: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_TRAILING_WS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[ \t]+\n").unwrap());
static RE_BLANK_RUN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\n{3,}").unwrap());
static RE_HTML_TITLE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());

pub(crate) fn html_to_text(html: &str) -> String {
    let stripped = html2text::from_read(html.as_bytes(), 100)
        .unwrap_or_else(|_| RE_TAG_STRIP.replace_all(html, " ").to_string());
    let s = RE_TRAILING_WS.replace_all(&stripped, "\n");
    let s = RE_BLANK_RUN.replace_all(&s, "\n\n");
    s.trim().to_string()
}

pub(crate) fn title_from_html(html: &str) -> Option<String> {
    let caps = RE_HTML_TITLE.captures(html)?;
    let raw = caps.get(1)?.as_str();
    let decoded = html_to_text(raw);
    let mut title = decoded.lines().next().unwrap_or("").trim().to_string();
    for sep in [" | ", " — ", " – ", " - "] {
        if let Some((left, _)) = title.split_once(sep) {
            let left = left.trim();
            if left.chars().count() >= 8 {
                title = left.to_string();
                break;
            }
        }
    }
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

pub(crate) struct ExtractedPage {
    pub title: String,
    pub text: String,
}

/// A page that never became an entry: the classified failure, plus the
/// user-facing message this path has always produced.
pub(crate) struct PageExtractError {
    pub failure: PageFailure,
    pub detail: String,
}

impl From<PageExtractError> for AppError {
    fn from(e: PageExtractError) -> AppError {
        AppError::msg(e.detail)
    }
}

/// Build a classified page failure, keeping the message this path has always
/// produced for the UI.
fn fail(failure: PageFailure, detail: impl Into<String>) -> PageExtractError {
    PageExtractError {
        failure,
        detail: detail.into(),
    }
}

/// Fetch a public article URL and extract title + main text (no paywall bypass),
/// reporting *why* a page was unusable.
pub(crate) fn extract_page(
    client: &Client,
    url: &str,
) -> Result<ExtractedPage, PageExtractError> {
    let parsed = ensure_public_http_url(url)
        .map_err(|e| fail(PageFailure::FetchFailed, e.to_string()))?;
    let response = client
        .get(url)
        .header(
            "Accept",
            "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .map_err(|e| fail(PageFailure::FetchFailed, format!("网络请求失败：{e}")))?
        .error_for_status()
        .map_err(|e| fail(PageFailure::FetchFailed, format!("网络请求失败：{e}")))?;
    let bytes = read_limited_bytes(response)
        .map_err(|e| fail(PageFailure::FetchFailed, e.to_string()))?;
    // Lossy like reqwest's old `.text()` decoding: undecodable bytes become
    // U+FFFD instead of rejecting a page that used to load.
    let html = String::from_utf8_lossy(&bytes).into_owned();

    if looks_like_paywall(&html) {
        return Err(fail(PageFailure::Paywall, "疑似付费墙，已跳过"));
    }

    let mut title = title_from_html(&html).unwrap_or_default();

    // Prefer readability extraction; fall back to html2text.
    let from_readability = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut cursor = std::io::Cursor::new(html.as_bytes());
        readability::extractor::extract(&mut cursor, &parsed).ok().map(|p| {
            // readability's `text` is a single blob that loses paragraph
            // breaks; the cleaned HTML keeps them, so prefer it.
            let from_html = html_to_text(&p.content);
            let text = if from_html.is_empty() {
                p.text.trim().to_string()
            } else {
                from_html
            };
            let page_title = p.title.trim().to_string();
            (page_title, text)
        })
    }))
    .ok()
    .flatten();

    let full_page = crate::reflow::clean_body(&html_to_text(&html));
    let text = match from_readability {
        Some((page_title, extracted)) => {
            if title.is_empty() && !page_title.is_empty() {
                title = page_title;
            }
            pick_body(crate::reflow::clean_body(&extracted), full_page)
        }
        None => full_page,
    };

    if let Some(defect) = body_defect(&text) {
        return Err(fail(
            defect,
            format!("未能抽到可用正文：{}", defect.label()),
        ));
    }

    if title.is_empty() {
        title = "Untitled".into();
    }

    Ok(ExtractedPage { title, text })
}

/// readability's output is the article proper, so it is the body we want — but
/// only when it actually brought the article home. Below the length a reading
/// session needs, the extraction has almost always grabbed an opening fragment,
/// and the page-wide text (chrome already stripped) reads better than a
/// truncated story that looks complete.
///
/// The bar is deliberately the same unit the ingest gate uses: measuring
/// acceptance in characters while keeping articles in words is what let a
/// 240-word fragment pass extraction and then get the whole article dropped.
pub(crate) fn pick_body(extracted: String, full_page: String) -> String {
    if extracted.split_whitespace().count() >= super::MIN_ARTICLE_WORDS {
        extracted
    } else {
        full_page
    }
}

/// Fetch a public article URL and extract title + main text (no paywall bypass).
pub(crate) fn extract_article_page(client: &Client, url: &str) -> Result<ExtractedPage, AppError> {
    extract_page(client, url).map_err(Into::into)
}

/// Fetch a public article URL and extract main text (no paywall bypass).
pub(crate) fn fetch_article_page(client: &Client, url: &str) -> Result<String, AppError> {
    Ok(extract_article_page(client, url)?.text)
}
