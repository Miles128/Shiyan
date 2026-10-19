//! Whole-word overlap between a learner's terms and an article body.

const MAX_TERMS: usize = 40;
const RECENT_WEIGHT: i64 = 3;
const BANK_WEIGHT: i64 = 1;

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "that", "with", "from", "this", "have", "was", "were",
    "are", "you", "your", "not", "but", "his", "her", "she", "they", "them",
    "its", "been", "will", "would", "there", "their", "what", "when", "which",
    "about", "into", "than", "then", "some", "just", "had", "has", "him", "our",
    "out", "can", "all", "one", "who", "how", "did", "get", "got",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedTerm {
    pub term: String,
    pub weight: i64,
}

/// Normalize, drop function words, and weight terms the learner saved recently.
pub fn weight_terms(terms: &[String], recent: &[String]) -> Vec<WeightedTerm> {
    let recent_set: std::collections::HashSet<String> = recent
        .iter()
        .filter_map(|term| normalize_term(term))
        .collect();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Recent terms first so the cap keeps the words from this week.
    let mut ordered: Vec<&String> = Vec::new();
    ordered.extend(recent.iter());
    ordered.extend(terms.iter());
    for raw in ordered {
        let Some(term) = normalize_term(raw) else {
            continue;
        };
        if !seen.insert(term.clone()) {
            continue;
        }
        let weight = if recent_set.contains(&term) {
            RECENT_WEIGHT
        } else {
            BANK_WEIGHT
        };
        out.push(WeightedTerm { term, weight });
        if out.len() >= MAX_TERMS {
            break;
        }
    }
    out
}

fn normalize_term(raw: &str) -> Option<String> {
    let term = raw.trim().to_ascii_lowercase();
    if term.len() < 3 || term.len() > 40 {
        return None;
    }
    if !term
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b == b'\'' || b == b'-')
    {
        return None;
    }
    if !term.bytes().next()?.is_ascii_lowercase() {
        return None;
    }
    if STOPWORDS.contains(&term.as_str()) {
        return None;
    }
    Some(term)
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'\''
}

/// Case-sensitive scan. Callers lowercase the haystack and the term.
pub fn contains_term(haystack: &str, term: &str) -> bool {
    if term.is_empty() || haystack.len() < term.len() {
        return false;
    }
    let hay = haystack.as_bytes();
    let needle = term.as_bytes();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if &hay[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !is_word_byte(hay[i - 1]);
            let after = i + needle.len();
            let after_ok = after == hay.len() || !is_word_byte(hay[after]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub fn score_text(haystack_lower: &str, terms: &[WeightedTerm]) -> i64 {
    terms
        .iter()
        .filter(|term| contains_term(haystack_lower, &term.term))
        .map(|term| term.weight)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_recent_terms_and_drops_stopwords() {
        let weighted = weight_terms(
            &["estate".into(), "the".into(), "prejudice".into()],
            &["prejudice".into(), "and".into()],
        );
        assert_eq!(
            weighted,
            vec![
                WeightedTerm {
                    term: "prejudice".into(),
                    weight: 3,
                },
                WeightedTerm {
                    term: "estate".into(),
                    weight: 1,
                },
            ]
        );
    }

    #[test]
    fn matches_whole_words_only() {
        assert!(contains_term("the tariff rose", "tariff"));
        assert!(!contains_term("tariffs rose", "tariff"));
        assert_eq!(
            score_text(
                "pride and prejudice",
                &weight_terms(&["prejudice".into(), "whale".into()], &["prejudice".into()]),
            ),
            3
        );
    }
}
