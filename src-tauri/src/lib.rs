mod agent;
mod ipc;
mod logging;
mod store;
mod template_tokens;
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
// 네이버 블로그 댓글 게시(#271). 카페 저장 쿠키를 재사용하는 댓글 전용 모듈(별도 로그인 없음).
pub mod naver_blog;
pub mod naver_clip;
// 네이버 증권 토론방 패킷 게시 엔진.
pub mod discussion_batch;
mod forum_stocks;
pub mod naver_automation;
// 조회수 부스트: 시크릿창을 여닫으며 게시글 조회수를 올린다(#400). launch_debug_chrome + CdpClient 재사용.
pub mod view_boost;

use discussion_batch::{
    parse_discussion_template_csv, run_discussion_batch, run_forum_publish, search_naver_stocks,
    DiscussionBatchReport, DiscussionBatchRequest, ForumPublishRequest, ForumPublishResult,
    StockCandidate, TemplateColumns,
};
use naver_automation::{
    run_naver_discussion_macro, run_naver_dislike, run_naver_like, AutomationReport,
    AutomationTarget, LikeVerdict, NaverDiscussionRequest,
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
    run_naver_discussion_macro(
        NaverDiscussionRequest {
            title,
            body,
            host: host.unwrap_or_else(|| "127.0.0.1".to_owned()),
            port: port.unwrap_or(9222),
            target: parse_automation_target(target)?,
            submit_after_fill: submit_after_fill.unwrap_or(false),
            stock: None,
            account_id: None,
            comment_url: None,
            comment_nickname_random: false,
            content_change: None,
        },
        &mut std::collections::HashSet::new(),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn parse_template_csv(csv_text: String) -> Result<TemplateColumns, String> {
    parse_discussion_template_csv(csv_text)
}

/// 글 관리 화면 "좋아요" 버튼의 한 (계정 × 링크) 처리 결과(프론트 표시용).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LikeOutcome {
    /// 좋아요를 시도한 계정 ID.
    account_id: String,
    /// 좋아요를 누른 게시글 링크(여러 링크 중 어느 것인지 표시용).
    post_url: String,
    /// 성공 여부(이미 좋아요 상태여도 성공으로 본다).
    success: bool,
    /// 표시용 메시지(성공 문구 또는 실패 사유).
    message: String,
}

/// "좋아요" 버튼: **여러 게시글 링크 × 선택한 계정들**의 모든 조합에 좋아요를 누른다. 페이지 이동
/// 없이 reactions API로만 처리하고(사수 지시), 호출 사이에 짧은 간격을 둬 연속요청 차단을 피한다.
/// 하나가 실패해도 중단하지 않고 다음으로 넘어가며, (계정×링크)별 성공/실패를 모아 돌려준다.
/// 좋아요 결과를 알림 로그(log_batches)에 남긴다 — 게시처럼 알림 패널에 뜨게 한다(사용자 지적
/// 2026-07-01: 좋아요가 토스트만 뜨고 알림엔 안 남았다). (계정×링크)별 성공/실패를 한 배치로 묶는다.
/// 좋아요 판정이 재로그인/비활성이면 **계정 상태를 바꾸고 쿠키를 지운다**(사용자 요청 2026-07-03:
/// 세션 만료=재로그인·차단=비활성으로 상태 전환, 둘 다 쿠키만료값 삭제). 좋아요는 프론트가
/// **loginId**(쿠키 키)로 넘기므로 `apply_status_by_login_id`(login_id 매칭)로 갱신하고
/// (account.id 매칭은 상태가 안 바뀌던 #383 회귀라 금지), `store.mutate`가 디스크 저장 + 프론트
/// 이벤트를 발생시킨다.
fn mark_account_status<R: Runtime>(
    app: &tauri::AppHandle<R>,
    login_id: &str,
    status: ipc::accounts::AccountStatus,
    msg: &str,
) {
    // 좋아요는 프론트가 **loginId**(쿠키 키, like-modal.tsx §85)로 넘긴다. 계정 상태 행도 login_id로
    // 매칭해야 갱신된다(id로 매칭하면 안 맞아 상태가 안 바뀜 — #383 회귀 원인).
    let store = app.state::<JsonStore<ipc::accounts::Account>>();
    let id = login_id.to_owned();
    let msg_owned = msg.to_owned();
    store.mutate(move |accounts| {
        // 좋아요 경로는 백트레이스가 없으므로 status_trace=None(#324 병합: 5번째 인자 추가됨).
        ipc::accounts::apply_status_by_login_id(accounts, &id, status, Some(msg_owned), None)
    });
    // 만료/차단 계정의 "쿠키만료" 카운트다운 제거 + 죽은/차단 세션 쿠키 삭제(쿠키 파일 키=loginId).
    if let Err(error) = crate::auth::clear_account_cookies(login_id) {
        tracing::warn!(
            account = %crate::auth::mask_id(login_id),
            error = %error,
            "좋아요 후 쿠키 삭제 실패"
        );
    }
}

fn record_like_batch<R: Runtime>(app: &tauri::AppHandle<R>, outcomes: &[LikeOutcome], label: &str) {
    use std::sync::atomic::{AtomicU64, Ordering};

    use ipc::accounts::PlatformId;
    use ipc::log_batches::{BatchItem, BatchItemStatus, LogBatch, MAX_LOG_BATCHES};

    if outcomes.is_empty() {
        return;
    }
    static LB_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LB_SEQ.fetch_add(1, Ordering::Relaxed);
    let at = util::now_ms();
    let items: Vec<BatchItem> = outcomes
        .iter()
        .map(|o| BatchItem {
            platform: PlatformId::Forum,
            target: o.post_url.clone(),
            code: None,
            board: None,
            login_id: o.account_id.clone(),
            status: if o.success {
                BatchItemStatus::Success
            } else {
                BatchItemStatus::Fail
            },
            msg: o.message.clone(),
            trace: if o.success {
                None
            } else {
                Some(o.message.clone())
            },
            posted: None,
        })
        .collect();
    let batch = LogBatch {
        id: format!("lb-react-{at}-{seq}"),
        title: label.to_owned(),
        body: None,
        comment: None,
        kind: ipc::posts::ModeValue::Post,
        at,
        state: None,
        items,
    };
    let logs = app.state::<JsonStore<LogBatch>>();
    logs.mutate(|mut v| {
        v.insert(0, batch);
        v.truncate(MAX_LOG_BATCHES);
        v
    });
}

/// 좋아요·싫어요 **공용** 배치 실행 — 계정×링크마다 reactions API를 눌러 결과를 모은다. 좋아요와
/// 싫어요는 패킷상 reactionType만 다르므로(URL·헤더 동일) 이 하나를 공유한다. `reaction_type`:
/// `"good"`=좋아요 / `"bad"`=싫어요, `label`: 사용자 메시지·알림 제목용(좋아요/싫어요).
async fn run_reaction_batch<R: Runtime>(
    app: tauri::AppHandle<R>,
    post_urls: Vec<String>,
    account_ids: Vec<String>,
    reaction_type: &'static str,
    label: &'static str,
) -> Result<Vec<LikeOutcome>, String> {
    let post_urls: Vec<String> = post_urls
        .into_iter()
        .map(|u| u.trim().to_owned())
        .filter(|u| !u.is_empty())
        .collect();
    if post_urls.is_empty() {
        return Err(format!(
            "{label}를 누를 게시글 링크를 한 개 이상 입력하세요."
        ));
    }
    if account_ids.is_empty() {
        return Err(format!("{label}를 누를 계정을 한 개 이상 선택하세요."));
    }
    // 반응 판정이 재로그인/비활성이면 계정 상태를 바꾸고 쿠키를 지워야 하므로 store 접근용으로
    // app을 블로킹 클로저에 함께 넘긴다(로그 배치 기록은 클로저 밖 record_like_batch가 담당).
    let app_for_status = app.clone();
    let is_dislike = reaction_type == "bad";
    let outcomes = tauri::async_runtime::spawn_blocking(move || {
        let mut outcomes = Vec::with_capacity(account_ids.len() * post_urls.len());
        let mut first = true;
        for account_id in &account_ids {
            for post_url in &post_urls {
                // 연속요청 도배 차단 회피용 간격(첫 호출 제외). 반응은 순식간이라 호출마다 텀을 둔다.
                if !first {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                }
                first = false;
                let verdict = if is_dislike {
                    run_naver_dislike(account_id, post_url)
                } else {
                    run_naver_like(account_id, post_url)
                };
                let (success, message) = match verdict {
                    LikeVerdict::Liked => (true, format!("{label} 완료")),
                    // 세션 만료 → 상태 '재로그인' + 쿠키 삭제(재로그인해야 회복).
                    LikeVerdict::Relogin(msg) => {
                        mark_account_status(
                            &app_for_status,
                            account_id,
                            ipc::accounts::AccountStatus::Relogin,
                            &msg,
                        );
                        (false, msg)
                    }
                    // 계정 차단 → 상태 '비활성(Blocked)' + 쿠키 삭제.
                    LikeVerdict::Blocked(msg) => {
                        mark_account_status(
                            &app_for_status,
                            account_id,
                            ipc::accounts::AccountStatus::Blocked,
                            &msg,
                        );
                        (false, msg)
                    }
                    // 글 삭제(404) 등 계정 문제 아님 — 상태는 바꾸지 않는다.
                    LikeVerdict::Failed(msg) => (false, msg),
                };
                outcomes.push(LikeOutcome {
                    account_id: account_id.clone(),
                    post_url: post_url.clone(),
                    success,
                    message,
                });
            }
        }
        outcomes
    })
    .await
    .map_err(|error| format!("{label} 작업 실행 실패: {error}"))?;
    // 반응 결과를 알림 로그에 기록 — 게시처럼 알림 패널에 남게 한다(사용자 지적: 토스트만 뜨고 알림엔 안 남음).
    record_like_batch(&app, &outcomes, label);
    Ok(outcomes)
}

#[tauri::command]
async fn like_discussion_post<R: Runtime>(
    app: tauri::AppHandle<R>,
    post_urls: Vec<String>,
    account_ids: Vec<String>,
) -> Result<Vec<LikeOutcome>, String> {
    run_reaction_batch(app, post_urls, account_ids, "good", "좋아요").await
}

#[tauri::command]
async fn dislike_discussion_post<R: Runtime>(
    app: tauri::AppHandle<R>,
    post_urls: Vec<String>,
    account_ids: Vec<String>,
) -> Result<Vec<LikeOutcome>, String> {
    run_reaction_batch(app, post_urls, account_ids, "bad", "싫어요").await
}

/// "조회수" 버튼: **여러 게시글 링크**를 각각 시크릿창으로 `repeats`번 여닫아 조회수를 올린다.
/// 각 회차는 "열기→완전로딩→그 창만 종료"로, 창을 완전히 닫은 뒤에야 다음
/// 회차를 연다(고아 프로세스 없음). 좋아요·게시 등 기존 기능은 건드리지 않는다(#400).
#[tauri::command]
async fn boost_view_count(
    links: Vec<String>,
    repeats: u32,
) -> Result<Vec<view_boost::ViewBoostOutcome>, String> {
    let links: Vec<String> = links
        .into_iter()
        .map(|link| link.trim().to_owned())
        .filter(|link| !link.is_empty())
        .collect();
    if links.is_empty() {
        return Err("조회수를 올릴 게시글 링크가 없습니다.".to_owned());
    }
    if repeats == 0 {
        return Err("반복 횟수는 1 이상이어야 합니다.".to_owned());
    }
    // 브라우저를 여러 번 여닫는 블로킹 작업이라 스레드 풀에서 실행해 GTK 메인 루프를 막지 않는다
    // (run_naver_discussion_batch와 동일 패턴).
    tauri::async_runtime::spawn_blocking(move || view_boost::boost_views(&links, repeats))
        .await
        .map_err(|error| format!("조회수 작업 스레드 오류: {error}"))
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
            // 큐 경로(build_log_batch)와 동일: 메인은 일반 친절 문구, 캡처된 호출 스택은
            // 자세히 보기(trace)로 분리한다(#199).
            msg: if r.ok {
                r.message.clone()
            } else {
                "종목토론방 게시에 실패했습니다".to_owned()
            },
            trace: r.trace.clone(),
            posted: None,
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
            // 종목토론방 게시는 Chrome 없이 순수 HTTP 패킷 API로 처리한다(#344 후속). 저장 쿠키를
            // 패킷 클라이언트에 직접 로드하므로 즉시게시도 Chrome을 띄우지 않는다(req.host/port는
            // 이제 macro가 안 쓰므로 그대로 둔다).
            // 즉시 게시 경로는 종목별 진행 콜백이 필요 없어 no-op을 넘긴다(#219는 큐 워커 전용).
            // 즉시게시("지금 바로")는 큐 kill 대상이 아니라 취소 없음(|| false) + 더미 임계신호.
            let results = run_forum_publish(
                request,
                app_for_job,
                |_| {},
                |_, _| {},
                |_, _, _| {},
                || false,
                std::sync::Arc::new(crate::ipc::kill::CancelSignal::default()),
            );
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

/// 밴드 게시 실패를 프론트로 보낼 때, 카페·종토방과 동일하게 사용자 사유(`reason`)와
/// "자세히 보기" 개발자 trace(`trace`, 런타임 backtrace 포함)를 분리해 전달한다(#199).
/// 프론트는 이 둘을 알림 항목의 메인 라인 / 자세히 보기로 나눠 기록한다(record_band_batch).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BandPublishError {
    reason: String,
    trace: String,
}

/// `BandPostError`를 사유 + trace로 나눠 프론트용 에러로 만든다. trace는 큐(예약) 경로와
/// 동일한 `band_failure_trace`(backtrace 동반)를 재사용해, 즉시 게시도 카페·종토방처럼
/// "자세히 보기"에 호출 스택이 나온다(#199).
fn band_publish_error(error: band_post::error::BandPostError) -> BandPublishError {
    BandPublishError {
        reason: ipc::queue_runner::band_failure_reason(&error),
        trace: ipc::queue_runner::band_failure_trace(&error),
    }
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
) -> Result<band_post::BandPublishOutcome, BandPublishError> {
    band_post::band_publish(&account_id, &band_link, &title, &content, &comments)
        .await
        .map_err(band_publish_error)
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
) -> Result<band_post::BandCommentOutcome, BandPublishError> {
    let sort = match mode.as_str() {
        "popular" => band_post::BandFeedSort::Popular,
        _ => band_post::BandFeedSort::Latest,
    };
    band_post::band_comment(&account_id, &band_link, sort, count, &comments)
        .await
        .map_err(band_publish_error)
}

/// 링크(band_no)로 밴드 이름을 조회한다(게시 모달에서 링크 저장 시 실제 밴드명 표시용).
#[tauri::command]
async fn band_resolve_name(account_id: String, band_link: String) -> Result<String, String> {
    band_post::resolve_band_name(&account_id, &band_link)
        .await
        .map_err(|e| e.to_string())
}

/// 로그인·게시 없이 연결된 ADB 디바이스(폰)의 비행기모드만 껐다 켜 IP를 회전시킨다(#247).
/// 로그인 경로(`auth/mod.rs`)와 동일하게 `assert_adb_device` → `toggle_airplane_mode` 순서로
/// 기존 함수를 그대로 재사용한다 — 비행기모드 ON/OFF·IP 회전 결과 로그도 기존과 동일.
#[tauri::command]
async fn rotate_ip() -> Result<auth::IpRotation, String> {
    auth::assert_adb_device().await.map_err(|e| e.to_string())?;
    // 'IP 변경' 버튼: 로그인과 동일하게 폰 인터넷 끊김(ping 8.8.8.8 fail) 확인 후 OFF하고, 외부
    // IP가 실제로 바뀔 때까지 폴링해 바뀌면 즉시 끝낸다(고정 대기 없음, IP 변경 확인 시점에 종료).
    auth::toggle_airplane_mode()
        .await
        .map_err(|e| e.to_string())
}

/// '수동추가' 버튼: headed Chrome을 띄워 사용자가 **직접** 네이버 로그인하게 한다(자동 타이핑·IP
/// 회전 없음). 성공하면 쿠키를 자동로그인과 동일하게 저장하고, 사람이 친 아이디/비밀번호로 계정
/// 행을 status=Active로 자동 추가한 뒤 그 계정을 돌려준다(프론트가 목록을 새로고침). 취소/타임아웃/
/// 창 닫힘이면 아무것도 추가하지 않고 오류 메시지를 돌려준다(프론트가 중립 토스트 표시).
#[tauri::command]
async fn manual_add_account<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<ipc::accounts::Account, String> {
    use ipc::accounts::{added_msg, apply_add, Account, AccountStatus, PlatformId};

    let result = auth::manual_add_account()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "수동추가가 취소되었거나 시간이 초과되어 계정을 추가하지 않았습니다.".to_owned()
        })?;

    // 계정관리 addRow와 동일한 형태로 새 행을 만든다(고유 id, 기본 플랫폼 forum, status=Active).
    let account = Account {
        id: format!("n{}", util::now_ms()),
        platform: PlatformId::Forum,
        login_id: result.login_id,
        pw: result.password,
        status: AccountStatus::Active,
        status_msg: None,
        status_trace: None,
        last: "방금".to_owned(),
        tags: vec![],
    };

    let store = app.state::<JsonStore<Account>>();
    store.mutate(|accounts| apply_add(accounts, account.clone()));
    let activity = app.state::<JsonStore<ipc::activity::ActivityItem>>();
    ipc::activity::record(
        activity.inner(),
        ipc::activity::ActivityType::Success,
        added_msg(&account.login_id),
    );
    Ok(account)
}

