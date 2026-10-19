//! Download one Project Gutenberg plain-text book into the local library.
//!
//! The shelf itself lives in the frontend catalog. This module only accepts
//! `gutenberg.org` `.txt` URLs, strips the site header/footer, and stores the
//! body as `origin = "book"` so refresh never purges it.

use crate::db::{self, Article, DbState};
use crate::error::AppError;
use crate::feeds::{self, net};
use chrono::Utc;
use uuid::Uuid;

const MAX_BOOK_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ImportBookRequest {
    pub url: String,
    pub title: String,
    pub author: String,
    pub category: String,
    pub summary_zh: String,
    pub tags: Vec<String>,
}

pub fn import_gutenberg_book(db: &DbState, req: &ImportBookRequest) -> Result<Article, AppError> {
    let url = parse_gutenberg_url(&req.url)?;
    let url = url.to_string();
    {
        let conn = db.lock_read()?;
        if let Some(existing) = db::get_article_by_url(&conn, &url)? {
            return Ok(existing);
        }
    }

    let title = clean_field(&req.title, 180).ok_or_else(|| AppError::msg("书名不能为空"))?;
    let author = clean_field(&req.author, 120).unwrap_or_else(|| "公版书".into());
    let category = clean_id(&req.category).unwrap_or_else(|| "other".into());
    let summary_zh = clamp_chars(req.summary_zh.trim(), 200);
    let tags = clean_tags(&req.tags);

    let resp = net::HTTP
        .get(&url)
        .send()
        .map_err(|e| AppError::msg(format!("下载失败：{e}")))?
        .error_for_status()
        .map_err(|e| AppError::msg(format!("下载失败：{e}")))?;
    let bytes = read_book_bytes(resp)?;
    let raw = String::from_utf8_lossy(&bytes);
    if looks_like_html(&raw) {
        return Err(AppError::msg("下载到的不是书籍正文"));
    }
    let content_text = strip_gutenberg_boilerplate(&raw);
    if content_text.chars().count() < feeds::MIN_FULLTEXT_CHARS {
        return Err(AppError::msg("这本书的正文没有解析出来"));
    }
    if !feeds::is_english_article(Some("en"), &title, &content_text) {
        return Err(AppError::msg("看起来不是英文书"));
    }

    let article = Article {
        id: Uuid::new_v4().to_string(),
        url: url.clone(),
        title,
        source: author,
        category,
        published_at: None,
        word_count: content_text.split_whitespace().count() as i64,
        quality: "fulltext".into(),
        extraction_source: "book".into(),
        content_text,
        fetched_at: Utc::now().to_rfc3339(),
        origin: "book".into(),
        summary_zh,
        last_opened_at: None,
        open_count: 0,
        dwell_ms: 0,
        read_completed: false,
        liked: false,
        tags: tags.clone(),
    };

    {
        let conn = db.lock_write()?;
        if !db::insert_article_if_new(&conn, &article)? {
            return db::get_article_by_url(&conn, &url)?
                .ok_or_else(|| AppError::msg("这本书已经在库里，但没有读出来"));
        }
        if !tags.is_empty() {
            db::set_article_tags(&conn, &article.id, &tags)?;
        }
    }
    Ok(article)
}

pub fn parse_gutenberg_url(url: &str) -> Result<url::Url, AppError> {
    let parsed = net::ensure_public_http_url(url.trim())?;
    if parsed.scheme() != "https" {
        return Err(AppError::msg("书籍下载只接受 https"));
    }
    let host = parsed
        .host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    if host != "gutenberg.org" {
        return Err(AppError::msg("只能下载 Project Gutenberg 的公版书"));
    }
    let path = parsed.path().to_ascii_lowercase();
    if !path.ends_with(".txt") {
        return Err(AppError::msg("书籍地址必须是纯文本"));
    }
    Ok(parsed)
}

