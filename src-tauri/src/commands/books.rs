//! Book search / import commands (Project Gutenberg → feed + chapter articles).

use shiyan_core::books::{self, CatalogStatus, ImportedBook};
use shiyan_core::db::CatalogBook;
use shiyan_core::error::AppError;
use tauri::AppHandle;

/// Make sure the local catalog mirror exists (downloads the 21 MB CSV the first
/// time). Idempotent — later calls only report the row count.
#[tauri::command]
pub async fn ensure_book_catalog(app: AppHandle) -> Result<CatalogStatus, AppError> {
    crate::commands::spawn_db(app, |state| books::ensure_catalog(state)).await
}

/// Local title/author search. `shelf` filters on the coarse genre, empty = all.
#[tauri::command]
pub async fn search_books(
    app: AppHandle,
    query: String,
    shelf: Option<String>,
) -> Result<Vec<CatalogBook>, AppError> {
    crate::commands::spawn_db(app, move |state| {
        books::search(state, &query, shelf.as_deref().unwrap_or(""), 40)
    })
    .await
}

/// Download one book and write its chapters as articles. Idempotent.
#[tauri::command]
pub async fn import_book(app: AppHandle, gid: i64) -> Result<ImportedBook, AppError> {
    crate::commands::spawn_db(app, move |state| books::import_book(state, gid)).await
}
