mod ipc;
mod logging;
mod store;
mod util;

use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::{AppHandle, Builder, Manager, Runtime};

use crate::ipc::{
    accounts, activity, bands, cafes, diagnostics, excel, log_batches, posts, queue, stats, stocks,
};
use crate::store::JsonStore;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod auth;
pub mod naver_cafe;
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
async fn run_naver_discussion_batch<R: Runtime>(
    app: tauri::AppHandle<R>,
    request: DiscussionBatchRequest,
) -> Result<DiscussionBatchReport, String> {
    // 동기 블로킹 작업을 스레드 풀에서 실행해 GTK 메인 루프를 막지 않습니다.
    // 메인 루프가 자유로워야 Rust에서 emit한 이벤트가 프론트엔드에 전달됩니다.
    tauri::async_runtime::spawn_blocking(move || run_discussion_batch(request, app))
        .await
        .map_err(|error| format!("배치 실행 스레드 오류: {error}"))?
}

// 패킷 게시 엔진이 붙는 로컬 Chrome DevTools 엔드포인트의 단일 출처(백엔드 소유).
// 포트를 프론트엔드 상수로 두지 않고 command로 노출해, 프론트는 이 값을 받아 쓴다.
const FORUM_DEVTOOLS_HOST: &str = "127.0.0.1";
const FORUM_DEVTOOLS_PORT: u16 = 9222;

/// 패킷 게시 엔진이 붙을 Chrome DevTools 엔드포인트(host/port).
#[derive(serde::Serialize)]
struct ForumEndpoint {
    host: String,
    port: u16,
}

/// 프론트엔드(publish-modal)가 게시 엔드포인트를 백엔드에서 받아오도록 노출하는 command.
#[tauri::command]
fn forum_endpoint() -> ForumEndpoint {
    ForumEndpoint {
        host: FORUM_DEVTOOLS_HOST.to_owned(),
        port: FORUM_DEVTOOLS_PORT,
    }
}

fn build_publish_batch(
    title: &str,
    run_post: bool,
    run_comment: bool,
    account_id: &str,
    body: &str,
    comment: &str,
    at: i64,
    results: &[ForumPublishResult],
) -> ipc::log_batches::LogBatch {
    use std::sync::atomic::{AtomicU64, Ordering};

    use ipc::accounts::PlatformId;
    use ipc::log_batches::{BatchItem, BatchItemStatus, LogBatch};
    use ipc::posts::ModeValue;

    static LB_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LB_SEQ.fetch_add(1, Ordering::Relaxed);

    let kind = if run_post && run_comment {
        ModeValue::Both
    } else if run_comment {
        ModeValue::Comment
    } else {
        // (false, false) is rejected upstream; default to Post for a total match.
        ModeValue::Post
    };
    let items = results
        .iter()
        .map(|r| BatchItem {
            platform: PlatformId::Forum,
            target: r.name.clone(),
            code: Some(r.code.clone()),
            board: None,
            login_id: account_id.to_owned(),
            status: if r.ok {
                BatchItemStatus::Success
            } else {
                BatchItemStatus::Fail
            },
            msg: r.message.clone(),
            trace: if r.ok { None } else { Some(r.message.clone()) },
        })
        .collect();
    LogBatch {
        id: format!("lb-{at}-{seq}"),
        title: title.to_owned(),
        // 게시 시점 원문 스냅샷: 실제 게시한 것만 남긴다.
        body: if run_post && !body.is_empty() {
            Some(body.to_owned())
        } else {
            None
        },
        comment: if run_comment && !comment.is_empty() {
            Some(comment.to_owned())
        } else {
            None
        },
        kind,
        at,
        state: None,
        items,
    }
}

