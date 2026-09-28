//! Article text reflow — turn extracted body text into clean, readable paragraphs.
//!
//! Applied by `feeds::split_paragraphs`, so the reader view and paragraph
//! translation always agree on paragraph boundaries and on the exact text.
//!
//! Rules:
//! 1. Sentence punctuation gets a following space; comma/semicolon/colon too.
//! 2. Paragraphs are recovered from blank lines, single newlines, or sentence
//!    grouping when the whole body is one unbroken block.
//! 3. Whitespace / soft hyphens / zero-width chars are normalized away.
//! 4. No space is kept before punctuation.
//! 5. Hyphenated line breaks from PDF extraction are rejoined.
//! 6. Quotes and apostrophes are unified to ASCII, inner spacing trimmed.
//! 7. `--` / `...` are unified to `—` / `…`.
//! 8. Invisible control characters are dropped.
//! 9. Structural junk from HTML→text is removed first (see [`clean_body`]):
//!    the trailing link-reference definition block, inline `[text][n]` markers
//!    (rewritten into normal links), and short page-chrome paragraphs.
//!
//! Word forms, casing and spelling are never altered — the text stays faithful
//! for lookup and vocabulary review.

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Paragraph-split generation. Bump this whenever a reflow rule changes how
/// text splits into paragraphs: the startup cleanup keys its one-time marker
/// off this version, so stale index-keyed paragraph translations are cleared
/// again instead of rendering against the wrong paragraphs.
pub const REFLOW_VERSION: u32 = 3;

/// A paragraph this short or shorter is a candidate for chrome removal. Longer
/// prose is always kept, even when it mentions subscribing.
const CHROME_MAX_WORDS: usize = 25;

/// Fragments that mark a short block as page furniture rather than the story.
/// Matched case-insensitively against the whole block.
const CHROME_MARKERS: &[&str] = &[
    "subscribe",
    "sign up",
    "sign in",
    "log in",
    "cookie",
    "newsletter",
    "related stories",
    "most popular",
    "read next",
    "up next",
    "share this",
    "follow us",
    "terms of use",
    "privacy",
    "all comments",
    "skip to",
    "skip past",
    "copyright",
    "©",
    "advertise",
    "careers",
    "join now",
    "already a subscriber",
    "create a free account",
    "get unlimited access",
    "continue reading",
    "read more",
    "read the full",
    "view the full",
    "full story at",
];

static RE_LINK_DEF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*\[(\d{1,4})\]:[ \t]*(\S+)[ \t]*$").unwrap());
static RE_REF_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\[(\d{1,4})\]").unwrap());
static RE_IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap());
static RE_LINK_ONLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(!?\[[^\]]*\]\([^)]*\)\s*\[?\d*\]?\s*)+$").unwrap());

/// Strip the structural junk HTML→text conversion drags in: the link-reference
/// definition block appended to nearly every article, the inline `[text][12]`
/// markers that depend on it, and short chrome paragraphs (subscription
/// prompts, cookie notices, related-story labels).
///
/// Inline references are rewritten into normal links *before* the definition
/// block is dropped — deleting the definitions alone would leave `[name][1]`
/// unresolved, and the reader would show the raw bracket text.
///
/// Idempotent, so it is safe both at ingest and again inside [`reflow`], which
/// is what lets already-stored articles be cleaned on display.
pub fn clean_body(text: &str) -> String {
    let mut urls: HashMap<String, String> = HashMap::new();
    for caps in RE_LINK_DEF.captures_iter(text) {
        urls.entry(caps[1].to_string()).or_insert_with(|| caps[2].to_string());
    }

    // Drop definition lines, then rewrite [text][n] into [text](url).
    let without_defs = RE_LINK_DEF.replace_all(text, "").into_owned();
    let resolved = RE_REF_LINK.replace_all(&without_defs, |c: &regex::Captures| match c.get(2) {
        Some(idx) => match urls.get(idx.as_str()) {
            Some(url) => format!("[{}]({})", &c[1], url),
            None => c[0].to_string(),
        },
        None => c[0].to_string(),
    });

    let mut blocks = Vec::new();
    for block in RE_BLANK_RUN.split(&resolved) {
        let trimmed = block.trim();
        if trimmed.is_empty() {
            continue;
        }
        if RE_LINK_ONLY.is_match(trimmed) {
            continue; // a block that is nothing but image/link markup
        }
        let words = trimmed.split_whitespace().count();
        if words <= CHROME_MAX_WORDS {
            let lower = trimmed.to_lowercase();
            // Images are stripped before counting so a caption plus a link does
            // not look like prose.
            let bare = RE_IMAGE.replace_all(&lower, "");
            if CHROME_MARKERS.iter().any(|m| bare.contains(m)) {
                continue;
            }
        }
        blocks.push(trimmed.to_string());
    }
    blocks.join("\n\n")
}

