use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::ipc::kill::CancelSignal;

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
    /// 사용자 완전 종료(kill, 설계서 08)로 게시하지 않은 종목이면 true. `ok=false`·`skipped=false`와
    /// 구분해 게시 결과에 "중지"로 집계한다("성공 N 중지 M"). 진행 중이던 종목 1개는 정상 마치고,
    /// 그 뒤 남은 종목들이 이 값으로 기록된다(로컬·Admin 공통 forum 경로).
    #[serde(default)]
    pub stopped: bool,
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
    const BLOCKING_MARKERS: [&str; 10] = [
        "SESSION_INVALID",
        "NO_COOKIES",
        "LOGIN_FAILED",
        "로그인이 만료",
        "로그인 정보가 없",
        "권한이 없",
        "쿠키를 찾지 못",
        "로그인이 필요",
        "다시 로그인",
        // 계정 보호조치(잠금)/세션 무효 추정 — npay가 nid 로그인 페이지로 튕긴 계정. 재로그인 필요이며
        // 남은 글은 어차피 전부 500나므로 차단으로 보아 건너뛴다(2026-07-01, clarify_profile_status_error).
        "보호조치",
    ];
    BLOCKING_MARKERS.iter().any(|marker| m.contains(marker))
}

/// 게시 실패 메시지가 "대기초과"(일시적 서버/타이밍 문제)를 뜻하는지 판별한다(#286, 순수 함수).
/// 세 부류를 잡는다: (1) 페이지/응답 **대기시간 초과**, (2) 네이버 **서버 오류(HTTP 500)**,
/// (3) **일시적 네트워크 끊김**(소켓 10060/10053/10054, 2026-06-30 추가). 이런 실패는 계정·자격증명
/// 문제가 아니라 잠시 후 풀릴 수 있는 일시 상태라, 차단(Blocked)이나 비번 오류와 구분해 계정을
/// `TimedOut`(대기초과)으로 표시하고 재시도 대상으로 둔다. 차단 계열(`is_blocking_failure`)이
/// 우선이므로, 호출부는 먼저 차단을 보고 그 다음 이걸 본다. **429(요청 과다)도 여기 포함**한다
/// (2026-07-01, 사용자 지시): 429는 계정이 죽은 게 아니라 잠깐 요청이 몰린 것이라 재시도로 풀린다.
pub fn is_timed_out_failure(message: &str) -> bool {
    let m = message;
    // (3) 429(Too Many Requests, 요청 과다): 계정 차단이 아니라 레이트리밋(일시). 실측 2026-07-01:
    // 429로 한 종목 실패한 계정(jwy****)이 직후 다른 3종목을 정상 게시 = 계정 살아있음. 이런 계정을
    // Error로 죽이지 말고 대기초과(TimedOut·재시도)로 둔다(사용자 지시: "최종결과로 판단 — 뒤에
    // 성공하면 살아있는 것"). 차단(is_blocking_failure)은 429를 false로 두므로, 차단 우선 규칙과
    // 충돌하지 않는다(재시도 끝에 진짜 차단되면 그때 Blocked로 확정).
    if m.contains("429") || m.contains("요청이 너무 많") || m.contains("요청 과다") {
        return true;
    }
    // (2) 일시적 네트워크 끊김(소켓 10053/54/60)은 우리 망/원격이 잠깐 끊긴 것이라 재시도하면
    // 풀릴 여지가 있다(사용자 지시: 일시적 네트워크 불안정은 재시도 타협).
    if is_transient_network_failure(m) {
        return true;
    }
    // (1) 대기시간 초과: 페이지/응답이 자리잡기 전에 시간이 다한 경우(로딩 지연 — 재시도하면
    // 자리잡는 경우가 많다). HTTP 500/403 같은 "네이버 서버의 판정"은 여기에 넣지 않는다 —
    // 사용자가 통제할 수 없는 서버 문제라 재시도해도 또 실패하므로 빨리 실패시킨다(2026-06-30,
    // 사용자 지시: 내가 컨트롤 못 하는 500/403은 빨리 실패). 500은 errored로 떨어져 즉시 실패한다.
    const TIMEOUT_MARKERS: [&str; 5] = [
        "대기시간 초과",
        "시간이 초과",
        "시간 초과",
        "timed out",
        "timeout",
    ];
    TIMEOUT_MARKERS.iter().any(|marker| m.contains(marker))
}

