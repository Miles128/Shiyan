//! Import local `.txt` / `.md` / `.pdf` / `.docx` files as articles.

use crate::db::{self, Article, DbState};
use crate::error::AppError;
use crate::feeds::{self, MIN_IMPORTED_BODY_CHARS};
use chrono::Utc;
use regex::Regex;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use uuid::Uuid;

const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Cap on text pulled out of an archive entry, so a small `.docx` zip bomb
/// cannot expand into gigabytes of memory.
const MAX_DECOMPRESSED_BYTES: u64 = 50 * 1024 * 1024;

static RE_BLANK_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

const UNSUPPORTED_FORMAT: &str = "暂不支持该格式（仅 .txt / .md / .pdf / .docx）";

pub fn import_article_from_file(db: &DbState, path: &str) -> Result<Article, AppError> {
    let raw = PathBuf::from(path.trim());
    if raw.as_os_str().is_empty() {
        return Err("未选择文件".into());
    }
    // Resolve symlinks / `..` up front so every check below runs against the
    // real file, not a path that changes meaning between checks.
    let path = std::fs::canonicalize(&raw).map_err(|_| AppError::msg("文件不存在"))?;
    if !path.is_file() {
        return Err("文件不存在".into());
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // "doc" passes through to its dedicated hint below.
    if !matches!(
        ext.as_str(),
        "txt" | "md" | "markdown" | "pdf" | "docx" | "doc"
    ) {
        return Err(UNSUPPORTED_FORMAT.into());
    }

    let meta = std::fs::metadata(&path).map_err(|e| format!("无法读取文件：{e}"))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("文件过大（上限 20MB）".into());
    }

    let bytes = std::fs::read(&path).map_err(|e| format!("无法读取文件：{e}"))?;
    // Parser crates can panic on malformed input; contain it instead of taking
    // down the command thread.
    let content_text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match ext.as_str() {
            // Markdown is read verbatim: the reader renders it via
            // react-markdown, and the `.md` suffix on the synthetic url is
            // what makes that rendering deterministic (see `urlLooksLikeMarkdown`).
            "txt" | "md" | "markdown" => extract_txt(&bytes),
            "pdf" => extract_pdf(&bytes),
            "docx" => extract_docx(&bytes),
            "doc" => Err(AppError::msg("暂不支持旧版 .doc，请另存为 .docx")),
            _ => Err(AppError::msg(UNSUPPORTED_FORMAT)),
        }
    }))
    .map_err(|_| AppError::msg("文件解析失败：文件损坏或格式不支持"))??;

    let content_text = normalize_whitespace(&content_text);
    if content_text.chars().count() < MIN_IMPORTED_BODY_CHARS {
        return Err("内容太短，无法作为阅读文章".into());
    }

    let title = title_from_path(&path);
    if !feeds::is_english_article(None, &title, &content_text) {
        return Err("看起来不是英文文章".into());
    }

    let id = Uuid::new_v4().to_string();
    // The synthetic url doubles as the Markdown render hint on the frontend
    // (`urlLooksLikeMarkdown`), so keep the original extension on it.
    let ext_suffix = match ext.as_str() {
        "md" | "markdown" => ".md",
        _ => "",
    };
    let article = Article {
        id: id.clone(),
        url: format!("file://import/{id}{ext_suffix}"),
        title,
        source: "导入".into(),
        category: "other".into(),
        published_at: None,
        word_count: content_text.split_whitespace().count() as i64,
        quality: "fulltext".into(),
        extraction_source: "file".into(),
        content_text,
        fetched_at: Utc::now().to_rfc3339(),
        origin: "file".into(),
        summary_zh: String::new(),
        last_opened_at: None,
        open_count: 0,
        dwell_ms: 0,
        read_completed: false,
        liked: false,
    };

    {
        let conn = db.lock_write()?;
        if !db::insert_article_if_new(&conn, &article)? {
            return Err("导入失败：文章未写入".into());
        }
    }
    let mut article = article;
    if let Ok(cfg) = crate::config::load_config() {
        let _ = feeds::fill_article_card_zh(db, &cfg, &mut article);
    }
    Ok(article)
}

