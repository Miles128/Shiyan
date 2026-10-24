//! Split a Gutenberg body into chapters.
//!
//! No two public-domain texts agree on headings, so this is a ranked heuristic
//! rather than a parser:
//!
//! 1. A block is a heading when it is short and either starts with a structural
//!    keyword (`CHAPTER 1.`, `BOOK II`, `LETTER XXIV`, …) or is set in all caps.
//! 2. Table-of-contents entries are headings too. They are identified as a long
//!    run of headings with almost no text between them, at the front of the
//!    book — Moby Dick contains 135 chapters and 270 heading-shaped blocks for
//!    exactly this reason.
//! 3. A book with fewer than three real headings is not chapter-structured
//!    (essays, poems, letters) and is split into word-count chunks instead.
//!
//! The output is what the reader shows: chapter title plus its own body, so
//! each chapter stays small enough for the paragraph-rendering reader.

use regex::Regex;
use std::sync::LazyLock;

/// Longer than this and the block is prose that merely starts like a heading.
/// Real chapter titles do get long — Moby Dick's chapter 56 runs to 92
/// characters — so this is generous on purpose.
const HEADING_MAX_CHARS: usize = 130;
/// Trailing text after `CHAPTER 12.` — a title, not a whole paragraph.
const HEADING_REST_MAX_CHARS: usize = 100;
/// Two headings this close together are TOC neighbours, not chapters. TOC
/// entries survive unwrapping as one block each, separated by a single blank
/// line, so the gap is exactly 1. Using 2 would merge the TOC run into the
/// first real chapters whenever a preface sits between them.
const TOC_MAX_GAP: usize = 1;
/// A run this long at the front of the book is a table of contents.
const TOC_MIN_RUN: usize = 4;
/// Only treat heading runs in the first tenth as TOC. Kept tight on purpose:
/// a book of very short pieces (poems, epigrams) legitimately has chapters a
/// couple of blocks apart, and dropping those would delete real content.
const TOC_PREFIX_RATIO: f64 = 0.10;
/// Chunk size for books with no chapter structure.
const HARD_SPLIT_WORDS: usize = 8000;
/// Front matter (dedication, preface) worth keeping as its own chapter.
const FRONT_MATTER_MIN_WORDS: usize = 100;
static RE_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(chapter|book|part|volume|section|canto|letter|act|scene|stave|essay|lecture)\s+([0-9]{1,4}|[ivxlcdm]{1,12})\b\.?\s*[:.\-–—]?\s*(.*)$",
    )
    .unwrap()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub title: String,
    pub text: String,
}

impl Chapter {
    pub fn words(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

/// Chapter list for one book. Never empty: a body that yields nothing becomes a
/// single chapter holding the whole text.
pub fn split(blocks: &[String]) -> Vec<Chapter> {
    if blocks.is_empty() {
        return Vec::new();
    }
    // (block index, display title, came from a `CHAPTER n`-style heading)
    let detected: Vec<(usize, String, bool)> = blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let numbered = RE_HEADING.is_match(b.trim());
            heading_title(b).map(|t| (i, t, numbered))
        })
        .collect();
    let numbered_count = detected.iter().filter(|d| d.2).count();
    let first_numbered = detected.iter().find(|d| d.2).map(|d| d.0);

    // In a numbered book, an ALL-CAPS block before the first numbered chapter
    // is front-matter furniture (editor byline, cast list), not a chapter.
    let mut headings: Vec<(usize, String)> = detected
        .into_iter()
        .filter(|(index, _, numbered)| {
            if *numbered || numbered_count < 3 {
                return true;
            }
            first_numbered.is_some_and(|first| *index > first)
        })
        .map(|(index, title, _)| (index, title))
        .collect();

    // An all-caps block glued to the previous heading is that chapter's own
    // sub-heading (SM's "HARPOONEERS AND SAILORS"), not a new chapter — keeping
    // it would split the chapter at its first line and leave the heading empty.
    let mut merged: Vec<(usize, String)> = Vec::with_capacity(headings.len());
    for heading in headings.drain(..) {
        let is_caps = !RE_HEADING.is_match(blocks[heading.0].trim());
        if is_caps && merged.last().is_some_and(|prev: &(usize, String)| prev.0 + 1 == heading.0) {
            continue;
        }
        merged.push(heading);
    }

