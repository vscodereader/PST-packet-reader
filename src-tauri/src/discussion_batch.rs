use std::collections::BTreeMap;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Runtime};

use crate::ipc::log_batches::PostedContent;
use crate::naver_automation::{
    run_naver_discussion_macro, run_naver_post_with_comment_macro, AutomationError,
    AutomationReport, AutomationTarget, DiscussionStock, NaverDiscussionRequest,
    NaverPostWithCommentRequest,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
// CSV 템플릿에서 읽은 제목, 내용, 댓글내용 목록을 담는 구조체입니다.
pub struct TemplateColumns {
    pub titles: Vec<String>,
    pub bodies: Vec<String>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// UI에서 검색하거나 선택할 네이버 종목 후보를 담는 구조체입니다.
pub struct StockCandidate {
    pub name: String,
    pub code: String,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
// 여러 문구 중 어떤 방식으로 값을 선택할지 나타내는 enum입니다.
pub enum PickMode {
    Random,
    Sequential,
    Single,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// UI에서 저장 후 실행 버튼을 눌렀을 때 Rust로 전달되는 batch 설정입니다.
#[serde(rename_all = "camelCase")]
pub struct DiscussionBatchRequest {
    pub host: String,
    pub port: u16,
    pub stocks: Vec<DiscussionStock>,
    pub run_post: bool,
    pub run_comment: bool,
    pub titles: Vec<String>,
    pub bodies: Vec<String>,
    pub comments: Vec<String>,
    pub title_mode: PickMode,
    pub body_mode: PickMode,
    pub comment_mode: PickMode,
    pub count: usize,
    // 로그인 자동화로 저장된 계정 ID(선택). 지정되면 해당 계정 쿠키를 Chrome에 주입합니다.
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// batch 실행 결과를 UI에 보여주기 위한 구조체입니다.
pub struct DiscussionBatchReport {
    pub completed: usize,
    pub reports: Vec<AutomationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// 사수 UI(publish-modal)의 "지금 바로 게시 + 종목토론방"에서 넘어오는 요청입니다.
// 하나의 글(LibraryPost) 내용을 선택한 종목들에 게시합니다.
#[serde(rename_all = "camelCase")]
pub struct ForumPublishRequest {
    pub host: String,
    pub port: u16,
    // 게시 계정의 loginId. 이 키로 저장된 로그인 쿠키(cookies/{loginId}.json)를 사용합니다.
    pub account_id: String,
    pub run_post: bool,
    pub run_comment: bool,
    pub title: String,
    pub body: String,
    pub comment: String,
    pub stocks: Vec<DiscussionStock>,
    /// `#{링크}` 토큰 치환에 쓸 사용자 지정 링크값. 비우면 종목별 시세 링크를 쓴다.
    /// 과거 요청과 호환되도록 기본값(빈 문자열)을 허용한다.
    #[serde(default)]
    pub link_override: String,
    /// "특정 게시글" 댓글 대상의 종목토론방 글 URL. 지정되면 댓글은 랜덤 글이 아니라
    /// 이 URL의 글에 달린다(run_comment=true 전용). None이면 기존 per-종목 동작.
    #[serde(default)]
    pub comment_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// 종목 한 곳의 게시 결과입니다(사수 UI의 PublishResult로 매핑).
#[serde(rename_all = "camelCase")]
pub struct ForumPublishResult {
    pub code: String,
    pub name: String,
    pub ok: bool,
    pub message: String,
    /// 실패 시 "자세히 보기"용 캡처된 호출 스택(개발자 trace). 성공이면 `None`.
    /// 사용자용 `message`와 분리해, 메인 라인엔 안 나오고 자세히 보기에만 노출한다(#199).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,
    /// 종목별 실제 게시 내용(제목/본문/댓글/URL). 게시 성공 시에만 채운다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted: Option<PostedContent>,
    /// 차단 계정(로그인/권한 오류)으로 앞선 글이 실패해 시도하지 않고 건너뛴 결과면 true(#267-9).
    /// `ok=false`이지만 실제 실패(X)가 아니라 "건너뜀(skip)"으로 구분 표시한다.
    #[serde(default)]
    pub skipped: bool,
}

/// 게시 실패 메시지가 "계정 차단(로그인/권한 만료)"을 뜻하는지 판별한다(#267-9, 순수 함수).
/// 이런 실패는 같은 계정의 남은 글도 전부 실패할 것이므로, 첫 글에서 감지되면 나머지를
/// 건너뛴다. "요청이 너무 많습니다"(HTTP 429, 일시적 과다요청)는 차단이 아니므로 제외한다 —
/// 잠시 후 풀릴 수 있어 건너뛰면 안 된다.
pub fn is_blocking_failure(message: &str) -> bool {
    let m = message;
    // 429(요청 과다)는 일시적이라 건너뛰지 않는다(사수 지침: 요청 과다 제외).
    if m.contains("429") || m.contains("요청이 너무 많") || m.contains("요청 과다") {
        return false;
    }
    // HTTP 401/403(권한 없음/로그인 만료) — 메시지에 박힌 상태코드로 본다.
    if blocking_http_status(m) {
        return true;
    }
    // 내부 코드/한국어 안내로 드러나는 로그인·권한·쿠키·세션 만료 계열.
    const BLOCKING_MARKERS: [&str; 9] = [
        "SESSION_INVALID",
        "NO_COOKIES",
        "LOGIN_FAILED",
        "로그인이 만료",
        "로그인 정보가 없",
        "권한이 없",
        "쿠키를 찾지 못",
        "로그인이 필요",
        "다시 로그인",
    ];
    BLOCKING_MARKERS.iter().any(|marker| m.contains(marker))
}

/// 게시 실패 메시지가 "대기초과"(일시적 서버/타이밍 문제)를 뜻하는지 판별한다(#286, 순수 함수).
/// 두 부류를 잡는다: (1) 페이지/응답 **대기시간 초과**, (2) 네이버 **서버 오류(HTTP 500)**. 이런
/// 실패는 계정·자격증명 문제가 아니라 잠시 후 풀릴 수 있는 일시 상태라, 차단(Blocked)이나 비번
/// 오류와 구분해 계정을 `TimedOut`(대기초과)으로 표시한다. 차단 계열(`is_blocking_failure`)이
/// 우선이므로, 호출부는 먼저 차단을 보고 그 다음 이걸 본다. 429(요청 과다)는 여기에 넣지 않는다.
pub fn is_timed_out_failure(message: &str) -> bool {
    let m = message;
    // (2) 서버 오류: 메시지에 박힌 HTTP 상태코드가 500이거나, 명시적 서버 오류 문구.
    if server_error_http_status(m)
        || m.contains("서버에 문제")
        || m.contains("네이버 서버")
        || m.contains("Internal Server")
    {
        return true;
    }
    // (1) 대기시간 초과: 페이지/응답이 자리잡기 전에 시간이 다한 경우.
    const TIMEOUT_MARKERS: [&str; 5] = [
        "대기시간 초과",
        "시간이 초과",
        "시간 초과",
        "timed out",
        "timeout",
    ];
    TIMEOUT_MARKERS.iter().any(|marker| m.contains(marker))
}

/// 메시지에 박힌 HTTP 상태코드가 500(서버 오류)인지 본다(순수 함수, #286).
/// `blocking_http_status`와 같은 "status 토큰 뒤 첫 3자리" 규칙을 따른다.
fn server_error_http_status(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let Some(idx) = lower.find("status") else {
        return false;
    };
    let after = &message[idx + "status".len()..];
    let digits: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    matches!(digits.parse::<u16>(), Ok(500))
}

/// 메시지에 박힌 HTTP 상태코드가 401/403(차단 계열)인지 본다(순수 함수, #267-9).
/// queue_runner의 parse_http_status와 같은 "status 토큰 뒤 첫 3자리" 규칙을 따른다.
fn blocking_http_status(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let Some(idx) = lower.find("status") else {
        return false;
    };
    let after = &message[idx + "status".len()..];
    let digits: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    matches!(digits.parse::<u16>(), Ok(401) | Ok(403))
}

// 선택한 종목들에 글/댓글을 게시하고 종목별 성공/실패 결과를 돌려주는 함수입니다.
// 한 종목이 실패해도 다음 종목을 계속 진행합니다. 종목 사이에는 1분 대기합니다.
pub fn run_forum_publish<R, FS, FR>(
    request: ForumPublishRequest,
    app: tauri::AppHandle<R>,
    mut on_start: FS,
    mut on_result: FR,
) -> Vec<ForumPublishResult>
where
    R: Runtime,
    // 종목 게시 시작 직전(인덱스)과 완료 직후(인덱스, 결과)에 호출한다(#219). 큐 워커가
    // 이걸 받아 "진행 전 → 진행 중 → 완료/실패"를 실시간으로 보여준다.
    FS: FnMut(usize),
    FR: FnMut(usize, &ForumPublishResult),
{
    let title = request.title.trim();
    let body = request.body.trim();
    let comment = request.comment.trim();
    let total = request.stocks.len();
    let mut results = Vec::with_capacity(total);

    // 로그용 계정 식별자(마스킹)와 작업 종류. PW는 넣지 않는다.
    let who = crate::auth::mask_id(&request.account_id);
    let kind = match (request.run_post, request.run_comment) {
        (true, true) => "글+댓글",
        (false, true) => "댓글",
        _ => "글",
    };
    // 차단 계정(로그인/권한 오류)으로 첫 글이 실패하면, 같은 계정의 남은 글은 전부 실패할
    // 것이므로 시도하지 않고 건너뛴다(#267-9). 한 번 켜지면 이후 모든 종목을 skip 처리한다.
    let mut blocked = false;

    for (index, stock) in request.stocks.iter().enumerate() {
        on_start(index);

        // 앞선 글이 차단/로그인 오류로 실패한 뒤라면, 실제 게시 시도도 60초 대기도 없이 건너뛴다.
        if blocked {
            tracing::info!(
                "[POST] {who}  \"{}\" 종목토론방 {kind} 건너뜀 ⏭ (앞선 글 로그인/권한 실패로 skip)",
                stock.name
            );
            results.push(ForumPublishResult {
                code: stock.code.clone(),
                name: stock.name.clone(),
                ok: false,
                message: "앞선 글이 로그인/권한 오류로 실패해 건너뜀".to_owned(),
                trace: None,
                posted: None,
                skipped: true,
            });
            if let Some(last) = results.last() {
                on_result(index, last);
            }
            continue;
        }

        let outcome =
            run_one_forum_stock_with_retry(&request, stock, title, body, comment, &app, &who, kind);
        // 실패면 사용자용 메시지(message)와 캡처된 스택(trace)을 분리해 들고 간다(#199).
        // 성공 시 작성된 글 URL을 메시지에 함께 실어, 완료 로그에서 올라간 글을 확인할 수 있게 한다.
        let (ok, message, trace, posted) = match outcome {
            Ok(posted) => (true, "게시 완료".to_owned(), None, Some(posted)),
            Err(error) => (false, error.message().to_owned(), Some(error.trace()), None),
        };

        // 작업 결과를 pstmacro.log에 기록(가독성·상세화).
        if ok {
            tracing::info!("[POST] {who}  \"{}\" 종목토론방 {kind} 성공 ✅", stock.name);
        } else {
            tracing::info!(
                "[POST] {who}  \"{}\" 종목토론방 {kind} 실패 ❌ — {message}",
                stock.name
            );
        }

        // 이번 실패가 계정 차단(로그인/권한 만료)이면, 다음 회차부터 남은 글을 건너뛴다(#267-9).
        // "요청 과다"(429)는 일시적이라 차단으로 보지 않는다(is_blocking_failure에서 제외).
        if !ok && is_blocking_failure(&message) {
            blocked = true;
        }

        results.push(ForumPublishResult {
            code: stock.code.clone(),
            name: stock.name.clone(),
            ok,
            message,
            trace,
            posted,
            skipped: false,
        });
        // 종목 1건 완료를 호출부에 통지한다(#219). 큐 워커는 여기서 진행률·라이브 상태를
        // 60초 대기 전에 갱신해, 종토방 작업이 0/N에 멈춰 보이지 않게 한다.
        if let Some(last) = results.last() {
            on_result(index, last);
        }

        // 마지막 종목이 아니고, 차단으로 남은 글을 건너뛸 게 아닐 때만 다음 게시 전 1분 대기.
        // 차단되면 곧장 다음 루프에서 skip하므로 60초를 낭비하지 않는다(#267-9 시간 절약).
        if index + 1 < total && !blocked {
            let _ = app.emit("batch-wait-start", serde_json::json!({ "seconds": 60u64 }));
            sleep(Duration::from_secs(60));
        }
    }

    results
}

/// 댓글 전용 결과의 '게시내용' 링크를 고른다. "특정 게시글" 댓글이면 그 글 URL(공백 제거
/// 후 비어있지 않을 때)을 쓰고, 아니면 매크로가 돌려준 글 URL(랜덤 글 댓글은 None)로
/// 떨어진다. 알림 '게시내용'에서 댓글 옆에 단 글의 링크를 보여주는 데 쓴다.
fn comment_detail_url(
    comment_url: &Option<String>,
    report_post_url: Option<String>,
) -> Option<String> {
    comment_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned)
        .or(report_post_url)
}

/// 종목 게시 실패가 "재시도 가치가 있는지"(대기초과 = 일시적 서버/타이밍 문제) 판별한다(순수).
/// 차단(로그인/권한/쿠키)·비번오류·약관동의 실패 같은 건 재시도해도 또 실패하므로 제외하고,
/// `is_timed_out_failure`(페이지/응답 시간초과·HTTP 500 네이버 서버 오류)만 재시도 대상으로 본다.
/// 차단이 대기초과보다 우선이므로 차단 계열이면 재시도하지 않는다.
fn is_retryable_forum_failure(message: &str) -> bool {
    !is_blocking_failure(message) && is_timed_out_failure(message)
}

/// 대기초과(일시적 시간초과/서버오류) 실패 시 같은 종목 게시를 재시도하는 정책(#대기초과 후속,
/// 사용자 지시). **주 종료조건은 "계정 막힘(차단) 감지"**다: 매 재시도마다 결과를 다시 분류해,
/// 실패가 차단/막힘 계열(`is_blocking_failure`: 로그인 만료·권한 없음·쿠키 못찾음 등)로 바뀌면
/// 즉시 재시도를 멈추고 그 막힘 메시지를 그대로 돌려준다 → 호출부가 계정을 Blocked로 표시한다.
/// 차단이 아니라 계속 시간초과(일시적)면 백오프(3→6→12→24→30…초)로 끈질기게 다시 시도한다.
/// 단 "네이버 자체 먹통/세션 사망"처럼 차단 메시지 없이 계속 timeout만 나는 경우엔 차단 감지가
/// 안 걸려 무한히 돌 수 있어, 넉넉한 시간 상한(`FORUM_RETRY_TOTAL_BUDGET`)을 안전망으로 둔다 —
/// 이 상한을 넘으면 그때 대기초과로 남겨 글을 잃지 않고 큐도 영영 막지 않는다.
const FORUM_TIMEOUT_RETRIES: usize = 30;
const FORUM_RETRY_BACKOFF_BASE_SECS: u64 = 3;
const FORUM_RETRY_BACKOFF_MAX_SECS: u64 = 30;
const FORUM_RETRY_TOTAL_BUDGET: Duration = Duration::from_secs(600);

/// `attempt`회차(0부터)의 재시도 전 대기 시간(초). 매회 2배로 늘리되 상한으로 캡한다(순수 함수).
fn forum_retry_delay_secs(attempt: usize) -> u64 {
    FORUM_RETRY_BACKOFF_BASE_SECS
        .checked_shl(attempt as u32)
        .unwrap_or(u64::MAX)
        .min(FORUM_RETRY_BACKOFF_MAX_SECS)
}

// 한 종목 게시를 시도하되, 대기초과(일시적 시간초과/서버오류)면 백오프 후 다시 시도한다. 차단/성공/
// 그 밖의 실패는 즉시 반환한다(재시도 무의미). 재시도는 횟수(`FORUM_TIMEOUT_RETRIES`)와 총 누적
// 시간(`FORUM_RETRY_TOTAL_BUDGET`) 둘 중 하나라도 넘으면 멈춰, 진짜 안 되는 종목이 큐를 무한정
// 막지 않게 한다. 로그인 경로는 건드리지 않고 게시 종목 단위에서만 재시도한다.
fn run_one_forum_stock_with_retry<R: Runtime>(
    request: &ForumPublishRequest,
    stock: &DiscussionStock,
    title: &str,
    body: &str,
    comment: &str,
    app: &tauri::AppHandle<R>,
    who: &str,
    kind: &str,
) -> Result<PostedContent, AutomationError> {
    let started = Instant::now();
    let mut attempt = 0usize;
    loop {
        match run_one_forum_stock(request, stock, title, body, comment, app) {
            Ok(posted) => return Ok(posted),
            Err(error) => {
                let within_budget = started.elapsed() < FORUM_RETRY_TOTAL_BUDGET;
                if attempt < FORUM_TIMEOUT_RETRIES
                    && within_budget
                    && is_retryable_forum_failure(error.message())
                {
                    let delay = forum_retry_delay_secs(attempt);
                    attempt += 1;
                    tracing::info!(
                        "[POST] {who}  \"{}\" 종목토론방 {kind} 대기초과 — {attempt}/{FORUM_TIMEOUT_RETRIES}회 재시도({delay}초 후): {}",
                        stock.name,
                        error.message()
                    );
                    sleep(Duration::from_secs(delay));
                    continue;
                }
                return Err(error);
            }
        }
    }
}

// 한 종목에 글/댓글을 게시하는 함수입니다(kind에 따라 엔진 함수를 고릅니다).
fn run_one_forum_stock<R: Runtime>(
    request: &ForumPublishRequest,
    stock: &DiscussionStock,
    title: &str,
    body: &str,
    comment: &str,
    app: &tauri::AppHandle<R>,
    // AutomationError를 그대로 돌려준다(메시지+캡처된 스택). 호출부가 message/backtrace로
    // 나눠 ForumPublishResult에 싣는다(#199).
) -> Result<PostedContent, AutomationError> {
    // 종목별로 변수 토큰을 치환한다(미리보기 resolveTemplate와 동일 결과).
    // #{종목명}/#{종목코드}는 이 종목 값으로, #{링크}는 링크값(있으면) 또는 종목 시세 링크로.
    let link = crate::template_tokens::resolve_link(&request.link_override, &stock.code);
    let title = crate::template_tokens::resolve_forum(title, &stock.name, &stock.code, &link);
    let body = crate::template_tokens::resolve_forum(body, &stock.name, &stock.code, &link);
    let comment = crate::template_tokens::resolve_forum(comment, &stock.name, &stock.code, &link);
    let (title, body, comment) = (title.as_str(), body.as_str(), comment.as_str());
    if request.run_post && request.run_comment {
        run_naver_post_with_comment_macro(
            NaverPostWithCommentRequest {
                title: title.to_owned(),
                body: body.to_owned(),
                comment: comment.to_owned(),
                host: request.host.clone(),
                port: request.port,
                stock: Some(stock.clone()),
                account_id: Some(request.account_id.clone()),
            },
            // 글+댓글 한 종목 안의 1분 대기는 종목 간 대기와 별개이므로 여기서는 끕니다.
            app,
            false,
        )
        .map(|reports| PostedContent {
            title: title.to_owned(),
            body: body.to_owned(),
            comment: Some(comment.to_owned()),
            url: reports.into_iter().find_map(|r| r.post_url),
        })
    } else {
        let target = if request.run_comment {
            AutomationTarget::Comment
        } else {
            AutomationTarget::Post
        };
        let macro_body = if request.run_comment { comment } else { body };
        run_naver_discussion_macro(NaverDiscussionRequest {
            title: title.to_owned(),
            body: macro_body.to_owned(),
            host: request.host.clone(),
            port: request.port,
            target,
            submit_after_fill: true,
            stock: Some(stock.clone()),
            account_id: Some(request.account_id.clone()),
            // "특정 게시글" 댓글이면 그 글 URL을 그대로 넘겨 랜덤 글 대신 이 글에 댓글을 단다.
            comment_url: request.comment_url.clone(),
        })
        .map(|report| {
            if request.run_comment {
                // 댓글 전용: 게시한 글은 없고 댓글 내용을 보존한다. "특정 게시글" 댓글이면
                // 그 글 URL을 url에 실어, 알림 '게시내용'에서 댓글만이 아니라 단 글의 링크도
                // 보이게 한다(랜덤 글 댓글은 대상 URL이 없어 기존대로 None).
                PostedContent {
                    title: String::new(),
                    body: String::new(),
                    comment: Some(comment.to_owned()),
                    url: comment_detail_url(&request.comment_url, report.post_url),
                }
            } else {
                PostedContent {
                    title: title.to_owned(),
                    body: body.to_owned(),
                    comment: None,
                    url: report.post_url,
                }
            }
        })
    }
}

// CSV 템플릿 문자열에서 제목, 내용, 댓글내용 컬럼을 파싱하는 함수입니다.
pub fn parse_discussion_template_csv(csv_text: String) -> Result<TemplateColumns, String> {
    let rows = parse_csv_rows(&csv_text)?;
    let Some(header) = rows.first() else {
        return Err("CSV 파일이 비어 있습니다.".to_owned());
    };

    if header.len() < 3 {
        return Err("CSV 첫 행에는 제목, 내용, 댓글내용 3개 열이 필요합니다.".to_owned());
    }

    let title_index = find_header_index(header, &["제목", "title"]).unwrap_or(0);
    let body_index = find_header_index(header, &["내용", "body", "content"]).unwrap_or(1);
    let comment_index =
        find_header_index(header, &["댓글내용", "댓글 내용", "comment"]).unwrap_or(2);

    let mut titles = Vec::new();
    let mut bodies = Vec::new();
    let mut comments = Vec::new();

    for row in rows.into_iter().skip(1) {
        push_non_empty(&mut titles, row.get(title_index));
        push_non_empty(&mut bodies, row.get(body_index));
        push_non_empty(&mut comments, row.get(comment_index));
    }

    if titles.is_empty() && bodies.is_empty() && comments.is_empty() {
        return Err("CSV에서 가져올 제목, 내용, 댓글내용이 없습니다.".to_owned());
    }

    Ok(TemplateColumns {
        titles,
        bodies,
        comments,
    })
}

// 네이버 증권 공개 API에서 종목 후보를 가져오고 query로 필터링하는 함수입니다.
pub fn search_naver_stocks(query: Option<String>) -> Result<Vec<StockCandidate>, String> {
    let query = query.unwrap_or_default().trim().to_owned();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("종목 검색 HTTP 클라이언트 생성 실패: {error}"))?;
    let paths = [
        "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=quantTop&startIdx=0&pageSize=80",
        "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=up&startIdx=0&pageSize=80",
        "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=down&startIdx=0&pageSize=80",
        "/api/community/discussion/rankings?nationType=KOR&page=1&size=80&postType=HOT",
    ];
    let mut candidates = Vec::new();

    for path in paths {
        let url = format!("https://stock.naver.com{path}");
        let Ok(response) = client
            .get(&url)
            .header("accept", "application/json, text/plain, */*")
            .header(
                "referer",
                "https://stock.naver.com/market/stock/kr/stocklist/priceTop",
            )
            .header("user-agent", "Mozilla/5.0")
            .send()
        else {
            continue;
        };
        let Ok(text) = response.text() else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };

        collect_stock_candidates(&value, &mut candidates);
    }

    if candidates.is_empty() {
        candidates.extend(fallback_stocks());
    }

    let mut unique = dedupe_stocks(candidates);

    if !query.is_empty() {
        let needle = query.to_lowercase();
        unique.retain(|stock| {
            stock.name.to_lowercase().contains(&needle) || stock.code.contains(&needle)
        });
    }

    unique.truncate(80);
    Ok(unique)
}

// UI 설정에 따라 글쓰기/댓글쓰기를 여러 번 실행하는 함수입니다.
pub fn run_discussion_batch<R: Runtime>(
    request: DiscussionBatchRequest,
    app: tauri::AppHandle<R>,
) -> Result<DiscussionBatchReport, String> {
    validate_batch_request(&request)?;

    let mut reports = Vec::new();
    let total_actions = request.count * usize::from(request.run_post)
        + request.count * usize::from(request.run_comment);
    let mut completed_actions = 0;

    // 로그용 계정 식별자(마스킹). PW는 넣지 않는다. 계정 미지정이면 표식만 남긴다.
    let who = request
        .account_id
        .as_deref()
        .map_or_else(|| "(계정 미지정)".to_owned(), crate::auth::mask_id);

    for index in 0..request.count {
        let stock = request.stocks[index % request.stocks.len()].clone();

        if request.run_post && request.run_comment {
            let title = pick_text(&request.titles, &request.title_mode, index, "제목")?;
            let body = pick_text(&request.bodies, &request.body_mode, index, "내용")?;
            let comment = pick_text(&request.comments, &request.comment_mode, index, "댓글내용")?;
            // 마지막 회차가 아닐 때만 글 등록 직후 타이머를 emit하고 1분을 채웁니다.
            let sleep_after = index + 1 < request.count;
            let stock_name = stock.name.clone();
            let pair_reports = match run_naver_post_with_comment_macro(
                NaverPostWithCommentRequest {
                    title,
                    body,
                    comment,
                    host: request.host.clone(),
                    port: request.port,
                    stock: Some(stock),
                    account_id: request.account_id.clone(),
                },
                &app,
                sleep_after,
            ) {
                Ok(pair_reports) => {
                    tracing::info!("[POST] {who}  \"{stock_name}\" 종목토론방 글+댓글 성공 ✅");
                    pair_reports
                }
                Err(error) => {
                    let message = error.to_string();
                    tracing::info!(
                        "[POST] {who}  \"{stock_name}\" 종목토론방 글+댓글 실패 ❌ — {message}"
                    );
                    return Err(message);
                }
            };

            for report in pair_reports {
                reports.push(report);
                completed_actions += 1;
                // sleep은 run_naver_post_with_comment_macro 안에서 처리합니다.
            }

            continue;
        }

        if request.run_post {
            let title = pick_text(&request.titles, &request.title_mode, index, "제목")?;
            let body = pick_text(&request.bodies, &request.body_mode, index, "내용")?;

            let stock_name = stock.name.clone();
            let report = match run_naver_discussion_macro(NaverDiscussionRequest {
                title,
                body,
                host: request.host.clone(),
                port: request.port,
                target: AutomationTarget::Post,
                submit_after_fill: true,
                stock: Some(stock.clone()),
                account_id: request.account_id.clone(),
                comment_url: None,
            }) {
                Ok(report) => {
                    tracing::info!("[POST] {who}  \"{stock_name}\" 종목토론방 글 성공 ✅");
                    report
                }
                Err(error) => {
                    let message = error.to_string();
                    tracing::info!(
                        "[POST] {who}  \"{stock_name}\" 종목토론방 글 실패 ❌ — {message}"
                    );
                    return Err(message);
                }
            };
            reports.push(report);
            completed_actions += 1;
            sleep_between_actions(completed_actions, total_actions, &app);
        }

        if request.run_comment {
            let comment = pick_text(&request.comments, &request.comment_mode, index, "댓글내용")?;

            let stock_name = stock.name.clone();
            let report = match run_naver_discussion_macro(NaverDiscussionRequest {
                title: String::new(),
                body: comment,
                host: request.host.clone(),
                port: request.port,
                target: AutomationTarget::Comment,
                submit_after_fill: true,
                stock: Some(stock),
                account_id: request.account_id.clone(),
                comment_url: None,
            }) {
                Ok(report) => {
                    tracing::info!("[POST] {who}  \"{stock_name}\" 종목토론방 댓글 성공 ✅");
                    report
                }
                Err(error) => {
                    let message = error.to_string();
                    tracing::info!(
                        "[POST] {who}  \"{stock_name}\" 종목토론방 댓글 실패 ❌ — {message}"
                    );
                    return Err(message);
                }
            };
            reports.push(report);
            completed_actions += 1;
            sleep_between_actions(completed_actions, total_actions, &app);
        }
    }

    Ok(DiscussionBatchReport {
        completed: reports.len(),
        reports,
    })
}

// 등록 또는 댓글 작성 한 건이 끝난 뒤 다음 실행 전 1분을 기다리는 함수입니다.
// 대기 직전 프론트엔드로 "batch-wait-start" 이벤트를 보내 카운트다운 타이머를 표시합니다.
fn sleep_between_actions<R: Runtime>(
    completed_actions: usize,
    total_actions: usize,
    app: &tauri::AppHandle<R>,
) {
    if completed_actions < total_actions {
        let _ = app.emit("batch-wait-start", serde_json::json!({ "seconds": 60u64 }));
        sleep(Duration::from_secs(60));
    }
}

// batch 실행 전 필수 입력과 선택 모드를 검증하는 함수입니다.
fn validate_batch_request(request: &DiscussionBatchRequest) -> Result<(), String> {
    if request.stocks.is_empty() {
        return Err("종목을 하나 이상 선택하세요.".to_owned());
    }

    if !request.run_post && !request.run_comment {
        return Err("행동을 선택하세요.".to_owned());
    }

    if request.count != 3 && request.count != 5 {
        return Err("실행 개수는 3개 또는 5개만 선택할 수 있습니다.".to_owned());
    }

    if request.run_post {
        validate_texts(&request.titles, &request.title_mode, "제목")?;
        validate_texts(&request.bodies, &request.body_mode, "내용")?;
    }

    if request.run_comment {
        validate_texts(&request.comments, &request.comment_mode, "댓글내용")?;
    }

    Ok(())
}

// 선택 모드별로 텍스트 목록이 올바른지 확인하는 함수입니다.
fn validate_texts(values: &[String], mode: &PickMode, label: &str) -> Result<(), String> {
    let count = values
        .iter()
        .filter(|value| !value.trim().is_empty())
        .count();

    if count == 0 {
        return Err(format!(
            "{label}이 비어 있습니다. CSV를 가져오거나 텍스트창에 입력하세요."
        ));
    }

    if matches!(mode, PickMode::Single) && count != 1 {
        return Err(format!(
            "{label}의 1개만 모드는 값이 정확히 1개일 때만 사용할 수 있습니다."
        ));
    }

    Ok(())
}

// 선택 모드에 따라 이번 실행에 사용할 문구 하나를 고르는 함수입니다.
fn pick_text(
    values: &[String],
    mode: &PickMode,
    index: usize,
    label: &str,
) -> Result<String, String> {
    let cleaned = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if cleaned.is_empty() {
        return Err(format!("{label}이 비어 있습니다."));
    }

    let picked = match mode {
        PickMode::Random => cleaned[pseudo_index(cleaned.len())],
        PickMode::Sequential => cleaned[index % cleaned.len()],
        PickMode::Single => cleaned[0],
    };

    Ok(picked.to_owned())
}

// CSV 헤더에서 원하는 열 이름의 위치를 찾는 함수입니다.
fn find_header_index(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|value| {
        let normalized = value.trim().to_lowercase().replace(' ', "");
        names
            .iter()
            .any(|name| normalized == name.to_lowercase().replace(' ', ""))
    })
}

// 값이 비어 있지 않을 때 목록에 추가하는 함수입니다.
fn push_non_empty(values: &mut Vec<String>, value: Option<&String>) {
    if let Some(value) = value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(value.to_owned());
    }
}

// 쉼표, 큰따옴표, 줄바꿈을 처리하는 간단한 CSV parser 함수입니다.
fn parse_csv_rows(csv_text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = csv_text.chars().peekable();
    let mut in_quotes = false;

    while let Some(character) = chars.next() {
        match character {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                row.push(field.trim().to_owned());
                field.clear();
            }
            '\n' if !in_quotes => {
                row.push(field.trim().trim_end_matches('\r').to_owned());
                field.clear();
                rows.push(row);
                row = Vec::new();
            }
            _ => field.push(character),
        }
    }

    if in_quotes {
        return Err("CSV 따옴표가 닫히지 않았습니다.".to_owned());
    }

    row.push(field.trim().trim_end_matches('\r').to_owned());

    if row.iter().any(|field| !field.is_empty()) {
        rows.push(row);
    }

    Ok(rows)
}

// 네이버 API 응답 JSON에서 종목 후보를 재귀적으로 수집하는 함수입니다.
fn collect_stock_candidates(value: &Value, candidates: &mut Vec<StockCandidate>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_stock_candidates(item, candidates);
            }
        }
        Value::Object(object) => {
            if let Some(code) = direct_string(
                object,
                &[
                    "itemCode",
                    "itemcode",
                    "stockCode",
                    "stockcode",
                    "code",
                    "symbolCode",
                    "symbolcode",
                    "localCode",
                    "localcode",
                ],
            ) {
                if looks_like_stock_code(&code) {
                    let name = direct_string(
                        object,
                        &[
                            "itemName",
                            "itemname",
                            "stockName",
                            "stockname",
                            "name",
                            "korName",
                            "korname",
                            "displayName",
                            "displayname",
                        ],
                    )
                    .unwrap_or_else(|| code.clone());
                    candidates.push(StockCandidate {
                        link: format!(
                            "https://stock.naver.com/domestic/stock/{code}/discussion?chip=all"
                        ),
                        name,
                        code,
                    });
                }
            }

            for child in object.values() {
                collect_stock_candidates(child, candidates);
            }
        }
        _ => {}
    }
}

