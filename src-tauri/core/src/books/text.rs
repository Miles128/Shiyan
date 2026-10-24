//! Plain-text cleanup for Gutenberg `.txt` bodies.
//!
//! Gutenberg texts are ASCII-era artifacts: CRLF line endings, hard line wraps
//! at ~70 columns, a licence header before the body and a licence footer after
//! it, and decorative rules (`***`) around chapter breaks. Feeding that raw to
//! the paragraph splitter would turn every visual line into a paragraph, so the
//! body is unwrapped into blank-line-separated blocks first.
//!
//! Deliberately *not* using [`crate::reflow`]: its chrome filter drops short
//! paragraphs that mention "copyright" / "subscribe", which is exactly what a
//! book's epigraph, dedication or colophon looks like. Faithfulness beats
//! tidiness here — the reader applies reflow at render time anyway.

use regex::Regex;
use std::sync::LazyLock;

/// Header/footer markers. Older files used "THE PROJECT GUTENBERG EBOOK …"
/// and "END THE PROJECT GUTENBERG EBOOK"; both start with this much.
static RE_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^\*\*\*\s*START OF (THE|THIS) PROJECT GUTENBERG.*$").unwrap()
});
static RE_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^\*\*\*\s*END OF (THE|THIS) PROJECT GUTENBERG.*$").unwrap()
});
/// Page furniture left over from scanned editions (`[Pg 12]`, `[Illustration]`).
static RE_ARTIFACT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*\[(pg|page|illustration|figure|image)[^\]]*\]\s*$").unwrap());
/// Decorative rules: `***`, `----`, `====`, `~~~~`.
static RE_RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[*=_~\-]{3,}\s*$").unwrap());

/// Body between the PG markers. Files without markers are returned as-is.
pub fn strip_markers(raw: &str) -> &str {
    let mut body = raw;
    if let Some(m) = RE_START.find(body) {
        body = &body[m.end()..];
    }
    if let Some(m) = RE_END.find(body) {
        body = &body[..m.start()];
    }
    body.trim_matches(['\r', '\n'])
}

/// Normalize line endings and drop trailing whitespace per line.
fn normalize_lines(raw: &str) -> Vec<String> {
    raw.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|l| l.trim_end().to_string())
        .collect()
}

/// The paragraph this line continues from: a hyphenated break ("under-\nstand")
/// rejoins without a space; anything else joins with one.
fn append_line(block: &mut String, line: &str) {
    let line = line.trim_start();
    if line.is_empty() {
        return;
    }
    if block.is_empty() {
        block.push_str(line);
        return;
    }
    let hyphen_break = block.ends_with('-')
        && !block.ends_with("--")
        && line.chars().next().is_some_and(|c| c.is_lowercase());
    if hyphen_break {
        block.pop();
        block.push_str(line);
    } else {
        block.push(' ');
        block.push_str(line);
    }
}

/// Blank-line-separated blocks with hard line wraps undone. Artifacts and
/// decorative rules are dropped.
pub fn blocks(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in normalize_lines(raw) {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        let trimmed = line.trim_start();
        if RE_ARTIFACT.is_match(trimmed) || RE_RULE.is_match(trimmed) {
            continue;
        }
        append_line(&mut current, trimmed);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Collapse runs of whitespace — titles and author strings come from a CSV that
/// occasionally wraps.
pub fn collapse_spaces(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_pg_header_and_footer() {
        let raw = "The Project Gutenberg eBook of X\n\n*** START OF THE PROJECT GUTENBERG EBOOK X ***\n\nChapter one.\n\n*** END OF THE PROJECT GUTENBERG EBOOK X ***\n\nlicence text";
        assert_eq!(strip_markers(raw), "Chapter one.");
    }

    #[test]
    fn unwraps_hard_line_breaks_and_hyphens() {
        let raw = "It was the best of\ntimes, it was the worst of times.\n\nA second para-\ngraph follows.";
        let b = blocks(raw);
        assert_eq!(b.len(), 2);
        assert!(b[0].ends_with("worst of times."), "{}", b[0]);
        assert_eq!(b[1], "A second paragraph follows.");
    }

    #[test]
    fn drops_page_artifacts_and_rules() {
        let raw = "[Pg 12]\n\n***\n\nReal text here.\n\n[Illustration]\n";
        assert_eq!(blocks(raw), vec!["Real text here.".to_string()]);
    }

    #[test]
    fn handles_crlf() {
        assert_eq!(blocks("One line\r\nand a second.\r\n\r\nNext.\r\n").len(), 2);
    }
}
