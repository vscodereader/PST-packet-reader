mod browser_flow;
mod cookie_bridge;
mod devtools_connection;
mod packet_client;
mod post_form;
pub mod types;

pub use types::{
    AutomationReport, AutomationTarget, DiscussionSelection, DiscussionStock,
    NaverDiscussionRequest, NaverLoginProfile, NaverPostWithCommentRequest,
};

use devtools_connection::{normalize_debug_host, select_or_create_target, websocket_url_for_host};
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
/// 게시(글쓰기/댓글) 경로의 페이지 로드(`wait_for_ready_state`) 대기 상한. WSL2 자원 경쟁 등으로
/// 일시적으로 로드가 느려질 때 "대기초과"로 빠지는 빈도를 줄이려 `DEFAULT_TIMEOUT`(20초)보다
/// 넉넉히 잡는다. 로그인 경로(auth)는 이 값을 쓰지 않으므로 영향이 없다(#대기초과 후속).
pub(crate) const POST_READY_TIMEOUT: Duration = Duration::from_secs(45);
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

// 네이버 로그인 확인부터 토론방 선택, 글쓰기/댓글 등록까지 전체 흐름을 실행하는 함수입니다.
// 글/글+댓글 매크로가 공유하는 진입 셋업 결과(Chrome 연결·패킷 클라이언트·로그인·선택 종목).
struct ForumDiscussionSession {
    chrome: CdpClient,
    packet_client: packet_client::NaverPacketClient,
    login_profile: NaverLoginProfile,
    selected: DiscussionSelection,
    // 종목토론방 URL(선택 종목은 코드로 생성, 랜덤은 패킷 API). 브라우저를 이 URL로 *이동시키지
    // 않고*, submit_post/submit_comment의 referer·target 파싱용 문자열로만 쓴다(페이지 이동/로드
    // 제거 — 사수 지시 2026-06-30).
    room_url: String,
}

// 글/글+댓글 매크로 공통 셋업: Chrome 연결 → 쿠키 주입 → 토론 페이지 → 패킷 클라이언트 →
// 로그인 확인 → 토론방 진입까지 한 번에 수행한다. 두 경로가 동일하게 중복하던 블록을
// 단일 함수로 합쳐 분기 누락·드리프트를 막는다(동작 변경 없음).
fn open_discussion_session(
    host: &str,
    port: u16,
    account_id: Option<&str>,
    stock: Option<&DiscussionStock>,
) -> AutomationResult<ForumDiscussionSession> {
    let host = normalize_debug_host(host);
    let mut chrome = CdpClient::connect_to_existing_chrome(&host, port)?;
    chrome.enable()?;
    // 계정 쿠키를 Chrome에 주입한다. 바로 다음 build_naver_packet_client가 Chrome에서 쿠키를 뽑아
    // HTTP 패킷 클라이언트를 만든다 — *여기까지만* Chrome이 필요하다. 글쓰기/댓글은 전부 HTTP 패킷
    // API로 처리하므로, 종목토론방 페이지로의 이동/로드는 하지 않는다(사수 지시 2026-06-30): 페이지
    // 렌더링이 게시에 불필요하고, 그 페이지 로드 대기가 가짜 "대기초과"의 원인이었다.
    if let Some(account_id) = account_id {
        chrome.inject_account_cookies(account_id)?;
    }

    let packet_client = chrome.build_naver_packet_client()?;
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
    packet_client.ensure_npay_financial_join();

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
        chrome,
        packet_client,
        login_profile,
        selected,
        room_url,
    })
}

/// 저장된 로그인 쿠키만으로 종목토론방 게시글에 **좋아요**를 누른다(Chrome·페이지 이동 없이
/// reactions API 전용 — 사수 지시). `post_url`은 특정 게시글 링크(예:
/// `https://stock.naver.com/domestic/stock/005930/discussion/424274129`)이며, 계정별로 호출한다.
/// 이미 좋아요면 성공으로 본다(멱등). 쿠키 없음/세션 만료 등은 `AutomationError`로 올라간다.
pub fn run_naver_like(account_id: &str, post_url: &str) -> AutomationResult<()> {
    let post_url = post_url.trim();
    if post_url.is_empty() {
        return Err(AutomationError::new("좋아요를 누를 게시글 링크가 비어 있습니다."));
    }
    tracing::info!(
        account = %crate::auth::mask_id(account_id),
        "종토방 좋아요 시작(API 전용, 페이지 이동 없음)"
    );
    let storage = crate::auth::read_account_cookies(account_id)
        .map_err(|error| {
            AutomationError::new(format!("계정 '{account_id}' 쿠키 조회 실패: {error}"))
        })?
        .ok_or_else(|| {
            AutomationError::new(format!(
                "계정 '{account_id}'의 저장된 로그인 쿠키가 없습니다. 먼저 로그인하세요."
            ))
        })?;
    let client = packet_client::NaverPacketClient::from_storage_state(&storage)?;
    client.like_post(post_url)
}