// 중복 종목 코드를 제거하는 함수입니다.
fn dedupe_stocks(candidates: Vec<StockCandidate>) -> Vec<StockCandidate> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::new();

    for stock in candidates {
        if seen.insert(stock.code.clone(), ()).is_none() {
            unique.push(stock);
        }
    }

    unique
}

// JSON 객체의 직접 필드에서 문자열 값을 읽는 함수입니다.
fn direct_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|value| match value {
            Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
    })
}

// 문자열이 네이버 종목 코드 형태인지 확인하는 함수입니다.
fn looks_like_stock_code(value: &str) -> bool {
    value.chars().count() == 6 && value.chars().all(|character| character.is_ascii_digit())
}

// API 실패 시 UI가 완전히 비지 않게 해주는 기본 종목 목록입니다.
fn fallback_stocks() -> Vec<StockCandidate> {
    [
        ("삼성전자", "005930"),
        ("SK하이닉스", "000660"),
        ("NAVER", "035420"),
        ("현대차", "005380"),
        ("LG전자", "066570"),
        ("한화시스템", "272210"),
    ]
    .into_iter()
    .map(|(name, code)| StockCandidate {
        name: name.to_owned(),
        code: code.to_owned(),
        link: format!("https://stock.naver.com/domestic/stock/{code}/discussion?chip=all"),
    })
    .collect()
}

