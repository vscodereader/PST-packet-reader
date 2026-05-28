pub mod auth;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn naver_login(id: String, pw: String) -> Result<serde_json::Value, String> {
    auth::login(&id, &pw).await.map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet, naver_login])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
