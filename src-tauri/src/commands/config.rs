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
