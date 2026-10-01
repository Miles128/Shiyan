//! Shiyan business core — everything behind the window.
//!
//! SQLite repositories (`db`), the RSS ingest pipeline (`feeds`), LLM clients
//! (`vocab`), translation orchestration (`translate`), spaced repetition
//! (`srs`), paragraph reflow, local file import, and config. The crate
//! deliberately has **no `tauri` dependency**: the command shell
//! (`src-tauri/src`) owns state resolution and event emission, so any
//! accidental `use tauri::` here fails to compile, and all logic stays
//! testable with plain `cargo test`.
//!
//! Progress and UI notifications flow outward through callbacks (see
//! `feeds` refresh/enrich entry points and `translate::translate_full_article`),
//! never through an `AppHandle`.

pub mod article_view;
pub mod config;
pub mod db;
pub mod error;
pub mod feeds;
pub mod import_file;
pub mod reflow;
pub mod srs;
pub mod translate;
pub mod vocab;

#[cfg(test)]
mod db_tests;
