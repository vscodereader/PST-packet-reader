mod browser_flow;
mod cookie_bridge;
mod devtools_connection;
mod discussion_room;
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

// 네이버 로그인 확인부터 토론방 선택, 글쓰기/댓글 등록까지 전체 흐름을 실행하는 함수입니다.
// 글/글+댓글 매크로가 공유하는 진입 셋업 결과(Chrome 연결·패킷 클라이언트·로그인·선택 종목).
struct ForumDiscussionSession {
    chrome: CdpClient,
    packet_client: packet_client::NaverPacketClient,
    login_profile: NaverLoginProfile,
    selected: DiscussionSelection,
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
    // 계정 ID가 지정되면 로그인 자동화가 저장한 쿠키를 Chrome에 주입합니다.
    if let Some(account_id) = account_id {
        chrome.inject_account_cookies(account_id)?;
    }
    chrome.ensure_discussion_page()?;

    let packet_client = chrome.build_naver_packet_client()?;
    let login_profile = packet_client.read_login_profile()?;

    if !login_profile.logged_in {
        return Err(AutomationError::new(format!(
            "네이버 로그인이 확인되지 않았습니다. Chrome에서 로그인한 뒤 다시 실행하세요. ({})",
            login_profile.message
        )));
    }

    let selected = match stock {
        Some(stock) => chrome.open_selected_discussion_room(stock)?,
        None => chrome.open_random_discussion_room(&packet_client)?,
    };

    Ok(ForumDiscussionSession {
        chrome,
        packet_client,
        login_profile,
        selected,
    })
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

    let ForumDiscussionSession {
        mut chrome,
        packet_client,
        login_profile,
        selected,
    } = open_discussion_session(
        &request.host,
        request.port,
        request.account_id.as_deref(),
        request.stock.as_ref(),
    )?;

    let mut posted_url: Option<String> = None;
    let (register_button_highlighted, submitted) = match request.target {
        AutomationTarget::Post => {
            // 글쓰기 전에 종목토론방 프로필(닉네임+소개 2222)을 보장한다. 프로필이 없으면
            // 글쓰기 토큰 발급(discussion/form)이 404가 난다. 멱등이라 이미 있으면 즉시 통과.
            let room_url = chrome.current_url()?;
            packet_client.ensure_profile_intro_setup(&room_url)?;
            if request.submit_after_fill {
                // 작성된 글 URL(add 응답 id 기반)을 보존해 완료 로그에서 확인할 수 있게 한다.
                posted_url = Some(chrome.submit_post_and_refresh(&packet_client, title, body)?);
                (false, true)
            } else {
                chrome.open_write_modal()?;
                chrome.fill_post_form(title, body)?;
                (chrome.highlight_manual_submit_target()?, false)
            }
        }
        AutomationTarget::Comment => {
            let selected_url = chrome.current_url()?;
            packet_client.ensure_profile_intro_setup(&selected_url)?;
            chrome.open_random_discussion_post(&packet_client)?;
            if request.submit_after_fill {
                chrome.submit_comment_and_refresh(&packet_client, body)?;
                (false, true)
            } else {
                chrome.fill_comment_form(body)?;
                (chrome.highlight_manual_submit_target()?, false)
            }
        }
    };

    Ok(AutomationReport {
        current_url: chrome.current_url()?,
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
        mut chrome,
        packet_client,
        login_profile,
        selected,
    } = open_discussion_session(
        &request.host,
        request.port,
        request.account_id.as_deref(),
        request.stock.as_ref(),
    )?;

    // 글쓰기 전에 종목토론방 프로필(닉네임+소개 2222)을 보장한다(없으면 글쓰기 form 404).
    // 멱등이라 이미 있으면 즉시 통과. 글 등록 후 댓글 직전의 셋업 호출은 그대로 둔다.
    let room_url = chrome.current_url()?;
    packet_client.ensure_profile_intro_setup(&room_url)?;

    let post_url = chrome.submit_post_and_refresh(&packet_client, title, body)?;
    let post_report = AutomationReport {
        current_url: chrome.current_url()?,
        post_url: Some(post_url.clone()),
        login_profile: login_profile.clone(),
        register_button_highlighted: false,
        submitted: true,
        selected: selected.clone(),
        target: AutomationTarget::Post,
    };

    // 글 등록+새로고침 직후: 타이머를 시작하고 댓글 작성에 걸린 시간을 기록합니다.
    let post_done_at = Instant::now();
    if sleep_after {
        let _ = app.emit("batch-wait-start", serde_json::json!({ "seconds": 60u64 }));
    }

    chrome.navigate(&post_url)?;
    chrome.wait_for_ready_state(Duration::from_secs(30))?;
    sleep(Duration::from_secs(2));
    packet_client.ensure_profile_intro_setup(&post_url)?;
    chrome.submit_comment_and_refresh(&packet_client, comment)?;
    let comment_report = AutomationReport {
        current_url: chrome.current_url()?,
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
}

impl CdpClient {
    // 이미 실행 중인 Chrome DevTools 탭에 WebSocket으로 연결하는 함수입니다.
    pub(crate) fn connect_to_existing_chrome(host: &str, port: u16) -> AutomationResult<Self> {
        let target = select_or_create_target(host, port).map_err(|error| {
            AutomationError::new(format!("Chrome DevTools 대상 탭 선택 실패: {error}"))
        })?;
        let url = websocket_url_for_host(&target.web_socket_debugger_url, host, port)?;

        // tungstenite `connect()`는 TCP 연결·핸드셰이크에 타임아웃이 없다. DevTools는
        // 127.0.0.1의 평문 ws라, TCP는 connect_timeout으로, 핸드셰이크/이후 입출력은
        // read/write 타임아웃으로 묶어 무한 대기를 막는다(#210). read_message/send_message의
        // WouldBlock+데드라인 루프가 이 read/write 타임아웃과 맞물려 실제로 동작하게 된다.
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

        Ok(Self { socket, next_id: 0 })
    }

    // Chrome DevTools의 Runtime/Page 도메인을 활성화하는 함수입니다.
    pub(crate) fn enable(&mut self) -> AutomationResult<()> {
        self.call("Runtime.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Runtime.enable 실패: {error}")))?;
        self.call("Page.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Page.enable 실패: {error}")))?;
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

    // Chrome DevTools Protocol 메서드를 호출하고 응답을 기다리는 함수입니다.
    pub(crate) fn call(&mut self, method: &str, params: Value) -> AutomationResult<Value> {
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

    // 현재 Chrome 탭의 URL을 읽는 함수입니다.
    pub(crate) fn current_url(&mut self) -> AutomationResult<String> {
        self.evaluate_string("location.href")
    }

    // Chrome 탭을 지정한 URL로 이동시키는 함수입니다.
    pub(crate) fn navigate(&mut self, url: &str) -> AutomationResult<()> {
        self.call("Page.navigate", json!({ "url": url }))?;
        self.wait_for_ready_state(DEFAULT_TIMEOUT)
    }

    // 페이지가 interactive 또는 complete 상태가 될 때까지 기다리는 함수입니다.
    pub(crate) fn wait_for_ready_state(&mut self, timeout: Duration) -> AutomationResult<()> {
        let end = Instant::now() + timeout;

        while Instant::now() < end {
            let state = self.evaluate_string("document.readyState")?;

            if state == "interactive" || state == "complete" {
                return Ok(());
            }

            sleep(Duration::from_millis(250));
        }

        Err(AutomationError::new(
            "페이지 로드 대기 시간이 초과되었습니다.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
