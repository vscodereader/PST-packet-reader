mod accounts;

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
        .manage(accounts::AccountStore::default())
        .invoke_handler(tauri::generate_handler![
            greet,
            accounts::list_accounts,
            accounts::add_account,
            accounts::update_account,
            accounts::delete_accounts,
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
