use shiyan_core::db::{self, DbState};
use shiyan_core::error::AppError;

#[tauri::command]
pub fn list_known_words(state: tauri::State<'_, DbState>) -> Result<Vec<String>, AppError> {
    let conn = state.lock_read()?;
    db::list_known_words(&conn)
}

#[tauri::command]
pub fn add_known_word(state: tauri::State<'_, DbState>, term: String) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::add_known_word(&conn, &term)
}

#[tauri::command]
pub fn remove_known_word(state: tauri::State<'_, DbState>, term: String) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::remove_known_word(&conn, &term)
}
