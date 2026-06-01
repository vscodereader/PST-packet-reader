mod accounts;
mod activity;
mod bands;
mod cafes;
mod log_batches;
mod posts;
mod queue;
mod scheduled;
mod stats;
mod stocks;
mod store;

use tauri::Manager;

use crate::store::JsonStore;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod auth;

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
fn enqueue_cookie_refresh(
    app: tauri::AppHandle,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // All domain data is persisted as JSON files in the app data dir.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
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
                dir.join("stats.json"),
                stats::seed(),
            ));
            app.manage(JsonStore::load_or_seed(
                dir.join("scheduled.json"),
                scheduled::seed(),
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            stocks::list_stocks,
            activity::list_activity,
            stats::list_stats,
            scheduled::list_scheduled,
            log_batches::list_log_batches,
            cafes::list_cafes,
            bands::list_bands,
        ])
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
