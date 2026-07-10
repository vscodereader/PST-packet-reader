mod browser_flow;
mod devtools_connection;
mod like_flow;
mod packet_client;
pub mod types;

pub use types::{
    AutomationReport, AutomationTarget, DiscussionSelection, DiscussionStock,
    NaverDiscussionRequest, NaverLoginProfile, NaverPostWithCommentRequest,
};

use devtools_connection::{select_or_create_target, websocket_url_for_host};
use serde_json::{json, Value};
use std::fmt::{Display, Formatter};
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::thread::sleep;
use std::time::{Duration, Instant};
use tauri::{Emitter, Runtime};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const DISCUSSION_URL: &str = "https://stock.naver.com/discussion";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// DevTools WebSocket 핸드셰이크 전 TCP 연결 타임아웃. tungstenite `connect()`는 연결에
/// 타임아웃이 없어, Chrome이 떴지만 DevTools가 응답하지 않으면 무한 대기한다(#210 로그인
/// 멈춤의 한 원인). TCP 연결을 이 시간으로 묶는다(이후 입출력은 DEFAULT_TIMEOUT).
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

type AutomationResult<T> = Result<T, AutomationError>;

#[derive(Debug)]
pub struct AutomationError {
    message: String,
    // 에러를 만든 호출 지점(파일:줄)을 컴파일타임에 기록 — 백트레이스의 앵커(#199).
    // #[track_caller]로 new()의 호출자(실제 실패 지점)를 잡으므로, 런타임 백트레이스 심볼이
    // 일부 <unknown>이어도 실패 지점만큼은 항상 보장된다.
    location: String,
    // 에러 생성 시점에 캡처한 런타임 호출 스택 — "자세히 보기" trace의 본문(#199).
    // 디버그 정보가 있으면(전 플랫폼: release debug=true) 프레임이 심볼로 해석된다.
    backtrace: String,
}

impl AutomationError {
    // 자동화 중 발생한 오류 메시지를 생성하는 함수입니다.
    #[track_caller]
    fn new(message: impl Into<String>) -> Self {
        let loc = std::panic::Location::caller();
        Self {
            message: message.into(),
            location: format!("at {}:{}:{}", loc.file(), loc.line(), loc.column()),
            backtrace: crate::util::backtrace_string(),
        }
    }

    /// 사용자용 한 줄 오류 메시지(위치 제외).
    pub fn message(&self) -> &str {
        &self.message
    }

    /// "자세히 보기"용 — 실패 지점 앵커(`at 파일:줄:열`).
    pub fn location(&self) -> &str {
        &self.location
    }

    /// "자세히 보기"용 — 앵커(항상 보장) + 캡처된 런타임 호출 스택을 합친 개발자 trace.
    pub fn trace(&self) -> String {
        format!("{}\n\n{}", self.location, self.backtrace)
    }
}

impl Display for AutomationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AutomationError {}

impl From<std::io::Error> for AutomationError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<serde_json::Error> for AutomationError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<tungstenite::Error> for AutomationError {
    fn from(error: tungstenite::Error) -> Self {
        Self::new(error.to_string())
    }
}

/// 에러 메시지가 "연결 중단/끊김"(재접속하면 살릴 수 있는 부류)인지 판별한다(순수 함수, 2026-06-30).
/// CDP WebSocket(127.0.0.1)이 호스트 소프트웨어(백신/방화벽/원격데스크톱)·망 변화로 끊긴 경우다.
/// Windows 소켓코드 10053(ECONNABORTED 호스트 SW가 끊음)·10054(ECONNRESET 상대 리셋)·10060
/// (ETIMEDOUT 연결 시간초과)과, tungstenite의 연결종료 문구를 본다. 타임아웃(읽기/쓰기 초과)은
/// 소켓이 살아있을 수 있어 제외한다 — 여기선 "끊김"만 재접속 대상으로 본다.
fn is_connection_lost_message(message: &str) -> bool {
    const MARKERS: [&str; 7] = [
        "os error 10053",
        "os error 10054",
        "os error 10060",
        "연결이 닫혔습니다",
        "Connection reset",
        "Connection aborted",
        "ConnectionClosed",
    ];
    MARKERS.iter().any(|marker| message.contains(marker))
}

/// CDP 와이어 트레이스 on/off. **기본 ON** — 다른 컴퓨터에서도 빌드만 하면 크롬과 주고받는 모든
/// CDP 명령·응답·이벤트가 원문 그대로 로그에 남는다(무슨 API 를 어떤 파라미터로 호출했고 응답이
/// 뭐였는지 통째로). 로그가 커지므로 끄려면 환경변수 `PSTMACRO_CDP_TRACE=0`(또는 `false`/`off`).
/// 순수 로깅이라 네이버로 보내는 내용·페이지를 바꾸지 않아 봇탐지/캡차엔 영향이 없다(Network 등
/// 새 도메인을 켜지 않음).
fn cdp_trace_enabled() -> bool {
    match std::env::var("PSTMACRO_CDP_TRACE") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        // 미설정 = 기본 ON(C안). 빌드만 하면 원문 트레이스가 나온다.
        Err(_) => true,
    }
}

