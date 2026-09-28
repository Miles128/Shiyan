use crate::error::AppError;
use crate::config::AppConfig;
use crate::db::{self, DbState, MemoryItem};
use chrono::Utc;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::LazyLock;
use ts_rs::TS;
use uuid::Uuid;

/// Shared LLM HTTP client — one connection pool instead of a new client per call.
static CHAT_CLIENT: LazyLock<Result<Client, String>> = LazyLock::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
});

fn chat_client() -> Result<Client, AppError> {
    CHAT_CLIENT
        .as_ref()
        .cloned()
        .map_err(|e| AppError::msg(format!("LLM 客户端初始化失败：{e}")))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VocabEnrichment {
    pub definition_zh: String,
    pub word_type: String,
    pub collocations: Vec<String>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

pub fn translate_text(cfg: &AppConfig, text: &str) -> Result<String, AppError> {
    ensure_configured(cfg)?;
    let system = "You are a precise English-to-Simplified-Chinese translator for language learners. Translate faithfully. Output ONLY the Chinese translation, no quotes or commentary.";
    let user = format!("Translate to Simplified Chinese:\n\n{text}");
    chat(cfg, system, &user)
}

fn strip_fences(raw: &str) -> &str {
    raw.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

const JSON_REMINDER: &str =
    "\n\nREMINDER: Reply with ONLY valid JSON in the exact shape requested. No markdown fences, no commentary.";

/// Chat → parse, with one stricter-instruction retry when the answer will not
/// parse. The two callers below differ only in the parse step: the provider's
/// JSON-object mode makes models wrap a requested array in an object, so array
/// jobs must unwrap a layer a plain object read does not.
fn chat_parsed<T>(
    cfg: &AppConfig,
    system: &str,
    user: &str,
    label: &str,
    parse: impl Fn(&str) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let raw = chat_json_mode(cfg, system, user)?;
    match parse(&raw) {
        Ok(v) => Ok(v),
        Err(first_err) => {
            let first_err = first_err.to_string();
            let raw2 =
                chat_json_mode(cfg, system, &format!("{user}{JSON_REMINDER}")).map_err(|e| {
                    AppError::msg(format!(
                        "{label}: first attempt failed to parse ({first_err}); retry request failed: {e}"
                    ))
                })?;
            parse(&raw2).map_err(|e| {
                AppError::msg(format!("parse {label}: {first_err}; retry also failed: {e}"))
            })
        }
    }
}

/// Chat → parse one JSON object. Uses the provider's JSON output mode so
/// responses are parseable in one shot; the stricter-instruction retry remains
/// as a fallback for providers without it.
fn chat_json<T: for<'de> Deserialize<'de>>(
    cfg: &AppConfig,
    system: &str,
    user: &str,
    label: &str,
) -> Result<T, AppError> {
    chat_parsed(cfg, system, user, label, |raw| {
        serde_json::from_str(strip_fences(raw)).map_err(|e| AppError::msg(e.to_string()))
    })
}

/// Parse an array-shaped answer. The provider's JSON-object output mode makes
/// models wrap the requested array in an object (`{"items":[...]}`) even when
/// the prompt asks for "ONLY a JSON array"; accept both shapes so a whole
/// batch is never thrown away over the wrapper.
pub fn parse_json_array<T: for<'de> Deserialize<'de>>(raw: &str) -> Result<Vec<T>, AppError> {
    let value: serde_json::Value = serde_json::from_str(strip_fences(raw))?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(map) => {
            const WRAPPER_KEYS: [&str; 6] =
                ["items", "results", "data", "cards", "list", "array"];
            let wrapped = WRAPPER_KEYS
                .iter()
                .find_map(|key| map.get(*key).filter(|v| v.is_array()))
                .or_else(|| {
                    // Fall back to the only array value in the object.
                    let mut arrays = map.values().filter(|v| v.is_array());
                    match (arrays.next(), arrays.next()) {
                        (Some(only), None) => Some(only),
                        _ => None,
                    }
                });
            match wrapped {
                Some(serde_json::Value::Array(items)) => items.clone(),
                _ => {
                    return Err(AppError::msg(
                        "expected a JSON array (or an object wrapping one)",
                    ))
                }
            }
        }
        other => {
            let kind = match other {
                serde_json::Value::Null => "null",
                serde_json::Value::Bool(_) => "a boolean",
                serde_json::Value::Number(_) => "a number",
                serde_json::Value::String(_) => "a string",
                _ => "another value",
            };
            return Err(AppError::msg(format!("expected a JSON array, got {kind}")));
        }
    };
    serde_json::from_value(serde_json::Value::Array(items)).map_err(AppError::from)
}

/// [`chat_json`] for array-shaped answers — same retry, tolerant of the
/// object wrapper described in [`parse_json_array`].
fn chat_json_array<T: for<'de> Deserialize<'de>>(
    cfg: &AppConfig,
    system: &str,
    user: &str,
    label: &str,
) -> Result<Vec<T>, AppError> {
    chat_parsed(cfg, system, user, label, |raw| parse_json_array::<T>(raw))
}

/// Translate article paragraphs in batch. Input order must match output order.
pub fn translate_texts(cfg: &AppConfig, texts: &[String]) -> Result<Vec<String>, AppError> {
    if texts.is_empty() {
        return Ok(vec![]);
    }
    ensure_configured(cfg)?;
    let system = r#"You translate English passages to Simplified Chinese for language learners.
Given a JSON array of English passages, return ONLY a JSON array of Chinese translations in the same order and length.
Translate faithfully. No markdown fences, no commentary."#;
    let payload = serde_json::to_string(texts)?;
    let out: Vec<String> = chat_json_array(cfg, system, &payload, "paragraph translations")?;
    if out.len() != texts.len() {
        return Err(AppError::msg(format!(
            "paragraph translation count mismatch: got {} expected {}",
            out.len(),
            texts.len()
        )));
    }
    Ok(out)
}

pub const CARD_SUMMARY_MAX_CHARS: usize = 60;
const CARD_EXCERPT_CHARS: usize = 200;

/// Truncate to at most `max_chars` Unicode scalars, then trim.
pub fn clip_zh(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect::<String>().trim().to_string()
}

#[derive(Serialize)]
pub struct ArticleCardIn {
    pub title: String,
    pub excerpt: String,
}

#[derive(Deserialize, Default)]
pub struct ArticleCardOut {
    #[serde(default)]
    pub summary_zh: String,
}

pub fn card_from_article(title: &str, content_text: &str) -> ArticleCardIn {
    ArticleCardIn {
        title: title.to_string(),
        excerpt: clip_zh(content_text, CARD_EXCERPT_CHARS),
    }
}

/// Batch: one-sentence Chinese synopsis, in one request.
/// Titles are intentionally left in English — no title tokens are spent.
/// Input order = output order.
pub fn translate_article_cards(
    cfg: &AppConfig,
    cards: &[ArticleCardIn],
) -> Result<Vec<ArticleCardOut>, AppError> {
    if cards.is_empty() {
        return Ok(vec![]);
    }
    ensure_configured(cfg)?;
    let system = r#"You write Simplified Chinese metadata for English articles for language learners.
Given a JSON array of objects {title, excerpt}, return ONLY a JSON array of the same length.
Each item must be {"summary_zh":"<Chinese synopsis>"}.
summary_zh is ONE complete Simplified Chinese sentence (about 30–60 characters) that says what the article is about. No ellipsis padding, no quotes, no English.
No markdown fences, no commentary."#;
    let payload = serde_json::to_string(cards)?;
    let out: Vec<ArticleCardOut> = chat_json_array(cfg, system, &payload, "article cards")?;
    if out.len() != cards.len() {
        return Err(AppError::msg(format!(
            "article card count mismatch: got {} expected {}",
            out.len(),
            cards.len()
        )));
    }
    Ok(out
        .into_iter()
        .map(|mut c| {
            c.summary_zh = clip_zh(&c.summary_zh, CARD_SUMMARY_MAX_CHARS);
            c
        })
        .collect())
}

pub fn enrich_vocab(cfg: &AppConfig, term: &str, context: &str) -> Result<VocabEnrichment, AppError> {
    ensure_configured(cfg)?;
    let system = r#"You help English learners. Given a word/phrase and its context sentence, return ONLY valid JSON with keys:
definition_zh (string, concise Chinese meaning),
word_type (string, e.g. noun / verb / adjective / phrase / idiom / usage),
collocations (array of 2-5 short common collocations or usage patterns in English).
No markdown fences."#;
    let user = format!("Term: {term}\nContext: {context}");
    chat_json(cfg, system, &user, "vocab JSON")
}

#[derive(serde::Deserialize)]
pub struct AddMemoryInput {
    /// "word" | "phrase" — selects the library and how enrichment runs.
    pub kind: String,
    pub term: String,
    pub context_sentence: String,
    pub article_id: Option<String>,
    pub definition_zh: Option<String>,
    pub word_type: Option<String>,
    pub collocations: Option<Vec<String>>,
}

/// Enrich (optional) and insert-or-merge a memory row (word or phrase).
/// LLM failure degrades to whatever fields the caller supplied.
/// Does this saved item still lack LLM-only fields (word_type / collocations,
/// or a definition when the popover had none)? Used to decide whether a
/// background enrichment round is worth an API call.
pub fn needs_enrichment(item: &MemoryItem) -> bool {
    if item.kind == "phrase" {
        item.definition_zh.is_empty() || item.word_type.is_empty() || item.word_type == "phrase"
    } else {
        item.definition_zh.is_empty() || item.word_type.is_empty()
    }
}

/// Enrichment payload for a just-saved memory item: dispatch to the LLM by
/// item kind. Returns `None` on any failure (e.g. no API key configured) —
/// callers treat enrichment as fire-and-forget and the row keeps what was
/// saved synchronously.
pub fn enrich_memory_fields(cfg: &AppConfig, item: &MemoryItem) -> Option<VocabEnrichment> {
    let enrichment = match item.kind.as_str() {
        "phrase" => {
            enrich_phrase(cfg, &item.term, &item.context_sentence).map(|e| VocabEnrichment {
                definition_zh: e.meaning_zh,
                word_type: if e.usage.is_empty() {
                    "phrase".to_string()
                } else {
                    e.usage
                },
                collocations: Vec::new(),
            })
        }
        _ => enrich_vocab(cfg, &item.term, &item.context_sentence),
    };
    enrichment.ok()
}

/// Fill the still-empty fields of the stored row. The row may have been
/// deleted or re-merged meanwhile; only touch it if it is still there, and
/// only fill what is still empty. Returns the updated row so the caller can
/// broadcast it, or `None` when there was nothing to update.
pub fn apply_memory_enrichment(
    conn: &rusqlite::Connection,
    item: &MemoryItem,
    enrichment: &VocabEnrichment,
) -> Result<Option<MemoryItem>, AppError> {
    let Ok(Some(mut updated)) = db::get_memory_by_term(conn, &item.kind, &item.term) else {
        return Ok(None);
    };
    if updated.definition_zh.is_empty() {
        updated.definition_zh = enrichment.definition_zh.clone();
    }
    if updated.word_type.is_empty() {
        updated.word_type = enrichment.word_type.clone();
    }
    for c in &enrichment.collocations {
        let c = c.trim();
        if !c.is_empty() && !updated.collocations.contains(&c.to_string()) {
            updated.collocations.push(c.to_string());
        }
    }
    if db::update_memory_meta(conn, &updated).is_ok() {
        Ok(Some(updated))
    } else {
        Ok(None)
    }
}

/// Save a word/phrase with whatever fields the popover already has — the
/// synchronous path never talks to the LLM, so the click returns in
/// milliseconds. Missing fields are filled in later by
/// `enrich_memory_background` (see `needs_enrichment`).
pub fn add_or_merge_memory(db: &DbState, input: AddMemoryInput) -> Result<MemoryItem, AppError> {
    // Whitespace-collapse works for both a single word and a phrase.
    let term = input.term.split_whitespace().collect::<Vec<_>>().join(" ");
    if term.is_empty() {
        return Err("词条不能为空".into());
    }
    let kind = if input.kind.trim() == "phrase" {
        "phrase"
    } else {
        "word"
    };
    let given_definition = input.definition_zh.clone().unwrap_or_default();
    let given_word_type = input.word_type.clone().unwrap_or_default();
    let collocations = input.collocations.clone().unwrap_or_default();

    let now = Utc::now().to_rfc3339();
    let conn = db.lock_write()?;

    if let Some(mut existing) = db::get_memory_by_term(&conn, kind, &term)? {
        if existing.definition_zh.is_empty() {
            existing.definition_zh = given_definition;
        }
        if existing.word_type.is_empty() || (kind == "phrase" && existing.word_type == "phrase") {
            existing.word_type = given_word_type;
        }
        for c in &collocations {
            let c = c.trim();
            if !c.is_empty() && !existing.collocations.contains(&c.to_string()) {
                existing.collocations.push(c.to_string());
            }
        }
        if existing.context_sentence.is_empty() {
            existing.context_sentence = input.context_sentence.clone();
        }
        if existing.article_id.is_none() {
            existing.article_id = input.article_id.clone();
        }
        db::update_memory_meta(&conn, &existing)?;
        return Ok(existing);
    }

    let item = MemoryItem {
        id: Uuid::new_v4().to_string(),
        kind: kind.to_string(),
        term,
        definition_zh: given_definition,
        word_type: given_word_type,
        collocations,
        context_sentence: input.context_sentence,
        article_id: input.article_id,
        status: "learning".into(),
        interval_days: 0.0,
        reps: 0,
        consecutive_know: 0,
        next_review_at: now.clone(),
        created_at: now,
    };
    db::insert_memory(&conn, &item)?;
    Ok(item)
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FeedDiscoverCandidate {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
}

/// Ask the configured LLM for free full-text English RSS feeds in a category.
pub fn discover_rss_feeds(
    cfg: &AppConfig,
    category_id: &str,
    category_label: &str,
) -> Result<Vec<FeedDiscoverCandidate>, AppError> {
    ensure_configured(cfg)?;
    let system = r#"You help curate free, publicly available English news RSS/Atom feeds for language learners.
Return ONLY a JSON array (no markdown fences) of 6–10 objects with keys:
name (string, publication name),
url (string, direct RSS or Atom feed URL, https preferred),
description (string, one short English sentence).
Prefer classic reputable outlets and blogs with free full-text or long excerpts.
Do NOT suggest podcasts, paywalled-only feeds, or non-English sources.
URLs must look like real feed endpoints (often ending in /feed, /rss, .xml)."#;
    let user = format!(
        "Category id: {category_id}\nCategory label: {category_label}\nRecommend English RSS feeds for this category."
    );
    let mut out: Vec<FeedDiscoverCandidate> =
        chat_json(cfg, system, &user, "discover JSON")?;
    out.retain(|c| {
        let u = c.url.trim();
        (u.starts_with("https://") || u.starts_with("http://")) && !c.name.trim().is_empty()
    });
    if out.is_empty() {
        return Err("模型未返回可用的 RSS 候选".into());
    }
    Ok(out)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PhraseEnrichment {
    pub meaning_zh: String,
    pub usage: String,
}

/// Chinese meaning + category for a phrase (idiom / phrasal verb / collocation).
pub fn enrich_phrase(
    cfg: &AppConfig,
    phrase: &str,
    context: &str,
) -> Result<PhraseEnrichment, AppError> {
    ensure_configured(cfg)?;
    let system = r#"You explain English phrases to Chinese learners.
Given a phrase and its context sentence, return ONLY valid JSON with keys:
meaning_zh (string, concise Simplified Chinese meaning of the phrase AS USED here),
usage (string, ONE of: idiom / phrasal-verb / collocation / fixed-expression / slang).
No markdown fences."#;
    let user = format!("Phrase: {phrase}\nContext: {context}");
    chat_json(cfg, system, &user, "phrase JSON")
}

fn ensure_configured(cfg: &AppConfig) -> Result<(), AppError> {
    if cfg.api_key.trim().is_empty() || cfg.api_key.contains("YOUR_API_KEY") {
        return Err("请先在设置中配置 API Key（config.local.json）".into());
    }
    if cfg.base_url.trim().is_empty() || cfg.model.trim().is_empty() {
        return Err("请配置 base_url 与 model".into());
    }
    Ok(())
}

/// OpenAI-compatible chat URL. Official DeepSeek / OpenAI roots need `/v1`.
pub fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return base.to_string();
    }
    let known_root = base.ends_with("api.deepseek.com") || base.ends_with("api.openai.com");
    if known_root && !base.ends_with("/v1") {
        return format!("{base}/v1/chat/completions");
    }
    format!("{base}/chat/completions")
}

fn chat(cfg: &AppConfig, system: &str, user: &str) -> Result<String, AppError> {
    chat_body(cfg, system, user, false)
}

/// Chat with the provider's JSON output mode — one-shot parseable responses,
/// no parse-failure retry round-trips (the retry stays as a fallback).
fn chat_json_mode(cfg: &AppConfig, system: &str, user: &str) -> Result<String, AppError> {
    chat_body(cfg, system, user, true)
}

fn chat_body(cfg: &AppConfig, system: &str, user: &str, json_mode: bool) -> Result<String, AppError> {
    let url = chat_completions_url(&cfg.base_url);
    let mut body = json!({
        "model": cfg.model,
        "temperature": 0.2,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    if json_mode {
        body["response_format"] = json!({"type": "json_object"});
    }

    let send = || -> Result<String, AppError> {
        let client = chat_client()?;
        let resp = client
            .post(&url)
            .bearer_auth(&cfg.api_key)
            .json(&body)
            .send()?;
        // Keep the reqwest error so `is_retryable` can see the HTTP status
        // (429 / 5xx must be retried; converting to `Msg` loses that).
        let resp = resp.error_for_status()?;
        // Bound the LLM response so a rogue endpoint can't OOM us; also
        // truncate error context so raw model output never leaks to the UI.
        let body: String = {
            let bytes = crate::feeds::net::read_limited_bytes(resp)?;
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let parsed: ChatResponse = serde_json::from_str(&body)?;
        parsed
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| AppError::msg("LLM returned empty content"))
    };
    with_retry(send, CHAT_ATTEMPTS)
}

/// Transient failures worth retrying: timeouts, connection issues, server
/// hiccups (429 / 5xx), and undecodable responses. Client mistakes (401/404…)
/// and our own messages are not.
pub(crate) fn is_retryable(err: &AppError) -> bool {
    match err {
        AppError::Json(_) => true,
        AppError::Http(e) => {
            e.is_timeout()
                || e.is_connect()
                || e.is_request()
                || e.status().is_some_and(|s| {
                    s.as_u16() == 429 || s.as_u16() >= 500
                })
        }
        _ => false,
    }
}

/// Exponential backoff between attempts: 0.6s, 1.2s, …
pub(crate) fn backoff_ms(attempt: u32) -> u64 {
    600u64 << attempt.min(4)
}

const CHAT_ATTEMPTS: u32 = 3;

/// Retry loop for transient LLM failures. A retryable error re-arms the
/// closure after a backoff sleep; anything else surfaces immediately.
pub(crate) fn with_retry<T, F>(mut attempt: F, max_attempts: u32) -> Result<T, AppError>
where
    F: FnMut() -> Result<T, AppError>,
{
    let mut last: Option<AppError> = None;
    for i in 0..max_attempts {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(e) => {
                if i + 1 < max_attempts && is_retryable(&e) {
                    last = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(backoff_ms(i)));
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| AppError::msg("retry loop ended without an error")))
}

#[cfg(test)]
mod clip_tests {
    use super::{
        backoff_ms, chat_completions_url, clip_zh, is_retryable, parse_json_array,
        translate_text, with_retry, ArticleCardOut,
    };
    use crate::error::AppError;

    fn json_err() -> AppError {
        AppError::Json(serde_json::from_str::<serde_json::Value>("not json").unwrap_err())
    }

    #[test]
    fn deepseek_root_gets_v1() {
        assert_eq!(
            chat_completions_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn retryable_matrix() {
        assert!(is_retryable(&json_err()));
        assert!(!is_retryable(&AppError::msg("请先配置 API Key")));
        assert!(!is_retryable(&AppError::Locked));
    }

    #[test]
    fn with_retry_recovers_after_transient_failure() {
        let mut calls = 0;
        let out: Result<i32, AppError> = with_retry(
            || {
                calls += 1;
                if calls == 1 {
                    Err(json_err())
                } else {
                    Ok(7)
                }
            },
            3,
        );
        assert_eq!(out.unwrap(), 7);
        assert_eq!(calls, 2);
    }

    #[test]
    fn translate_without_key_fails_before_network() {
        // Default config carries no key: every LLM entry point must refuse
        // with an actionable message instead of attempting a request.
        let cfg = crate::config::AppConfig::default();
        assert!(cfg.api_key.trim().is_empty());
        let err = translate_text(&cfg, "hello").expect_err("must not call LLM without a key");
        assert!(
            err.to_string().contains("API Key"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn with_retry_surfaces_permanent_errors_immediately() {
        let mut calls = 0;
        let out: Result<i32, AppError> = with_retry(
            || {
                calls += 1;
                Err(AppError::msg("no key"))
            },
            3,
        );
        assert!(out.unwrap_err().to_string().contains("no key"));
        assert_eq!(calls, 1);
    }

    #[test]
    fn with_retry_gives_up_after_max_attempts() {
        let mut calls = 0;
        let out: Result<i32, AppError> = with_retry(
            || {
                calls += 1;
                Err(json_err())
            },
            2,
        );
        assert!(out.is_err());
        assert_eq!(calls, 2);
    }

    #[test]
    fn backoff_is_exponential() {
        assert_eq!(backoff_ms(0), 600);
        assert_eq!(backoff_ms(1), 1200);
        assert_eq!(backoff_ms(2), 2400);
        assert_eq!(backoff_ms(20), 600 << 4, "capped");
    }


    #[test]
    fn clip_zh_counts_unicode_scalars() {
        assert_eq!(clip_zh("abcdefghij", 5), "abcde");
        assert_eq!(clip_zh("一二三四五六七八九十", 5), "一二三四五");
        assert_eq!(clip_zh("  短简介  ", 50), "短简介");
        let long: String = "字".repeat(80);
        assert_eq!(clip_zh(&long, 50).chars().count(), 50);
        let two_sentences: String = "字".repeat(160);
        assert_eq!(
            clip_zh(&two_sentences, super::CARD_SUMMARY_MAX_CHARS)
                .chars()
                .count(),
            super::CARD_SUMMARY_MAX_CHARS
        );
    }

    #[test]
    fn parse_json_array_accepts_a_bare_array() {
        let raw = r#"[{"summary_zh":"甲"},{"summary_zh":"乙"}]"#;
        let rows: Vec<ArticleCardOut> = parse_json_array(raw).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].summary_zh, "甲");
        assert_eq!(rows[1].summary_zh, "乙");
    }

    #[test]
    fn parse_json_array_unwraps_an_object_wrapper() {
        // JSON output mode pushes models to wrap the array; that must not cost
        // us the whole batch.
        for raw in [
            r#"{"items":[{"summary_zh":"甲"},{"summary_zh":"乙"}]}"#,
            r#"{"cards":[{"summary_zh":"甲"},{"summary_zh":"乙"}]}"#,
            r#"{"whatever":[{"summary_zh":"甲"},{"summary_zh":"乙"}]}"#,
        ] {
            let rows: Vec<ArticleCardOut> = parse_json_array(raw).unwrap();
            assert_eq!(rows.len(), 2, "raw: {raw}");
            assert_eq!(rows[1].summary_zh, "乙");
        }
    }

    #[test]
    fn parse_json_array_tolerates_fences_and_errors_clearly() {
        let fenced = "```json\n[\"一\",\"二\"]\n```";
        let rows: Vec<String> = parse_json_array(fenced).unwrap();
        assert_eq!(rows, vec!["一".to_string(), "二".to_string()]);

        // Two arrays in one object is ambiguous → error, never a wrong pairing.
        let ambiguous = r#"{"a":[1],"b":[2]}"#;
        assert!(parse_json_array::<u8>(ambiguous).is_err());
        assert!(parse_json_array::<u8>("{\"note\":\"no array here\"}").is_err());
        assert!(parse_json_array::<u8>("\"just a string\"").is_err());
    }
}
