//! Interest-based article ranking for the home list.
//!
//! Deterministic, explainable scoring — no ML runtime required:
//! freshness × affinity + explicit signals (liked / read / dwell) + a
//! bounded exploration jitter so unseen articles still surface.

use crate::db::ArticleListItem;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Aggregated open counts by source / category, plus the local semantic
/// profile (title terms from engaged articles — no LLM involved).
#[derive(Debug, Default, Clone)]
pub struct Affinity {
    pub source_opens: HashMap<String, i64>,
    pub category_opens: HashMap<String, i64>,
    /// term → engagement weight, accumulated from titles the learner engaged
    /// with (liked = 2, read-to-end = 1). Semantic profile input.
    pub term_weights: HashMap<String, f64>,
    /// feed display name → user-assigned sidebar priority within its own
    /// category (higher = dragged nearer the top of that category). Absent or 0
    /// means the learner never ordered this source.
    pub source_priority: HashMap<String, i64>,
    /// category → highest priority among that category's feeds, used to
    /// normalise the per-category bonus to [0, 1] so ordering one category does
    /// not outrank the top of another.
    pub category_priority_max: HashMap<String, i64>,
}

impl Affinity {
    pub fn from_maps(
        source_opens: HashMap<String, i64>,
        category_opens: HashMap<String, i64>,
        term_weights: HashMap<String, f64>,
    ) -> Self {
        Self {
            source_opens,
            category_opens,
            term_weights,
            source_priority: HashMap::new(),
            category_priority_max: HashMap::new(),
        }
    }

    /// Attach the sidebar priority map (by source name) plus the per-category
    /// ceiling (by category), so the bonus is normalised within a category.
    pub fn with_source_priority(
        mut self,
        source_priority: HashMap<String, i64>,
        category_priority_max: HashMap<String, i64>,
    ) -> Self {
        self.source_priority = source_priority;
        self.category_priority_max = category_priority_max;
        self
    }
}

/// Log-scaled affinity in [0, 1]; 10 opens saturate.
pub fn affinity_score(opens: i64) -> f64 {
    if opens <= 0 {
        0.0
    } else {
        ((opens + 1) as f64).ln() / 10f64.ln()
    }
}

/// English stopwords + reporting verbs: frequent in headlines but carrying no
/// topic signal. Kept small and obvious — this is a heuristic, not NLP.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "with", "as",
    "at", "by", "from", "is", "are", "was", "were", "be", "been", "it", "its",
    "that", "this", "these", "those", "will", "would", "could", "should",
    "has", "have", "had", "not", "but", "they", "their", "them", "he", "she",
    "we", "you", "s", "t", "say", "says", "said", "new", "over", "after",
    "before", "more", "most", "than", "into", "out", "up", "all", "also",
    "just", "like", "get", "amid", "among", "between", "while", "without",
    "what", "when", "how", "why", "who",
];

/// Topic-bearing terms of a headline/body: lowercase alphanumeric tokens,
/// ≥2 chars, no pure digits, no stopwords. Single set per article — repeats
/// within one article don't count twice.
pub fn content_terms(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| {
            t.len() >= 2
                && !t.chars().all(|c| c.is_ascii_digit())
                && !STOPWORDS.contains(t)
        })
        .map(str::to_owned)
        .collect()
}

/// Build the semantic profile: accumulate engagement weights per title term.
/// `engaged` is (title, weight) with liked = 2.0, read-to-end = 1.0.
pub fn build_term_weights(engaged: &[(String, f64)]) -> HashMap<String, f64> {
    let mut weights = HashMap::new();
    for (title, w) in engaged {
        for term in content_terms(title) {
            *weights.entry(term).or_insert(0.0) += w;
        }
    }
    weights
}