fn title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("未命名文档")
        .to_string()
}

fn normalize_whitespace(text: &str) -> String {
    let collapsed: String = text
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    RE_BLANK_RUN.replace_all(collapsed.trim(), "\n\n").into_owned()
}

fn extract_txt(bytes: &[u8]) -> Result<String, AppError> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Ok(String::from_utf8_lossy(bytes).into_owned()),
    }
}

fn extract_pdf(bytes: &[u8]) -> Result<String, AppError> {
    let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(bytes)
    }))
    .map_err(|_| AppError::msg("PDF 解析失败：文件损坏或格式不支持"))?
    .map_err(|e| format!("PDF 解析失败：{e}"))?;
    let trimmed = text.trim().to_string();
    if trimmed.chars().filter(|c| c.is_alphanumeric()).count() < 40 {
        return Err("未能从 PDF 提取文字（可能是扫描件，暂不支持 OCR）".into());
    }
    Ok(trimmed)
}

fn extract_docx(bytes: &[u8]) -> Result<String, AppError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|_| "不是有效的 .docx 文件".to_string())?;
    let file = archive
        .by_name("word/document.xml")
        .map_err(|_| "docx 缺少正文（word/document.xml）".to_string())?;
    let mut buf = Vec::new();
    file.take(MAX_DECOMPRESSED_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("读取 docx 失败：{e}"))?;
    if buf.len() as u64 > MAX_DECOMPRESSED_BYTES {
        return Err(AppError::msg("docx 解压后过大，已拒绝"));
    }
    let xml = String::from_utf8_lossy(&buf);
    Ok(docx_xml_to_text(&xml))
}