// 사수 UI(publish-modal)의 "지금 바로 게시 + 종목토론방"이 호출하는 command입니다.
#[tauri::command]
async fn run_forum_publish_now<R: Runtime>(
    app: tauri::AppHandle<R>,
    request: ForumPublishRequest,
) -> Result<Vec<ForumPublishResult>, String> {
    let title = request.title.clone();
    let body = request.body.clone();
    let comment = request.comment.clone();
    let account_id = request.account_id.clone();
    let (run_post, run_comment) = (request.run_post, request.run_comment);
    let app_for_job = app.clone();
    let results =
        tauri::async_runtime::spawn_blocking(move || -> Result<Vec<ForumPublishResult>, String> {
            // 게시용 Chrome을 앱이 직접 디버그 포트로 띄운다(헤드리스). 사용자가 따로
            // `--remote-debugging-port`로 Chrome을 실행할 필요가 없다. 게시가 끝나면
            // 핸들이 Drop되며 Chrome을 종료한다. (로그인과 같은 런처 재사용)
            let chrome = auth::launch_debug_chrome(true).map_err(|error| error.to_string())?;
            let mut request = request;
            request.host = FORUM_DEVTOOLS_HOST.to_owned();
            request.port = chrome.port;
            let results = run_forum_publish(request, app_for_job);
            drop(chrome);
            Ok(results)
        })
        .await
        .map_err(|error| format!("게시 실행 스레드 오류: {error}"))??;

    let at = util::now_ms();
    let batch = build_publish_batch(
        &title,
        run_post,
        run_comment,
        &account_id,
        &body,
        &comment,
        at,
        &results,
    );
    let ok = results.iter().filter(|r| r.ok).count();
    let logs = app.state::<JsonStore<ipc::log_batches::LogBatch>>();
    logs.mutate(|mut v| {
        v.insert(0, batch);
        v.truncate(ipc::log_batches::MAX_LOG_BATCHES);
        v
    });
    let activity = app.state::<JsonStore<ipc::activity::ActivityItem>>();
    ipc::activity::record(
        activity.inner(),
        if ok == results.len() {
            ipc::activity::ActivityType::Success
        } else {
            ipc::activity::ActivityType::Error
        },
        format!("'{title}' 게시 — {}곳 중 {ok}곳 성공", results.len()),
    );
    Ok(results)
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
fn append_activity(
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    kind: String,
    text: String,
) {
    use ipc::activity::ActivityType;
    let ty = match kind.as_str() {
        "success" => ActivityType::Success,
        "error" => ActivityType::Error,
        _ => ActivityType::Info,
    };
    ipc::activity::record(activity.inner(), ty, text);
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
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    account_ids: Vec<String>,
    headless: Option<bool>,
    use_adb: Option<bool>,
    force: Option<bool>,
) -> Result<auth::QueueStatus, String> {
    let n = account_ids.len();
    let result = auth::enqueue_accounts(
        &state,
        app,
        account_ids,
        headless.unwrap_or(false),
        use_adb.unwrap_or(false),
        force.unwrap_or(false),
    )
    .map_err(|e| e.to_string())?;
    if n > 0 {
        ipc::activity::record(
            activity.inner(),
            ipc::activity::ActivityType::Info,
            format!("계정 {n}건 로그인 시작"),
        );
    }
    Ok(result)
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

#[tauri::command]
fn export_accounts_xlsx(
    store: tauri::State<'_, JsonStore<ipc::accounts::Account>>,
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    path: String,
) -> Result<(), String> {
    let accounts = store.snapshot();
    let n = accounts.len();
    excel::write_accounts_xlsx(&path, &accounts)?;
    ipc::activity::record(
        activity.inner(),
        ipc::activity::ActivityType::Info,
        format!("계정 {n}건을 엑셀로 내보냈어요"),
    );
    Ok(())
}

#[tauri::command]
fn export_activity_xlsx(
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    logs: tauri::State<'_, JsonStore<ipc::log_batches::LogBatch>>,
    path: String,
) -> Result<(), String> {
    excel::write_activity_xlsx(&path, &logs.snapshot(), &activity.snapshot())?;
    ipc::activity::record(
        activity.inner(),
        ipc::activity::ActivityType::Info,
        "알림 내역을 엑셀로 내보냈어요",
    );
    Ok(())
}

#[tauri::command]
fn import_accounts_xlsx(
    store: tauri::State<'_, JsonStore<ipc::accounts::Account>>,
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    path: String,
) -> Result<ipc::excel::ImportSummary, String> {
    let (next, summary) = ipc::excel::import_accounts(&path, store.snapshot())?;
    store.mutate(|_| next.clone());
    ipc::activity::record(
        activity.inner(),
        ipc::activity::ActivityType::Info,
        format!("엑셀에서 계정 {}건 가져옴", summary.imported),
    );
    Ok(summary)
}

#[tauri::command]
fn import_posts_xlsx(
    store: tauri::State<'_, JsonStore<ipc::posts::LibraryPost>>,
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    path: String,
) -> Result<ipc::excel::ImportSummary, String> {
    let (next, summary) = ipc::excel::import_posts(&path, store.snapshot())?;
    store.mutate(|_| next.clone());
    ipc::activity::record(
        activity.inner(),
        ipc::activity::ActivityType::Info,
        format!("엑셀에서 게시글 {}건 가져옴", summary.imported),
    );
    Ok(summary)
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
        queue::reorder_queue_now,
        stocks::list_stocks,
        activity::list_activity,
        append_activity,
        stats::list_stats,
        log_batches::list_log_batches,
        cafes::list_cafes,
        cafes::resolve_cafe,
        cafes::upsert_cafe,
        cafes::run_post_jobs,
        cafes::run_comment_jobs,
        cafes::list_joined_cafes,
        cafes::list_cafe_articles,
        bands::list_bands,
        diagnostics::get_environment_status,
        diagnostics::open_chrome_download,
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
        forum_endpoint,
        run_forum_publish_now,
        export_accounts_xlsx,
        export_activity_xlsx,
        import_accounts_xlsx,
        import_posts_xlsx,
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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            // 로그는 도메인 데이터와 같은 앱 데이터 디렉터리(<app_data>/logs)에 남긴다.
            logging::init_file_logging(&dir.join("logs"));
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                "pstmacro backend starting"
            );
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
    fn build_publish_batch_maps_results_to_items() {
        let results = vec![
            ForumPublishResult {
                code: "005930".into(),
                name: "삼성전자".into(),
                ok: true,
                message: "게시 완료".into(),
            },
            ForumPublishResult {
                code: "000660".into(),
                name: "SK하이닉스".into(),
                ok: false,
                message: "로그인 만료".into(),
            },
        ];
        let b = build_publish_batch(
            "실적 정리",
            true,
            false,
            "invest_king7",
            "",
            "",
            1_700_000_000_000,
            &results,
        );
        assert_eq!(b.items.len(), 2);
        assert_eq!(b.title, "실적 정리");
        assert!(matches!(b.kind, ipc::posts::ModeValue::Post));
        assert!(matches!(
            b.items[0].status,
            ipc::log_batches::BatchItemStatus::Success
        ));
        assert_eq!(b.items[1].trace.as_deref(), Some("로그인 만료"));
    }

    #[test]
    fn build_publish_batch_snapshots_nonempty_body_and_comment() {
        let results = vec![ForumPublishResult {
            code: "005930".into(),
            name: "삼성전자".into(),
            ok: true,
            message: "게시 완료".into(),
        }];
        let b = build_publish_batch(
            "제목",
            true,
            true,
            "acct",
            "본문내용",
            "댓글내용",
            1,
            &results,
        );
        assert_eq!(b.body.as_deref(), Some("본문내용"));
        assert_eq!(b.comment.as_deref(), Some("댓글내용"));
    }

    #[test]
    fn build_publish_batch_omits_empty_and_unused_text() {
        let results = vec![];
        // run_comment=false → comment 무시; body 빈 문자열 → None
        let b = build_publish_batch("제목", true, false, "acct", "", "안쓴댓글", 1, &results);
        assert_eq!(b.body, None);
        assert_eq!(b.comment, None);
    }

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
