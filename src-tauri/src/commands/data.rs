use crate::db::{self, DbState};
use crate::error::AppError;
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

/// Stage a backup file for restore. The file is validated, copied next to the
/// live database, and swapped in on the next launch (before any connection
/// opens). Returns the confirmation message.
#[tauri::command]
pub async fn restore_database(app: tauri::AppHandle) -> Result<String, AppError> {
    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .add_filter("SQLite 数据库", &["db"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?;
    let Some(picked) = picked else {
        return Err(AppError::msg("未选择备份文件"));
    };
    let src = picked
        .into_path()
        .map_err(|e| AppError::msg(e.to_string()))?;
    db::validate_backup_file(&src)?;

    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::msg(e.to_string()))?;
    let staged = db::pending_restore_path(&app_data);
    std::fs::copy(&src, &staged)?;
    Ok("恢复文件已就绪。重启拾言后生效，当前数据会自动备份为 .pre-restore.bak。".into())
}