/// 일시적 네트워크/연결 실패인지 판별한다(순수 함수, 2026-06-30). CDP WebSocket·HTTP 연결이 망
/// 끊김/무응답으로 끊어진 경우다. Windows 소켓 오류코드로 식별한다: 10060(WSAETIMEDOUT 연결
/// 시간초과)·10053(WSAECONNABORTED 호스트 SW가 끊음)·10054(WSAECONNRESET 상대가 리셋). 계정·
/// 자격증명 문제가 아니라 잠시 후 풀릴 수 있는 일시 상태라, 대기초과처럼 재시도 대상으로 본다
/// (사용자 지시: 풀릴 수 있는 건 재시도 — 모바일 IP 전환·원격 끊김 등으로 흔히 발생).
fn is_transient_network_failure(message: &str) -> bool {
    const NET_MARKERS: [&str; 3] = ["os error 10060", "os error 10053", "os error 10054"];
    NET_MARKERS.iter().any(|marker| message.contains(marker))
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
pub fn run_forum_publish<R, FS, FR, FRT, FC>(
    request: ForumPublishRequest,
    app: tauri::AppHandle<R>,
    mut on_start: FS,
    mut on_result: FR,
    mut on_retry: FRT,
    should_cancel: FC,
    critical: Arc<CancelSignal>,
) -> Vec<ForumPublishResult>
where
    R: Runtime,
    // 종목 게시 시작 직전(인덱스)과 완료 직후(인덱스, 결과)에 호출한다(#219). 큐 워커가
    // 이걸 받아 "진행 전 → 진행 중 → 완료/실패"를 실시간으로 보여준다.
    FS: FnMut(usize),
    FR: FnMut(usize, &ForumPublishResult),
    // 대기초과로 재시도할 때마다(인덱스, 현재 회차, 최대 회차) 호출한다(2026-06-30). 큐 워커가
    // 그 종목 칸을 "재시도중 N/M"으로 갱신해, 오래 걸리는 종목이 "게시 중…"으로 멈춘 듯 보이거나
    // 사라진 것처럼 보이지 않게 한다(사용자 지적).
    FRT: FnMut(usize, usize, usize),
    // 사용자 완전 종료(kill) 확인(설계서 08). 새 종목을 시작하기 전과 종목 사이 60초 대기 중에
    // 호출해, true면 남은 종목을 게시하지 않고 안전하게 멈춘다. 진행 중이던 종목 1개는 이미
    // 게시+결과기록까지 끝난 뒤라 반쪽글·중복이 없다.
    FC: Fn() -> bool,
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
        // 사용자 완전 종료(kill, 설계서 08): 새 종목을 시작하기 전에 확인해, 취소됐으면 남은
        // 종목을 게시하지 않고 멈춘다. 진행 중이던 종목 1개는 이미 게시+결과기록까지 끝난
        // 상태라 반쪽글·중복이 없고, Chrome은 호출부(run_forum_targets)가 정상 drop한다.
        if should_cancel() {
            tracing::info!(
                "[POST] {who} 종목토론방 {kind} 사용자 중지 — 남은 {}종목 게시 안 함(안전 경계에서 정지)",
                total - index
            );
            // 남은 종목(index..)을 "중지"로 기록한다 — 게시 결과에 "성공 N 중지 M"으로 뜨게(설계서 08).
            // on_result로 라이브 스켈레톤도 갱신하고, results에 담아 완료 로그·post-report에 실린다.
            // 로컬 🗑·Admin 원격 중지 모두 이 경로를 타므로 두 방식 다 동일하게 집계된다.
            for (i, s) in request.stocks.iter().enumerate().skip(index) {
                let stopped_result = ForumPublishResult {
                    code: s.code.clone(),
                    name: s.name.clone(),
                    ok: false,
                    message: "사용자 중지 — 게시하지 않음".to_owned(),
                    trace: None,
                    posted: None,
                    skipped: false,
                    stopped: true,
                };
                on_result(i, &stopped_result);
                results.push(stopped_result);
            }
            break;
        }
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
                stopped: false,
            });
            if let Some(last) = results.last() {
                on_result(index, last);
            }
            continue;
        }

        // add POST 임계구역(설계서 08 Stage2): 한 종목의 실제 게시(form→add→응답, 재시도 포함)
        // 동안 강제 kill(taskkill 에스컬레이션)을 보류시켜 "글은 올라갔는데 기록 못 함=이중게시"
        // 창을 막는다. 협조적 정지는 이 구간을 건드리지 않고(종목 사이에서만 멈춤) 여기 무관하다.
        critical.enter_critical();
        let outcome = run_one_forum_stock_with_retry(
            &request, stock, title, body, comment, &app, &who, kind, index, &mut on_retry,
        );
        critical.leave_critical();
        // 실패면 사용자용 메시지(message)와 캡처된 스택(trace)을 분리해 들고 간다(#199).
        // 성공 시 작성된 글 URL을 메시지에 함께 실어, 완료 로그에서 올라간 글을 확인할 수 있게 한다.
        let (ok, message, trace, posted) = match outcome {
            Ok(posted) => (true, "게시 완료".to_owned(), None, Some(posted)),
            Err(error) => (false, error.message().to_owned(), Some(error.trace()), None),
        };

        // 작업 결과를 pstmacro.log에 기록(가독성·상세화). 실패는 *실제 에러 메시지 + 캡처된
        // 백트레이스*를 함께 남긴다 — 내가 만든 요약("대기초과")만이 아니라 원본 실패 지점이
        // 로그 파일에 남아야 한다는 사수 지시 반영(백트레이스에 들어가는 내용).
        if ok {
            tracing::info!("[POST] {who}  \"{}\" 종목토론방 {kind} 성공 ✅", stock.name);
        } else {
            tracing::warn!(
                trace = %trace.as_deref().unwrap_or("(트레이스 없음)"),
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
            stopped: false,
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
            // 60초 대기를 1초 단위로 쪼개, 대기 중 사용자 중지(kill)가 즉시 반영되게 한다(설계서 08).
            // 취소되면 대기를 끊고, 다음 루프 상단의 should_cancel 검사가 남은 종목을 멈춘다.
            for _ in 0..60 {
                if should_cancel() {
                    break;
                }
                sleep(Duration::from_secs(1));
            }
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
// 재시도 횟수(사용자 지시 2026-06-30: 30→9). 대기초과·일시 500은 네이버 일시 부하일 수도,
// 계정 간 네트워크 선점 경합·우리 프로그램 일시 결함일 수도 있어 "재시도로 풀릴 수 있는" 실패다.
// 그래서 즉시 실패시키지 않고 9회까지 재시도하고, 9회 내내 같은 실패면 그때 "재시도로 못 고치는
// 문제"로 확정해 실패로 남긴다(한 계정이 30회로 배치를 수십 분 붙잡던 문제는 9회로 완화).
const FORUM_TIMEOUT_RETRIES: usize = 9;
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
#[allow(clippy::too_many_arguments)]
fn run_one_forum_stock_with_retry<R: Runtime>(
    request: &ForumPublishRequest,
    stock: &DiscussionStock,
    title: &str,
    body: &str,
    comment: &str,
    app: &tauri::AppHandle<R>,
    who: &str,
    kind: &str,
    // 이 종목의 스켈레톤 인덱스 + 재시도마다 UI를 "재시도중 N/M"으로 갱신할 콜백(2026-06-30).
    index: usize,
    on_retry: &mut impl FnMut(usize, usize, usize),
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
                    // UI 갱신: 이 종목 칸을 "재시도중 attempt/max"로 — 백오프 대기 동안 멈춘 듯/
                    // 사라진 듯 보이지 않게(사용자 지적). 백오프 sleep 전에 호출해 즉시 반영한다.
                    on_retry(index, attempt, FORUM_TIMEOUT_RETRIES);
                    sleep(Duration::from_secs(delay));
                    continue;
                }
                return Err(error);
            }
        }
    }
}

