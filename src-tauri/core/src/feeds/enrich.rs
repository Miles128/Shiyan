//! LLM enrichment: backfill translated cards (summary_zh).

use crate::config::AppConfig;
use crate::db::{self, DbState};
use crate::error::AppError;
use crate::vocab;
use rusqlite::Connection;

/// Articles per batch LLM request. Big enough to amortise the per-request
/// overhead; small enough that a retry split stays cheap.
const CHUNK: usize = 16;
/// Cards (Chinese summary) processed per refresh. Bounded so a large backlog
/// can never turn one refresh into an unbounded job — but high enough that a
/// backlog drains in a couple of runs instead of dozens.
pub const CARDS_PER_REFRESH: usize = 200;

/// Run `job` over `items`, halving a batch whenever the model fails to answer
/// it (transport error, unparseable JSON, wrong item count). Models routinely
/// drop or merge one item out of sixteen; without the split that single bad
/// row would silently take its 15 good neighbours down with it.
///
/// Returns one slot per input, in input order (`None` = no usable row), plus
/// the last error seen.
fn run_with_split<T, O>(
    items: &[T],
    job: &mut impl FnMut(&[T]) -> Result<Vec<O>, AppError>,
) -> (Vec<Option<O>>, Option<String>) {
    let mut out = Vec::with_capacity(items.len());
    let mut last_err = None;
    fill_slots(items, job, &mut out, &mut last_err);
    (out, last_err)
}

fn fill_slots<T, O>(
    items: &[T],
    job: &mut impl FnMut(&[T]) -> Result<Vec<O>, AppError>,
    out: &mut Vec<Option<O>>,
    last_err: &mut Option<String>,
) {
    if items.is_empty() {
        return;
    }
    let failure = match job(items) {
        Ok(rows) if rows.len() == items.len() => {
            out.extend(rows.into_iter().map(Some));
            return;
        }
        Ok(rows) => format!("返回 {} 条，期望 {} 条", rows.len(), items.len()),
        Err(e) => e.to_string(),
    };
    if items.len() == 1 {
        *last_err = Some(failure);
        out.push(None);
        return;
    }
    let mid = items.len() / 2;
    fill_slots(&items[..mid], job, out, last_err);
    fill_slots(&items[mid..], job, out, last_err);
}