/// 프론트가 보내는 밴드 게시 결과 1건(알림 배치 기록용 최소 입력).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BandBatchItemInput {
    target: String,
    login_id: String,
    ok: bool,
    msg: String,
    /// "자세히 보기" 개발자 trace(런타임 backtrace 포함, #199). 프론트가 밴드 command 실패의
    /// `trace`를 실어 보낸다. URL 댓글 미지원 등 프론트 자체 합성 실패는 없을 수 있어 옵션.
    #[serde(default)]
    trace: Option<String>,
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
            // 실패 행은 trace(backtrace 동반)를 그대로 싣고, 없으면 메시지로 폴백한다(#199).
            trace: if i.ok {
                None
            } else {
                i.trace.clone().or_else(|| Some(i.msg.clone()))
            },
            msg: i.msg,
            posted: None,
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

/// 계정관리 "쿠키만료" 카운트다운용: 로그인 유지 쿠키의 만료 시각(unix seconds) 최댓값.
/// 세션 쿠키만 있거나 쿠키가 없으면 `None`. `id`는 쿠키 파일 키(loginId).
#[tauri::command]
fn account_cookie_expiry(id: String) -> Result<Option<f64>, String> {
    auth::account_cookie_expiry(&id).map_err(|e| e.to_string())
}