// 종목토론방 댓글 사이 최소 간격(사용자 요청 2026-07-01: 댓글 하나 달고 3초 텀). "특정 게시글"
// 댓글은 URL마다 별도 요청(plan_to_forum_requests)이라 같은 계정이 병렬로 동시에 댓글을 달면
// 네이버가 도배방지(code 5010)·"In process"(code 8001)로 막는다. 아래 스로틀로 같은 계정 댓글을
// 3초 간격으로 직렬화해 그 차단을 피한다(다른 계정은 서로 독립적으로 진행).
const COMMENT_MIN_GAP: Duration = Duration::from_secs(3);

// 계정별 "다음 댓글 허용 시각" 예약대장. 병렬 요청(계정별 Chrome, 각자 스레드)이 이 전역 대장을
// 공유해, 같은 계정의 댓글이 서로 최소 COMMENT_MIN_GAP 간격이 되게 슬롯을 잡는다.
static COMMENT_NEXT_ALLOWED: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 이 계정의 댓글 슬롯을 예약하고, 그 시각까지 대기한다. 같은 계정 댓글 N건이 동시에 들어와도
/// 각자 now, now+3s, now+6s… 슬롯을 잡아 3초 간격으로 직렬화된다. 락은 슬롯 계산 동안만 잡고
/// 실제 대기(sleep)는 락 밖에서 하므로, 다른 계정은 막히지 않는다.
fn throttle_account_comment(account_id: &str) {
    let now = Instant::now();
    let scheduled = {
        let mut map = COMMENT_NEXT_ALLOWED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reserve_comment_slot(&mut map, account_id, now)
    };
    if scheduled > now {
        sleep(scheduled - now);
    }
}

