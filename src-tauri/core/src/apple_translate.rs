//! On-device translation via the macOS system engine (Apple Translation
//! framework, macOS 26 Tahoe or newer).
//!
//! The translation itself runs in a tiny Swift helper (`shiyan-translate`,
//! built from `src-tauri/swift/shiyan-translate/main.swift` by `build.rs` and
//! shipped inside the app bundle). It speaks a JSON pipe: `{"texts":[...]}`
//! on stdin, `{"translations":[...]}` on stdout. Nothing leaves the Mac — no
//! network, no API key, no bundled weights.
//!
//! Everything here degrades to "unavailable" instead of failing hard: a
//! missing helper (no Swift toolchain at build time), macOS < 26, or a
//! missing en→zh-Hans language pack all mean the caller falls back to the API
//! engine (see [`crate::vocab::translation_engine`]).

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::LazyLock;
use std::time::Duration;
use ts_rs::TS;

/// File name of the helper binary (also the name it is bundled under).
pub const HELPER_NAME: &str = "shiyan-translate";

/// Availability of the on-device engine, for the settings screen.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TranslationEngineStatus {
    /// Whether the macOS system engine can translate right now.
    pub available: bool,
    /// One-line explanation shown next to the engine selector.
    pub message: String,
}

/// Engine status for the settings screen.
pub fn engine_status() -> TranslationEngineStatus {
    TranslationEngineStatus {
        available: is_available(),
        message: status_line(),
    }
}

/// Whole-batch budget. System translation of a few paragraphs is fast, but the
/// first session start plus model warm-up on a cold Mac is not.
const TIMEOUT: Duration = Duration::from_secs(120);

/// What the user has to do when the system engine cannot serve a request.
pub const UNAVAILABLE_HINT: &str =
    "系统翻译不可用：需要 macOS 26（Tahoe）及以上，并在 系统设置 → 通用 → 语言与地区 → 翻译语言 中安装简体中文与英语";

/// Result of asking the helper whether it can translate.
enum Probe {
    Ready,
    /// Not usable, with the reason the helper gave.
    NotReady(String),
}

#[derive(Serialize)]
struct HelperIn {
    texts: Vec<String>,
}

#[derive(Deserialize)]
struct HelperOut {
    translations: Vec<String>,
}

/// Availability probe result: 0 = not probed yet, 1 = usable, 2 = unusable.
///
/// A positive result is cached for the process lifetime; a negative one is
/// not, because the user can install the language pack while the app runs.
static PROBE: AtomicU8 = AtomicU8::new(0);

/// The helper path is stable for the process lifetime — resolve it once.
static HELPER_PATH: LazyLock<Option<PathBuf>> = LazyLock::new(resolve_helper);

/// True when the system engine can translate right now.
pub fn is_available() -> bool {
    match PROBE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let usable = matches!(probe(), Probe::Ready);
            if usable {
                PROBE.store(1, Ordering::Relaxed);
            }
            usable
        }
    }
}

/// Why the system engine is unavailable — the helper's own diagnosis when it
/// has one ("language pack missing", "needs macOS 26"), else the generic hint.
pub fn unavailable_detail() -> String {
    match probe() {
        Probe::Ready => "系统翻译可用".to_string(),
        Probe::NotReady(detail) => detail,
    }
}

/// One-line status for the settings screen.
pub fn status_line() -> String {
    match probe() {
        Probe::Ready => "系统翻译可用（macOS 内置，离线免费）".to_string(),
        Probe::NotReady(detail) => detail,
    }
}

/// Ask the helper whether it can translate. `--check` exits 0 only when the
/// en→zh-Hans language pack is installed.
fn probe() -> Probe {
    let Some(helper) = HELPER_PATH.as_ref() else {
        return Probe::NotReady(format!("{UNAVAILABLE_HINT}（未找到 {HELPER_NAME}）"));
    };
    match probe_helper(helper) {
        Ok(()) => Probe::Ready,
        Err(err) => Probe::NotReady(err.to_string()),
    }
}

/// Translate English texts to Simplified Chinese with the system engine.
///
/// Returns an error when the engine is unavailable — the caller decides
/// whether that means "fall back to the API" or "surface the problem".
pub fn translate_batch(texts: &[String]) -> Result<Vec<String>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let helper = HELPER_PATH
        .as_ref()
        .ok_or_else(|| AppError::msg(format!("{UNAVAILABLE_HINT}（未找到 {HELPER_NAME}）")))?;

    let payload = serde_json::to_vec(&HelperIn {
        texts: texts.to_vec(),
    })?;
    let expected = texts.len();

    // The helper owns the timeout: a wedged process would otherwise keep this
    // thread (and the reader's progress bar) waiting forever.
    let (tx, rx) = std::sync::mpsc::channel();
    let helper = helper.clone();
    std::thread::spawn(move || {
        let result = run_helper(&helper, &payload, expected);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err(AppError::msg("系统翻译超时，未响应")),
    }
}