/// 로그인 CDP **네트워크 원문 로깅** on/off. **기본 OFF(opt-in)** — 로그인은 봇탐지 표면을 줄이려
/// Network/Runtime 도메인을 일부러 끈다(`enable_page_only` 주석 참고: naver wtm 의 CDP 탐지 =
/// navigator.webdriver·타이핑과 무관하게 캡차를 띄우는 최강 신호). 진단이 필요할 때만 환경변수
/// `PSTMACRO_LOGIN_NETLOG=1`(또는 `true`/`on`)로 켜면, 로그인 브라우저가 네이버와 실제로 주고받는
/// 요청/응답/쿠키를 CDP `Network.*` 이벤트로 받아 기존 `cdp:` 트레이스에 **원문 그대로** 남긴다.
/// ⚠️ 켜면 Network 도메인 활성화로 탐지 표면이 늘어 **캡차율이 오를 수 있다**(그래서 기본 OFF).
/// 실제 통신을 리스크 0으로 뜨려면 브라우저를 안 건드리는 tshark 패킷 캡처(+TLS keylog)를 쓸 것.
fn login_netlog_enabled() -> bool {
    matches!(
        std::env::var("PSTMACRO_LOGIN_NETLOG"),
        Ok(v) if { let v = v.trim(); v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on") }
    )
}

/// 트레이스에 남길 파라미터를 문자열로 만든다. `Input.dispatchKeyEvent` 의 실제 글자
/// (`text`/`key`/`unmodifiedText`)만 `•` 로 가린다 — 아이디/비밀번호 원문이 로그 파일에 남아
/// 공유 시 유출되는 것을 막기 위함(진단에 필요한 code·keyCode·modifiers·응답은 그대로 남는다).
fn redact_cdp_params(method: &str, params: &Value) -> String {
    if method != "Input.dispatchKeyEvent" {
        return params.to_string();
    }
    let mut p = params.clone();
    if let Some(obj) = p.as_object_mut() {
        for field in ["text", "key", "unmodifiedText"] {
            if let Some(v) = obj.get_mut(field) {
                if v.is_string() {
                    *v = Value::String("•".to_owned());
                }
            }
        }
    }
    p.to_string()
}

// 네이버 로그인 확인부터 토론방 선택, 글쓰기/댓글 등록까지 전체 흐름을 실행하는 함수입니다.
// 글/글+댓글 매크로가 공유하는 진입 셋업 결과(패킷 클라이언트·로그인·선택 종목). Chrome은 더
// 이상 쓰지 않는다 — 저장 쿠키를 패킷 클라이언트에 직접 로드하므로(카페 경로와 동일).
struct ForumDiscussionSession {
    packet_client: packet_client::NaverPacketClient,
    login_profile: NaverLoginProfile,
    // npay 가입 판정 — 프로필 상태 500이 났을 때 "계정 보호조치(nid 인증 거부)"인지 가르는 데 쓴다.
    npay_status: packet_client::NpayJoinStatus,
    selected: DiscussionSelection,
    // 종목토론방 URL(선택 종목은 코드로 생성, 랜덤은 패킷 API). 브라우저를 이 URL로 *이동시키지
    // 않고*, submit_post/submit_comment의 referer·target 파싱용 문자열로만 쓴다(페이지 이동/로드
    // 제거 — 사수 지시 2026-06-30).
    room_url: String,
}

// 글/글+댓글 매크로 공통 셋업: 저장 쿠키 로드 → 패킷 클라이언트 → 로그인 확인 → npay 가입 →
// 토론방 선택까지 한 번에 수행한다. 두 경로가 동일하게 중복하던 블록을 단일 함수로 합쳐 분기
// 누락·드리프트를 막는다.
//
// Chrome은 더 이상 쓰지 않는다(#344 후속). 예전엔 Chrome에 쿠키를 주입→getAllCookies로 되뽑아
// 패킷 클라이언트를 만들었지만, 그건 저장 쿠키를 우회로 옮기는 것일 뿐 페이지 조작은 없었다.
// 카페(`naver_cafe`) 경로처럼 저장 쿠키(`read_account_cookies`)를 곧바로 패킷 클라이언트에
// 로드하면 Chrome이 전혀 필요 없다. 글쓰기/댓글/수정은 전부 순수 HTTP 패킷 API로 처리한다.
fn open_discussion_session(
    account_id: Option<&str>,
    stock: Option<&DiscussionStock>,
) -> AutomationResult<ForumDiscussionSession> {
    // 종목토론방 게시는 항상 계정 ID로 저장 쿠키를 찾는다 — 없으면 로드할 세션이 없어 명확히 실패.
    let account_id = account_id.ok_or_else(|| {
        AutomationError::new(
            "종목토론방 게시에 계정 ID가 없습니다(저장된 로그인 쿠키를 찾을 수 없음).",
        )
    })?;
    let mut packet_client = packet_client::NaverPacketClient::from_saved_cookies(account_id)?;
    let login_profile = packet_client.read_login_profile()?;

    // 세션 만료(getProfile가 비로그인으로 응답)면 여기서 차단 처리한다 — 예전엔 게시 직전 페이지
    // 리다이렉트(nid.naver.com)로 감지했으나, 이제 페이지 이동을 안 하므로 getProfile 결과로 본다.
    if !login_profile.logged_in {
        return Err(AutomationError::new(format!(
            "네이버 로그인이 확인되지 않았습니다(세션 만료 추정). 계정을 다시 로그인한 뒤 시도하세요. ({})",
            login_profile.message
        )));
    }

    // 네이버페이 금융서비스 가입(=종목토론방 "동의하기")을 패킷으로 보장한다. 예전엔 브라우저
    // 이동(ensure_discussion_page)이 약관 페이지를 만나면 처리했는데, #344가 페이지 이동을 없애며
    // 이 단계가 빠졌다 — 동의가 안 된 계정은 글쓰기 form 발급이 404로 막힌다. "동의하기"는 사실
    // 가입 URL로 가는 GET 리다이렉트 체인이라(체크박스 아님), 로그인 쿠키를 든 패킷 클라이언트로
    // 그 URL을 GET 하면 가입이 완료된다. 멱등(이미 가입이면 무해)이고 비치명적(전송 실패해도
    // 글쓰기는 시도) — 가입 완료 여부는 메서드가 로그로 남긴다.
    let npay_status = packet_client.ensure_npay_financial_join();

    // 종목토론방 URL을 코드로 직접 만들거나(선택 종목) 패킷 API로 랜덤 선택한다. 브라우저를 그 URL로
    // 이동시키지 않는다 — submit_post/submit_comment는 이 URL을 referer·target 파싱용 문자열로만
    // 쓰며(예전 current_url()이 돌려주던 값과 동일), 실제 게시는 HTTP 패킷 API가 한다.
    let (selected, room_url) = match stock {
        Some(stock) => {
            let url = format!(
                "https://stock.naver.com/domestic/stock/{}/discussion?chip=all",
                stock.code.trim()
            );
            let selection = DiscussionSelection {
                category: "사용자 선택".to_owned(),
                rank: "-".to_owned(),
                item_text: format!("{} ({})", stock.name.trim(), stock.code.trim()),
                method: "ui-selected-stock".to_owned(),
            };
            (selection, url)
        }
        None => {
            let room = packet_client.select_random_discussion_room()?;
            (room.selection, room.discussion_url)
        }
    };

    Ok(ForumDiscussionSession {
        packet_client,
        login_profile,
        npay_status,
        selected,
        room_url,
    })
}

/// 프로필 상태 조회가 500으로 실패했을 때, 그 원인이 서로 달라도 똑같이 "프로필 상태 500"으로만
/// 보이던 걸(사용자 지적 2026-07-01) npay 판정에 따라 **상황별 메시지**로 바꾼다. 성공(Ok)이면 그대로.
/// - `LoginRequired`(nid가 로그인 페이지로 튕김): 세션 무효/계정 보호조치 추정 → **차단성 메시지**로
///   바꿔(is_blocking_failure) 이 계정의 남은 글을 건너뛴다(어차피 전부 500). "재로그인 필요".
/// - `TermsPending`(commonTermAgree): npay 필수약관 미완료 → "재로그인하면 자동 가입(#364)" 안내.
/// - `Completed`/`Unknown` + 500: npay는 됐는데 500 → 원본 메시지 유지(진짜 다른 프로필 문제).
/// 500이 아닌 실패나 성공은 건드리지 않는다.
fn clarify_profile_status_error(
    result: AutomationResult<bool>,
    npay_status: packet_client::NpayJoinStatus,
) -> AutomationResult<bool> {
    let Err(error) = result else {
        return result;
    };
    let msg = error.message();
    if !(msg.contains("프로필 상태") && msg.contains("500")) {
        return Err(error);
    }
    match npay_status {
        packet_client::NpayJoinStatus::LoginRequired => Err(AutomationError::new(format!(
            "계정 세션 무효/보호조치 추정 — 재로그인이 필요합니다. npay 가입이 nid 로그인 페이지로 튕겨(이 계정 인증 거부) 프로필 상태가 500으로 막혔습니다. 재로그인해도 막혀 있으면 계정 보호조치입니다. 이 계정의 남은 글은 건너뜁니다. (원본: {msg})"
        ))),
        packet_client::NpayJoinStatus::TermsPending => Err(AutomationError::new(format!(
            "npay 금융서비스(필수약관) 미완료로 프로필 상태 조회가 500입니다 — 이 계정을 재로그인하면 로그인 시점에 자동 가입을 시도합니다(#364). (원본: {msg})"
        ))),
        packet_client::NpayJoinStatus::Completed | packet_client::NpayJoinStatus::Unknown => {
            Err(error)
        }
    }
}

/// 좋아요 전용 흐름은 [`like_flow`] 모듈에 있다(게시 경로와 분리 — 게시의 npay/프로필 로직을
/// 건드리지 않는다). 여기서 재수출해 기존 호출부(`lib.rs`)의 import 경로를 유지한다.
pub use like_flow::{run_naver_dislike, run_naver_like, LikeVerdict};

/// 글 내용 변경(설계서 §5): `content_change`가 있으면 글 게시(submit_post) 후 `delay_sec`초 뒤
/// 새 제목/본문으로 edit한다(같은 세션·크롬 kill 전). edit 실패는 로그만 남기고 게시 자체는
/// 성공으로 둔다(edit 실패가 게시를 실패로 만들지 않게 — 사수 지시). `None`이면 아무것도 안 한다.
fn maybe_edit_after_post(
    packet_client: &mut packet_client::NaverPacketClient,
    post_id: &str,
    content_change: Option<&crate::ipc::queue::ContentChange>,
) {
    let Some(change) = content_change else {
        return;
    };
    sleep(Duration::from_secs(u64::from(change.delay_sec)));
    if let Err(error) = packet_client.edit_post(post_id, &change.title, &change.body) {
        tracing::warn!(
            post_id = %post_id,
            "글 게시 후 내용 변경(edit) 실패 — 게시는 성공으로 둠: {}",
            error.message()
        );
    }
}

/// 닉네임 랜덤 댓글(설계서 §2): `used`에 없는 닉네임으로 프로필을 바꾸고 성공한 닉네임을 `used`에
/// 넣어 같은 계정의 다음 댓글과 겹치지 않게 한다. 변경 실패(네트워크/프로필 오류)는 로그만 남기고
/// 기존 닉네임으로 진행한다(닉네임 변경 실패가 댓글을 막지 않게).
fn maybe_randomize_nickname(
    packet_client: &mut packet_client::NaverPacketClient,
    used: &mut std::collections::HashSet<String>,
) {
    match packet_client.change_nickname_avoiding(used) {
        Ok(nickname) => {
            used.insert(nickname);
        }
        Err(error) => {
            tracing::warn!(
                "댓글 닉네임 랜덤 변경 실패 — 기존 닉네임으로 진행: {}",
                error.message()
            );
        }
    }
}

pub fn run_naver_discussion_macro(
    request: NaverDiscussionRequest,
    // 닉네임 랜덤 댓글(설계서 §2)에서 계정 내 이미 쓴 닉네임을 누적하는 집합. 호출부(run_forum_publish)가
    // 계정 단위로 소유해, 매크로가 stock마다 세션을 새로 열어도 같은 계정 안에서 닉네임이 겹치지 않게 한다.
    used: &mut std::collections::HashSet<String>,
) -> AutomationResult<AutomationReport> {
    let title = request.title.trim();
    let body = request.body.trim();

    if matches!(request.target, AutomationTarget::Post) && title.is_empty() {
        return Err(AutomationError::new("제목이 비어 있습니다."));
    }

    if body.is_empty() {
        return Err(AutomationError::new("내용이 비어 있습니다."));
    }

    // 매크로 시작~세션 오픈(저장 쿠키 로드·로그인 확인)까지의 실제 소요시간을 로그로 남긴다.
    let macro_started = Instant::now();
    tracing::info!(target_kind = ?request.target, "게시 매크로 시작 — 세션 오픈 진입");
    let ForumDiscussionSession {
        mut packet_client,
        login_profile,
        npay_status,
        selected,
        room_url,
    } = open_discussion_session(request.account_id.as_deref(), request.stock.as_ref())?;
    tracing::info!(
        elapsed_secs = macro_started.elapsed().as_secs(),
        "세션 오픈 완료 — 글/댓글 등록 단계 시작(페이지 이동 없이 패킷 API로 게시)"
    );

    let mut posted_url: Option<String> = None;
    // 결과 보고용 URL — 글이면 종목토론방, 댓글이면 댓글 단 글 URL.
    let mut report_url = room_url.clone();
    let (register_button_highlighted, submitted) = match request.target {
        AutomationTarget::Post => {
            // 글쓰기 전에 종목토론방 프로필(닉네임+소개 2222)을 보장한다. 프로필이 없으면
            // 글쓰기 토큰 발급(discussion/form)이 404가 난다. 멱등이라 이미 있으면 즉시 통과.
            clarify_profile_status_error(
                packet_client.ensure_profile_intro_setup(&room_url),
                npay_status,
            )?;
            if request.submit_after_fill {
                // 글쓰기 add 패킷 API로 게시하고, 응답 id로 작성 글 URL을 만든다(브라우저 이동 없음).
                let post_id = packet_client.submit_post(&room_url, title, body)?;
                posted_url = Some(packet_client.post_url_from_id(&room_url, &post_id)?);
                maybe_edit_after_post(
                    &mut packet_client,
                    &post_id,
                    request.content_change.as_ref(),
                );
                (false, true)
            } else {
                // 수동 확인 모드(브라우저 폼 채우기)는 종목토론방 Chrome 제거로 더 이상 지원하지
                // 않는다(#344 후속). 실제 게시 경로(큐·즉시게시·CLI)는 전부 submit_after_fill=true다.
                return Err(AutomationError::new(
                    "수동 확인 모드(브라우저 폼 채우기)는 지원되지 않습니다. 자동 게시(submit_after_fill)를 사용하세요.",
                ));
            }
        }
        AutomationTarget::Comment => {
            // 댓글 대상 글 URL을 정한다: "특정 게시글"이면 그 URL, 아니면 종목토론방에서 랜덤 글을
            // 패킷 API로 고른다(brower 이동 없음).
            let comment_target_url = match request
                .comment_url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
            {
                Some(url) => url.to_owned(),
                None => {
                    packet_client
                        .select_random_discussion_post(&room_url)?
                        .post_url
                }
            };
            report_url = comment_target_url.clone();
            clarify_profile_status_error(
                packet_client.ensure_profile_intro_setup(&comment_target_url),
                npay_status,
            )?;
            if request.submit_after_fill {
                if request.comment_nickname_random {
                    maybe_randomize_nickname(&mut packet_client, used);
                }
                packet_client.submit_comment(&comment_target_url, body)?;
                (false, true)
            } else {
                // 수동 확인 모드는 Chrome 제거로 미지원(위 글쓰기 분기와 동일).
                return Err(AutomationError::new(
                    "수동 확인 모드(브라우저 폼 채우기)는 지원되지 않습니다. 자동 게시(submit_after_fill)를 사용하세요.",
                ));
            }
        }
    };

    Ok(AutomationReport {
        current_url: report_url,
        post_url: posted_url,
        login_profile,
        register_button_highlighted,
        submitted,
        selected,
        target: request.target,
    })
}

// 글쓰기 패킷 등록 후 방금 작성한 글 URL에 댓글 패킷을 이어서 전송하는 함수입니다.
// sleep_after가 true이면 글 등록 직후 "batch-wait-start" 이벤트를 emit하고,
// 댓글 작성이 끝난 뒤 1분 중 남은 시간을 기다립니다.
pub fn run_naver_post_with_comment_macro<R: Runtime>(
    request: NaverPostWithCommentRequest,
    app: &tauri::AppHandle<R>,
    sleep_after: bool,
    // 닉네임 랜덤 댓글(설계서 §2) 계정 내 누적 집합 — run_naver_discussion_macro와 동일 계약.
    used: &mut std::collections::HashSet<String>,
) -> AutomationResult<Vec<AutomationReport>> {
    let title = request.title.trim();
    let body = request.body.trim();
    let comment = request.comment.trim();

    if title.is_empty() {
        return Err(AutomationError::new("제목이 비어 있습니다."));
    }

    if body.is_empty() {
        return Err(AutomationError::new("내용이 비어 있습니다."));
    }

    if comment.is_empty() {
        return Err(AutomationError::new("댓글 내용이 비어 있습니다."));
    }

    let ForumDiscussionSession {
        mut packet_client,
        login_profile,
        npay_status,
        selected,
        room_url,
    } = open_discussion_session(request.account_id.as_deref(), request.stock.as_ref())?;

    // 글쓰기 전에 종목토론방 프로필(닉네임+소개 2222)을 보장한다(없으면 글쓰기 form 404). 멱등.
    clarify_profile_status_error(
        packet_client.ensure_profile_intro_setup(&room_url),
        npay_status,
    )?;

    // 글쓰기 add 패킷 API로 게시하고, 응답 id로 작성 글 URL을 만든다(페이지 이동 없음).
    let post_id = packet_client.submit_post(&room_url, title, body)?;
    let post_url = packet_client.post_url_from_id(&room_url, &post_id)?;
    // 글 내용 변경(설계서 §5): 글→edit→댓글 순서를 유지하려 댓글 전에 여기서 edit한다.
    maybe_edit_after_post(&mut packet_client, &post_id, request.content_change.as_ref());
    let post_report = AutomationReport {
        current_url: room_url.clone(),
        post_url: Some(post_url.clone()),
        login_profile: login_profile.clone(),
        register_button_highlighted: false,
        submitted: true,
        selected: selected.clone(),
        target: AutomationTarget::Post,
    };

    // 글 등록 직후: 타이머를 시작하고 댓글 작성에 걸린 시간을 기록합니다.
    let post_done_at = Instant::now();
    if sleep_after {
        let _ = app.emit("batch-wait-start", serde_json::json!({ "seconds": 60u64 }));
    }

    // 방금 쓴 글에 댓글을 단다 — 페이지 이동 없이 글 URL을 referer로 패킷 API 호출.
    clarify_profile_status_error(
        packet_client.ensure_profile_intro_setup(&post_url),
        npay_status,
    )?;
    if request.comment_nickname_random {
        maybe_randomize_nickname(&mut packet_client, used);
    }
    packet_client.submit_comment(&post_url, comment)?;
    let comment_report = AutomationReport {
        current_url: post_url.clone(),
        post_url: None,
        login_profile,
        register_button_highlighted: false,
        submitted: true,
        selected,
        target: AutomationTarget::Comment,
    };

    // 댓글 작성에 걸린 시간을 제외한 나머지 시간을 기다려 총 1분을 채웁니다.
    if sleep_after {
        let elapsed = post_done_at.elapsed();
        let total_wait = Duration::from_secs(60);
        if elapsed < total_wait {
            sleep(total_wait - elapsed);
        }
    }

    Ok(vec![post_report, comment_report])
}

pub(crate) struct CdpClient {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    // 재접속(reconnect)용으로 연결 대상을 보관한다. 폴링 중 소켓이 호스트 소프트웨어/원격끊김
    // 등으로 중단(10053/10054 등)되면 같은 Chrome 디버그 포트로 다시 붙어 명령을 재시도한다.
    host: String,
    port: u16,
    // 페이지 로드 중 *브라우저(크롬)가 직접 던진* 네트워크 요청을 추적한다(대기초과 진단용).
    // 우리 Rust 패킷이 아니라 브라우저 내부 요청이라, CDP Network 도메인 이벤트로만 "무슨 요청이
    // 무슨 status로 멈췄나"를 알 수 있다. requestId → (url, 받은 status). loadingFinished면
    // 제거(정상 완료)하므로, 타임아웃 시 남아있는 항목이 곧 '응답을 못 받고 멈춘 요청'이다.
    net_inflight: std::collections::HashMap<String, (String, Option<u16>)>,
    // 최근 끝난 요청 중 *실패/4xx·5xx* 만 요약해 모은다(정상 2xx는 노이즈라 제외, 상한 있음).
    net_recent: Vec<String>,
    // 지금까지 관측한 `Page.loadEventFired`(브라우저의 window.load 이벤트) 횟수. navigate/reload 가
    // "직전 페이지의 stale readyState=complete" 를 오판하지 않도록, 폴링 대신 이 이벤트 카운터로
    // "새 문서 로드 완료" 를 판정한다(조회수 부스트 view_boost 등에서 사용). Page.enable 필요.
    page_loads: u64,
}

impl CdpClient {
    // 이미 실행 중인 Chrome DevTools 탭에 WebSocket으로 연결하는 함수입니다.
    pub(crate) fn connect_to_existing_chrome(host: &str, port: u16) -> AutomationResult<Self> {
        let socket = Self::establish_socket(host, port)?;
        Ok(Self {
            socket,
            next_id: 0,
            host: host.to_owned(),
            port,
            net_inflight: std::collections::HashMap::new(),
            net_recent: Vec::new(),
            page_loads: 0,
        })
    }

    // Chrome 디버그 포트로 WebSocket을 새로 맺는다(연결·재접속 공용). 대상 탭을 고르고 TCP·핸드셰이크
    // 타임아웃을 건다. tungstenite `connect()`는 TCP 연결·핸드셰이크에 타임아웃이 없어, TCP는
    // connect_timeout으로, 이후 입출력은 read/write 타임아웃으로 묶어 무한 대기를 막는다(#210).
    fn establish_socket(
        host: &str,
        port: u16,
    ) -> AutomationResult<WebSocket<MaybeTlsStream<TcpStream>>> {
        let target = select_or_create_target(host, port).map_err(|error| {
            AutomationError::new(format!("Chrome DevTools 대상 탭 선택 실패: {error}"))
        })?;
        let url = websocket_url_for_host(&target.web_socket_debugger_url, host, port)?;

        let addr = (host, port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .ok_or_else(|| {
                AutomationError::new(format!(
                    "Chrome DevTools 주소를 해석하지 못했습니다: {host}:{port}"
                ))
            })?;
        let stream = TcpStream::connect_timeout(&addr, WS_CONNECT_TIMEOUT).map_err(|error| {
            AutomationError::new(format!(
                "Chrome DevTools TCP 연결 실패({host}:{port}): {error}"
            ))
        })?;
        stream
            .set_read_timeout(Some(DEFAULT_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(DEFAULT_TIMEOUT)))
            .map_err(|error| {
                AutomationError::new(format!("Chrome DevTools 소켓 타임아웃 설정 실패: {error}"))
            })?;

        let (socket, _) = tungstenite::client(url.as_str(), MaybeTlsStream::Plain(stream))
            .map_err(|error| {
                AutomationError::new(format!(
                    "Chrome DevTools WebSocket 연결 실패({url}): {error:?}"
                ))
            })?;
        Ok(socket)
    }

    // 끊긴 소켓을 같은 Chrome 디버그 포트로 다시 맺어 교체한다. 성공하면 죽은 소켓을 새 소켓으로
    // 갈아끼우고, 실패하면(크롬이 정말 죽음) 기존 소켓을 그대로 두고 Err를 돌려준다 — 호출부가
    // 재시도 상한을 넘기면 그때 최종 실패시킨다. Page/Runtime enable 은 다시 켜지 않는다:
    // 로그인 폴링이 쓰는 Network.getCookies·Runtime.evaluate 는 enable 없이 동작하는 명령이고,
    // 재접속 시 enable 을 다시 부르면 같은 끊김에 또 막힐 수 있어 최소 동작만 한다.
    fn reconnect(&mut self) -> AutomationResult<()> {
        self.socket = Self::establish_socket(&self.host, self.port)?;
        Ok(())
    }

    // 로그인 전용 CDP 셋업: Page 도메인만 켜고 **Runtime.enable 은 호출하지 않는다.**
    //
    // Runtime.enable 은 콘솔 인자 직렬화 경로를 활성화해, 페이지가 Error 객체의 `stack`
    // 게터나 `Symbol.toPrimitive`/`toString` 트랩으로 "이 브라우저는 CDP(DevTools)로 제어
    // 중"임을 탐지하게 만든다(네이버 봇탐지 번들 wtm.pstatic.net 에 `.stack`·
    // `Symbol.toPrimitive`·`console` 시그니처가 실재한다). 이 CDP 탐지는 navigator.webdriver·
    // 타이핑·마우스와 무관하게 곧장 봇으로 판정해 보안문자를 띄우는 가장 강한 신호다.
    //
    // 로그인 시퀀스는 Runtime.evaluate·Network.getCookies·Input.dispatchKeyEvent/MouseEvent·
    // Page.navigate·Page.addScriptToEvaluateOnNewDocument 만 쓰며, 이들은 Runtime.enable
    // 없이도 동작한다. 따라서 탐지 표면을 줄이려 로그인에선 Runtime 도메인을 켜지 않는다.
    pub(crate) fn enable_page_only(&mut self) -> AutomationResult<()> {
        self.call("Page.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Page.enable 실패: {error}")))?;
        // 진단 옵트인(기본 OFF): PSTMACRO_LOGIN_NETLOG 가 켜졌을 때만 Network 도메인을 활성화해,
        // 로그인 브라우저가 네이버와 실제로 주고받는 요청/응답/쿠키를 CDP 이벤트로 받아 기존 cdp
        // 트레이스(← {text})에 원문 그대로 남긴다(로그인 통신을 게시처럼 원문으로 보기 위함). 평소엔
        // 끈다 — Network.enable 은 위 주석대로 봇탐지 표면을 늘려 캡차율을 올릴 수 있다. 실패는 비치명적.
        if login_netlog_enabled() {
            match self.call("Network.enable", json!({})) {
                Ok(_) => tracing::warn!(
                    "[LOGIN][netlog] PSTMACRO_LOGIN_NETLOG=on — Network 도메인 활성(요청/응답 원문 로깅). \
                     ⚠️ 봇탐지 표면 증가로 캡차율이 오를 수 있음(진단 전용). 리스크 0 캡처는 tshark 사용."
                ),
                Err(error) => {
                    tracing::warn!("[LOGIN][netlog] Network.enable 실패 — 네트워크 원문 없이 계속: {error}")
                }
            }
        }
        Ok(())
    }

    // CDP 메서드를 호출하되, 소켓이 호스트 소프트웨어/원격끊김 등으로 중단(10053/10054 등)되면
    // 같은 Chrome 디버그 포트로 재접속해 재시도한다(사용자 지시 2026-06-30). 끊긴 그 순간 네이버
    // 쪽엔 이미 로그인(쿠키 발급)이 됐을 수 있어, 재접속 후 다시 읽으면 성공으로 건질 수 있다.
    //
    // - 정상(끊김 없음) 경로엔 영향 0 — 재접속은 "연결 중단" 에러일 때만 발동한다.
    // - 재시도 가능한 건 **멱등 명령만**이다. `Input.*`(키/마우스 입력)은 재전송하면 중복 입력될 수
    //   있어 제외한다(끊긴 시점 이미 전달됐을 수 있음). 폴링이 쓰는 Network.getCookies·
    //   Runtime.evaluate·Page.navigate 등은 재전송해도 안전(읽기/이동은 멱등)하다.
    pub(crate) fn call(&mut self, method: &str, params: Value) -> AutomationResult<Value> {
        const MAX_RECONNECT: u32 = 2;
        let retryable = !method.starts_with("Input.");
        let mut attempt = 0u32;
        loop {
            match self.call_once(method, params.clone()) {
                Ok(value) => return Ok(value),
                Err(error)
                    if retryable
                        && attempt < MAX_RECONNECT
                        && is_connection_lost_message(error.message()) =>
                {
                    attempt += 1;
                    tracing::info!(
                        "[CDP] 연결 중단 감지({method}) — 재접속 후 재시도 {attempt}/{MAX_RECONNECT}: {}",
                        error.message()
                    );
                    sleep(Duration::from_millis(500));
                    // 재접속 실패(크롬이 정말 죽음)면 죽은 소켓이 남아 다음 call_once가 또 연결중단으로
                    // 떨어지고, 상한을 넘기면 최종 실패한다. best-effort.
                    let _ = self.reconnect();
                }
                Err(error) => return Err(error),
            }
        }
    }

    // Chrome DevTools Protocol 메서드를 호출하고 응답을 기다리는 함수입니다(재접속 없는 1회 호출).
    fn call_once(&mut self, method: &str, params: Value) -> AutomationResult<Value> {
        self.next_id += 1;
        let id = self.next_id;
        // CDP 와이어 트레이스(켜졌을 때만): 보내는 명령을 method+파라미터 원문으로 남긴다.
        // payload 로 params 가 이동(move)하기 전에 참조해 남긴다.
        if cdp_trace_enabled() {
            tracing::info!(target: "cdp", "→ #{id} {method} {}", redact_cdp_params(method, &params));
        }
        let payload = json!({
            "id": id,
            "method": method,
            "params": params,
        });

        self.send_message(Message::Text(payload.to_string()))?;

        loop {
            let message = self.read_message()?;
            let text = match message {
                Message::Text(text) => text,
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                Message::Ping(bytes) => {
                    self.send_message(Message::Pong(bytes))?;
                    continue;
                }
                Message::Pong(_) => continue,
                Message::Close(_) => {
                    return Err(AutomationError::new("Chrome DevTools 연결이 닫혔습니다."));
                }
                Message::Frame(_) => continue,
            };

            // CDP 와이어 트레이스(켜졌을 때만): 들어오는 응답·이벤트를 원문 그대로 남긴다.
            if cdp_trace_enabled() {
                tracing::info!(target: "cdp", "← {text}");
            }
            let value: Value = serde_json::from_str(&text)?;

            if value.get("id").and_then(Value::as_u64) != Some(id) {
                // 우리 명령 응답이 아니면(브라우저가 보낸 method 이벤트) 네트워크 진단용으로 수집하고
                // 계속 읽는다 — 페이지 로드 멈춤의 진짜 원인(어떤 요청이 멈췄나)을 잡기 위함.
                if let Some(method) = value.get("method").and_then(Value::as_str) {
                    if method == "Page.loadEventFired" {
                        // 새 페이지의 window.load 가 실제로 발생 — 카운터로 남겨, 대기 로직이
                        // stale readyState 오판 없이 "진짜 로드 완료"를 판정하게 한다.
                        self.page_loads = self.page_loads.wrapping_add(1);
                    }
                    self.record_network_event(method, &value);
                }
                continue;
            }

            if let Some(error) = value.get("error") {
                return Err(AutomationError::new(format!(
                    "CDP 호출 실패({method}): {error}"
                )));
            }

            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    // 브라우저(크롬)가 페이지 로드 중 던진 네트워크 요청/응답/실패 이벤트를 추적한다(대기초과 진단).
    // 우리 Rust 패킷이 아니라 *브라우저 내부* 요청이라 이 이벤트로만 보인다. Network 도메인이 켜져
    // 있을 때만 흐른다(게시/댓글 경로의 enable()에서 켠다).
    fn record_network_event(&mut self, method: &str, value: &Value) {
        let params = value.get("params");
        let request_id = params
            .and_then(|p| p.get("requestId"))
            .and_then(Value::as_str);
        let Some(request_id) = request_id else {
            return;
        };
        match method {
            "Network.requestWillBeSent" => {
                if let Some(url) = params
                    .and_then(|p| p.pointer("/request/url"))
                    .and_then(Value::as_str)
                {
                    // 한 페이지에 요청이 폭주해도 메모리를 묶어둔다(상한 초과분은 추적하지 않음).
                    if self.net_inflight.len() < 500 {
                        self.net_inflight
                            .insert(request_id.to_owned(), (url.to_owned(), None));
                    }
                }
            }
            "Network.responseReceived" => {
                let status = params
                    .and_then(|p| p.pointer("/response/status"))
                    .and_then(Value::as_u64)
                    .map(|s| s as u16);
                if let Some(status) = status {
                    if status >= 400 {
                        let url = self
                            .net_inflight
                            .get(request_id)
                            .map(|(u, _)| u.clone())
                            .unwrap_or_default();
                        self.push_net_recent(format!("HTTP {status} ← {url}"));
                    }
                }
                if let Some(entry) = self.net_inflight.get_mut(request_id) {
                    entry.1 = status;
                }
            }
            // 정상적으로 로드가 끝난 요청은 추적에서 뺀다 — 타임아웃 때 남은 것만 '멈춘 요청'이다.
            "Network.loadingFinished" => {
                self.net_inflight.remove(request_id);
            }
            "Network.loadingFailed" => {
                let url = self
                    .net_inflight
                    .remove(request_id)
                    .map(|(u, _)| u)
                    .unwrap_or_default();
                let canceled = params
                    .and_then(|p| p.get("canceled"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                // 의도된 취소(네비게이션으로 중단 등)는 잡음이라 제외하고, 진짜 실패만 모은다.
                if !canceled {
                    let err = params
                        .and_then(|p| p.get("errorText"))
                        .and_then(Value::as_str)
                        .unwrap_or("(원인 불명)");
                    self.push_net_recent(format!("로드 실패({err}) ← {url}"));
                }
            }
            _ => {}
        }
    }

    // 최근 실패/4xx·5xx 요약을 상한 내에서 보관한다(오래된 것부터 버린다).
    fn push_net_recent(&mut self, line: String) {
        const MAX_RECENT: usize = 30;
        if self.net_recent.len() >= MAX_RECENT {
            self.net_recent.remove(0);
        }
        self.net_recent.push(line);
    }

    // 새 페이지로 이동할 때 직전 페이지의 네트워크 추적을 비운다(이번 navigation 기준으로만 진단).
    fn reset_network_trace(&mut self) {
        self.net_inflight.clear();
        self.net_recent.clear();
    }

    // 대기초과 시점에 '아직 응답을 못 받았거나 완료 안 된' 브라우저 요청 목록(진단 로그용).
    fn pending_network_requests(&self) -> Vec<String> {
        self.net_inflight
            .values()
            .take(20)
            .map(|(url, status)| match status {
                Some(s) => format!("[status {s} 받았으나 미완료] {url}"),
                None => format!("[응답 대기중(status 없음)] {url}"),
            })
            .collect()
    }

    // Chrome DevTools WebSocket으로 메시지를 보내는 함수입니다.
    fn send_message(&mut self, message: Message) -> AutomationResult<()> {
        let end = Instant::now() + Duration::from_secs(20);

        loop {
            match self.socket.send(message.clone()) {
                Ok(()) => return Ok(()),
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= end {
                        return Err(AutomationError::new(
                            "Chrome DevTools WebSocket 쓰기 시간이 초과되었습니다.",
                        ));
                    }

                    sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    // Chrome DevTools WebSocket에서 메시지를 읽는 함수입니다.
    fn read_message(&mut self) -> AutomationResult<Message> {
        let end = Instant::now() + Duration::from_secs(20);

        loop {
            match self.socket.read() {
                Ok(message) => return Ok(message),
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= end {
                        return Err(AutomationError::new(
                            "Chrome DevTools WebSocket 읽기 시간이 초과되었습니다.",
                        ));
                    }

                    sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    // 연결된 Chrome 탭 안에서 JavaScript 표현식을 실행하는 함수입니다.
    pub(crate) fn evaluate(&mut self, expression: &str) -> AutomationResult<Value> {
        let result = self.call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": true,
                "userGesture": true,
            }),
        )?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Err(AutomationError::new(format!(
                "브라우저 스크립트 실행 오류: {exception}"
            )));
        }

        Ok(result
            .get("result")
            .and_then(|remote| remote.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    // JavaScript 실행 결과를 bool 값으로 읽는 함수입니다.
    pub(crate) fn evaluate_bool(&mut self, expression: &str) -> AutomationResult<bool> {
        Ok(self.evaluate(expression)?.as_bool().unwrap_or(false))
    }

    // JavaScript 실행 결과를 문자열로 읽는 함수입니다.
    pub(crate) fn evaluate_string(&mut self, expression: &str) -> AutomationResult<String> {
        Ok(self
            .evaluate(expression)?
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_default())
    }

    /// `target_expr`(예: `document.querySelector('#pw')`)가 가리키는 객체에 `event_type` 리스너가
    /// **실제로 붙어 있는지** CDP로 직접 관측한다(best-effort). 봇탐지 keydown 암호화 후킹이
    /// "파일 다운로드"를 넘어 정말 설치됐는지 확인하는 데 쓴다(로그인 폼 게이트 강화).
    ///
    /// `DOMDebugger.getEventListeners`는 RemoteObject `objectId`가 필요하므로, 먼저
    /// `Runtime.evaluate`(returnByValue=false)로 노드 핸들을 얻은 뒤 조회한다. 어떤 단계든 실패하면
    /// `false`를 돌려준다 — 호출부가 안전 폴백으로 진행하므로 `false`가 로그인을 막지 않는다.
    pub(crate) fn expr_has_listener(&mut self, target_expr: &str, event_type: &str) -> bool {
        let object_id = match self.call(
            "Runtime.evaluate",
            json!({ "expression": target_expr, "returnByValue": false }),
        ) {
            Ok(v) => v
                .get("result")
                .and_then(|r| r.get("objectId"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            Err(_) => None,
        };
        let Some(object_id) = object_id else {
            return false;
        };
        let listeners = match self.call(
            "DOMDebugger.getEventListeners",
            json!({ "objectId": object_id }),
        ) {
            Ok(v) => v,
            Err(_) => return false,
        };
        listeners
            .get("listeners")
            .and_then(Value::as_array)
            .is_some_and(|arr| {
                arr.iter()
                    .any(|li| li.get("type").and_then(Value::as_str) == Some(event_type))
            })
    }

    // 현재 Chrome 탭의 URL을 읽는 함수입니다.
    pub(crate) fn current_url(&mut self) -> AutomationResult<String> {
        self.evaluate_string("location.href")
    }

    // Chrome 탭을 지정한 URL로 이동시키는 함수입니다.
    pub(crate) fn navigate(&mut self, url: &str) -> AutomationResult<()> {
        // 어느 페이지로 이동하는지 로그에 남긴다 — "대기초과"가 났을 때 *어떤 페이지가* 안 열렸는지
        // 바로 짚을 수 있게(사수 지적: 무슨 호출에서 무슨 문제인지 로그에 보여야 함).
        tracing::info!(url = %url, "브라우저 페이지 이동(navigate) 시작");
        // 직전 페이지의 네트워크 추적을 비워, 대기초과 진단이 이번 페이지 요청만 반영하게 한다.
        self.reset_network_trace();
        self.call("Page.navigate", json!({ "url": url }))?;
        self.wait_for_ready_state(DEFAULT_TIMEOUT)
    }

    // 페이지가 interactive 또는 complete 상태가 될 때까지 기다리는 함수입니다.
    pub(crate) fn wait_for_ready_state(&mut self, timeout: Duration) -> AutomationResult<()> {
        let start = Instant::now();
        let end = start + timeout;
        let mut last_state = String::new();

        while Instant::now() < end {
            let state = self.evaluate_string("document.readyState")?;

            if state == "interactive" || state == "complete" {
                return Ok(());
            }
            last_state = state;
            sleep(Duration::from_millis(250));
        }

        // 페이지 로드가 상한까지 차서 타임아웃 — 이건 *API 호출 실패가 아니라* 브라우저 페이지가
        // 끝까지 로딩 상태에서 못 벗어난 것이다. 어느 URL이 멈췄는지(best-effort)까지 남겨, 다음에
        // 어떤 페이지가 문제인지 로그만 보고 알 수 있게 한다(사수 지적). 글쓰기 add 같은 API는 이
        // 단계를 못 넘으면 *애초에 호출되지 않는다* — 그래서 실패 로그에 API가 안 보이는 것이다.
        let stuck_url = self
            .evaluate_string("location.href")
            .unwrap_or_else(|_| "(URL 확인 실패)".to_owned());
        // 브라우저가 이 페이지를 로드하다 멈춘 *진짜 원인*: 응답을 못 받고 멈춰있는 요청들과 최근
        // 실패/4xx·5xx 응답을 함께 남긴다(CDP Network 이벤트 기반). 이게 "무슨 요청이 무슨 status로
        // 멈췄나"의 답이다 — 우리 Rust API 호출이 아니라 브라우저 내부 요청이라 이 경로로만 보인다.
        let pending = self.pending_network_requests();
        tracing::warn!(
            waited_secs = start.elapsed().as_secs(),
            last_ready_state = %last_state,
            url = %stuck_url,
            pending_count = self.net_inflight.len(),
            pending_requests = ?pending,
            recent_failures = ?self.net_recent,
            "페이지 로드 대기 시간 초과 — 멈춘/실패한 브라우저 요청 포함(우리 API 호출이 아니라 브라우저 페이지 로딩)"
        );
        Err(AutomationError::new(format!(
            "페이지 로드 대기 시간이 초과되었습니다. (멈춘 페이지: {stuck_url})"
        )))
    }

    // 지금까지 관측한 `Page.loadEventFired`(브라우저 window.load) 횟수. navigate/reload 직전에
    // 스냅샷을 찍고 `wait_for_new_load`에 넘겨, 그 이후에 발생한 *새* load 이벤트만 기다린다.
    pub(crate) fn page_load_count(&self) -> u64 {
        self.page_loads
    }

    // `since`(대기 시작 전 `page_load_count` 스냅샷) 이후에 **새 `Page.loadEventFired` 이벤트가 한 번
    // 이상** 발생할 때까지 기다린다. `readyState` 폴링과 달리 직전 페이지의 stale "complete"에 속지
    // 않는다 — navigate/reload 가 실제로 새 문서를 다 로드했을 때만 통과한다(조회수 부스트에서 창을
    // 너무 일찍 닫는 문제를 막는다).
    //
    // CDP 이벤트는 `call` 응답을 읽는 도중에만 소켓에서 흘러오므로, 가벼운 멱등 호출
    // (`Runtime.evaluate "0"`)로 메시지 펌프를 돌려 그 사이 큐된 load 이벤트를 읽어들인다(그 read
    // 루프에서 `page_loads` 가 증가한다). 이동 중 execution context 파괴로 evaluate 가 실패해도
    // 무해 — 어차피 카운터만 보고 판단하며 재시도한다.
    pub(crate) fn wait_for_new_load(
        &mut self,
        since: u64,
        timeout: Duration,
    ) -> AutomationResult<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.page_loads > since {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(AutomationError::new(
                    "페이지 load 이벤트(Page.loadEventFired) 대기 시간이 초과되었습니다.",
                ));
            }
            // 메시지 펌프: 가벼운 호출의 read 루프가 큐된 load 이벤트를 처리한다. 실패는 무시.
            let _ = self.evaluate("0");
            sleep(Duration::from_millis(150));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clarify_profile_status_error_differs_by_npay_status() {
        use packet_client::NpayJoinStatus;
        // 서로 다른 원인이 똑같이 "프로필 상태 500"으로만 보이던 걸(사용자 지적) npay 판정별로 가른다.
        let profile_500 =
            || Err(AutomationError::new("프로필 상태 패킷 HTTP 실패: status=500, body={\"message\":\"Failed to fetch profile user status\"}"));

        // LoginRequired(nid 로그인 튕김) → 보호조치/재로그인 차단성 메시지(is_blocking_failure의 "보호조치").
        let e = clarify_profile_status_error(profile_500(), NpayJoinStatus::LoginRequired)
            .expect_err("에러여야");
        assert!(
            e.message().contains("보호조치") && e.message().contains("재로그인"),
            "보호조치/재로그인 안내여야: {}",
            e.message()
        );

        // TermsPending(commonTermAgree) → npay 미완료 안내. 차단 마커("다시 로그인")는 피한다.
        let e = clarify_profile_status_error(profile_500(), NpayJoinStatus::TermsPending)
            .expect_err("에러여야");
        assert!(
            e.message().contains("npay") && e.message().contains("미완료"),
            "npay 미완료 안내여야: {}",
            e.message()
        );
        assert!(
            !e.message().contains("다시 로그인") && !e.message().contains("보호조치"),
            "TermsPending은 차단으로 오분류되면 안 됨: {}",
            e.message()
        );

        // Completed/Unknown + 500 → 원본 유지(진짜 다른 프로필 문제).
        let e = clarify_profile_status_error(profile_500(), NpayJoinStatus::Completed)
            .expect_err("에러여야");
        assert!(!e.message().contains("보호조치"), "원본 유지: {}", e.message());

        // 500이 아닌 실패는 npay 판정과 무관하게 원본 그대로.
        let e = clarify_profile_status_error(
            Err(AutomationError::new("HTTP status 429 Too Many Requests")),
            NpayJoinStatus::LoginRequired,
        )
        .expect_err("에러여야");
        assert_eq!(e.message(), "HTTP status 429 Too Many Requests");

        // Ok는 절대 건드리지 않는다.
        assert!(clarify_profile_status_error(Ok(true), NpayJoinStatus::LoginRequired).is_ok());
    }

    #[test]
    fn connection_lost_detects_socket_abort_but_not_plain_timeout() {
        // 재접속 대상: 소켓 중단(10053/10054/10060)·연결 종료.
        assert!(is_connection_lost_message(
            "IO error: ... 호스트 시스템의 소프트웨어에 의해 중단되었습니다. (os error 10053)"
        ));
        assert!(is_connection_lost_message("connection reset (os error 10054)"));
        assert!(is_connection_lost_message(
            "응답이 없어 연결이 끊어졌습니다. (os error 10060)"
        ));
        assert!(is_connection_lost_message("Chrome DevTools 연결이 닫혔습니다."));
        // 재접속 비대상: 우리 읽기/쓰기 타임아웃(소켓은 살아있을 수 있음)·일반 CDP 오류.
        assert!(!is_connection_lost_message(
            "Chrome DevTools WebSocket 읽기 시간이 초과되었습니다."
        ));
        assert!(!is_connection_lost_message(
            "CDP 호출 실패(Runtime.evaluate): {\"code\":-32000}"
        ));
    }

    #[test]
    fn automation_error_keeps_message_and_records_caller_location() {
        // new() 호출 지점(이 줄)을 컴파일타임에 기록한다 — "자세히 보기" trace의 원천(#199).
        let err = AutomationError::new("게시 실패");
        assert_eq!(err.message(), "게시 실패");
        // 호출 지점 파일이 location에 들어가야 한다(<unknown> 없이 항상).
        assert!(
            err.location().starts_with("at "),
            "위치 형식: {}",
            err.location()
        );
        assert!(
            err.location().contains("naver_automation.rs"),
            "호출 파일이 들어가야 함: {}",
            err.location()
        );
        // Display는 사용자용 메시지만(위치 미포함).
        assert_eq!(format!("{err}"), "게시 실패");
        // trace()는 앵커(이 함수) + 캡처된 런타임 스택을 합친다 — "자세히 보기" 본문(#199).
        let trace = err.trace();
        assert!(
            trace.starts_with(err.location()),
            "trace는 앵커로 시작: {trace}"
        );
        assert!(
            trace.contains("automation_error_keeps_message_and_records_caller_location"),
            "캡처한 스택에 호출 함수가 보여야 함(심볼 해석됨): {trace}"
        );
    }
}
