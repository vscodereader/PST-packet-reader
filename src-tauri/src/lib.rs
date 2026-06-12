mod ipc;
mod logging;
mod store;
mod util;

use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Builder, Manager, Runtime, WindowEvent};

use crate::ipc::{
    accounts, activity, bands, cafes, diagnostics, excel, log_batches, posts, queue, stats, stocks,
};
use crate::store::JsonStore;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod auth;
// band.us 이메일 로그인(네이버 로그인 병행 모듈).
pub mod band_auth;
// band.us 가입·글쓰기·댓글(순수 HTTP, md 서명). band_auth 로그인 쿠키를 소비한다.
pub mod band_post;
pub mod naver_cafe;
// 네이버 증권 토론방 패킷 게시 엔진.
pub mod discussion_batch;
mod forum_stocks;
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

// 게시 결과를 LogBatch로 묶는 단일 호출 빌더라, 인자가 8개여도 구조체로 묶을 실익이
// 적다. clippy 한도(7)만 넘으므로 이 함수에 한해 허용한다.
#[allow(clippy::too_many_arguments)]
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
fn enqueue_band_login<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, band_auth::BandQueueState>,
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    account_ids: Vec<String>,
    headless: Option<bool>,
    use_adb: Option<bool>,
    force: Option<bool>,
) -> Result<auth::QueueStatus, String> {
    let n = account_ids.len();
    let result = band_auth::enqueue_band_accounts(
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
            format!("밴드 계정 {n}건 로그인 시작"),
        );
    }
    Ok(result)
}

#[tauri::command]
fn get_band_queue_status(
    state: tauri::State<'_, band_auth::BandQueueState>,
) -> Result<auth::QueueStatus, String> {
    band_auth::get_band_queue_status(&state).map_err(|e| e.to_string())
}

/// 밴드 링크로 가입한 뒤 글(+선택 댓글)을 순수 HTTP로 게시한다.
///
/// `account_id`는 band 로그인 쿠키 파일 키(loginId)다. `band_link`로 가입 →
/// 제목·내용 게시 → 댓글(있으면) 순으로 진행한다(band_post::band_publish).
#[tauri::command]
async fn band_publish(
    account_id: String,
    band_link: String,
    title: String,
    content: String,
    comments: Vec<String>,
) -> Result<band_post::BandPublishOutcome, String> {
    band_post::band_publish(&account_id, &band_link, &title, &content, &comments)
        .await
        .map_err(|e| e.to_string())
}

/// 밴드 댓글 전용: 기존 글(최신글/인기글) 상위 `count`개를 조회해 댓글을 단다.
/// `mode`는 `"latest"`(최신글) 또는 `"popular"`(인기글).
#[tauri::command]
async fn band_comment(
    account_id: String,
    band_link: String,
    mode: String,
    count: u32,
    comments: Vec<String>,
) -> Result<band_post::BandCommentOutcome, String> {
    let sort = match mode.as_str() {
        "popular" => band_post::BandFeedSort::Popular,
        _ => band_post::BandFeedSort::Latest,
    };
    band_post::band_comment(&account_id, &band_link, sort, count, &comments)
        .await
        .map_err(|e| e.to_string())
}

/// 링크(band_no)로 밴드 이름을 조회한다(게시 모달에서 링크 저장 시 실제 밴드명 표시용).
#[tauri::command]
async fn band_resolve_name(account_id: String, band_link: String) -> Result<String, String> {
    band_post::resolve_band_name(&account_id, &band_link)
        .await
        .map_err(|e| e.to_string())
}

/// 프론트가 보내는 밴드 게시 결과 1건(알림 배치 기록용 최소 입력).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BandBatchItemInput {
    target: String,
    login_id: String,
    ok: bool,
    msg: String,
}

/// `run_post`/`run_comment` 플래그를 배치 kind로 변환한다(순수 함수, 테스트 가능).
fn band_batch_kind(run_post: bool, run_comment: bool) -> ipc::posts::ModeValue {
    use ipc::posts::ModeValue;
    if run_post && run_comment {
        ModeValue::Both
    } else if run_comment {
        ModeValue::Comment
    } else {
        ModeValue::Post
    }
}