/// Shared driver for the two LLM backfills below. It batches the work, halves a
/// batch when the model answers badly (see [`run_with_split`]), writes back each
/// surviving row, reports progress, and gives up as soon as a whole batch comes
/// back empty — that means the provider is down or misconfigured, so burning
/// split-retries on the remaining chunks is pointless.
///
/// `write_back` reports whether it actually stored anything; that count is the
/// job's return value.
fn run_backfill<O>(
    db: &DbState,
    cfg: &AppConfig,
    label: &str,
    missing: Vec<db::Article>,
    mut call: impl FnMut(&[vocab::ArticleCardIn]) -> Result<Vec<O>, AppError>,
    mut write_back: impl FnMut(&Connection, &db::Article, O) -> Result<bool, AppError>,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<usize, AppError> {
    // Fail fast instead of splitting every batch against a dead configuration.
    if cfg.api_key.trim().is_empty() {
        return Err(AppError::msg(
            "请先在设置中配置 API Key（config.local.json）",
        ));
    }
    let total = missing.len();
    if total == 0 {
        on_progress(0, 0);
        return Ok(0);
    }
    // `done` counts rows actually written; `processed` drives progress so a
    // split-retrying batch still moves the bar.
    let mut done = 0usize;
    let mut processed = 0usize;
    let mut last_err: Option<String> = None;
    on_progress(0, total);

    for chunk in missing.chunks(CHUNK) {
        let cards: Vec<vocab::ArticleCardIn> = chunk
            .iter()
            .map(|a| vocab::card_from_article(&a.title, &a.content_text))
            .collect();
        let (rows, err) = run_with_split(&cards, &mut call);
        let usable = rows.iter().filter(|row| row.is_some()).count();
        if let Some(e) = err {
            last_err = Some(format!(
                "{label}（第 {}–{} 条）：{e}",
                processed + 1,
                processed + cards.len()
            ));
        }
        {
            let conn = db.lock_write()?;
            for (article, row) in chunk.iter().zip(rows) {
                let Some(out) = row else { continue };
                if write_back(&conn, article, out)? {
                    done += 1;
                }
            }
        }
        processed += cards.len();
        on_progress(processed, total);
        if usable == 0 {
            return Err(AppError::msg(
                last_err.unwrap_or_else(|| format!("{label}生成失败")),
            ));
        }
    }
    Ok(done)
}

pub fn fill_missing_card_zh(
    db: &DbState,
    cfg: &AppConfig,
    limit: usize,
    on_progress: impl FnMut(usize, usize),
) -> Result<usize, AppError> {
    let missing = {
        let conn = db.lock_read()?;
        db::articles_missing_card_zh(&conn, limit)?
    };
    run_backfill(
        db,
        cfg,
        "简介",
        missing,
        |batch: &[vocab::ArticleCardIn]| vocab::translate_article_cards(cfg, batch),
        |conn: &Connection, article: &db::Article, card: vocab::ArticleCardOut| {
            let wrote_summary =
                article.summary_zh.is_empty() && !card.summary_zh.is_empty();
            if wrote_summary {
                db::set_article_summary_zh(conn, &article.id, &card.summary_zh)?;
            }
            Ok(wrote_summary)
        },
        on_progress,
    )
}

/// Translate + persist the summary card for a single article (used right
/// after a URL import).
pub fn fill_article_card_zh(
    db: &DbState,
    cfg: &AppConfig,
    article: &mut crate::db::Article,
) -> Result<(), AppError> {
    if !article.summary_zh.is_empty() {
        return Ok(());
    }
    if cfg.api_key.trim().is_empty() {
        return Ok(());
    }
    let cards = [vocab::card_from_article(&article.title, &article.content_text)];
    let translated = vocab::translate_article_cards(cfg, &cards)?;
    let Some(card) = translated.into_iter().next() else {
        return Ok(());
    };
    let conn = db.lock_write()?;
    if article.summary_zh.is_empty() && !card.summary_zh.is_empty() {
        db::set_article_summary_zh(&conn, &article.id, &card.summary_zh)?;
        article.summary_zh = card.summary_zh;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake batch model: answers a batch only when it is at most `max_batch`
    /// items, so bigger batches must be split to get a result.
    fn size_limited(max_batch: usize) -> impl FnMut(&[String]) -> Result<Vec<String>, AppError> {
        move |batch: &[String]| {
            if batch.len() > max_batch {
                return Err(AppError::msg("返回条数不符"));
            }
            Ok(batch.iter().map(|s| s.to_uppercase()).collect())
        }
    }

    #[test]
    fn split_keeps_every_item_when_whole_batch_answers() {
        let items: Vec<String> = (0..16).map(|i| i.to_string()).collect();
        let mut job = size_limited(16);
        let (rows, err) = run_with_split(&items, &mut job);
        assert!(err.is_none());
        assert_eq!(rows.len(), 16);
        assert!(rows.iter().all(|r| r.is_some()));
    }

    #[test]
    fn split_recovers_items_from_a_short_batch_answer() {
        // The model merges items and answers only up to 4 at a time: the code
        // must split down until every item has its own slot, in input order.
        let items: Vec<String> = (0..16).map(|i| i.to_string()).collect();
        let mut job = size_limited(4);
        let (rows, err) = run_with_split(&items, &mut job);
        assert!(err.is_none(), "split should have found a working batch size");
        assert_eq!(rows.len(), 16);
        let got: Vec<String> = rows.into_iter().map(|r| r.expect("slot")).collect();
        assert_eq!(got, (0..16).map(|i| i.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn split_gives_up_on_a_single_bad_item_and_records_the_error() {
        let items: Vec<String> = (0..4).map(|i| i.to_string()).collect();
        let mut job = |batch: &[String]| -> Result<Vec<String>, AppError> {
            if batch.iter().any(|s| s == "2") {
                return Err(AppError::msg("model refused"));
            }
            Ok(batch.to_vec())
        };
        let (rows, err) = run_with_split(&items, &mut job);
        assert_eq!(rows.iter().filter(|r| r.is_some()).count(), 3);
        assert!(rows[2].is_none(), "the bad item keeps its own slot");
        assert_eq!(rows[0].as_deref(), Some("0"));
        assert!(err.unwrap().contains("model refused"));
    }

    #[test]
    fn split_reports_when_every_batch_fails() {
        let items: Vec<String> = (0..8).map(|i| i.to_string()).collect();
        let mut job = |_: &[String]| -> Result<Vec<String>, AppError> {
            Err(AppError::msg("provider down"))
        };
        let (rows, err) = run_with_split(&items, &mut job);
        assert_eq!(rows.len(), 8);
        assert!(rows.iter().all(|r| r.is_none()));
        assert!(err.unwrap().contains("provider down"));
    }

    #[test]
    fn split_handles_a_length_mismatch_without_an_error() {
        // Ok(rows) with the wrong length must split too (a model can answer
        // JSON that parses but covers the wrong number of items).
        let items: Vec<String> = (0..4).map(|i| i.to_string()).collect();
        let mut job = |batch: &[String]| -> Result<Vec<String>, AppError> {
            if batch.len() > 2 {
                return Ok(vec!["only one".to_string()]);
            }
            Ok(batch.to_vec())
        };
        let (rows, err) = run_with_split(&items, &mut job);
        assert!(err.is_none());
        assert_eq!(rows.iter().filter(|r| r.is_some()).count(), 4);
    }
}
