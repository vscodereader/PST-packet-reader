mod ipc;
mod store;

use std::path::Path;

use tauri::{AppHandle, Builder, Manager, Runtime};

use crate::ipc::{accounts, activity, bands, cafes, log_batches, posts, queue, stats, stocks};
use crate::store::JsonStore;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod auth;
pub mod naver_cafe;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn bootstrap_runtime() -> Result<auth::RuntimePaths, String> {
    auth::bootstrap_runtime().await.map_err(|e| e.to_string())
}

#[tauri::command]
fn save_accounts(accounts: Vec<auth::Account>) -> Result<Vec<auth::Account>, String> {
    auth::save_accounts_file(&accounts).map_err(|e| e.to_string())
}

#[tauri::command]
fn enqueue_cookie_refresh<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, auth::QueueState>,
    account_ids: Vec<String>,
    headless: Option<bool>,
    use_adb: Option<bool>,
) -> Result<auth::QueueStatus, String> {
    auth::enqueue_accounts(
        &state,
        app,
        account_ids,
        headless.unwrap_or(false),
        use_adb.unwrap_or(false),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_queue_status(
    state: tauri::State<'_, auth::QueueState>,
) -> Result<auth::QueueStatus, String> {
    auth::get_queue_status(&state).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_account_cookies(account_id: String) -> Result<Option<serde_json::Value>, String> {
    auth::read_account_cookies(&account_id).map_err(|e| e.to_string())
}

/// Registers every IPC command handler on the builder.
///
/// Extracted from [`run`] so integration tests can mount the exact same
/// command surface on a mock runtime.
pub fn register_handlers<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        greet,
        accounts::list_accounts,
        accounts::add_account,
        accounts::update_account,
        accounts::delete_accounts,
        posts::list_posts,
        posts::upsert_post,
        posts::delete_post,
        queue::list_queue_now,
        queue::list_queue_scheduled,
        queue::cancel_queue_now,
        queue::cancel_queue_scheduled,
        queue::add_queue_scheduled,
        queue::promote_queue_scheduled,
        stocks::list_stocks,
        activity::list_activity,
        stats::list_stats,
        log_batches::list_log_batches,
        cafes::list_cafes,
        cafes::resolve_cafe,
        cafes::upsert_cafe,
        cafes::run_post_jobs,
        bands::list_bands,
        bootstrap_runtime,
        save_accounts,
        enqueue_cookie_refresh,
        get_queue_status,
        get_account_cookies,
    ])
}

/// Seeds and manages every domain [`JsonStore`] (plus the cookie-refresh
/// [`auth::QueueState`]) under `dir`.
///
/// Extracted from [`run`] so integration tests can manage the same state
/// against a temp directory instead of the real app data dir.
pub fn manage_stores<R: Runtime>(app: &AppHandle<R>, dir: &Path) -> std::io::Result<()> {
    // All domain data is persisted as JSON files in the app data dir.
    std::fs::create_dir_all(dir)?;
    app.manage(JsonStore::load_or_seed(
        dir.join("accounts.json"),
        accounts::seed(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("posts.json"),
        posts::seed(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("queue-now.json"),
        queue::seed_now(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("queue-scheduled.json"),
        queue::seed_scheduled(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("stocks.json"),
        stocks::seed(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("activity.json"),
        activity::seed(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("log-batches.json"),
        log_batches::seed(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("cafes.json"),
        cafes::seed(),
    ));
    app.manage(JsonStore::load_or_seed(
        dir.join("bands.json"),
        bands::seed(),
    ));
    // Naver-login cookie-refresh queue state (empty until enqueued).
    app.manage(auth::QueueState::default());
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register_handlers(tauri::Builder::default())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            manage_stores(app.handle(), &dir)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_includes_name() {
        let out = greet("Pallas");
        assert!(out.contains("Pallas"), "greet output missing name: {out}");
    }

    #[test]
    fn greet_uses_friendly_template() {
        assert_eq!(
            greet("world"),
            "Hello, world! You've been greeted from Rust!"
        );
    }

    #[test]
    fn greet_handles_empty_name() {
        // Empty input shouldn't panic — it's accepted into the template.
        assert_eq!(greet(""), "Hello, ! You've been greeted from Rust!");
    }
}