// 목록에서 실행 시점 기준으로 하나를 고르는 함수입니다.
fn pseudo_index(len: usize) -> usize {
    if len == 0 {
        return 0;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    (nanos as usize) % len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_detail_url_prefers_specific_post_url_then_falls_back() {
        let url = "https://stock.naver.com/domestic/stock/035720/discussion/421063210?chip=all";
        // "특정 게시글" 댓글: 그 글 URL이 '게시내용' 링크가 된다(댓글만 보이지 않게).
        assert_eq!(
            comment_detail_url(&Some(url.to_owned()), None).as_deref(),
            Some(url)
        );
        // 공백뿐인 comment_url은 무시하고 매크로가 돌려준 글 URL로 떨어진다.
        assert_eq!(
            comment_detail_url(&Some("   ".to_owned()), Some("p".to_owned())).as_deref(),
            Some("p")
        );
        // 랜덤 글 댓글(대상 URL 없음, post_url도 None)은 링크가 없다(기존 동작).
        assert_eq!(comment_detail_url(&None, None), None);
        assert_eq!(
            comment_detail_url(&None, Some("p".to_owned())).as_deref(),
            Some("p")
        );
    }

    #[test]
    fn parses_template_csv_without_header_values() {
        let csv = "제목,내용,댓글내용\n제목1,내용1,댓글1\n제목2,내용2,댓글2\n";

        let parsed = parse_discussion_template_csv(csv.to_owned()).expect("csv should parse");

        assert_eq!(parsed.titles, vec!["제목1", "제목2"]);
        assert_eq!(parsed.bodies, vec!["내용1", "내용2"]);
        assert_eq!(parsed.comments, vec!["댓글1", "댓글2"]);
    }

    #[test]
    fn parses_quoted_csv_fields() {
        let csv = "제목,내용,댓글내용\n\"제목, 쉼표\",\"여러\n줄\",\"댓글\"\"따옴표\"";

        let parsed = parse_discussion_template_csv(csv.to_owned()).expect("csv should parse");

        assert_eq!(parsed.titles, vec!["제목, 쉼표"]);
        assert_eq!(parsed.bodies, vec!["여러\n줄"]);
        assert_eq!(parsed.comments, vec!["댓글\"따옴표"]);
    }

    #[test]
    fn rejects_single_mode_when_multiple_values_exist() {
        let values = vec!["a".to_owned(), "b".to_owned()];

        let error = validate_texts(&values, &PickMode::Single, "제목").unwrap_err();

        assert!(error.contains("정확히 1개"));
    }

    #[test]
    fn blocking_failure_detects_login_permission_but_not_rate_limit() {
        // #267-9: 로그인/권한/세션 만료·쿠키 없음 계열은 차단으로 본다(같은 계정 남은 글 skip).
        assert!(is_blocking_failure(
            "글쓰기 form 패킷 HTTP 실패: HTTP status 403 Forbidden for url (https://x)"
        ));
        assert!(is_blocking_failure("HTTP status 401 Unauthorized"));
        assert!(is_blocking_failure("SESSION_INVALID: contentJson…"));
        assert!(is_blocking_failure(
            "Chrome에서 네이버 로그인 쿠키를 찾지 못했습니다"
        ));
        assert!(is_blocking_failure(
            "로그인이 만료되었습니다. 다시 로그인해 주세요"
        ));
        // 요청 과다(429)는 일시적이라 차단이 아니다 — 건너뛰면 안 된다(사수 지침: 요청 과다 제외).
        assert!(!is_blocking_failure(
            "요청이 너무 많습니다. 잠시 후 다시 시도해 주세요"
        ));
        assert!(!is_blocking_failure("HTTP status 429 Too Many Requests"));
        // 일반 게시 실패(서버 오류/본문 구성 오류 등)는 차단이 아니다 — 다음 글은 정상 시도.
        assert!(!is_blocking_failure(
            "HTTP status 500 Internal Server Error"
        ));
        assert!(!is_blocking_failure(
            "글 내용을 구성하는 중 문제가 발생했습니다"
        ));
    }

    #[test]
    fn timed_out_failure_detects_timeout_and_server_500_not_others() {
        // #286: 페이지/응답 대기시간 초과 → 대기초과.
        assert!(is_timed_out_failure("페이지 대기시간 초과로 글 실패"));
        assert!(is_timed_out_failure("응답 시간이 초과되었습니다"));
        assert!(is_timed_out_failure("request timed out"));
        // #286: 네이버 서버 오류(HTTP 500) → 대기초과.
        assert!(is_timed_out_failure("HTTP status 500 Internal Server Error"));
        assert!(is_timed_out_failure(
            "네이버 서버에 문제가 발생했습니다"
        ));
        // 차단/비번오류/요청과다/일반실패는 대기초과가 아니다(다른 상태로 처리).
        assert!(!is_timed_out_failure("HTTP status 403 Forbidden"));
        assert!(!is_timed_out_failure("HTTP status 401 Unauthorized"));
        assert!(!is_timed_out_failure("HTTP status 429 Too Many Requests"));
        assert!(!is_timed_out_failure(
            "글 내용을 구성하는 중 문제가 발생했습니다"
        ));
        // 차단(401/403)이 동시에 잡히는 메시지는 호출부에서 차단을 먼저 보므로 여기선 500만 검사.
        assert!(!is_timed_out_failure("HTTP status 404 Not Found"));
    }

    #[test]
    fn retryable_forum_failure_only_for_timed_out_not_blocking_or_other() {
        // 대기초과(일시적 시간초과/서버오류)만 재시도한다.
        assert!(is_retryable_forum_failure("페이지 로드 대기 시간이 초과되었습니다."));
        assert!(is_retryable_forum_failure("네이버 서버에 문제가 발생했습니다"));
        assert!(is_retryable_forum_failure("HTTP status 500 Internal Server Error"));
        // 차단(401/403/쿠키)은 재시도해도 또 실패 → 재시도 금지.
        assert!(!is_retryable_forum_failure("HTTP status 403 Forbidden"));
        assert!(!is_retryable_forum_failure("쿠키를 찾지 못했습니다"));
        // 약관 동의하기 비활성 같은 미분류 실패도 재시도 대상 아님(대기초과가 아니므로).
        assert!(!is_retryable_forum_failure(
            "동의하기 버튼이 아직 비활성화 상태입니다."
        ));
        // 요청 과다(429)는 차단도 대기초과도 아니라 재시도 대상이 아니다.
        assert!(!is_retryable_forum_failure("HTTP status 429 Too Many Requests"));
    }

    #[test]
    fn forum_retry_backoff_doubles_then_caps() {
        // 끈질기되 상한: 3→6→12→24→30(캡)→30… 으로 늘되 FORUM_RETRY_BACKOFF_MAX_SECS에서 멈춘다.
        assert_eq!(forum_retry_delay_secs(0), 3);
        assert_eq!(forum_retry_delay_secs(1), 6);
        assert_eq!(forum_retry_delay_secs(2), 12);
        assert_eq!(forum_retry_delay_secs(3), 24);
        assert_eq!(forum_retry_delay_secs(4), FORUM_RETRY_BACKOFF_MAX_SECS); // 48→30 캡
        assert_eq!(forum_retry_delay_secs(5), FORUM_RETRY_BACKOFF_MAX_SECS);
        // 큰 회차에서도 오버플로 없이 캡 유지(saturating_shl).
        assert_eq!(forum_retry_delay_secs(99), FORUM_RETRY_BACKOFF_MAX_SECS);
    }

    #[test]
    fn collect_stock_candidates_reads_lowercase_naver_stock_fields() {
        let value = serde_json::json!([
            {
                "itemname": "삼성전자",
                "itemcode": "005930"
            }
        ]);
        let mut candidates = Vec::new();

        collect_stock_candidates(&value, &mut candidates);

        assert_eq!(candidates[0].name, "삼성전자");
        assert_eq!(candidates[0].code, "005930");
    }
}
