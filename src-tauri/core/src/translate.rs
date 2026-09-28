//! Paragraph / selection / full-article translation orchestration.
//! Commands only spawn work and emit progress.

use crate::config::AppConfig;
use crate::db::{self, DbState, TranslationRow};
use crate::error::AppError;
use crate::feeds;
use crate::vocab;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Condvar, LazyLock, Mutex};
use ts_rs::TS;

#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct TranslateProgress {
    pub article_id: String,
    pub current: usize,
    pub total: usize,
    pub scope_key: String,
    pub translated_text: String,
    pub done: bool,
}

#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct FullTranslateResult {
    pub rows: Vec<TranslationRow>,
    pub errors: Vec<String>,
}

/// FNV-1a 64: stable across process restarts and compiler versions, unlike
/// [`std::collections::hash_map::DefaultHasher`] (SipHash with a random key).
pub fn fnv1a_64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Stable across process restarts and compiler versions (unlike DefaultHasher).
pub fn stable_scope_key(text: &str) -> String {
    format!("{:x}", fnv1a_64(text))
}

/// Keys currently being translated by another thread. A second caller for the
/// same key waits instead of paying for a duplicate LLM call.
static INFLIGHT_TRANSLATIONS: LazyLock<(Mutex<HashSet<String>>, Condvar)> =
    LazyLock::new(|| (Mutex::new(HashSet::new()), Condvar::new()));

/// Released on drop (including on LLM error), waking one waiter to re-check
/// the cache and take over if the row is still missing.
struct InflightGuard {
    key: String,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        if let Ok(mut inflight) = INFLIGHT_TRANSLATIONS.0.lock() {
            inflight.remove(&self.key);
        }
        INFLIGHT_TRANSLATIONS.1.notify_all();
    }
}

/// Claim the in-flight slot for `key`, blocking while another thread holds
/// it. Callers must re-check the cache after this returns: the previous
/// holder may have filled the row (or failed, leaving it for us).
fn claim_inflight_key(key: &str) -> Result<InflightGuard, AppError> {
    let (lock, cvar) = &*INFLIGHT_TRANSLATIONS;
    let mut inflight = lock
        .lock()
        .map_err(|_| AppError::msg("翻译排队中，请稍候"))?;
    while inflight.contains(key) {
        inflight = cvar
            .wait(inflight)
            .map_err(|_| AppError::msg("翻译排队中，请稍候"))?;
    }
    inflight.insert(key.to_string());
    Ok(InflightGuard {
        key: key.to_string(),
    })
}

pub fn translate_and_cache(
    state: &DbState,
    cfg: &AppConfig,
    article_id: &str,
    scope: &str,
    scope_key: &str,
    text: &str,
) -> Result<TranslationRow, AppError> {
    let inflight_key = format!("{article_id}\u{1f}{scope}\u{1f}{scope_key}");
    {
        let conn = state.lock_read()?;
        if let Some(existing) = db::get_translation(&conn, article_id, scope, scope_key)? {
            // Validate the cached row actually matches the text we want.
            // `source_text` is stored precisely for this check.
            if existing.source_text == text {
                return Ok(existing);
            }
            // Stale entry — fall through to re-translate.
        }
    }
    // Serialize duplicate callers: the winner translates while the waiter
    // blocks in `claim_inflight_key`, then re-checks the cache below.
    let _guard = claim_inflight_key(&inflight_key)?;
    let conn = state.lock_write()?;
    if let Some(existing) = db::get_translation(&conn, article_id, scope, scope_key)? {
        if existing.source_text == text {
            return Ok(existing);
        }
    }
    drop(conn);
    let translated = vocab::translate_text(cfg, text)?;
    let conn = state.lock_write()?;
    if let Some(existing) = db::get_translation(&conn, article_id, scope, scope_key)? {
        if existing.source_text == text {
            return Ok(existing);
        }
    }
    db::save_translation(&conn, article_id, scope, scope_key, text, &translated, &cfg.model)
}