/// 이 계정의 이번 댓글 시각(슬롯)을 정하고, 대장의 "다음 허용 시각"을 +COMMENT_MIN_GAP로 민다
/// (순수 로직, 테스트 대상). 예약이 없거나 과거면 now, 있으면 그 예약 시각(≥now)을 쓴다.
fn reserve_comment_slot(
    map: &mut HashMap<String, Instant>,
    account_id: &str,
    now: Instant,
) -> Instant {
    let slot = map.get(account_id).copied().unwrap_or(now).max(now);
    map.insert(account_id.to_owned(), slot + COMMENT_MIN_GAP);
    slot
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
    // 댓글이 포함된 작업이면(특정 게시글 댓글·글+댓글), 같은 계정 댓글을 3초 간격으로 직렬화해
    // 도배방지 차단을 피한다(사용자 요청). 글만 올리는 작업은 영향 없다(스로틀 안 탐).
    if request.run_comment {
        throttle_account_comment(&request.account_id);
    }
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
    fn comment_slots_space_same_account_by_gap_but_not_across_accounts() {
        // 같은 계정 댓글 3건이 같은 순간(now)에 들어와도 now, now+3s, now+6s로 3초씩 벌어지고,
        // 다른 계정은 서로 영향 없이 각자 now에 시작한다(도배방지 회피 + 병렬성 유지).
        let mut map: HashMap<String, Instant> = HashMap::new();
        let now = Instant::now();

        let a1 = reserve_comment_slot(&mut map, "acc-A", now);
        let a2 = reserve_comment_slot(&mut map, "acc-A", now);
        let a3 = reserve_comment_slot(&mut map, "acc-A", now);
        assert_eq!(a1, now);
        assert_eq!(a2, now + COMMENT_MIN_GAP);
        assert_eq!(a3, now + COMMENT_MIN_GAP * 2);

        // 다른 계정 B는 A의 예약과 무관하게 now에 시작한다.
        let b1 = reserve_comment_slot(&mut map, "acc-B", now);
        assert_eq!(b1, now);

        // 예약 시각이 이미 지난(과거) 계정은 다시 now부터 시작한다(불필요한 대기 없음).
        let later = now + COMMENT_MIN_GAP * 10;
        let a_after = reserve_comment_slot(&mut map, "acc-A", later);
        assert_eq!(a_after, later);
    }

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
        // 계정 보호조치/세션 무효(npay가 nid 로그인 페이지로 튕김) → 차단으로 본다(2026-07-01).
        assert!(is_blocking_failure(
            "계정 세션 무효/보호조치 추정 — 재로그인이 필요합니다. 이 계정의 남은 글은 건너뜁니다."
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
    fn timed_out_failure_detects_timeout_not_server_500() {
        // 페이지/응답 대기시간 초과(로딩 지연) → 재시도 대상(대기초과).
        assert!(is_timed_out_failure("페이지 대기시간 초과로 글 실패"));
        assert!(is_timed_out_failure("응답 시간이 초과되었습니다"));
        assert!(is_timed_out_failure("request timed out"));
        // 2026-06-30(사용자 지시): HTTP 500/서버오류는 네이버 서버의 판정(통제 불가)이라 재시도해도
        // 또 실패 → 대기초과가 아니다(빨리 실패시킨다). 일시 네트워크 끊김(소켓)만 재시도한다.
        assert!(!is_timed_out_failure("HTTP status 500 Internal Server Error"));
        assert!(!is_timed_out_failure(
            "네이버 서버에 문제가 발생했습니다"
        ));
        // 차단/비번오류/일반실패는 대기초과가 아니다(다른 상태로 처리).
        assert!(!is_timed_out_failure("HTTP status 403 Forbidden"));
        assert!(!is_timed_out_failure("HTTP status 401 Unauthorized"));
        assert!(!is_timed_out_failure(
            "글 내용을 구성하는 중 문제가 발생했습니다"
        ));
        assert!(!is_timed_out_failure("HTTP status 404 Not Found"));
        // 2026-07-01(사용자 지시): 429(요청 과다)는 계정 죽은 게 아니라 레이트리밋(일시) → 대기초과로
        // 재시도한다(실측: 429 실패 계정이 직후 다른 종목 정상 게시). 실제 로그 메시지 형태로도 검증.
        assert!(is_timed_out_failure(
            "글쓰기 form 패킷 HTTP 실패: HTTP status 429 Too Many Requests for url (https://m.stock.naver.com/…)"
        ));
        assert!(is_timed_out_failure(
            "요청이 너무 많습니다. 잠시 후 다시 시도해 주세요"
        ));
        // 429는 차단이 아니어야(is_blocking_failure=false) 대기초과 규칙과 충돌하지 않는다.
        assert!(!is_blocking_failure("HTTP status 429 Too Many Requests"));
    }

    #[test]
    fn transient_network_failures_are_retryable_timeouts() {
        // 2026-06-30: 일시적 소켓 끊김(10060/10053/10054)도 대기초과처럼 재시도 대상이다.
        let e10060 = "IO error: 연결된 구성원으로부터 응답이 없어 연결하지 못했거나, \
             호스트로부터 응답이 없어 연결이 끊어졌습니다. (os error 10060)";
        let e10053 = "IO error: 현재 연결은 사용자의 호스트 시스템의 소프트웨어에 의해 중단되었습니다. (os error 10053)";
        for m in [e10060, e10053, "connection reset (os error 10054)"] {
            assert!(is_transient_network_failure(m), "네트워크 끊김 감지: {m}");
            assert!(is_timed_out_failure(m), "대기초과로 분류: {m}");
            assert!(is_retryable_forum_failure(m), "재시도 대상: {m}");
        }
        // 차단/비번오류 등 비-네트워크는 영향 없음(regression 방지).
        assert!(!is_transient_network_failure("HTTP status 403 Forbidden"));
        assert!(!is_transient_network_failure("동의하기 버튼이 아직 비활성화 상태입니다."));
    }

    #[test]
    fn server_500_fails_fast_not_retried() {
        // 2026-06-30(사용자 지시): 500은 네이버 서버의 문제(통제 불가)라 재시도해도 또 실패한다 →
        // 대기초과/재시도 대상이 아니라 빨리 실패시킨다. 프로필 상태 500도, 일반 500도 동일.
        let profile_500 = "프로필 상태 패킷 HTTP 실패: status=500, \
             body={\"message\":\"Failed to fetch profile user status\"}";
        assert!(!is_timed_out_failure(profile_500));
        assert!(!is_retryable_forum_failure(profile_500));
        assert!(!is_retryable_forum_failure("HTTP status 500 Internal Server Error"));
    }

    #[test]
    fn forum_timeout_retries_capped_at_nine() {
        // 사용자 지시 2026-06-30: 30 → 9. 한 계정이 30회로 배치를 수십 분 붙잡던 문제 완화.
        assert_eq!(FORUM_TIMEOUT_RETRIES, 9);
    }

    #[test]
    fn retryable_forum_failure_only_for_timed_out_not_blocking_or_other() {
        // 대기초과(로딩 지연)·일시 네트워크 끊김만 재시도한다.
        assert!(is_retryable_forum_failure("페이지 로드 대기 시간이 초과되었습니다."));
        assert!(is_retryable_forum_failure("connection reset (os error 10054)"));
        // 500/서버오류는 네이버 판정(통제 불가) → 재시도 금지(빨리 실패, 2026-06-30 사용자 지시).
        assert!(!is_retryable_forum_failure("네이버 서버에 문제가 발생했습니다"));
        assert!(!is_retryable_forum_failure("HTTP status 500 Internal Server Error"));
        // 차단(401/403/쿠키)은 재시도해도 또 실패 → 재시도 금지.
        assert!(!is_retryable_forum_failure("HTTP status 403 Forbidden"));
        assert!(!is_retryable_forum_failure("쿠키를 찾지 못했습니다"));
        // 약관 동의하기 비활성 같은 미분류 실패도 재시도 대상 아님(대기초과가 아니므로).
        assert!(!is_retryable_forum_failure(
            "동의하기 버튼이 아직 비활성화 상태입니다."
        ));
        // 요청 과다(429)는 2026-07-01(사용자 지시)부터 대기초과(일시)로 보아 재시도 대상이다 —
        // 계정이 죽은 게 아니라 레이트리밋이므로 재시도로 풀린다(차단은 아니라 blocking=false 유지).
        assert!(is_retryable_forum_failure("HTTP status 429 Too Many Requests"));
        assert!(is_retryable_forum_failure(
            "글쓰기 form 패킷 HTTP 실패: HTTP status 429 Too Many Requests for url (https://x)"
        ));
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