pub fn run_naver_discussion_macro(
    request: NaverDiscussionRequest,
) -> AutomationResult<AutomationReport> {
    let title = request.title.trim();
    let body = request.body.trim();

    if matches!(request.target, AutomationTarget::Post) && title.is_empty() {
        return Err(AutomationError::new("제목이 비어 있습니다."));
    }

    if body.is_empty() {
        return Err(AutomationError::new("내용이 비어 있습니다."));
    }

    // 매크로 시작~세션 오픈(크롬 연결·쿠키 추출·로그인 확인)까지의 실제 소요시간을 로그로 남긴다.
    let macro_started = Instant::now();
    tracing::info!(target_kind = ?request.target, "게시 매크로 시작 — 세션 오픈 진입");
    let ForumDiscussionSession {
        mut chrome,
        packet_client,
        login_profile,
        selected,
        room_url,
    } = open_discussion_session(
        &request.host,
        request.port,
        request.account_id.as_deref(),
        request.stock.as_ref(),
    )?;
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
            packet_client.ensure_profile_intro_setup(&room_url)?;
            if request.submit_after_fill {
                // 글쓰기 add 패킷 API로 게시하고, 응답 id로 작성 글 URL을 만든다(브라우저 이동 없음).
                let post_id = packet_client.submit_post(&room_url, title, body)?;
                posted_url = Some(packet_client.post_url_from_id(&room_url, &post_id)?);
                (false, true)
            } else {
                // 수동 확인 모드(CLI)만 브라우저 폼이 필요하므로 이 경로에서만 페이지를 연다.
                chrome.navigate(&room_url)?;
                chrome.open_write_modal()?;
                chrome.fill_post_form(title, body)?;
                (chrome.highlight_manual_submit_target()?, false)
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
            packet_client.ensure_profile_intro_setup(&comment_target_url)?;
            if request.submit_after_fill {
                packet_client.submit_comment(&comment_target_url, body)?;
                (false, true)
            } else {
                chrome.navigate(&comment_target_url)?;
                chrome.fill_comment_form(body)?;
                (chrome.highlight_manual_submit_target()?, false)
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
        // Chrome은 쿠키 추출·로그인 확인까지만 쓰였다. 이후 게시는 전부 패킷 API라 더는 쓰지
        // 않지만, 세션이 끝날 때까지 살려둔다(Drop 시 소켓 정리).
        chrome: _chrome,
        packet_client,
        login_profile,
        selected,
        room_url,
    } = open_discussion_session(
        &request.host,
        request.port,
        request.account_id.as_deref(),
        request.stock.as_ref(),
    )?;

    // 글쓰기 전에 종목토론방 프로필(닉네임+소개 2222)을 보장한다(없으면 글쓰기 form 404). 멱등.
    packet_client.ensure_profile_intro_setup(&room_url)?;

    // 글쓰기 add 패킷 API로 게시하고, 응답 id로 작성 글 URL을 만든다(페이지 이동 없음).
    let post_id = packet_client.submit_post(&room_url, title, body)?;
    let post_url = packet_client.post_url_from_id(&room_url, &post_id)?;
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
    packet_client.ensure_profile_intro_setup(&post_url)?;
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

    // Chrome DevTools의 Runtime/Page 도메인을 활성화하는 함수입니다.
    pub(crate) fn enable(&mut self) -> AutomationResult<()> {
        self.call("Runtime.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Runtime.enable 실패: {error}")))?;
        self.call("Page.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Page.enable 실패: {error}")))?;
        // Network 도메인을 켜서 브라우저가 던지는 요청/응답/실패 이벤트를 받는다 — 페이지 로드
        // 대기초과 시 "어떤 브라우저 요청이 무슨 status로 멈췄나"를 로그에 남기기 위함. 게시/댓글
        // 경로 전용(로그인은 enable_page_only로 Network·Runtime 미활성 — 봇탐지 표면 유지). 실패는
        // 비치명적으로 둔다 — 이벤트 진단이 안 될 뿐 게시 흐름 자체는 그대로 동작해야 한다.
        if let Err(error) = self.call("Network.enable", json!({})) {
            tracing::warn!("Network.enable 실패 — 네트워크 진단 이벤트 없이 계속: {error}");
        }
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
    // (다운스트림 글쓰기/댓글의 `enable()`은 이벤트가 필요할 수 있어 그대로 둔다.)
    pub(crate) fn enable_page_only(&mut self) -> AutomationResult<()> {
        self.call("Page.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Page.enable 실패: {error}")))?;
        Ok(())
    }

    // 로그인 자동화가 저장한 계정 쿠키를 Chrome 세션에 주입하는 함수입니다.
    // 이렇게 하면 사용자가 수동 로그인하지 않아도 Chrome이 로그인된 상태가 되고,
    // 이후 기존 글쓰기/댓글 흐름이 그대로 동작합니다.
    fn inject_account_cookies(&mut self, account_id: &str) -> AutomationResult<()> {
        let saved = crate::auth::read_account_cookies(account_id)
            .map_err(|error| {
                AutomationError::new(format!("계정 쿠키 파일을 읽지 못했습니다: {error}"))
            })?
            .ok_or_else(|| {
                AutomationError::new(format!(
                    "계정 '{account_id}'의 유효한 로그인 쿠키가 없습니다. 먼저 로그인 자동화를 실행해 쿠키를 저장하세요."
                ))
            })?;

        let params = cookie_bridge::cdp_params_from_saved_cookies(&saved);

        if params.is_empty() {
            return Err(AutomationError::new(format!(
                "계정 '{account_id}'의 쿠키 파일에서 주입할 쿠키를 찾지 못했습니다."
            )));
        }

        self.call("Network.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Network.enable 실패: {error}")))?;

        for param in params {
            self.call("Network.setCookie", param)
                .map_err(|error| AutomationError::new(format!("쿠키 주입 실패: {error}")))?;
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

            let value: Value = serde_json::from_str(&text)?;

            if value.get("id").and_then(Value::as_u64) != Some(id) {
                // 우리 명령 응답이 아니면(브라우저가 보낸 method 이벤트) 네트워크 진단용으로 수집하고
                // 계속 읽는다 — 페이지 로드 멈춤의 진짜 원인(어떤 요청이 멈췄나)을 잡기 위함.
                if let Some(method) = value.get("method").and_then(Value::as_str) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
