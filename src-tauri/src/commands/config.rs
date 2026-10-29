use shiyan_core::apple_translate::{self, TranslationEngineStatus};
use shiyan_core::config::{self, AppConfig};
use shiyan_core::error::AppError;

#[tauri::command]
pub fn get_config() -> Result<AppConfig, AppError> {
    config::load_config()
}

#[tauri::command]
pub fn save_config_cmd(cfg: AppConfig) -> Result<(), AppError> {
    config::save_config(&cfg)
}

/// Availability of the macOS on-device translation engine — the settings
/// screen shows it next to the engine selector.
#[tauri::command]
pub fn translation_engine_status() -> TranslationEngineStatus {
    apple_translate::engine_status()
}