/// 밴드 게시 결과를 알림(게시 배치)에 기록한다.
///
/// 종토방(`run_forum_publish_now`)이 백엔드에서 LogBatch를 남기는 것과 동일하게,
/// 프론트(publish-modal)가 모든 밴드 잡을 마친 뒤 한 번 호출해 `platform: Band` 배치 +
/// activity를 저장한다. 밴드 게시는 프론트가 잡별로 호출하므로 집계는 여기서 한 번에 한다.
#[tauri::command]
fn record_band_batch<R: Runtime>(
    app: tauri::AppHandle<R>,
    title: String,
    body: String,
    comment: String,
    run_post: bool,
    run_comment: bool,
    items: Vec<BandBatchItemInput>,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};

    use ipc::accounts::PlatformId;
    use ipc::log_batches::{BatchItem, BatchItemStatus, LogBatch, MAX_LOG_BATCHES};

    static LB_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LB_SEQ.fetch_add(1, Ordering::Relaxed);

    let at = util::now_ms();
    let total = items.len();
    let ok = items.iter().filter(|i| i.ok).count();
    let batch_items: Vec<BatchItem> = items
        .into_iter()
        .map(|i| BatchItem {
            platform: PlatformId::Band,
            target: i.target,
            code: None,
            board: None,
            login_id: i.login_id,
            status: if i.ok {
                BatchItemStatus::Success
            } else {
                BatchItemStatus::Fail
            },
            // 실패 행은 trace에도 메시지를 실어 "자세히 보기"에서 원인을 본다.
            trace: if i.ok { None } else { Some(i.msg.clone()) },
            msg: i.msg,
        })
        .collect();

    let batch = LogBatch {
        id: format!("lb-band-{at}-{seq}"),
        title: title.clone(),
        // 게시 시점 원문 스냅샷: 실제 게시(run_post)한 것만 남긴다(종토방 배치와 동일 규칙).
        body: if run_post && !body.is_empty() {
            Some(body)
        } else {
            None
        },
        comment: if run_comment && !comment.is_empty() {
            Some(comment)
        } else {
            None
        },
        kind: band_batch_kind(run_post, run_comment),
        at,
        state: None,
        items: batch_items,
    };

    let logs = app.state::<JsonStore<LogBatch>>();
    logs.mutate(|mut v| {
        v.insert(0, batch);
        v.truncate(MAX_LOG_BATCHES);
        v
    });

    let activity = app.state::<JsonStore<ipc::activity::ActivityItem>>();
    ipc::activity::record(
        activity.inner(),
        if total > 0 && ok == total {
            ipc::activity::ActivityType::Success
        } else {
            ipc::activity::ActivityType::Error
        },
        format!("밴드 '{title}' 게시 — {total}곳 중 {ok}곳 성공"),
    );
    Ok(())
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
        queue::add_queue_now,
        queue::add_queue_scheduled,
        queue::promote_queue_scheduled,
        queue::reschedule_queue_scheduled,
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
        enqueue_band_login,
        get_band_queue_status,
        band_publish,
        band_comment,
        band_resolve_name,
        get_account_cookies,
        run_naver_discussion,
        parse_template_csv,
        search_stocks,
        forum_stocks::list_forum_stocks,
        forum_stocks::search_forum_stocks,
        open_incognito_chrome,
        run_naver_discussion_batch,
        forum_endpoint,
        run_forum_publish_now,
        record_band_batch,
        export_accounts_xlsx,
        export_activity_xlsx,
        import_accounts_xlsx,
        import_posts_xlsx,
        get_autostart_enabled,
        set_autostart,
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
    app.manage(band_auth::BandQueueState::default());
    // 게시 큐 실행 워커 상태(promote 시 기동, 이슈 #144).
    app.manage(ipc::queue_runner::NowQueueRunner::default());
    Ok(())
}

/// 프로세스가 부팅 자동 시작(`--autostart` 인자)으로 실행됐는지 판별한다. 자동 시작이면
/// 창을 숨긴 채(트레이) 시작한다(재부팅 후 조용히 백그라운드 복귀).
/// 부팅 자동 시작으로 실행됐음을 표시하는 인자. 플러그인 등록(`init`)과
/// `launched_via_autostart` 검사가 같은 값을 쓰도록 상수로 묶는다.
const AUTOSTART_FLAG: &str = "--autostart";

fn launched_via_autostart<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter().any(|arg| arg == AUTOSTART_FLAG)
}

/// 부팅 자동 시작 등록 여부를 돌려준다(설정 토글 표시용).
#[tauri::command]
fn get_autostart_enabled<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// 부팅 자동 시작 등록을 켜고/끈다(OS 로그인 시 자동 실행). 갱신된 상태를 돌려준다.
#[tauri::command]
fn set_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())?;
    } else {
        manager.disable().map_err(|e| e.to_string())?;
    }
    manager.is_enabled().map_err(|e| e.to_string())
}

