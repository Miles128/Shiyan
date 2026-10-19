mod books;
mod commands;
mod config;
mod db;
mod error;
mod feeds;
mod import_file;
mod rank;
pub(crate) mod reflow;
mod srs;
mod term_match;
mod translate;
mod vocab;

#[cfg(test)]
mod db_tests;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            // First launch after the bundle-id rename: adopt the old data
            // dir (best-effort; old data is never deleted). Runs before the
            // restore swap so a staged restore moves over too.
            let _ = db::migrate_legacy_app_dir(&dir);
            // Swap in a staged restore file (if any) before connections open.
            db::apply_pending_restore(&dir)?;
            let state = db::DbState::open(db::db_path(dir))?;
            {
                let mut cfg = config::load_config()?;
                if !cfg.disabled_feeds.is_empty() {
                    let conn = state.lock_write()?;
                    db::apply_legacy_disabled_feeds(&conn, &cfg.disabled_feeds)?;
                    drop(conn);
                    cfg.disabled_feeds.clear();
                    config::save_config(&cfg)?;
                }
            }
            {
                // Drop empty articles, stamp never-measured word counts, and
                // remove link roundups / podcast transcripts already stored.
                let conn = state.lock_write()?;
                let _ = db::backfill_word_counts(&conn);
                let _ = feeds::purge_blocked_articles(&conn);
                let _ = feeds::clear_stale_paragraph_translations_once(&conn);
                let _ = db::normalize_known_words_once(&conn);
            }
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_config,
            commands::config::save_config_cmd,
            commands::data::backup_database,
            commands::data::restore_database,
            commands::articles::list_articles,
            commands::articles::list_articles_ranked,
            commands::articles::list_library,
            commands::articles::list_article_sources,
            commands::articles::get_article_view,
            commands::articles::mark_article_opened,
            commands::articles::mark_article_progress,
            commands::articles::set_article_liked,
            commands::articles::get_learning_stats,
            commands::articles::get_reading_stats,
            commands::feeds::list_feeds,
            commands::feeds::set_feed_enabled,
            commands::feeds::delete_feed_source,
            commands::feeds::list_feed_categories,
            commands::feeds::add_feed_category,
            commands::feeds::subscribe_feed,
            commands::feeds::validate_feed,
            commands::feeds::discover_feeds,
            commands::feeds::refresh_feeds,
            commands::articles::fill_missing_card_zh,
            commands::articles::fill_missing_tags,
            commands::articles::import_article_url,
            commands::articles::import_article_file,
            commands::articles::import_book,
            commands::articles::list_book_articles,
            commands::articles::articles_matching_terms,
            commands::articles::repair_paragraphs,
            commands::articles::translate_paragraph,
            commands::articles::translate_selection,
            commands::articles::translate_plain_text,
            commands::articles::translate_full_article,
            commands::memory::add_memory,
            commands::memory::list_memory,
            commands::memory::due_memory,
            commands::memory::review_memory,
            commands::memory::set_memory_status,
            commands::memory::delete_memory,
            commands::memory::export_memory_csv,
            commands::memory::record_lookup,
            commands::memory::list_lookups,
            commands::memory::delete_lookup,
            commands::memory::clear_lookups,
            commands::known::list_known_words,
            commands::known::add_known_word,
            commands::known::remove_known_word
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}