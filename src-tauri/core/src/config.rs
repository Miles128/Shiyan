use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub disabled_feeds: Vec<String>,
    /// User CEFR level (A1–C2). Words above this are underlined when reading.
    #[serde(default = "default_cefr_level")]
    pub cefr_level: String,
    /// Known vocabulary band (1000 / 3000 / 5000 / 10000 / 20000).
    /// Words with frequency rank above this are underlined.
    #[serde(default = "default_freq_band")]
    pub freq_band: u32,
    /// Whether the adaptive vocab placement test has been completed.
    #[serde(default)]
    pub vocab_placement_done: bool,
    /// Final continuous ability L from the last placement test.
    #[serde(default)]
    pub vocab_placement_l: Option<f64>,
    /// ISO timestamp of the last placement test.
    #[serde(default)]
    pub vocab_placement_at: Option<String>,
    /// User dismissed the placement prompt; don't force it again.
    #[serde(default)]
    pub vocab_placement_skipped: bool,
    /// Reader body font preset (serif / palatino / georgia / newyork / songti / sans).
    #[serde(default = "default_reader_font")]
    pub reader_font: String,
    /// Reader body font size in px (16 / 18 / 20 / 22 / 24).
    #[serde(default = "default_reader_font_size")]
    pub reader_font_size: u32,
    /// Reader body line-height (1.5 / 1.65 / 1.75 / 1.9 / 2.1).
    #[serde(default = "default_reader_line_height")]
    pub reader_line_height: f64,
    /// Reader measure preset (narrow / medium / wide / full).
    #[serde(default = "default_reader_line_width")]
    pub reader_line_width: String,
    /// UI theme preference: system | light | dark.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Auto-ingested articles older than this many days are purged on refresh.
    /// 0 = keep forever. Liked articles and user imports are always kept.
    #[serde(default = "default_article_retention_days")]
    pub article_retention_days: u32,
    /// Show a small Chinese gloss beneath "super-hard" words while reading
    /// (≥2 CEFR steps above the user's level, or frequency rank past 2× the
    /// band). Off = the reader stays as clean as before; underline still works.
    #[serde(default = "default_show_hard_word_gloss")]
    pub show_hard_word_gloss: bool,
}

fn default_cefr_level() -> String {
    "B1".into()
}

fn default_freq_band() -> u32 {
    3000
}

fn default_reader_font() -> String {
    "serif".into()
}

fn default_reader_font_size() -> u32 {
    18
}

fn default_reader_line_height() -> f64 {
    1.75
}

fn default_reader_line_width() -> String {
    "full".into()
}

fn default_article_retention_days() -> u32 {
    14
}

fn default_show_hard_word_gloss() -> bool {
    true
}

fn default_theme() -> String {
    "system".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: "gpt-4o-mini".into(),
            disabled_feeds: vec![],
            cefr_level: default_cefr_level(),
            freq_band: default_freq_band(),
            vocab_placement_done: false,
            vocab_placement_l: None,
            vocab_placement_at: None,
            vocab_placement_skipped: false,
            reader_font: default_reader_font(),
            reader_font_size: default_reader_font_size(),
            reader_line_height: default_reader_line_height(),
            reader_line_width: default_reader_line_width(),
            article_retention_days: default_article_retention_days(),
            theme: default_theme(),
            show_hard_word_gloss: default_show_hard_word_gloss(),
        }
    }
}

/// Packaged-app data dir. Must match `identifier` in tauri.conf.json.
const BUNDLE_ID_DIR: &str = "com.sihai.shiyan";

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("SHIYAN_CONFIG") {
        return PathBuf::from(p);
    }
    // Only probe relative paths in debug builds (dev convenience). Release
    // builds always use the packaged app-data directory to prevent accidentally
    // reading/writing a `config.local.json` from a random working directory.
    #[cfg(debug_assertions)]
    {
        let candidates = [
            PathBuf::from("config.local.json"),
            PathBuf::from("../config.local.json"),
            PathBuf::from("../../config.local.json"),
        ];
        for c in candidates {
            if c.exists() {
                return c;
            }
        }
    }
    // Packaged app fallback: ~/Library/Application Support/<BUNDLE_ID_DIR>/
    if let Some(home) = std::env::var_os("HOME") {
        let dir = PathBuf::from(home)
            .join("Library/Application Support")
            .join(BUNDLE_ID_DIR);
        let _ = fs::create_dir_all(&dir);
        return dir.join("config.local.json");
    }
    PathBuf::from("config.local.json")
}

/// In-process config cache: paragraph-level LLM calls used to re-read the
/// file on every command. First `load_config` wins; `save_config` keeps it in
/// step. External edits to config.local.json are only picked up on restart.
static CONFIG_CACHE: std::sync::LazyLock<std::sync::Mutex<Option<AppConfig>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

pub fn load_config() -> Result<AppConfig, AppError> {
    let mut cache = CONFIG_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(cfg) = cache.as_ref() {
        return Ok(cfg.clone());
    }
    let cfg = read_config_file()?;
    *cache = Some(cfg.clone());
    Ok(cfg)
}

fn read_config_file() -> Result<AppConfig, AppError> {
    let path = config_path();
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    // Self-heal legacy permissions: keys live here, so a 0644 file from an
    // older version is tightened to 0600 on read (best-effort).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.permissions().mode() & 0o777 != 0o600 {
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
        }
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_config(cfg: &AppConfig) -> Result<(), AppError> {
    let mut cfg = cfg.clone();
    // Legacy field: enablement lives on feed_sources.enabled only.
    cfg.disabled_feeds.clear();
    let path = config_path();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let raw = serde_json::to_string_pretty(&cfg)?;
    fs::write(&path, raw)?;
    // Keys live here — lock it down to the owner only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    *CONFIG_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(cfg.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_gets_reader_defaults() {
        let raw = r#"{
            "base_url": "https://api.openai.com/v1",
            "api_key": "x",
            "model": "gpt-4o-mini"
        }"#;
        let cfg: AppConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.reader_font, "serif");
        assert_eq!(cfg.reader_font_size, 18);
        assert_eq!(cfg.reader_line_height, 1.75);
        assert_eq!(cfg.reader_line_width, "full");
        assert_eq!(cfg.cefr_level, "B1");
        assert_eq!(cfg.freq_band, 3000);
        assert!(cfg.disabled_feeds.is_empty());
        // Gloss-under-hard-word is a "helpful by default" reading aid.
        assert!(cfg.show_hard_word_gloss);
    }

    #[test]
    fn show_hard_word_gloss_respects_explicit_false() {
        let raw = r#"{
            "base_url": "https://api.openai.com/v1",
            "api_key": "x",
            "model": "gpt-4o-mini",
            "show_hard_word_gloss": false
        }"#;
        let cfg: AppConfig = serde_json::from_str(raw).unwrap();
        assert!(!cfg.show_hard_word_gloss);
    }

    #[test]
    fn legacy_disabled_feeds_still_deserialize() {
        let raw = r#"{
            "base_url": "https://api.openai.com/v1",
            "api_key": "x",
            "model": "gpt-4o-mini",
            "disabled_feeds": ["vox"]
        }"#;
        let cfg: AppConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.disabled_feeds, vec!["vox"]);
    }
}
