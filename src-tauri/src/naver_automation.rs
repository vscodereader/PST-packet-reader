mod browser_flow;
mod cookie_bridge;
mod devtools_connection;
mod discussion_room;
pub(crate) mod packet_client;
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
use std::net::TcpStream;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tauri::{Emitter, Runtime};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

const DISCUSSION_URL: &str = "https://stock.naver.com/discussion";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

type AutomationResult<T> = Result<T, AutomationError>;

#[derive(Debug)]
pub struct AutomationError {
    message: String,
}

impl AutomationError {
    // 자동화 중 발생한 오류 메시지를 생성하는 함수입니다.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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

    let host = normalize_debug_host(&request.host);
    let mut chrome = CdpClient::connect_to_existing_chrome(&host, request.port)?;
    chrome.enable()?;
    // 계정 ID가 지정되면 로그인 자동화가 저장한 쿠키를 Chrome에 주입합니다.
    if let Some(account_id) = request.account_id.as_deref() {
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

    let selected = match request.stock.as_ref() {
        Some(stock) => chrome.open_selected_discussion_room(stock)?,
        None => chrome.open_random_discussion_room(&packet_client)?,
    };

    let (register_button_highlighted, submitted) = match request.target {
        AutomationTarget::Post => {
            if request.submit_after_fill {
                chrome.submit_post_and_refresh(&packet_client, title, body)?;
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

    let host = normalize_debug_host(&request.host);
    let mut chrome = CdpClient::connect_to_existing_chrome(&host, request.port)?;
    chrome.enable()?;
    // 계정 ID가 지정되면 로그인 자동화가 저장한 쿠키를 Chrome에 주입합니다.
    if let Some(account_id) = request.account_id.as_deref() {
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

    let selected = match request.stock.as_ref() {
        Some(stock) => chrome.open_selected_discussion_room(stock)?,
        None => chrome.open_random_discussion_room(&packet_client)?,
    };

    let post_url = chrome.submit_post_and_refresh(&packet_client, title, body)?;
    let post_report = AutomationReport {
        current_url: chrome.current_url()?,
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
        let (socket, _) = connect(url.as_str()).map_err(|error| {
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