/// 우리 임시 프로필로 아직 실행 중인 Chrome 프로세스 개수(고아 헬퍼 포함). UI가 작업관리자
/// 없이 "실행 중 크롬 N개"를 보여주는 데 쓴다. 조회 실패 시 0(진단용이라 무해).
#[tauri::command]
fn running_chrome_count() -> usize {
    auth::running_chrome_count()
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
        queue::kill_queue_now,
        queue::clear_done_queue_now,
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
        diagnostics::open_url,
        bootstrap_runtime,
        save_accounts,
        band_publish,
        band_comment,
        band_resolve_name,
        rotate_ip,
        manual_add_account,
        get_account_cookies,
        account_cookie_expiry,
        running_chrome_count,
        run_naver_discussion,
        parse_template_csv,
        like_discussion_post,
        dislike_discussion_post,
        boost_view_count,
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
        get_now_concurrency_limit,
        set_now_concurrency_limit,
        agent::agent_register,
        agent::agent_status,
        agent::agent_unregister,
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
    // now 큐 "최대 작동가능 작업 수" 설정(#284). 단일 원소 컬렉션(무제한=0)으로 영속화한다.
    app.manage(JsonStore::load_or_seed(
        dir.join("concurrency.json"),
        ipc::queue_runner::seed_concurrency(),
    ));
    // 게시 큐 실행 워커 상태(promote 시 기동, 이슈 #144).
    app.manage(ipc::queue_runner::NowQueueRunner::default());
    // 실행 중 게시큐 "완전 종료(kill)"용 취소 신호 레지스트리(설계서 08). 큐 id별 신호를
    // in-memory로 보관 — 게시 루프가 종목 사이·대기 중에 확인하고 스스로 멈춘다.
    app.manage(ipc::kill::CancelRegistry::default());
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

/// now 큐 "최대 작동가능 작업 수" 설정을 돌려준다(#284). 0 = 무제한. 단일 원소
/// 스토어의 첫 값(없으면 무제한 0)을 읽는다.
#[tauri::command]
fn get_now_concurrency_limit(
    store: tauri::State<'_, JsonStore<ipc::queue_runner::ConcurrencyConfig>>,
) -> u32 {
    store.snapshot().first().map(|c| c.limit).unwrap_or(0)
}

/// now 큐 "최대 작동가능 작업 수"를 영속화한다(#284). 0(또는 빈 입력=프론트가 0으로 변환) =
/// 무제한, N = 동시 작업을 N개로 제한. 워커는 claim 시점마다 이 값을 다시 읽으므로, 낮춰도
/// 이미 돌고 있는 작업은 멈추지 않고 새 claim만 active < limit까지 기다린다.
#[tauri::command]
fn set_now_concurrency_limit(
    store: tauri::State<'_, JsonStore<ipc::queue_runner::ConcurrencyConfig>>,
    limit: u32,
) {
    store.mutate(|_| vec![ipc::queue_runner::ConcurrencyConfig { limit }]);
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
        // 원격제어 에이전트(§9) 기동 — SSE 명령 수신·하트비트 루프. 등록 전이면 대기만 한다.
        agent::start(app.handle().clone());
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
    fn band_publish_error_splits_reason_and_trace() {
        use band_post::error::BandPostError;
        // 즉시 게시 경로도 사유(메인)와 backtrace 동반 trace(자세히 보기)를 분리해야 한다(#199).
        let e = band_publish_error(BandPostError::no_session());
        assert_eq!(
            e.reason,
            "밴드 로그인 세션이 없습니다. 먼저 밴드 로그인을 해주세요"
        );
        // trace는 코드 상세로 시작하고 뒤에 런타임 backtrace가 붙는다(reason과 다르다).
        assert!(e.trace.starts_with("code: BAND_NO_SESSION"));
        assert!(e.trace.contains("\n\n"));
        assert_ne!(e.reason, e.trace);
    }

    #[test]
    fn build_publish_batch_maps_results_to_items() {
        let results = vec![
            ForumPublishResult {
                code: "005930".into(),
                name: "삼성전자".into(),
                ok: true,
                message: "게시 완료".into(),
                trace: None,
                posted: None,
                skipped: false,
                stopped: false,
            },
            ForumPublishResult {
                code: "000660".into(),
                name: "SK하이닉스".into(),
                ok: false,
                message: "로그인 만료".into(),
                trace: Some("stack backtrace:\n  0: forum::login_check".into()),
                posted: None,
                skipped: false,
                stopped: false,
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
        // 실패 항목: 메인은 일반 친절 문구, 자세히 보기엔 캡처된 호출 스택(#199).
        assert_eq!(b.items[1].msg, "종목토론방 게시에 실패했습니다");
        assert_eq!(
            b.items[1].trace.as_deref(),
            Some("stack backtrace:\n  0: forum::login_check")
        );
    }

    #[test]
    fn build_publish_batch_snapshots_nonempty_body_and_comment() {
        let results = vec![ForumPublishResult {
            code: "005930".into(),
            name: "삼성전자".into(),
            ok: true,
            message: "게시 완료".into(),
            trace: None,
            posted: None,
            skipped: false,
            stopped: false,
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