/// Sidebar-priority bonus in [0, 1]: the learner ranked this source within its
/// category. `max` is that category's highest priority. 0 when the source was
/// never ordered (priority absent/0) or the category ceiling is 0.
pub fn priority_score(priority: i64, max: i64) -> f64 {
    if priority <= 0 || max <= 0 {
        0.0
    } else {
        (priority as f64 / max as f64).clamp(0.0, 1.0)
    }
}

/// Exponential freshness decay: 1.0 today, ~0.24 at 30 days, floor 0.05.
pub fn freshness(age_days: f64) -> f64 {
    (-age_days.max(0.0) / 21.0).exp().max(0.05)
}

/// Body-length fit: very short and very long articles are worse reads;
/// ~300–3000 words is the sweet spot.
pub fn word_fit(word_count: i64) -> f64 {
    match word_count {
        0 => 0.0,                    // unknown, no opinion
        wc if wc < 150 => -0.6,      // likely stub/teaser that slipped through
        wc if wc <= 3000 => 0.4,     // comfortable single-sitting read
        wc if wc <= 6000 => 0.1,
        _ => -0.3,                   // heavy commitment
    }
}

/// Deterministic pseudo-random jitter in [-0.3, 0.3], re-seeded daily, so
/// unseen articles get exploration slots without shuffling on every render.
/// FNV-1a (shared with `translate::stable_scope_key`): unlike DefaultHasher,
/// the same day really yields the same order in every process and build.
pub fn exploration_jitter(article_id: &str, day_key: i64) -> f64 {
    let hash = crate::translate::fnv1a_64(&format!("{article_id}:{day_key}"));
    (hash % 1000) as f64 / 999.0 * 0.6 - 0.3
}

/// Age in days, preferring `published_at` then `fetched_at`; unparseable/missing → 0 (now).
pub fn article_age_days(published_at: Option<&str>, fetched_at: &str, now: DateTime<Utc>) -> f64 {
    let parsed = published_at
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .or_else(|| DateTime::parse_from_rfc3339(fetched_at).ok());
    match parsed {
        Some(t) => (now - t.with_timezone(&Utc)).num_seconds() as f64 / 86_400.0,
        None => 0.0,
    }
}

/// Dwell-time signals (ms of visible, focused reading).
/// Long dwell = real engagement even if not finished; a sub-30s bounce on an
/// opened article is a weak negative.
pub fn dwell_adjustment(dwell_ms: i64, read_completed: bool) -> f64 {
    if dwell_ms >= 300_000 {
        if read_completed {
            0.8 // deep engaged read to the end
        } else {
            0.6 // long dwell, unfinished
        }
    } else if dwell_ms >= 30_000 {
        0.2
    } else if dwell_ms > 0 && !read_completed {
        -0.15 // bounced quickly
    } else {
        0.0
    }
}

pub fn article_rank_score(
    article: &ArticleListItem,
    affinity: &Affinity,
    now: DateTime<Utc>,
    day_key: i64,
) -> f64 {
    let age = article_age_days(article.published_at.as_deref(), &article.fetched_at, now);
    let fresh = freshness(age);
    let mut score = fresh;
    score += 0.6 * affinity_score(*affinity.source_opens.get(&article.source).unwrap_or(&0)) * fresh;
    score += 0.4
        * affinity_score(
            *affinity
                .category_opens
                .get(&article.category)
                .unwrap_or(&0),
        )
        * fresh;
    // Sidebar priority is an explicit within-category ordering, so it gets a
    // strong, freshness-modulated boost normalised by that category's ceiling.
    score += 1.5
        * priority_score(
            *affinity.source_priority.get(&article.source).unwrap_or(&0),
            *affinity
                .category_priority_max
                .get(&article.category)
                .unwrap_or(&0),
        )
        * fresh;
    if article.liked {
        score += 2.5;
    }
    score += word_fit(article.word_count);
    if article.open_count == 0 {
        score += exploration_jitter(&article.id, day_key);
    } else {
        if article.read_completed {
            score -= 0.6;
        } else {
            score -= 0.25;
        }
        score += dwell_adjustment(article.dwell_ms, article.read_completed);
    }
    score
}