static RE_HYPHEN_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z])-[ \t]*\n[ \t]*([a-z])").unwrap());
/// Left-hand words of hyphenated compounds ("well-known", "part-time"): a
/// line break after such a hyphen is typographic, not syllable hyphenation,
/// so the hyphen must survive the rejoin.
const COMPOUND_HEADS: &[&str] = &[
    "well", "self", "half", "full", "part", "cross", "long", "short", "low", "high", "mid", "all",
    "ill", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
static RE_BLANK_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n[ \t]*\n+").unwrap());
static RE_SPACE_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").unwrap());
static RE_SPACE_BEFORE_PUNCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[ \t]+([,.;:!?%)\]}\u{201d}\u{2019}\u{ff09}])").unwrap()
});
static RE_SPACE_AFTER_OPEN: LazyLock<Regex> =
    LazyLock::new(||     Regex::new(r#"([\u{201c}\u{2018}]|\() [ \t]*"#).unwrap());
static RE_ELLIPSIS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.\s*\.\s*\.").unwrap());
static RE_EM_DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]*(?:--|\u{2014})[ \t]*").unwrap());

/// A single unbroken block longer than this is re-paragraphed by sentence groups.
const LONG_BLOCK_CHARS: usize = 1200;
/// Target length of a sentence-group paragraph.
const GROUP_CHARS: usize = 420;

/// Abbreviations that end with a period but do not end a sentence.
/// Stored lowercase, without the trailing period.
const ABBREV: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "mt", "vs", "vol", "pp", "fig", "eq",
    "ch", "sec", "dept", "univ", "inc", "ltd", "co", "corp", "etc", "e.g", "i.e", "cf", "al",
    "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "mon",
    "tue", "tues", "wed", "thu", "thur", "thurs", "fri", "sat", "sun", "gen", "gov", "sen", "rep",
    "capt", "lt", "sgt", "adm", "rev", "hon", "pres", "approx", "est", "min", "max", "avg", "a.m",
    "p.m", "ph.d", "u.s", "u.k", "e.u", "u.n",
];

/// Common TLDs / file extensions that follow a dot in a token like
/// `example.com`, so the dot must not be treated as a sentence end.
const TOKEN_SUFFIX: &[&str] = &[
    "com", "org", "net", "io", "co", "gov", "edu", "uk", "de", "fr", "jp", "cn", "au", "ca", "ai",
    "app", "dev", "info", "biz", "xyz", "tv", "news", "ly", "gg", "js", "ts", "py", "rs", "go",
    "html", "css", "json", "xml", "txt", "pdf", "md", "png", "jpg", "jpeg", "svg", "sh", "zip",
];

pub fn reflow(text: &str) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = clean_body(&normalized);
    let joined = rejoin_hyphen_breaks(&normalized);

    let mut paragraphs: Vec<String> = Vec::new();
    for block in RE_BLANK_RUN.split(&joined) {
        for raw in split_block_into_paragraphs(block) {
            let p = normalize_paragraph(&raw);
            if !p.is_empty() {
                paragraphs.push(p);
            }
        }
    }

    // Any over-long paragraph gets re-split at sentence boundaries, not just
    // when the whole body happens to be one block.
    paragraphs = paragraphs
        .into_iter()
        .flat_map(|p| {
            if p.chars().count() > LONG_BLOCK_CHARS {
                split_long_block(&p)
            } else {
                vec![p]
            }
        })
        .collect();
    paragraphs
}