/// 메인 창을 보이게 하고 포커스한다(트레이 "창 열기"·아이콘 클릭에서 호출).
fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 시스템 트레이 아이콘 + 메뉴(창 열기 / 완전 종료)를 구성한다. 창을 닫아도(트레이로
/// 숨김) 백그라운드 스케줄러가 살아 있으므로, 트레이에서 창을 다시 열거나 완전히 종료한다.
fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray-show", "창 열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "완전 종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray");
    // 번들 아이콘이 있으면 트레이 아이콘으로 쓴다(없어도 패닉 없이 트레이는 만든다).
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder
        .tooltip("pstmacro — 예약 게시 백그라운드 실행 중")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-show" => show_main_window(app),
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 좌클릭(버튼 떼는 순간)으로 창을 다시 연다.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register_handlers(
        tauri::Builder::default()
            // 단일 인스턴스 가드. 이미 트레이로 상주 중인데 앱을 다시 실행하면, 새 프로세스는
            // 곧바로 종료되고 이 콜백이 기존 프로세스에서 호출된다 → 트레이가 두 개로 늘지 않고
            // 숨어 있던 창만 다시 뜬다. 플러그인은 등록 순서대로 동작하므로 가장 먼저 등록한다.
            .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
                show_main_window(app);
            })),
    )
    .plugin(tauri_plugin_dialog::init())
    // 데스크톱(OS) 토스트 — 트레이 상주 중 게시 완료·놓침을 능동 통지(#163).
    // Rust 측에서만 발송하므로 별도 capability 권한 엔트리는 필요 없다.
    .plugin(tauri_plugin_notification::init())
    // 부팅 자동 시작 플러그인. 자동 시작으로 실행되면 `--autostart` 인자가 붙어
    // (아래 setup에서 창을 숨긴 채 시작). 등록 on/off는 set_autostart 커맨드로 한다.
    .plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        Some(vec![AUTOSTART_FLAG]),
    ))
    // 창 닫기(X)를 종료가 아니라 트레이로 숨김 처리 → 백그라운드 스케줄러 유지.
    // 완전 종료는 트레이 메뉴의 "완전 종료"로 한다.
    .on_window_event(|window, event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            let _ = window.hide();
            api.prevent_close();
        }
    })
    .setup(|app| {
        let dir = app.path().app_data_dir()?;
        // 로그는 도메인 데이터와 같은 앱 데이터 디렉터리(<app_data>/logs)에 남긴다.
        logging::init_file_logging(&dir.join("logs"));
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            "pstmacro backend starting"
        );
        manage_stores(app.handle(), &dir)?;
        // 창을 닫아도 백그라운드로 도는 앱이므로 트레이 아이콘을 띄운다. 트레이 생성
        // 실패는 비치명적으로 둔다 — 앱(과 창)은 정상 동작해야 한다(창이 숨은 채로
        // brick 되지 않게).
        if let Err(error) = build_tray(app.handle()) {
            tracing::error!(%error, "트레이 아이콘 생성 실패 — 트레이 없이 계속");
        }
        // 창은 기본 숨김(conf visible:false). 일반 실행이면 보이고, 부팅 자동 시작
        // (`--autostart`)이면 숨긴 채 트레이로만 시작한다.
        if !launched_via_autostart(std::env::args()) {
            show_main_window(app.handle());
        }
        // 앱 시작 reconciliation: 종료 중 시각이 지난 미발행 예약을 missed로 표시하고
        // 알림으로 남긴다(자동 게시하지 않음). 반드시 스케줄러 spawn 전에 동기 수행해
        // 첫 tick이 미발행 예약을 잘못 게시하지 않게 한다.
        ipc::queue::reconcile_missed_on_startup(app.handle());
        // 예약 시각 자동 트리거 스케줄러를 기동한다(앱 수명 동안 1회).
        let scheduler_app = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            ipc::queue::scheduler_loop(scheduler_app).await;
        });
        Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launched_via_autostart_detects_the_flag() {
        let with = ["pstmacro.exe", "--autostart"].map(String::from);
        let without = ["pstmacro.exe"].map(String::from);
        assert!(launched_via_autostart(with));
        assert!(!launched_via_autostart(without));
        assert!(!launched_via_autostart(Vec::<String>::new()));
    }

    #[test]
    fn band_batch_kind_maps_run_flags() {
        use ipc::posts::ModeValue;
        assert_eq!(band_batch_kind(true, false), ModeValue::Post);
        assert_eq!(band_batch_kind(false, true), ModeValue::Comment);
        assert_eq!(band_batch_kind(true, true), ModeValue::Both);
    }

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