/// Score + sort a window of list items in place (descending score, id as a
/// stable tie-break so cursor pagination has a total order), stamping
/// `rank_score` for the frontend to pass through.
///
/// The semantic bonus is computed here (not in `article_rank_score`) because
/// IDF needs the whole window: rare-among-candidates profile terms count more
/// than terms every article carries.
pub fn rank_articles(
    mut items: Vec<ArticleListItem>,
    affinity: &Affinity,
    now: DateTime<Utc>,
    day_key: i64,
) -> Vec<ArticleListItem> {
    // Document frequency over the window (title + excerpt terms per article).
    let mut doc_counts: HashMap<String, i64> = HashMap::new();
    let mut item_terms: Vec<std::collections::HashSet<String>> = Vec::with_capacity(items.len());
    for item in &items {
        let mut terms = content_terms(&item.title);
        terms.extend(content_terms(&item.excerpt));
        for t in &terms {
            *doc_counts.entry(t.clone()).or_insert(0) += 1;
        }
        item_terms.push(terms);
    }
    let docs = items.len() as f64;
    let idf = |term: &str| -> f64 {
        let df = *doc_counts.get(term).unwrap_or(&0) as f64;
        ((1.0 + docs) / (1.0 + df)).ln().max(0.0)
    };
    let mass: f64 = affinity
        .term_weights
        .iter()
        .map(|(term, w)| w * idf(term))
        .sum();
    for (item, terms) in items.iter_mut().zip(item_terms) {
        let mut score = article_rank_score(item, affinity, now, day_key);
        // Semantic profile: engaged-title terms, IDF-weighted so generic
        // terms don't dominate. Only active once a profile exists.
        if mass > 0.0 {
            let hit: f64 = terms
                .iter()
                .map(|term| {
                    affinity.term_weights.get(term).copied().unwrap_or(0.0) * idf(term)
                })
                .sum();
            score += 0.8 * (hit / mass).min(1.0);
        }
        item.rank_score = score;
    }
    items.sort_by(|a, b| {
        b.rank_score
            .partial_cmp(&a.rank_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    items
}

/// Slice one page out of a ranked list. A cursor (score + id of the last
/// shown item) returns everything strictly after it, so inserts above the
/// cursor no longer shift later pages; without a cursor the legacy offset
/// window applies.
///
/// Score comparison is tolerance-based: `rank_score` crosses a JSON
/// round-trip between pages, and a 1ulp drift on an exact `==` used to skip
/// or repeat the boundary item. Within tolerance the `id` tiebreak decides.
pub fn page_ranked(
    items: Vec<ArticleListItem>,
    cursor: Option<(f64, String)>,
    offset: usize,
    limit: usize,
) -> Vec<ArticleListItem> {
    /// Two scores from the same computation that differ only by a JSON
    /// round-trip land within a few ulps; 1e-9 relative covers that without
    /// blurring genuinely distinct ranks.
    fn same_score(a: f64, b: f64) -> bool {
        if a == b {
            return true;
        }
        let scale = a.abs().max(b.abs()).max(1.0);
        (a - b).abs() <= 1e-9 * scale
    }
    let start = match cursor {
        Some((score, id)) => items
            .iter()
            .position(|a| a.rank_score < score && !same_score(a.rank_score, score) || (same_score(a.rank_score, score) && a.id > id))
            .unwrap_or(items.len()),
        None => offset.min(items.len()),
    };
    items.into_iter().skip(start).take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ArticleListItem;

    fn item(id: &str) -> ArticleListItem {
        ArticleListItem {
            id: id.into(),
            url: format!("https://example.com/{id}"),
            title: id.into(),
                source: "Test".into(),
            category: "tech".into(),
            published_at: None,
            excerpt: String::new(),
            fetched_at: "2020-01-01T00:00:00Z".into(),
            origin: "rss".into(),
            summary_zh: String::new(),
            last_opened_at: None,
            open_count: 0,
            word_count: 800,
            rank_score: 0.0,
            dwell_ms: 0,
            read_completed: false,
            liked: false,
        }
    }

    #[test]
    fn freshness_decays_with_age() {
        assert!((freshness(0.0) - 1.0).abs() < 1e-9);
        assert!(freshness(7.0) < freshness(1.0));
        assert!(freshness(400.0) >= 0.05, "never decays to zero");
    }

    #[test]
    fn affinity_saturates_and_handles_zero() {
        assert_eq!(affinity_score(0), 0.0);
        assert!(affinity_score(3) > affinity_score(1));
        assert!(affinity_score(9) >= 1.0);
        assert!(affinity_score(100) >= 1.0);
    }

    #[test]
    fn liked_and_completed_dominate() {
        let now = Utc::now();
        let affinity = Affinity::default();
        let base = item("a");
        let liked = {
            let mut i = item("b");
            i.liked = true;
            i
        };
        let completed = {
            let mut i = item("c");
            i.open_count = 1;
            i.read_completed = true;
            i
        };
        assert!(
            article_rank_score(&liked, &affinity, now, 0)
                > article_rank_score(&base, &affinity, now, 0),
            "liked beats a plain article"
        );
        assert!(
            article_rank_score(&completed, &affinity, now, 0)
                < article_rank_score(&base, &affinity, now, 0),
            "already finished articles sink"
        );
    }

    #[test]
    fn word_fit_penalizes_stubs_and_marathons() {
        assert!(word_fit(80) < 0.0);
        assert!(word_fit(1200) > word_fit(9000));
        assert!(word_fit(800) > word_fit(100));
        assert_eq!(word_fit(0), 0.0);
    }

    #[test]
    fn jitter_bounded_and_day_seeded() {
        let j1 = exploration_jitter("abc", 20_000);
        let j1_again = exploration_jitter("abc", 20_000);
        assert!((j1 - j1_again).abs() < f64::EPSILON, "same day is stable");
        assert!((-0.3..=0.3).contains(&j1));
        // Varies across days for at least a sample of ids.
        let differs = (0..20)
            .any(|d| exploration_jitter("abc", d) != exploration_jitter("abc", d + 1));
        assert!(differs, "jitter should re-seed across days");
    }

    #[test]
    fn dwell_signals_engagement() {
        assert!((dwell_adjustment(0, false)).abs() < f64::EPSILON);
        assert_eq!(dwell_adjustment(10_000, false), -0.15, "quick bounce");
        assert_eq!(dwell_adjustment(60_000, false), 0.2, "read a minute");
        assert_eq!(dwell_adjustment(400_000, false), 0.6, "long dwell");
        assert_eq!(
            dwell_adjustment(400_000, true),
            0.8,
            "long dwell read to the end"
        );
    }

    #[test]
    fn ranking_is_descending_and_stamps_scores() {
        let now = Utc::now();
        let affinity = Affinity::default();
        let mut fresh = item("fresh");
        fresh.fetched_at = now.to_rfc3339();
        let mut old = item("old");
        old.fetched_at = (now - chrono::Duration::days(120)).to_rfc3339();
        old.open_count = 2;
        old.read_completed = true;

        let ranked = rank_articles(vec![old, fresh], &affinity, now, 1);
        assert_eq!(ranked[0].id, "fresh");
        assert!(ranked[0].rank_score > ranked[1].rank_score);
        assert!(ranked[0].rank_score != 0.0, "score is stamped for the UI");
    }

    fn scored(id: &str, score: f64) -> ArticleListItem {
        let mut i = item(id);
        i.rank_score = score;
        i
    }

    #[test]
    fn ties_break_by_id_for_cursor_stability() {
        let now = Utc::now();
        let affinity = Affinity::default();
        // open_count > 0 disables the per-id jitter; everything else equal
        // → identical scores, so the id tie-break decides the order.
        let mut items = vec![item("b"), item("a"), item("c")];
        for i in items.iter_mut() {
            i.open_count = 1;
        }
        let ranked = rank_articles(items, &affinity, now, 1);
        assert_eq!(
            ranked.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            vec!["a", "b", "c"],
            "equal scores must order by id so a cursor is unambiguous"
        );
    }

    #[test]
    fn cursor_page_skips_inserts_above() {
        let ranked = vec![
            scored("fresh", 3.0),
            scored("b", 2.0),
            scored("c", 2.0),
            scored("d", 1.0),
        ];
        // Page 1 took ("fresh", "b"); page 2 resumes after b.
        let page2 = page_ranked(ranked.clone(), Some((2.0, "b".into())), 0, 10);
        assert_eq!(
            page2.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            vec!["c", "d"]
        );
        // A newly inserted article above the cursor does not shift page 2.
        let mut with_insert = vec![scored("new", 9.0)];
        with_insert.extend(ranked.clone());
        let page2_again = page_ranked(with_insert, Some((2.0, "b".into())), 0, 10);
        assert_eq!(
            page2_again.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            vec!["c", "d"]
        );
        // No cursor → legacy offset window.
        let offset_page = page_ranked(ranked, None, 1, 2);
        assert_eq!(
            offset_page.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            vec!["b", "c"]
        );
    }

    #[test]
    fn cursor_tolerates_json_round_trip_drift() {
        let ranked = vec![
            scored("fresh", 3.0),
            scored("b", 2.0),
            scored("c", 2.0),
            scored("d", 1.0),
        ];
        // Simulate a 1ulp drift on the score coming back from the frontend.
        let drifted = 2.0 + f64::EPSILON;
        let page = page_ranked(ranked, Some((drifted, "b".into())), 0, 10);
        assert_eq!(
            page.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            vec!["c", "d"],
            "1ulp drift must not skip or repeat the boundary item"
        );
    }

    #[test]
    fn priority_score_is_normalised_and_bounded() {
        assert_eq!(priority_score(0, 10), 0.0, "unordered source gets no bonus");
        assert_eq!(priority_score(10, 0), 0.0, "zero ceiling never divides");
        assert!((priority_score(10, 10) - 1.0).abs() < f64::EPSILON);
        assert!((priority_score(5, 10) - 0.5).abs() < f64::EPSILON);
        assert_eq!(priority_score(20, 10), 1.0, "clamped to 1");
    }

    #[test]
    fn higher_priority_source_ranks_above_same_freshness_rival() {
        let now = Utc::now();
        // Both sources live in category "tech", ceiling 30 → normalise there.
        let affinity = Affinity::default().with_source_priority(
            HashMap::from([("Favorite".to_string(), 30i64), ("Rival".to_string(), 20i64)]),
            HashMap::from([("tech".to_string(), 30i64)]),
        );
        // open_count > 0 disables exploration jitter → deterministic compare.
        let mut fav = item("fav");
        fav.source = "Favorite".into();
        fav.fetched_at = now.to_rfc3339();
        fav.open_count = 1;
        let mut riv = item("riv");
        riv.source = "Rival".into();
        riv.fetched_at = now.to_rfc3339();
        riv.open_count = 1;

        let s_fav = article_rank_score(&fav, &affinity, now, 1);
        let s_riv = article_rank_score(&riv, &affinity, now, 1);
        assert!(s_fav > s_riv, "dragged-to-top source outranks lower one");
    }

    #[test]
    fn priority_is_normalised_within_category() {
        let now = Utc::now();
        // A #1 in a small "world" category should match a #1 in a large "tech"
        // category (both normalise to 1.0), not lose because tech's max is bigger.
        let affinity = Affinity::default().with_source_priority(
            HashMap::from([("TechTop".to_string(), 10i64), ("WorldTop".to_string(), 2i64)]),
            HashMap::from([("tech".to_string(), 10i64), ("world".to_string(), 2i64)]),
        );
        let mut tech = item("t");
        tech.source = "TechTop".into();
        tech.category = "tech".into();
        tech.open_count = 1;
        tech.fetched_at = now.to_rfc3339();
        let mut world = item("w");
        world.source = "WorldTop".into();
        world.category = "world".into();
        world.open_count = 1;
        world.fetched_at = now.to_rfc3339();

        let s_tech = article_rank_score(&tech, &affinity, now, 1);
        let s_world = article_rank_score(&world, &affinity, now, 1);
        assert!(
            (s_tech - s_world).abs() < 1e-9,
            "both are #1 in their category → equal priority bonus: {s_tech} vs {s_world}"
        );
    }

    #[test]
    fn affinity_boosts_familiar_sources() {
        let now = Utc::now();
        let mut affinity = Affinity::default();
        affinity.source_opens.insert("Test".into(), 50);
        let mut a = item("fam");
        a.fetched_at = now.to_rfc3339();
        let mut b = item("other");
        b.fetched_at = now.to_rfc3339();
        b.source = "Other".into();
        let score_fam = article_rank_score(&a, &affinity, now, 1);
        let score_other = article_rank_score(&b, &affinity, now, 1);
        assert!(score_fam > score_other);
        // Identical except source → gap comes purely from affinity × freshness.
        assert!((score_fam - score_other) > 0.5);
    }

    #[test]
    fn content_terms_drop_stopwords_digits_and_singles() {
        let terms = content_terms("The Fed holds rates steady in 2026, officials say");
        assert!(terms.contains("fed"));
        assert!(terms.contains("rates"));
        assert!(terms.contains("officials"));
        assert!(!terms.contains("the"), "stopword");
        assert!(!terms.contains("in"), "stopword");
        assert!(!terms.contains("2026"), "pure digits");
        assert!(!terms.contains("say"), "reporting verb");
    }

    #[test]
    fn term_profile_accumulates_engagement_weights() {
        let weights = build_term_weights(&[
            ("Central bank holds rates".into(), 2.0),
            ("Bank earnings beat estimates".into(), 1.0),
        ]);
        assert_eq!(weights.get("bank"), Some(&3.0));
        assert_eq!(weights.get("rates"), Some(&2.0));
        assert!(!weights.contains_key("the"));
    }

    fn titled(id: &str, title: &str) -> ArticleListItem {
        let mut i = item(id);
        i.title = title.into();
        i.fetched_at = Utc::now().to_rfc3339();
        i.open_count = 1;
        i
    }

    #[test]
    fn semantic_bonus_boosts_profile_matching_articles() {
        let now = Utc::now();
        let mut affinity = Affinity::default();
        affinity.term_weights = build_term_weights(&[
            ("Central bank holds interest rates".into(), 2.0),
        ]);
        let matching = titled("m", "Interest rates and the central bank outlook");
        let plain = titled("p", "Night trains return across quiet borders");
        let ranked = rank_articles(vec![plain, matching], &affinity, now, 1);
        assert_eq!(ranked[0].id, "m");
        assert!(ranked[0].rank_score - ranked[1].rank_score > 0.2);
    }

    #[test]
    fn no_profile_means_no_semantic_bonus() {
        let now = Utc::now();
        let affinity = Affinity::default();
        let a = titled("a", "Central bank holds interest rates");
        let b = titled("b", "Central bank holds interest rates");
        let ranked = rank_articles(vec![a, b], &affinity, now, 1);
        // Same content, empty profile → identical scores, id decides.
        assert!((ranked[0].rank_score - ranked[1].rank_score).abs() < 1e-9);
    }
}
