use shiyan_core::db::{self, DbState};
use shiyan_core::error::AppError;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

/// Export a consistent snapshot of the whole database via `VACUUM INTO`, to a
/// path chosen with the system save dialog. Returns the written path, or
/// `None` when the user cancels.
#[tauri::command]
pub async fn backup_database(
    app: tauri::AppHandle,
    state: tauri::State<'_, DbState>,
) -> Result<Option<String>, AppError> {
    if crate::commands::is_refreshing() {
        return Err(AppError::msg("刷新进行中，请刷新结束后再备份"));
    }    let file_name = format!(
        "shiyan-backup-{}.db",
        chrono::Local::now().format("%Y%m%d-%H%M")
    );
    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .add_filter("SQLite 数据库", &["db"])
            .set_file_name(file_name)
            .blocking_save_file()
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let dest = picked
        .into_path()
        .map_err(|e| AppError::msg(e.to_string()))?;

    // Vacuum to a temp file first: VACUUM INTO refuses to overwrite, while the
    // save dialog may point at an existing file the user agreed to replace.
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::msg(e.to_string()))?;
    let tmp = app_data.join(format!("backup-tmp-{}.db", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_file(&tmp);

    {
        let conn = state.lock_read()?;
        db::vacuum_into_file(&conn, &tmp)?;
    }
    std::fs::copy(&tmp, &dest)?;
    let _ = std::fs::remove_file(&tmp);
    Ok(Some(dest.to_string_lossy().into_owned()))
}