    let kept = drop_toc(merged, blocks.len());
    if kept.len() >= 3 {
        let chapters = build(blocks, &kept);
        if chapters.len() >= 2 {
            return chapters;
        }
    }
    hard_split(blocks)
}

/// Title text when `block` looks like a chapter heading.
pub fn heading_title(block: &str) -> Option<String> {
    let text = block.trim();
    if text.is_empty() || text.chars().count() > HEADING_MAX_CHARS {
        return None;
    }
    if let Some(caps) = RE_HEADING.captures(text) {
        let unit = capitalize(&caps[1]);
        let number = caps[2].to_uppercase();
        let rest = caps
            .get(3)
            .map(|m| m.as_str().trim_matches([' ', '.', ':', '-', '—', ']', '[', '_']))
            .unwrap_or("");
        if rest.chars().count() > HEADING_REST_MAX_CHARS {
            return None;
        }
        return Some(if rest.is_empty() {
            format!("{unit} {number}")
        } else {
            format!("{unit} {number} · {}", title_case(rest))
        });
    }
    if is_all_caps_heading(text) {
        return Some(title_case(text.trim_end_matches(['.', ':'])));
    }
    None
}

fn is_all_caps_heading(text: &str) -> bool {
    // Quoted or bracketed all-caps blocks are stage directions, epigraphs and
    // cast lists ("“CAROLINE BINGLEY.”"), never chapter titles.
    if text.starts_with(['"', '“', '‘', '\'', '_', '(', '[']) {
        return false;
    }
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    // Eight letters minimum: "THE END" is not a heading, "EPILOGUE" is.
    if letters.len() < 8 || letters.len() > 60 {
        return false;
    }
    if !letters.iter().all(|c| c.is_uppercase()) {
        return false;
    }
    // Two words minimum: a lone "YES" or "I" is dialogue, not a heading.
    let words = text.split_whitespace().count();
    (2..=10).contains(&words) && !text.ends_with(',') && !text.ends_with(';')
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// `THE WHITE WHALE` → `The White Whale`. Leaves already-mixed case alone.
fn title_case(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) if first.is_uppercase() => {
                    format!("{first}{}", chars.as_str().to_lowercase())
                }
                _ => w.to_string(),
            }
        })
        .collect();
    words.join(" ")
}

/// Remove table-of-contents heading runs (and early duplicates of them).
fn drop_toc(headings: Vec<(usize, String)>, total_blocks: usize) -> Vec<(usize, String)> {
    if headings.len() < TOC_MIN_RUN {
        return headings;
    }
    let prefix_limit = (total_blocks as f64 * TOC_PREFIX_RATIO) as usize;
    let mut drop = vec![false; headings.len()];

    // Adjacent heading runs with no prose between them = a TOC block.
    let mut i = 0;
    while i < headings.len() {
        let mut j = i;
        while j + 1 < headings.len() && headings[j + 1].0 - headings[j].0 <= TOC_MAX_GAP {
            j += 1;
        }
        if j - i + 1 >= TOC_MIN_RUN && headings[i].0 <= prefix_limit {
            for flag in drop.iter_mut().take(j + 1).skip(i) {
                *flag = true;
            }
        }
        i = j + 1;
    }

    // A short TOC (fewer entries than TOC_MIN_RUN) survives the run rule; its
    // copies are still exact duplicates sitting near the front of the book.
    for later in 0..headings.len() {
        if drop[later] {
            continue;
        }
        let key = headings[later].1.to_lowercase();
        for earlier in 0..later {
            if !drop[earlier]
                && headings[earlier].1.to_lowercase() == key
                && headings[earlier].0 <= prefix_limit
            {
                drop[earlier] = true;
                break;
            }
        }
    }

    headings
        .into_iter()
        .zip(drop)
        .filter_map(|(h, dropped)| (!dropped).then_some(h))
        .collect()
}

