use crate::error::AppError;
use crate::db::{self, DbState, FeedCategory, FeedSource};
use crate::feeds::{self, FeedValidation, RefreshProgress, RefreshResult};
use crate::vocab::{self, FeedDiscoverCandidate};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub fn list_feeds(state: tauri::State<'_, DbState>) -> Result<Vec<FeedSource>, AppError> {
    let conn = state.lock_read()?;
    db::list_feeds(&conn)
}

#[tauri::command]
pub fn set_feed_enabled(
    state: tauri::State<'_, DbState>,
    id: String,
    enabled: bool,
) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::set_feed_enabled(&conn, &id, enabled)
}

/// Persist the sidebar drag order of feeds. `ordered_ids` is top-to-bottom.
#[tauri::command]
pub fn reorder_feeds(
    state: tauri::State<'_, DbState>,
    ordered_ids: Vec<String>,
) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::reorder_feeds(&conn, &ordered_ids)
}

/// Delete a user-subscribed feed (curated feeds are disable-only).
#[tauri::command]
pub fn delete_feed_source(state: tauri::State<'_, DbState>, id: String) -> Result<(), AppError> {
    let conn = state.lock_write()?;
    db::delete_user_feed(&conn, &id)
}

#[tauri::command]
pub fn list_feed_categories(
    state: tauri::State<'_, DbState>,
) -> Result<Vec<FeedCategory>, AppError> {
    let conn = state.lock_read()?;
    db::list_feed_categories(&conn)
}

#[tauri::command]
pub fn add_feed_category(
    state: tauri::State<'_, DbState>,
    label: String,
) -> Result<FeedCategory, AppError> {
    let conn = state.lock_write()?;
    db::add_feed_category(&conn, &label)
}

#[derive(serde::Deserialize)]
pub struct SubscribeFeedInput {
    pub name: String,
    pub category: String,
    pub url: String,
    pub description: Option<String>,
}

#[tauri::command]
pub fn subscribe_feed(
    state: tauri::State<'_, DbState>,
    input: SubscribeFeedInput,
) -> Result<FeedSource, AppError> {
    let conn = state.lock_write()?;
    db::subscribe_feed(
        &conn,
        &input.name,
        &input.category,
        &input.url,
        input.description.as_deref().unwrap_or(""),
    )
}

#[tauri::command]
pub async fn validate_feed(url: String) -> Result<FeedValidation, AppError> {
    crate::commands::spawn_blocking_err(move || Ok(feeds::validate_feed_url(&url))).await
}

#[tauri::command]
pub async fn discover_feeds(
    app: AppHandle,
    category_id: String,
) -> Result<Vec<FeedDiscoverCandidate>, AppError> {
    let cfg = crate::config::load_config()?;
    crate::commands::spawn_db(app, move |state| {
        let cat = {
            let conn = state.lock_read()?;
            db::get_feed_category(&conn, &category_id)?
        };
        let cat = cat.ok_or_else(|| format!("未知分类：{category_id}"))?;
        vocab::discover_rss_feeds(&cfg, &cat.id, &cat.label)
    })
    .await
}
#[tauri::command]
pub async fn refresh_feeds(app: AppHandle) -> Result<RefreshResult, AppError> {
    use crate::commands::REFRESHING;
    if REFRESHING
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err(AppError::msg("正在刷新，请稍后再试"));
    }
    struct RefreshGuard;
    impl Drop for RefreshGuard {
        fn drop(&mut self) {
            REFRESHING.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _guard = RefreshGuard;
    let _ = app.emit(
        "refresh-progress",
        RefreshProgress {
            phase: "download".into(),
            current: 0,
            total: 0,
            label: "开始刷新…".into(),
            percent: 0,
            articles: 0,
        },
    );

    let cfg = crate::config::load_config()?;
    let emit_app = app.clone();
    crate::commands::spawn_db(app, move |state| {
        feeds::refresh_feeds(
            state,
            &cfg,
            move |progress: RefreshProgress| {
                let _ = emit_app.emit("refresh-progress", &progress);
            },
        )
    })
    .await
}

/// Cooperative cancel for an in-flight `refresh_feeds`: workers stop after
/// the current feed and enrich phases are skipped. Committed articles stay.
#[tauri::command]
pub fn cancel_refresh() -> Result<(), AppError> {
    feeds::request_refresh_cancel();
    Ok(())
}
