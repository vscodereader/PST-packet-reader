pub mod orchestrator;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn bootstrap_runtime() -> Result<orchestrator::RuntimePaths, String> {
    orchestrator::bootstrap_runtime()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_accounts(
    accounts: Vec<orchestrator::Account>,
) -> Result<Vec<orchestrator::Account>, String> {
    orchestrator::save_accounts_file(&accounts).map_err(|e| e.to_string())
}

#[tauri::command]
fn enqueue_cookie_refresh(
    app: tauri::AppHandle,
    state: tauri::State<'_, orchestrator::QueueState>,
    account_ids: Vec<String>,
    headless: Option<bool>,
) -> Result<orchestrator::QueueStatus, String> {
    orchestrator::enqueue_accounts(&state, app, account_ids, headless.unwrap_or(false))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_queue_status(
    state: tauri::State<'_, orchestrator::QueueState>,
) -> Result<orchestrator::QueueStatus, String> {
    orchestrator::get_queue_status(&state).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_account_cookies(account_id: String) -> Result<Option<serde_json::Value>, String> {
    orchestrator::read_account_cookies(&account_id).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(orchestrator::QueueState::default())
        .invoke_handler(tauri::generate_handler![
            greet,
            bootstrap_runtime,
            save_accounts,
            enqueue_cookie_refresh,
            get_queue_status,
            get_account_cookies
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
