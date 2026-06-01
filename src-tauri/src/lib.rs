mod accounts;
mod activity;
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
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