/// Join line-break hyphens ("exam-\\nple" → "example"), keeping the hyphen of
/// compound words ("state-of-the-\\nart" → "state-of-the-art").
fn rejoin_hyphen_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for caps in RE_HYPHEN_BREAK.captures_iter(text) {
        let m = caps.get(0).unwrap();
        out.push_str(&text[last..m.start()]);
        let word_start = {
            let b = text.as_bytes();
            let mut i = m.start();
            while i > 0 && b[i - 1].is_ascii_alphabetic() {
                i -= 1;
            }
            i
        };
        let left_word = &text[word_start..m.start() + 1];
        let chain = word_start > 0 && text.as_bytes()[word_start - 1] == b'-';
        let compound =
            chain || COMPOUND_HEADS.contains(&left_word.to_ascii_lowercase().as_str());
        out.push_str(caps.get(1).unwrap().as_str());
        if compound {
            out.push('-');
        }
        out.push_str(caps.get(2).unwrap().as_str());
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

/// Normalize typography and spacing inside a single paragraph.
pub fn normalize_paragraph(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|&c| !is_invisible(c))
        .map(|c| if is_space_like(c) { ' ' } else { c })
        .collect();
    let mut s = cleaned;
    s = RE_SPACE_RUN.replace_all(&s, " ").into_owned();
    s = RE_ELLIPSIS.replace_all(&s, "\u{2026}").into_owned();
    s = RE_EM_DASH.replace_all(&s, "\u{2014}").into_owned();
    s = RE_SPACE_BEFORE_PUNCT.replace_all(&s, "$1").into_owned();
    s = RE_SPACE_AFTER_OPEN.replace_all(&s, "$1").into_owned();
    s = unify_quotes(&s);
    s = RE_SPACE_RUN.replace_all(&s, " ").into_owned();
    s = add_missing_spaces(&s);
    s.trim().to_string()
}

/// Split one blank-line-free block into paragraphs. Single newlines are soft
/// wraps unless the previous line ends a sentence and the next starts a new one.
fn split_block_into_paragraphs(block: &str) -> Vec<String> {
    let lines: Vec<&str> = block
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() <= 1 {
        return lines.into_iter().map(str::to_string).collect();
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in lines {
        if cur.is_empty() {
            cur.push_str(line);
            continue;
        }
        if is_list_item(line) || line_breaks_paragraph(&cur, line) {
            out.push(std::mem::take(&mut cur));
            cur.push_str(line);
        } else {
            cur.push(' ');
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    match chars.next() {
        Some('-' | '*' | '\u{2022}' | '\u{00b7}' | '\u{2013}') => {
            chars.next().is_none_or(char::is_whitespace)
        }
        Some(c) if c.is_ascii_digit() => {
            // Multi-digit numbering: "10. Tenth", "12) Twelfth".
            let rest: String = chars.by_ref().take_while(|c| c.is_ascii_digit()).collect();
            let _ = rest;
            matches!(chars.next(), Some('.' | ')'))
                && chars.next().is_some_and(char::is_whitespace)
        }
        _ => false,
    }
}

fn line_breaks_paragraph(prev: &str, next: &str) -> bool {
    let Some(first) = next.chars().next() else {
        return false;
    };
    let starts_new = first.is_uppercase()
        || first.is_ascii_digit()
        || matches!(
            first,
            '"' | '\'' | '\u{201c}' | '\u{2018}' | '(' | '['
        );
    starts_new && sentence_ends_at(prev)
}

fn sentence_ends_at(s: &str) -> bool {
    let trimmed = s.trim_end_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '\u{201d}' | '\u{2019}' | ')' | ']' | '\u{ff09}'
        )
    });
    let chars: Vec<char> = trimmed.chars().collect();
    match chars.last() {
        Some('!' | '?') => true,
        Some('.') => period_is_boundary(&chars, chars.len() - 1),
        _ => false,
    }
}

/// Insert a space after sentence punctuation (and comma/semicolon/colon) when
/// the next character follows without one.
fn add_missing_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        let Some(&next) = chars.get(i + 1) else {
            continue;
        };
        if next.is_whitespace() {
            continue;
        }
        let insert = match c {
            '.' => next.is_alphanumeric() && period_is_boundary(&chars, i),
            '!' | '?' => next.is_alphanumeric(),
            ',' | ';' | ':' => next.is_alphabetic(),
            _ => false,
        };
        if insert {
            out.push(' ');
        }
    }
    out
}