/// Persist one translated paragraph and stream its progress event.
/// Eight flat params keep both call sites readable; no struct needed.
#[allow(clippy::too_many_arguments)]
fn store_paragraph_translation(
    state: &DbState,
    article_id: &str,
    model: &str,
    paragraphs: &[String],
    index: usize,
    translated: &str,
    total: usize,
    on_progress: &mut impl FnMut(&TranslateProgress),
) -> Result<TranslationRow, AppError> {
    let scope_key = index.to_string();
    let row = {
        let conn = state.lock_write()?;
        db::save_translation(
            &conn,
            article_id,
            "paragraph",
            &scope_key,
            &paragraphs[index],
            translated,
            model,
        )?
    };
    on_progress(&TranslateProgress {
        article_id: article_id.to_string(),
        current: index + 1,
        total,
        scope_key: row.scope_key.clone(),
        translated_text: row.translated_text.clone(),
        done: false,
    });
    Ok(row)
}

pub fn translate_full_article(
    state: &DbState,
    cfg: &AppConfig,
    article_id: &str,
    mut on_progress: impl FnMut(&TranslateProgress),
) -> Result<FullTranslateResult, AppError> {
    let paragraphs = {
        let conn = state.lock_read()?;
        let article = db::get_article(&conn, article_id)?.ok_or_else(|| "article not found".to_string())?;
        feeds::split_paragraphs(&article.content_text)
    };

    let total = paragraphs.len();
    let mut out = Vec::new();
    let mut errors = Vec::new();

    for chunk_start in (0..total).step_by(8) {
        let indices: Vec<usize> = (chunk_start..(chunk_start + 8).min(total)).collect();
        let mut missing: Vec<usize> = Vec::new();
        let mut missing_texts: Vec<String> = Vec::new();
        for &i in &indices {
            let scope_key = i.to_string();
            let expected = &paragraphs[i];
            let existing = {
                let conn = state.lock_read()?;
                db::get_translation(&conn, article_id, "paragraph", &scope_key)?
            };
            match existing {
                // Reuse only if the cached source matches the current paragraph.
                Some(row) if row.source_text == *expected => out.push(row),
                _ => {
                    missing.push(i);
                    missing_texts.push(paragraphs[i].clone());
                }
            }
        }
        if missing_texts.is_empty() {
            continue;
        }
        match vocab::translate_texts(cfg, &missing_texts) {
            Ok(translated) => {
                for (i, text) in missing.iter().zip(translated.iter()) {
                    let row = store_paragraph_translation(
                        state,
                        article_id,
                        &cfg.model,
                        &paragraphs,
                        *i,
                        text,
                        total,
                        &mut on_progress,
                    )?;
                    out.push(row);
                }
            }
            Err(_) => {
                // Batch failed: retry paragraphs one by one so a single bad
                // paragraph doesn't sink its seven neighbours.
                for (i, source) in missing.iter().zip(missing_texts.iter()) {
                    match vocab::translate_text(cfg, source) {
                        Ok(text) => {
                            let row = store_paragraph_translation(
                                state,
                                article_id,
                                &cfg.model,
                                &paragraphs,
                                *i,
                                &text,
                                total,
                                &mut on_progress,
                            )?;
                            out.push(row);
                        }
                        Err(e) => errors.push(format!("段落 {} 翻译失败：{e}", i + 1)),
                    }
                }
            }
        }
    }
    on_progress(&TranslateProgress {
        article_id: article_id.to_string(),
        current: total,
        total,
        scope_key: String::new(),
        translated_text: String::new(),
        done: true,
    });
    Ok(FullTranslateResult { rows: out, errors })
}

#[cfg(test)]
mod tests {
    use super::{claim_inflight_key, stable_scope_key};

    #[test]
    fn scope_key_is_stable_and_distinct() {
        assert_eq!(stable_scope_key("hello"), stable_scope_key("hello"));
        assert_ne!(stable_scope_key("hello"), stable_scope_key("world"));
    }

    #[test]
    fn inflight_claim_blocks_until_guard_drops() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let key = format!(
            "test-claim-{}",
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let guard = claim_inflight_key(&key).expect("first claim");
        let entered = Arc::new(AtomicBool::new(false));
        let entered_clone = Arc::clone(&entered);
        let key_clone = key.clone();
        let waiter = std::thread::spawn(move || {
            let _guard = claim_inflight_key(&key_clone).expect("second claim");
            entered_clone.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !entered.load(Ordering::SeqCst),
            "second claim must wait while the guard is held"
        );
        let start = Instant::now();
        drop(guard);
        waiter.join().expect("waiter thread");
        assert!(entered.load(Ordering::SeqCst));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "waiter must wake promptly after release"
        );
    }
}
