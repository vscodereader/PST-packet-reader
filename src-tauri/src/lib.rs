mod ipc;
mod store;

use std::path::PathBuf;
use std::process::Command;

use tauri::Manager;

use crate::ipc::{accounts, activity, bands, cafes, log_batches, posts, queue, stats, stocks};
use crate::store::JsonStore;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod auth;
// 네이버 증권 토론방 패킷 게시 엔진.
pub mod discussion_batch;
pub mod naver_automation;

use discussion_batch::{
    parse_discussion_template_csv, run_discussion_batch, run_forum_publish, search_naver_stocks,
    DiscussionBatchReport, DiscussionBatchRequest, ForumPublishRequest, ForumPublishResult,
    StockCandidate, TemplateColumns,
};
use naver_automation::{
    run_naver_discussion_macro, AutomationReport, AutomationTarget, NaverDiscussionRequest,
};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// === 네이버 증권 토론방 패킷 게시 command ===

#[tauri::command]
fn run_naver_discussion(
    title: String,
    body: String,
    host: Option<String>,
    port: Option<u16>,
    target: Option<String>,
    submit_after_fill: Option<bool>,
) -> Result<AutomationReport, String> {
    run_naver_discussion_macro(NaverDiscussionRequest {
        title,
        body,
        host: host.unwrap_or_else(|| "127.0.0.1".to_owned()),
        port: port.unwrap_or(9222),
        target: parse_automation_target(target)?,
        submit_after_fill: submit_after_fill.unwrap_or(false),
        stock: None,
        account_id: None,
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn parse_template_csv(csv_text: String) -> Result<TemplateColumns, String> {
    parse_discussion_template_csv(csv_text)
}

#[tauri::command]
fn search_stocks(query: Option<String>) -> Result<Vec<StockCandidate>, String> {
    search_naver_stocks(query)
}

#[tauri::command]
fn open_incognito_chrome() -> Result<String, String> {
    launch_incognito_chrome()
}

#[tauri::command]
async fn run_naver_discussion_batch(
    app: tauri::AppHandle,
    request: DiscussionBatchRequest,
) -> Result<DiscussionBatchReport, String> {
    // 동기 블로킹 작업을 스레드 풀에서 실행해 GTK 메인 루프를 막지 않습니다.
    // 메인 루프가 자유로워야 Rust에서 emit한 이벤트가 프론트엔드에 전달됩니다.
    tauri::async_runtime::spawn_blocking(move || run_discussion_batch(request, app))
        .await
        .map_err(|error| format!("배치 실행 스레드 오류: {error}"))?
}

// 사수 UI(publish-modal)의 "지금 바로 게시 + 종목토론방"이 호출하는 command입니다.
#[tauri::command]
async fn run_forum_publish_now(
    app: tauri::AppHandle,
    request: ForumPublishRequest,
) -> Result<Vec<ForumPublishResult>, String> {
    tauri::async_runtime::spawn_blocking(move || run_forum_publish(request, app))
        .await
        .map_err(|error| format!("게시 실행 스레드 오류: {error}"))
}

#[cfg(target_os = "windows")]
fn launch_incognito_chrome() -> Result<String, String> {
    let chrome_path = find_windows_chrome_path().ok_or_else(|| {
        "Chrome 실행 파일을 찾지 못했습니다. Google Chrome 설치를 확인하세요.".to_owned()
    })?;
    let profile_dir = std::env::temp_dir().join("pstmacro-chrome-incognito-debug");
    std::fs::create_dir_all(&profile_dir)
        .map_err(|error| format!("Chrome 임시 프로필 폴더 생성 실패: {error}"))?;

    spawn_chrome(chrome_path, profile_dir)?;
    Ok("시크릿 Chrome을 열었습니다. 열린 창에서 네이버 로그인 후 실행하세요.".to_owned())
}

#[cfg(not(target_os = "windows"))]
fn launch_incognito_chrome() -> Result<String, String> {
    let cmd_path = PathBuf::from("/mnt/c/Windows/System32/cmd.exe");

    if !cmd_path.exists() {
        return Err(
            "시크릿 Chrome 자동 실행은 Windows 배포판에서 사용합니다. 개발 환경에서는 기존 실행 스크립트로 Chrome을 먼저 여세요."
                .to_owned(),
        );
    }

    Command::new(cmd_path)
        .args([
            "/C",
            "start",
            "",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            "--remote-debugging-port=9222",
            "--remote-debugging-address=127.0.0.1",
            "--user-data-dir=%TEMP%\\pstmacro-chrome-incognito-debug",
            "--incognito",
            "--disable-quic",
            "--no-first-run",
            "--no-default-browser-check",
            "https://www.naver.com",
        ])
        .spawn()
        .map_err(|error| format!("시크릿 Chrome 실행 실패: {error}"))?;

    Ok("시크릿 Chrome을 열었습니다. 열린 창에서 네이버 로그인 후 실행하세요.".to_owned())
}

#[cfg(target_os = "windows")]
fn find_windows_chrome_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            candidates.push(
                PathBuf::from(base)
                    .join("Google")
                    .join("Chrome")
                    .join("Application")
                    .join("chrome.exe"),
            );
        }
    }

    candidates.into_iter().find(|path| path.exists())
}

#[cfg(target_os = "windows")]
fn spawn_chrome(chrome_path: PathBuf, profile_dir: PathBuf) -> Result<(), String> {
    let profile_arg = format!("--user-data-dir={}", profile_dir.display());

    Command::new(chrome_path)
        .args([
            "--remote-debugging-port=9222",
            "--remote-debugging-address=127.0.0.1",
            profile_arg.as_str(),
            "--incognito",
            "--disable-quic",
            "--no-first-run",
            "--no-default-browser-check",
            "https://www.naver.com",
        ])
        .spawn()
        .map_err(|error| format!("시크릿 Chrome 실행 실패: {error}"))?;

    Ok(())
}

fn parse_automation_target(target: Option<String>) -> Result<AutomationTarget, String> {
    match target
        .unwrap_or_else(|| "post".to_owned())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "post" | "write" | "글쓰기" => Ok(AutomationTarget::Post),
        "comment" | "reply" | "댓글" => Ok(AutomationTarget::Comment),
        _ => Err("target 값은 post 또는 comment 여야 합니다.".to_owned()),
    }
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
            queue::add_queue_scheduled,
            queue::promote_queue_scheduled,
            stocks::list_stocks,
            activity::list_activity,
            stats::list_stats,
            log_batches::list_log_batches,
            cafes::list_cafes,
            bands::list_bands,
            bootstrap_runtime,
            save_accounts,
            enqueue_cookie_refresh,
            get_queue_status,
            get_account_cookies,
            run_naver_discussion,
            parse_template_csv,
            search_stocks,
            open_incognito_chrome,
            run_naver_discussion_batch,
            run_forum_publish_now,
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