fn read_book_bytes(mut resp: reqwest::blocking::Response) -> Result<Vec<u8>, AppError> {
    if let Some(len) = resp.content_length() {
        if len > MAX_BOOK_BYTES as u64 {
            return Err(AppError::msg("这本书太大，已跳过"));
        }
    }
    let mut out = Vec::new();
    use std::io::Read;
    resp.by_ref()
        .take(MAX_BOOK_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| AppError::msg(format!("下载失败：{e}")))?;
    if out.len() > MAX_BOOK_BYTES {
        return Err(AppError::msg("这本书太大，已跳过"));
    }
    Ok(out)
}

fn looks_like_html(text: &str) -> bool {
    let head: String = text.chars().take(200).collect();
    let lower = head.trim_start().to_ascii_lowercase();
    lower.starts_with("<!doctype") || lower.starts_with("<html") || lower.starts_with("<head")
}

/// Drop the Gutenberg license header and footer. Bodies without the markers
/// are kept (some mirrors already serve bare text).
pub fn strip_gutenberg_boilerplate(raw: &str) -> String {
    let text = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let start_at = find_marker(
        &text,
        &[
            "*** START OF THE PROJECT GUTENBERG",
            "***START OF THE PROJECT GUTENBERG",
            "*** START OF THIS PROJECT GUTENBERG",
        ],
    );
    let mut body = if let Some(idx) = start_at {
        let after = &text[idx..];
        match after.find('\n') {
            Some(nl) => after[nl + 1..].trim_start_matches('\n'),
            None => "",
        }
    } else {
        text.trim()
    };
    if let Some(idx) = find_marker(
        body,
        &[
            "*** END OF THE PROJECT GUTENBERG",
            "***END OF THE PROJECT GUTENBERG",
            "*** END OF THIS PROJECT GUTENBERG",
        ],
    ) {
        body = body[..idx].trim_end();
    }
    body.trim().to_string()
}

fn find_marker(text: &str, markers: &[&str]) -> Option<usize> {
    let upper = text.to_ascii_uppercase();
    markers
        .iter()
        .filter_map(|marker| upper.find(&marker.to_ascii_uppercase()))
        .min()
}

fn clean_field(raw: &str, max_chars: usize) -> Option<String> {
    let text = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = clamp_chars(&text, max_chars);
    if text.is_empty() { None } else { Some(text) }
}

fn clean_id(raw: &str) -> Option<String> {
    let text = raw.trim().to_ascii_lowercase();
    if text.is_empty() || text.len() > 40 {
        return None;
    }
    if !text
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return None;
    }
    Some(text)
}

fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for tag in tags {
        let Some(id) = clean_id(tag) else { continue };
        if out.iter().any(|existing: &String| existing == &id) {
            continue;
        }
        out.push(id);
        if out.len() >= 8 {
            break;
        }
    }
    out
}

fn clamp_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_gutenberg_header_and_footer() {
        let raw = "\u{feff}The Project Gutenberg eBook\r\n*** START OF THE PROJECT GUTENBERG EBOOK PRIDE ***\r\n\r\nChapter 1\r\n\r\nIt is a truth.\r\n\r\n*** END OF THE PROJECT GUTENBERG EBOOK PRIDE ***\r\nlicense";
        assert_eq!(
            strip_gutenberg_boilerplate(raw),
            "Chapter 1\n\nIt is a truth."
        );
    }

    #[test]
    fn keeps_bare_text_without_markers() {
        assert_eq!(strip_gutenberg_boilerplate("  Hello world.  "), "Hello world.");
    }

    #[test]
    fn accepts_only_gutenberg_txt() {
        assert!(parse_gutenberg_url(
            "https://www.gutenberg.org/cache/epub/1342/pg1342.txt"
        )
        .is_ok());
        assert!(parse_gutenberg_url("https://example.com/pg1342.txt").is_err());
        assert!(parse_gutenberg_url("http://www.gutenberg.org/cache/epub/1342/pg1342.txt").is_err());
        assert!(parse_gutenberg_url("https://www.gutenberg.org/ebooks/1342").is_err());
    }
}