/// Pull paragraph text from WordprocessingML (`<w:p>` / `<w:t>`).
fn docx_xml_to_text(xml: &str) -> String {
    let mut reader = quick_xml::Reader::from_str(xml);
    // Best-effort extraction: a stray `&` or mismatched tag must not throw
    // away the whole document, so stay tolerant instead of strict.
    reader.config_mut().allow_dangling_amp = true;
    reader.config_mut().check_end_names = false;

    let mut out = String::new();
    let mut para = String::new();
    let mut in_t = false;

    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(_) => break,
        };
        use quick_xml::events::Event;
        match event {
            Event::Start(e) => match e.name().as_ref() {
                b"w:t" => in_t = true,
                b"w:tab" => para.push('\t'),
                b"w:br" => para.push('\n'),
                _ => {}
            },
            Event::Empty(e) => match e.name().as_ref() {
                b"w:tab" => para.push('\t'),
                b"w:br" => para.push('\n'),
                _ => {}
            },
            Event::End(e) => match e.name().as_ref() {
                b"w:t" => in_t = false,
                b"w:p" => flush_paragraph(&mut out, &mut para),
                _ => {}
            },
            Event::Text(t) => {
                if in_t {
                    if let Ok(text) = t.xml10_content() {
                        para.push_str(&text);
                    }
                }
            }
            Event::CData(c) => {
                if in_t {
                    if let Ok(text) = c.xml10_content() {
                        para.push_str(&text);
                    }
                }
            }
            Event::GeneralRef(r) => {
                if in_t {
                    para.push_str(&decode_xml_ref(&r));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    flush_paragraph(&mut out, &mut para);
    out
}

fn flush_paragraph(out: &mut String, para: &mut String) {
    let line = para.trim();
    if !line.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(line);
    }
    para.clear();
}

/// Resolve an `&entity;` / `&#x2019;` reference (quick-xml strips the `&`/`;`).
/// Unknown entities keep their literal text instead of silently vanishing.
fn decode_xml_ref(r: &quick_xml::events::BytesRef<'_>) -> String {
    let raw = format!("&{};", String::from_utf8_lossy(r));
    match quick_xml::escape::unescape(&raw) {
        Ok(text) => text.into_owned(),
        Err(_) => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::io::Write;

    #[test]
    fn extract_txt_utf8() {
        let s = extract_txt(b"Hello world from a text file.\n").unwrap();
        assert!(s.contains("Hello world"));
    }

    #[test]
    fn docx_xml_paragraphs() {
        let xml = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>First paragraph.</w:t></w:r></w:p>
    <w:p><w:r><w:t>Second &amp; last.</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        let text = docx_xml_to_text(xml);
        assert_eq!(text, "First paragraph.\n\nSecond & last.");
    }

    #[test]
    fn docx_xml_numeric_entities() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>It&#x2019;s &#8220;fine&#8221; &amp; well.</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        assert_eq!(docx_xml_to_text(xml), "It’s “fine” & well.");
    }

    #[test]
    fn docx_xml_empty_tags_and_breaks() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t><w:br/><w:t>C</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        assert_eq!(docx_xml_to_text(xml), "A\tB\nC");
    }

    #[test]
    fn import_txt_file_ok() {
        let dir = std::env::temp_dir().join(format!("shiyan-import-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample essay.txt");
        let body = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();

        let db_path = dir.join("t.sqlite");
        let db = db::DbState::open(db_path).unwrap();

        let article = import_article_from_file(&db, path.to_str().unwrap()).unwrap();
        assert_eq!(article.title, "sample essay");
        assert_eq!(article.source, "导入");
        assert_eq!(article.category, "other");
        assert!(article.url.starts_with("file://import/"));
        assert!(article.content_text.chars().count() >= MIN_IMPORTED_BODY_CHARS);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_markdown_file_keeps_syntax_and_md_url() {
        let dir = std::env::temp_dir().join(format!("shiyan-import-md-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.md");
        let body = format!(
            "# Heading\n\nSome **bold** prose about economics and markets.\n\n{}",
            "The committee reviewed the disinflation trend across the eurozone. "
                .repeat(12)
        );
        std::fs::write(&path, body.as_bytes()).unwrap();

        let db_path = dir.join("t.sqlite");
        let db = db::DbState::open(db_path).unwrap();

        let article = import_article_from_file(&db, path.to_str().unwrap()).unwrap();
        assert_eq!(article.title, "notes");
        // Markdown syntax survives verbatim; the reader renders it via
        // react-markdown, which needs the raw source.
        assert!(article.content_text.starts_with("# Heading"));
        assert!(article.content_text.contains("**bold**"));
        // The `.md` suffix on the synthetic url is what makes
        // `urlLooksLikeMarkdown` fire deterministically.
        assert!(article.url.ends_with(".md"), "{}", article.url);
        assert_eq!(article.origin, "file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reject_unsupported_extension_and_missing_path() {
        let dir = std::env::temp_dir().join(format!("shiyan-import-gate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("t.sqlite");
        let db = db::DbState::open(db_path).unwrap();

        let exe = dir.join("evil.exe");
        std::fs::write(&exe, b"not a document").unwrap();
        let err =
            import_article_from_file(&db, exe.to_str().unwrap()).expect_err("exe rejected");
        assert!(err.to_string().contains("暂不支持该格式"), "{err}");

        let missing = dir.join("nope.txt");
        let err =
            import_article_from_file(&db, missing.to_str().unwrap()).expect_err("missing file");
        assert!(err.to_string().contains("不存在"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reject_short_txt() {
        let dir = std::env::temp_dir().join(format!("shiyan-import-short-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tiny.txt");
        std::fs::write(&path, b"hi").unwrap();
        let db_path = dir.join("t.sqlite");
        let db = db::DbState::open(db_path).unwrap();
        let err = import_article_from_file(&db, path.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("太短"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