fn build(blocks: &[String], headings: &[(usize, String)]) -> Vec<Chapter> {
    let mut chapters = Vec::new();
    let first = headings[0].0;
    if first > 0 {
        let front = blocks[..first].join("\n\n");
        if front.split_whitespace().count() >= FRONT_MATTER_MIN_WORDS {
            chapters.push(Chapter {
                title: "Front Matter".into(),
                text: front,
            });
        }
    }
    for (n, (index, title)) in headings.iter().enumerate() {
        let end = headings.get(n + 1).map(|(i, _)| *i).unwrap_or(blocks.len());
        if index + 1 >= end {
            continue;
        }
        let text = blocks[index + 1..end].join("\n\n");
        if text.trim().is_empty() {
            continue;
        }
        chapters.push(Chapter {
            title: title.clone(),
            text,
        });
    }
    chapters
}

fn hard_split(blocks: &[String]) -> Vec<Chapter> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut words = 0usize;
    for block in blocks {
        let block_words = block.split_whitespace().count();
        if words >= HARD_SPLIT_WORDS && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            words = 0;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(block);
        words += block_words;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        return Vec::new();
    }
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, text)| Chapter {
            title: format!("Part {}", i + 1),
            text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn reads_structural_headings() {
        assert_eq!(
            heading_title("CHAPTER 1. Loomings.").as_deref(),
            Some("Chapter 1 · Loomings")
        );
        assert_eq!(heading_title("BOOK II").as_deref(), Some("Book II"));
        assert_eq!(
            heading_title("LETTER XXIV").as_deref(),
            Some("Letter XXIV")
        );
    }

    #[test]
    fn heading_must_be_short() {
        let long = format!(
            "CHAPTER 1. {}",
            "a very long sentence that is clearly prose in disguise ".repeat(3)
        );
        assert_eq!(heading_title(&long), None);
    }

    #[test]
    fn all_caps_headings_are_titlecased() {
        assert_eq!(
            heading_title("THE WHITE WHALE").as_deref(),
            Some("The White Whale")
        );
        // Dialogue shouted in caps is not a heading.
        assert_eq!(heading_title("YES"), None);
    }

    #[test]
    fn splits_on_headings() {
        let b = blocks(&[
            "CHAPTER 1. One.",
            "First body paragraph with enough words to count as prose here.",
            "CHAPTER 2. Two.",
            "Second body paragraph.",
            "CHAPTER 3. Three.",
            "Third body paragraph.",
        ]);
        let chapters = split(&b);
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title, "Chapter 1 · One");
        assert!(chapters[1].text.starts_with("Second body"));
    }

    #[test]
    fn drops_table_of_contents_run() {
        // Six adjacent TOC entries, then a preface, then the real chapters.
        let mut items: Vec<&str> = vec![
            "CONTENTS",
            "CHAPTER 1. One.",
            "CHAPTER 2. Two.",
            "CHAPTER 3. Three.",
            "CHAPTER 4. Four.",
            "CHAPTER 5. Five.",
            "CHAPTER 6. Six.",
            "The preface runs long enough to look like prose rather than a heading, which is the signal that the table of contents has ended.",
        ];
        for n in 1..=6 {
            items.push(match n {
                1 => "CHAPTER 1. One.",
                2 => "CHAPTER 2. Two.",
                3 => "CHAPTER 3. Three.",
                4 => "CHAPTER 4. Four.",
                5 => "CHAPTER 5. Five.",
                _ => "CHAPTER 6. Six.",
            });
            items.push("Body paragraph for this chapter, which is unmistakably prose.");
        }
        let chapters = split(&blocks(&items));
        assert_eq!(chapters.len(), 6, "{chapters:?}");
        assert_eq!(chapters[0].title, "Chapter 1 · One");
    }

    #[test]
    fn front_matter_kept_when_substantial() {
        let long_dedication = "For the readers who stayed, and for the ones who will come later with the same questions about how a life is put together and why the words matter so much. ".repeat(5);
        let b = blocks(&[
            long_dedication.as_str(),
            "CHAPTER 1. One.",
            "Body one.",
            "CHAPTER 2. Two.",
            "Body two.",
            "CHAPTER 3. Three.",
            "Body three.",
        ]);
        let chapters = split(&b);
        assert_eq!(chapters[0].title, "Front Matter");
    }

    #[test]
    fn falls_back_to_word_chunks() {
        let long = "word ".repeat(HARD_SPLIT_WORDS + 100);
        let b = blocks(&[long.as_str(), "tail paragraph"]);
        let chapters = split(&b);
        assert!(chapters.len() >= 2);
        assert_eq!(chapters[0].title, "Part 1");
    }
}
