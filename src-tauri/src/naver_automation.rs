mod browser_flow;
mod devtools_connection;
mod discussion_room;
mod packet_profile;
mod post_form;
pub mod types;

pub use types::{
    AutomationReport, AutomationTarget, DiscussionSelection, NaverDiscussionRequest,
    NaverLoginProfile,
};

use devtools_connection::{normalize_debug_host, select_or_create_target, websocket_url_for_host};
use serde_json::{json, Value};
use std::fmt::{Display, Formatter};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::thread::sleep;
use std::time::{Duration, Instant};
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
    chrome.ensure_discussion_page()?;

    let login_profile = chrome.read_login_profile_from_packet()?;

    if !login_profile.logged_in {
        return Err(AutomationError::new(format!(
            "네이버 로그인이 확인되지 않았습니다. Chrome에서 로그인한 뒤 다시 실행하세요. ({})",
            login_profile.message
        )));
    }

    let selected = chrome.open_random_discussion_room()?;

    let (register_button_highlighted, submitted) = match request.target {
        AutomationTarget::Post => {
            chrome.open_write_modal()?;
            if request.submit_after_fill {
                chrome.submit_post_and_refresh(title, body)?;
                (false, true)
            } else {
                chrome.fill_post_form(title, body)?;
                (chrome.highlight_manual_submit_target()?, false)
            }
        }
        AutomationTarget::Comment => {
            chrome.ensure_profile_setup_for_comment()?;
            chrome.open_random_discussion_post()?;
            if request.submit_after_fill {
                chrome.submit_comment_and_refresh(body)?;
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

struct CdpClient {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpClient {
    // 이미 실행 중인 Chrome DevTools 탭에 WebSocket으로 연결하는 함수입니다.
    fn connect_to_existing_chrome(host: &str, port: u16) -> AutomationResult<Self> {
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
    fn enable(&mut self) -> AutomationResult<()> {
        self.call("Runtime.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Runtime.enable 실패: {error}")))?;
        self.call("Page.enable", json!({}))
            .map_err(|error| AutomationError::new(format!("Page.enable 실패: {error}")))?;
        Ok(())
    }

    // Chrome DevTools Protocol 메서드를 호출하고 응답을 기다리는 함수입니다.
    fn call(&mut self, method: &str, params: Value) -> AutomationResult<Value> {
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
    fn evaluate(&mut self, expression: &str) -> AutomationResult<Value> {
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
    fn evaluate_bool(&mut self, expression: &str) -> AutomationResult<bool> {
        Ok(self.evaluate(expression)?.as_bool().unwrap_or(false))
    }

    // JavaScript 실행 결과를 문자열로 읽는 함수입니다.
    fn evaluate_string(&mut self, expression: &str) -> AutomationResult<String> {
        Ok(self
            .evaluate(expression)?
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_default())
    }

    // 현재 Chrome 탭의 URL을 읽는 함수입니다.
    fn current_url(&mut self) -> AutomationResult<String> {
        self.evaluate_string("location.href")
    }

    // Chrome 탭을 지정한 URL로 이동시키는 함수입니다.
    fn navigate(&mut self, url: &str) -> AutomationResult<()> {
        self.call("Page.navigate", json!({ "url": url }))?;
        self.wait_for_ready_state(DEFAULT_TIMEOUT)
    }

    // 페이지가 interactive 또는 complete 상태가 될 때까지 기다리는 함수입니다.
    fn wait_for_ready_state(&mut self, timeout: Duration) -> AutomationResult<()> {
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
