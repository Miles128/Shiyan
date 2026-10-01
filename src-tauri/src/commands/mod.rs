//! Tauri command handlers, grouped by domain.
//!
//! Each module owns the `#[tauri::command]` functions for one domain and stays
//! free of business logic — it serializes the request, drives the domain module,
//! and maps errors out. `lib.rs` registers the full handler list.

pub mod articles;
pub mod config;
pub mod data;
pub mod feeds;
pub mod known;
pub mod memory;

use crate::db::DbState;
use crate::error::AppError;
use tauri::{AppHandle, Manager};

/// True while a `refresh_feeds` run holds the refresh guard. `backup_database`
/// consults it to refuse a concurrent VACUUM (5B: 刷新中禁用备份，确定性强).
pub(crate) static REFRESHING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn is_refreshing() -> bool {
    REFRESHING.load(std::sync::atomic::Ordering::SeqCst)
}

/// Run blocking DB/network work off the UI thread, with `DbState` already resolved.
pub async fn spawn_db<T, F>(app: AppHandle, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&DbState) -> Result<T, AppError> + Send + 'static,
{
    crate::error::flatten_blocking(
        tauri::async_runtime::spawn_blocking(move || {
            let state = app
                .try_state::<DbState>()
                .ok_or_else(|| AppError::from("数据库未就绪"))?;
            f(&state)
        })
        .await,
    )
}

/// Same pool, no database.
pub async fn spawn_blocking_err<T, F>(f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    crate::error::flatten_blocking(tauri::async_runtime::spawn_blocking(f).await)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs::read_dir;
    use std::path::Path;

    /// A command with no `invoke`/`typedInvoke` in the frontend cannot be
    /// smoke-tested from a running window, so "registered" and "called" are
    /// compared as text: registered-but-uncalled is dead surface (a `lib.rs`
    /// line and an adapter nothing exercises), called-but-unregistered fails at
    /// runtime.
    ///
    /// A command that legitimately has no UI yet belongs in `UNCALLED` with the
    /// reason, so the decision is written down instead of rediscovered.
    const UNCALLED: &[(&str, &str)] = &[];

    fn ts_files(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in read_dir(dir).expect("read frontend dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                ts_files(&path, found);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("ts" | "tsx")
            ) {
                found.push(path);
            }
        }
    }

    fn registered() -> BTreeSet<String> {
        let lib = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
            .expect("read lib.rs");
        let start = lib.find("generate_handler![").expect("handler list");
        let body = &lib[start..start + lib[start..].find("])").expect("handler list end")];
        body.lines()
            .filter_map(|line| {
                let line = line.trim().trim_end_matches(',');
                line.strip_prefix("commands::")
                    .and_then(|rest| rest.split("::").last())
                    .map(str::to_string)
            })
            .collect()
    }

    fn invoked() -> BTreeSet<String> {
        static RE_INVOKE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            // `[iI]nvoke` matches both a bare `invoke(` and the `Invoke(` inside
            // `typedInvoke(` (the frontend's typed wrapper). The inner
            // `invoke(cmd as string, …)` in invoke.ts has no leading string
            // literal, so it is not a false positive.
            regex::Regex::new(r#"[iI]nvoke(?:<[^;()]{0,120}>)?\s*\(\s*"([a-z0-9_]+)""#).unwrap()
        });
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let mut files = Vec::new();
        ts_files(&root, &mut files);
        let mut names = BTreeSet::new();
        for file in files {
            let source = std::fs::read_to_string(file).expect("read frontend file");
            for cap in RE_INVOKE.captures_iter(&source) {
                names.insert(cap[1].to_string());
            }
        }
        names
    }

    #[test]
    fn every_registered_command_has_a_frontend_call_site() {
        let (handlers, called) = (registered(), invoked());
        let allow: BTreeSet<String> =
            UNCALLED.iter().map(|(name, _)| (*name).to_string()).collect();
        let orphans: Vec<&String> = handlers
            .difference(&called)
            .filter(|name| !allow.contains(*name))
            .collect();
        assert!(orphans.is_empty(), "registered but never invoked: {orphans:?}");
    }

    #[test]
    fn every_invoked_command_is_registered() {
        let (handlers, called) = (registered(), invoked());
        let missing: Vec<&String> = called.difference(&handlers).collect();
        assert!(missing.is_empty(), "invoked but not registered: {missing:?}");
    }

    #[test]
    fn uncalled_allowlist_is_not_stale() {
        let (handlers, called) = (registered(), invoked());
        for (name, reason) in UNCALLED {
            let name = name.to_string();
            assert!(
                handlers.contains(&name) && !called.contains(&name),
                "{name} is allowlisted ({reason}) but is gone or now called"
            );
        }
        assert!(
            handlers.len() > 40,
            "only {} commands parsed out of the handler list, which is not a real read of it",
            handlers.len()
        );
    }
}
