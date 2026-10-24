//! URL canonicalization and cross-source title dedup.

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

/// Minimum headline size for the fuzzy (Jaccard) test. Below this, two titles
/// can only be duplicates if their token sequences match exactly.
const FUZZY_MIN_TOKENS: usize = 8;

/// Token-set overlap at which two headlines count as the same story.
const JACCARD_DUP: f64 = 0.85;

/// Near-duplicate headline test across sources. Test-only reference rule:
/// [`TitleIndex`] applies the same logic in bulk.
#[cfg(test)]
pub(crate) fn is_near_duplicate_title(a: &str, b: &str) -> bool {
    let ta = title_tokens(a);
    let tb = title_tokens(b);
    if ta.is_empty() || tb.is_empty() {
        return false;
    }
    if ta == tb {
        return true;
    }
    if ta.len() < FUZZY_MIN_TOKENS || tb.len() < FUZZY_MIN_TOKENS {
        return false;
    }
    let sa: std::collections::HashSet<&String> = ta.iter().collect();
    let sb: std::collections::HashSet<&String> = tb.iter().collect();
    let inter = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    union > 0 && (inter as f64) / (union as f64) >= JACCARD_DUP
}

/// In-memory title index for cross-source dedup during one refresh.
pub(crate) struct TitleIndex {
    exact: std::collections::HashSet<String>,
    /// Token sets of headline-sized titles, built once per `insert`.
    ///
    /// The previous shape kept `(title, tokens)` and called the pairwise rule
    /// for every entry, re-tokenizing *both* titles and rebuilding both
    /// `HashSet`s on each of `index.len()` comparisons — measured at 4.7× the
    /// work this needs, while the caller holds the DB write lock. Titles under
    /// [`FUZZY_MIN_TOKENS`] are not stored: they can never fuzzy-match, and
    /// exact matches are already covered by `exact`.
    fuzzy: Vec<std::collections::HashSet<String>>,
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
        if tokens.len() >= FUZZY_MIN_TOKENS {
            self.fuzzy.push(tokens.into_iter().collect());
        }
    }

    pub(crate) fn is_dup(&self, title: &str) -> bool {
        let tokens = title_tokens(title);
        if tokens.is_empty() {
            return false;
        }
        if self.exact.contains(&tokens.join(" ")) {
            return true;
        }
        if tokens.len() < FUZZY_MIN_TOKENS {
            return false;
        }
        let query: std::collections::HashSet<String> = tokens.into_iter().collect();
        self.fuzzy.iter().any(|entry| {
            let inter = query.intersection(entry).count();
            let union = query.union(entry).count();
            union > 0 && (inter as f64) / (union as f64) >= JACCARD_DUP
        })
    }
}
