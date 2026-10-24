mod commands;

// The business layer lives in the `shiyan-core` workspace member and is
// tauri-free by construction (a `use tauri::` there fails to compile). Both the
// setup glue below and the command layer reference it as `shiyan_core::…`.
// `reflow` stays core-internal: only `feeds` and core tests call it.
use shiyan_core::{db, feeds};

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let state = db::DbState::open(db::db_path(dir))?;
            {
                // One-time library pass: drop empty articles, stamp never-measured
                // word counts, and remove link roundups / podcast transcripts.
                let conn = state.lock_write()?;
                let _ = feeds::run_startup_cleanup_once(&conn);
                let _ = feeds::clear_stale_paragraph_translations_once(&conn);
            }
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_config,
            commands::config::save_config_cmd,
            commands::data::backup_database,
            commands::articles::list_rank_window,
            commands::articles::list_library,
            commands::articles::get_article_view,
            commands::articles::mark_article_opened,
            commands::articles::mark_article_progress,
            commands::articles::set_article_liked,
            commands::articles::get_learning_stats,
            commands::articles::get_reading_stats,
            commands::feeds::list_feeds,
            commands::feeds::set_feed_enabled,
            commands::feeds::reorder_feeds,
            commands::feeds::delete_feed_source,
            commands::feeds::list_feed_categories,
            commands::feeds::list_source_article_counts,
            commands::feeds::add_feed_category,
            commands::feeds::subscribe_feed,
            commands::feeds::validate_feed,
            commands::feeds::refresh_feeds,
            commands::feeds::cancel_refresh,
            commands::articles::fill_missing_card_zh,
            commands::articles::translate_paragraph,
            commands::articles::translate_selection,
            commands::articles::translate_plain_text,
            commands::articles::translate_full_article,
            commands::memory::add_memory,
            commands::memory::list_memory,
            commands::memory::set_memory_status,
            commands::memory::delete_memory,
            commands::memory::export_memory_csv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}