//! URL canonicalization and cross-source title dedup.

#[cfg(test)]
use std::collections::HashSet;

/// Tracking parameters stripped by [`canonical_article_url`].
const TRACKING_PARAMS: &[&str] = &[
    "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content", "utm_id",
    "fbclid", "gclid", "gclsrc", "dclid", "msclkid", "twclid", "igshid",
    "ref", "ref_src", "ref_url", "mc_cid", "mc_eid", "yclid",
    "_hsenc", "_hsmi", "vero_id", "pk_campaign", "pk_kwd", "si",
];

/// Canonical form of an article URL for dedup: drops fragment + tracking
/// params and a trailing slash. Only http(s); other schemes pass through.
pub(crate) fn canonical_article_url(raw: &str) -> String {
    let Ok(mut u) = url::Url::parse(raw) else {
        return raw.to_string();
    };
    if u.scheme() != "http" && u.scheme() != "https" {
        return raw.to_string();
    }
    u.set_fragment(None);
    if u.query().is_some() {
        let kept: Vec<(String, String)> = u
            .query_pairs()
            .filter(|(k, _)| !TRACKING_PARAMS.contains(&k.as_ref()))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        u.set_query(None);
        if !kept.is_empty() {
            u.query_pairs_mut().extend_pairs(kept);
        }
    }
    if u.path().len() > 1 && u.path().ends_with('/') {
        let trimmed = u.path().trim_end_matches('/').to_string();
        u.set_path(&trimmed);
    }
    u.to_string()
}

/// Lowercase alphanumeric tokens of a title — the fuzzy-dedup key.
pub(crate) fn title_tokens(title: &str) -> Vec<String> {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Near-duplicate headline test across sources:
/// - exact match after normalization always counts;
/// - otherwise Jaccard ≥ 0.85 on token sets, but only for headline-sized
///   titles (≥8 tokens) so short unrelated headlines can't collide.
pub(crate) fn is_near_duplicate_title(a: &str, b: &str) -> bool {
    let ta = title_tokens(a);
    let tb = title_tokens(b);
    if ta.is_empty() || tb.is_empty() {
        return false;
    }
    if ta == tb {
        return true;
    }
    if ta.len() < 8 || tb.len() < 8 {
        return false;
    }
    let sa: std::collections::HashSet<&String> = ta.iter().collect();
    let sb: std::collections::HashSet<&String> = tb.iter().collect();
    let inter = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    union > 0 && (inter as f64) / (union as f64) >= 0.85
}

/// In-memory title index for cross-source dedup during one refresh.
pub(crate) struct TitleIndex {
    exact: std::collections::HashSet<String>,
    fuzzy: Vec<(String, Vec<String>)>,
}

impl TitleIndex {
    pub(crate) fn new(rows: Vec<(String, String)>) -> Self {
        let mut index = Self {
            exact: std::collections::HashSet::new(),
            fuzzy: Vec::new(),
        };
        for (title, _url) in rows {
            index.insert(&title);
        }
        index
    }

    pub(crate) fn insert(&mut self, title: &str) {
        let tokens = title_tokens(title);
        if tokens.is_empty() {
            return;
        }
        self.exact.insert(tokens.join(" "));
        self.fuzzy.push((title.to_string(), tokens));
    }

    pub(crate) fn is_dup(&self, title: &str) -> bool {
        let tokens = title_tokens(title);
        if tokens.is_empty() {
            return false;
        }
        if self.exact.contains(&tokens.join(" ")) {
            return true;
        }
        self.fuzzy
            .iter()
            .any(|(existing, _)| is_near_duplicate_title(title, existing))
    }
}

/// Split candidate URLs into new vs already-known. Returns (new_urls, skipped_count).
#[cfg(test)]
pub fn partition_new_urls(candidates: &[String], known: &HashSet<String>) -> (Vec<String>, usize) {
    let mut new_urls = Vec::new();
    let mut skipped = 0usize;
    for url in candidates {
        if known.contains(url) {
            skipped += 1;
        } else {
            new_urls.push(url.clone());
        }
    }
    (new_urls, skipped)
}