/// Ask the helper whether it can translate. `--check` exits 0 only when the
/// en→zh-Hans language pack is installed.
fn probe_helper(helper: &Path) -> Result<(), AppError> {
    let output = Command::new(helper)
        .arg("--check")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AppError::msg(format!("{UNAVAILABLE_HINT}（{e}）")))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        // The helper prefixes its messages with its own name — drop it, the
        // user never typed that command.
        let detail = detail
            .strip_prefix("shiyan-translate: ")
            .unwrap_or(detail)
            .trim();
        if detail.is_empty() {
            Err(AppError::msg(UNAVAILABLE_HINT))
        } else {
            Err(AppError::msg(detail.to_string()))
        }
    }
}

/// Run the helper once: JSON in, JSON out.
fn run_helper(helper: &Path, payload: &[u8], expected: usize) -> Result<Vec<String>, AppError> {
    let mut child = Command::new(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::msg(format!("{UNAVAILABLE_HINT}（{e}）")))?;

    // stdin on its own thread: a payload larger than the pipe buffer would
    // otherwise block here while the helper waits for us to read stdout.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let payload = payload.to_vec();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(&payload);
    });

    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .expect("piped stdout")
        .read_to_end(&mut stdout)?;
    writer.join().ok();

    let status = child.wait()?;
    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        let detail = stderr.trim();
        // Drop the helper's own name prefix — the user never typed it.
        let detail = detail
            .strip_prefix("shiyan-translate: ")
            .unwrap_or(detail)
            .trim();
        return Err(AppError::msg(if detail.is_empty() {
            format!("系统翻译失败（退出码 {:?}）", status.code())
        } else {
            detail.to_string()
        }));
    }

    let parsed: HelperOut = serde_json::from_slice(&stdout)?;
    if parsed.translations.len() != expected {
        return Err(AppError::msg(format!(
            "系统翻译返回条数不符：{} 条，输入 {expected} 条",
            parsed.translations.len(),
        )));
    }
    Ok(parsed.translations)
}

/// Where the helper may live, in priority order:
/// 1. `SHIYAN_APPLE_TRANSLATE` — manual override (dev, custom builds)
/// 2. `<app>/Contents/Resources/shiyan-translate` — packaged .app bundle
/// 3. `<exe dir>/shiyan-translate` — dev builds: `build.rs` copies it there
/// 4. debug-only working-directory probes — `cargo test -p shiyan-core` runs
///    from the crate directory, one level below `src-tauri/resources/`
fn resolve_helper() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("SHIYAN_APPLE_TRANSLATE") {
        let path = PathBuf::from(raw);
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [
                dir.join("../Resources").join(HELPER_NAME),
                dir.join(HELPER_NAME),
            ] {
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    #[cfg(debug_assertions)]
    {
        for candidate in ["resources", "../resources", "../../resources"] {
            let path = Path::new(candidate).join(HELPER_NAME);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_output_parses() {
        let parsed: HelperOut = serde_json::from_str(r#"{"translations":["你好","世界"]}"#).unwrap();
        assert_eq!(parsed.translations, vec!["你好", "世界"]);
    }

    #[test]
    fn helper_input_serializes_texts() {
        let payload = serde_json::to_string(&HelperIn {
            texts: vec!["a".into(), "b".into()],
        })
        .unwrap();
        assert_eq!(payload, r#"{"texts":["a","b"]}"#);
    }

    #[test]
    fn empty_batch_never_touches_the_helper() {
        // No helper on the machine must still answer an empty batch cleanly.
        assert!(translate_batch(&[]).unwrap().is_empty());
    }

    #[test]
    fn status_line_mentions_the_requirement() {
        let line = status_line();
        eprintln!("system translation status: {line}");
        assert!(!line.is_empty());
        if !is_available() {
            // Either the helper's own diagnosis (language pack / OS) or the
            // generic hint — never a bare IO error.
            assert!(
                line.contains("系统设置") || line.contains("macOS"),
                "unavailable status must point at the fix: {line}"
            );
        }
    }

    /// End-to-end against the real system engine. Skipped when the language
    /// pack is not installed — the CI/dev machine may not have it.
    #[test]
    fn translates_a_sentence_when_the_pack_is_installed() {
        if !is_available() {
            eprintln!("skipped: system translation not available on this machine");
            return;
        }
        let out = translate_batch(&["The central bank raised rates again.".to_string()]).unwrap();
        assert_eq!(out.len(), 1);
        let text = &out[0];
        assert!(!text.trim().is_empty(), "translation must not be empty");
        assert_ne!(text, "The central bank raised rates again.");
    }
}