/// True if the `.` at `i` plausibly ends a sentence (not a decimal, initial,
/// abbreviation, dotted acronym, domain or file extension).
fn period_is_boundary(chars: &[char], i: usize) -> bool {
    let prev = if i > 0 { chars.get(i - 1) } else { None };
    let next = chars.get(i + 1);
    if let (Some(&p), Some(&n)) = (prev, next) {
        if p.is_ascii_digit() && n.is_ascii_digit() {
            return false;
        }
    }
    if matches!(next, Some('.')) {
        return false;
    }

    let word = preceding_word(chars, i);
    if let Some(following) = following_letters(chars, i + 1) {
        let after = i + 1 + following.chars().count();
        if chars.get(after) == Some(&'.') {
            return false;
        }
        if !word.is_empty() && TOKEN_SUFFIX.contains(&following.to_lowercase().as_str()) {
            return false;
        }
    }
    if word.is_empty() {
        return true;
    }
    let lower = word.to_lowercase();
    if ABBREV.contains(&lower.as_str()) {
        return false;
    }
    if word.chars().count() == 1 && word.chars().all(|c| c.is_ascii_uppercase()) {
        return false;
    }
    if word.contains('.') && word.chars().all(|c| c.is_ascii_uppercase() || c == '.') {
        return false;
    }
    true
}

fn preceding_word(chars: &[char], i: usize) -> String {
    let mut j = i;
    let mut v: Vec<char> = Vec::new();
    while j > 0 {
        let c = chars[j - 1];
        if c.is_alphanumeric() || c == '.' {
            v.push(c);
            j -= 1;
        } else {
            break;
        }
    }
    v.reverse();
    v.into_iter().collect()
}

fn following_letters(chars: &[char], start: usize) -> Option<String> {
    let mut s = String::new();
    let mut j = start;
    while let Some(&c) = chars.get(j) {
        if c.is_ascii_alphabetic() {
            s.push(c);
            j += 1;
        } else {
            break;
        }
    }
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Re-paragraph an over-long single block at sentence boundaries.
fn split_long_block(text: &str) -> Vec<String> {
    if text.chars().count() <= LONG_BLOCK_CHARS {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;
    let mut cur_chars = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let boundary = match chars[i] {
            '!' | '?' => true,
            '.' => period_is_boundary(&chars, i),
            _ => false,
        };
        if boundary {
            let mut end = i + 1;
            while end < chars.len()
                && matches!(
                    chars[end],
                    '"' | '\'' | '\u{201d}' | '\u{2019}' | ')' | ']' | '\u{ff09}'
                )
            {
                end += 1;
            }
            cur_chars += end - start;
            if cur_chars >= GROUP_CHARS {
                let group: String = chars[start..end].iter().collect();
                out.push(group.trim().to_string());
                start = end;
                cur_chars = 0;
            }
            i = end;
            continue;
        }
        i += 1;
    }
    if start < chars.len() {
        let tail: String = chars[start..].iter().collect();
        let tail = tail.trim().to_string();
        if !tail.is_empty() {
            if let Some(last) = out.last_mut() {
                if tail.chars().count() < GROUP_CHARS / 2 {
                    last.push(' ');
                    last.push_str(&tail);
                } else {
                    out.push(tail);
                }
            } else {
                out.push(tail);
            }
        }
    }
    if out.is_empty() {
        vec![text.to_string()]
    } else {
        out
    }
}

fn unify_quotes(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
            _ => c,
        })
        .collect()
}

fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}' | '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}'
    ) || (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

fn is_space_like(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\t')
        || c == '\u{00a0}'
        || c == '\u{202f}'
        || c == '\u{205f}'
        || c == '\u{3000}'
        || ('\u{2000}'..='\u{200a}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_blank_lines() {
        assert_eq!(reflow("A\n\nB\n\n\nC"), vec!["A", "B", "C"]);
    }

    #[test]
    fn rejoins_hyphenated_line_breaks() {
        assert_eq!(reflow("exam-\nple text"), vec!["example text"]);
        // A real hyphen inside a line is untouched.
        assert_eq!(reflow("a well-known fact"), vec!["a well-known fact"]);
    }

    #[test]
    fn keeps_compound_hyphens_at_line_breaks() {
        assert_eq!(
            reflow("a state-of-the-\nart model"),
            vec!["a state-of-the-art model"]
        );
        assert_eq!(
            reflow("a well-\nknown fact"),
            vec!["a well-known fact"]
        );
        assert_eq!(
            reflow("the twenty-\nfirst century"),
            vec!["the twenty-first century"]
        );
    }

    #[test]
    fn adds_space_after_sentence_period() {
        assert_eq!(reflow("He left.She stayed."), vec!["He left. She stayed."]);
        assert_eq!(reflow("Wow!Really?"), vec!["Wow! Really?"]);
    }

    #[test]
    fn keeps_decimals_and_abbreviations() {
        assert_eq!(reflow("Pi is 3.14 today."), vec!["Pi is 3.14 today."]);
        assert_eq!(reflow("Mr.Smith went home."), vec!["Mr.Smith went home."]);
    }

    #[test]
    fn keeps_domains_and_files() {
        assert_eq!(reflow("Visit example.com now."), vec!["Visit example.com now."]);
        assert_eq!(reflow("Open main.rs today."), vec!["Open main.rs today."]);
    }

    #[test]
    fn unifies_dashes_and_ellipsis() {
        assert_eq!(reflow("Wait... then go"), vec!["Wait\u{2026} then go"]);
        assert_eq!(reflow("a -- b"), vec!["a\u{2014}b"]);
        assert_eq!(reflow("a \u{2014} b"), vec!["a\u{2014}b"]);
    }

    #[test]
    fn unifies_quotes_and_trims_inner_space() {
        assert_eq!(reflow("He said \u{201c} hi \u{201d}."), vec!["He said \"hi\"."]);
        assert_eq!(reflow("it\u{2019}s fine"), vec!["it's fine"]);
    }

    #[test]
    fn strips_invisible_chars_and_nbsp() {
        assert_eq!(reflow("a\u{00a0}b"), vec!["a b"]);
        assert_eq!(reflow("soft\u{00ad}hyphen"), vec!["softhyphen"]);
        assert_eq!(reflow("zero\u{200b}width"), vec!["zerowidth"]);
    }

    #[test]
    fn removes_space_before_punctuation() {
        assert_eq!(reflow("ok , fine ."), vec!["ok, fine."]);
    }

    #[test]
    fn splits_single_newline_paragraphs() {
        assert_eq!(
            reflow("First sentence.\nSecond sentence."),
            vec!["First sentence.", "Second sentence."]
        );
    }

    #[test]
    fn joins_soft_wrapped_lines() {
        assert_eq!(
            reflow("This is a line\nwrapped sentence."),
            vec!["This is a line wrapped sentence."]
        );
    }

    #[test]
    fn splits_giant_single_block() {
        let body = "This is a sentence about the world. ".repeat(60);
        let parts = reflow(&body);
        assert!(parts.len() > 1, "expected the block to be re-paragraphed");
        assert!(parts.iter().all(|p| !p.is_empty()));
    }

    #[test]
    fn inlines_reference_links_then_drops_the_definition_block() {
        let body = "By [Ethan Kessler][1] on Sunday.\n\n\
                    [1]: https://example.com/ethan\n[2]: https://example.com/other";
        let clean = clean_body(body);
        assert!(
            clean.contains("[Ethan Kessler](https://example.com/ethan)"),
            "reference must survive as a normal link: {clean}"
        );
        assert!(!clean.contains("[1]:"), "definitions must go: {clean}");
    }

    #[test]
    fn drops_chrome_but_keeps_prose_that_mentions_the_same_words() {
        let newsletter_line = "Sign up for our newsletter to get every story.".to_string();
        let long_prose = format!(
            "I have written a newsletter for eleven years, and the part that surprises \
             every new subscriber is how much of the work is simply reading. {}",
            "Sentences about the craft keep coming. ".repeat(4)
        );
        let clean = clean_body(&format!("{newsletter_line}\n\n{long_prose}"));
        assert!(!clean.contains("Sign up for our newsletter"), "{clean}");
        assert!(clean.contains("eleven years"), "long prose must survive: {clean}");
    }

    #[test]
    fn cleaning_is_idempotent() {
        let body = "Real paragraph about policy and practice.\n\nSubscribe now[1]\n\n\
                    [1]: https://example.com/join";
        let once = clean_body(body);
        assert_eq!(once, clean_body(&once), "second pass must change nothing");
    }

    #[test]
    fn reflow_drops_the_reference_wall_it_is_given() {
        let parts = reflow(
            "The court ruled on Friday.\n\n[1]: https://example.com/ruling\n[2]: https://example.com/x",
        );
        assert_eq!(parts, vec!["The court ruled on Friday."]);
    }
}
