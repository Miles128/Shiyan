use crate::db::{self, DbState, LookupEntry, MemoryItem};
use crate::error::AppError;
use crate::srs::{apply_rating, Rating};
use crate::vocab::{self, AddMemoryInput};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

/// Enrich a just-saved memory item in the background: call the LLM, fill only
/// the still-empty fields of the stored row, then emit `memory-updated` so any
/// open library view can refresh. Fire-and-forget — failures (e.g. no API key
/// configured) are silently dropped and the row keeps what was saved
/// synchronously.
///
/// Shell-side glue only: the LLM call and the row merge live in
/// `shiyan_core::vocab`; this wrapper exists because state access and the
/// event emit need the `AppHandle`. Runs on Tauri's bounded blocking pool
/// (not a raw OS thread per save) so rapid saves can't spawn unbounded threads.
fn enrich_memory_background(app: AppHandle, item: MemoryItem) {
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(cfg) = crate::config::load_config() else {
            return;
        };
        let Some(enrichment) = vocab::enrich_memory_fields(&cfg, &item) else {
            return;
        };
        let Some(state) = app.try_state::<DbState>() else {
            return;
        };
        let Ok(conn) = state.lock_write() else {
            return;
        };
        if let Ok(Some(updated)) = vocab::apply_memory_enrichment(&conn, &item, &enrichment) {
            let _ = app.emit("memory-updated", &updated);
        }
    });
}

#[tauri::command]
pub async fn add_memory(app: AppHandle, input: AddMemoryInput) -> Result<MemoryItem, AppError> {
    let item = crate::commands::spawn_db(app.clone(), move |state| {
        vocab::add_or_merge_memory(state, input)
    })
    .await?;
    // LLM enrichment (word_type / collocations / missing definition) runs in
    // the background — the UI must not wait seconds on an API round-trip.
    if vocab::needs_enrichment(&item) {
        enrich_memory_background(app, item.clone());
    }
    Ok(item)
}

#[tauri::command]
pub fn list_memory(
    state: tauri::State<'_, DbState>,
    kind: Option<String>,
    status: Option<String>,
) -> Result<Vec<MemoryItem>, AppError> {
    let conn = state.lock_read()?;
    db::list_memory(&conn, kind.as_deref(), status.as_deref())
}

#[tauri::command]
pub fn due_memory(
    state: tauri::State<'_, DbState>,
    kind: Option<String>,
) -> Result<Vec<MemoryItem>, AppError> {
    let conn = state.lock_read()?;
    db::due_memory(&conn, kind.as_deref())
}

#[tauri::command]
pub fn review_memory(
    state: tauri::State<'_, DbState>,
    id: String,
    rating: String,
) -> Result<MemoryItem, AppError> {
    let r = Rating::from_str(&rating)?;
    let conn = state.lock_write()?;
    let mut item =
        db::get_memory(&conn, &id)?.ok_or_else(|| "memory item not found".to_string())?;
    apply_rating(&mut item, r);
    db::update_memory_review(&conn, &item)?;
    Ok(item)
}

#[tauri::command]
pub fn set_memory_status(
    state: tauri::State<'_, DbState>,
    id: String,
    status: String,
) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::set_memory_status(&conn, &id, &status)
}

#[tauri::command]
pub fn delete_memory(state: tauri::State<'_, DbState>, id: String) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::delete_memory(&conn, &id)
}

// ---- Lookup history ----

/// Fire-and-forget record of one selection-popover lookup.
#[tauri::command]
pub fn record_lookup(
    state: tauri::State<'_, DbState>,
    term: String,
    context: Option<String>,
    article_id: Option<String>,
) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::record_lookup(
        &conn,
        &term,
        context.as_deref().unwrap_or(""),
        article_id.as_deref(),
    )
}

#[tauri::command]
pub fn list_lookups(
    state: tauri::State<'_, DbState>,
    search: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<LookupEntry>, AppError> {
    let conn = state.lock_read()?;
    db::list_lookups(
        &conn,
        search.as_deref(),
        limit.unwrap_or(100),
        offset.unwrap_or(0),
    )
}

#[tauri::command]
pub fn delete_lookup(state: tauri::State<'_, DbState>, id: i64) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::delete_lookup(&conn, id)
}

#[tauri::command]
pub fn clear_lookups(state: tauri::State<'_, DbState>) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::clear_lookups(&conn)
}

/// Export the whole vocab library (words + phrases, all statuses) as a CSV
/// file chosen via the system save dialog. Returns the written path, or
/// `None` when the user cancels the dialog.
#[tauri::command]
pub async fn export_memory_csv(
    app: tauri::AppHandle,
    state: tauri::State<'_, DbState>,
) -> Result<Option<String>, AppError> {
    // Build the payload under a short-lived read lock, then release before
    // showing the modal dialog.
    let csv = {
        let conn = state.lock_read()?;
        db::export_memory_csv(&conn)?
    };
    let file_name = format!(
        "shiyan-vocab-{}.csv",
        chrono::Local::now().format("%Y%m%d-%H%M")
    );
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("CSV", &["csv"])
            .set_file_name(file_name)
            .blocking_save_file()
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| AppError::msg(e.to_string()))?;
    std::fs::write(&path, csv)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
