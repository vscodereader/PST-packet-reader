//! 네이버 카페 게시판(메뉴) 목록 조회 HTTP 클라이언트.
//!
//! reqwest를 사용해 `apis.naver.com`에서 게시판 목록을 조회한다.
//! 테스트에서는 [`CafeMenuClient::with_base_url`]로 wiremock 서버를 주입할 수 있다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을
//! 로그, 에러 메시지, `Debug` 출력에 절대 포함하지 않는다.

use crate::naver_cafe::error::ErrorEnvelope;
use crate::naver_cafe::post::BROWSER_USER_AGENT;
use crate::naver_cafe::{
    error::NaverCafeCommonErrorData,
    menu::models::{general_writable_boards, Menu, MenuError},
    response::{NaverApiErrorBody, ResultEnvelope},
};

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

/// 게시판 목록 API 호스트.
pub const MENU_API_HOST: &str = "apis.naver.com";

/// 응답 바디 최대 보존 길이(바이트). 초과 시 잘라내고 주석을 추가한다.
const RAW_BODY_MAX_LEN: usize = 2000;

// ---------------------------------------------------------------------------
// 경로 헬퍼
// ---------------------------------------------------------------------------

/// 게시판 목록 API 경로를 반환한다.
pub fn menu_list_path(cafe_id: &str) -> String {
    format!(
        "/cafe-web/cafe-cafeinfo-api/v1.0/cafes/{}/editor/menus",
        cafe_id
    )
}

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

/// 응답 바디 텍스트를 최대 `RAW_BODY_MAX_LEN` 바이트로 잘라낸다.
/// 잘린 경우 끝에 `[truncated]` 주석을 추가한다.
fn truncate_body(raw: String) -> String {
    if raw.len() <= RAW_BODY_MAX_LEN {
        raw
    } else {
        let cutoff = raw
            .char_indices()
            .take_while(|(i, _)| *i < RAW_BODY_MAX_LEN)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(RAW_BODY_MAX_LEN);
        format!("{} [truncated]", &raw[..cutoff])
    }
}

/// non-2xx 응답 시 `MenuError`를 생성한다.
///
/// 실측 캡처된 실패 스키마(`{"error":{"errorCode","message","more":{"requestId"}}}`)로
/// 파싱을 시도한다. 파싱 성공 시 `api_error_code`, `api_error_message`, `trace_id`를 채우고,
/// 실패 시 원본 바디(최대 2000자)를 `api_error_message`에 담는다.
///
/// 쿠키/세션 값은 절대 이 오류에 포함되지 않는다.
fn make_http_error(status: u16, raw_body: String) -> MenuError {
    let retryable = status >= 500;

    if let Some(error_body) = NaverApiErrorBody::parse(&raw_body) {
        let trace_id = error_body
            .error
            .more
            .as_ref()
            .and_then(|m| m.request_id.clone())
            .unwrap_or_default();
        return ErrorEnvelope {
            trace_id,
            code: "MENU_HTTP_ERROR".to_string(),
            message: "게시판 목록 조회 요청이 실패했습니다.".to_string(),
            error_data: Some(NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status),
                api_error_code: Some(error_body.error.error_code),
                api_error_message: Some(error_body.error.message),
                retryable,
            }),
        };
    }

    // 알 수 없는 형태 폴백
    let api_error_message = truncate_body(raw_body);
    ErrorEnvelope {
        trace_id: String::new(),
        code: "MENU_HTTP_ERROR".to_string(),
        message: "게시판 목록 조회 요청이 실패했습니다.".to_string(),
        error_data: Some(NaverCafeCommonErrorData {
            target: None,
            http_status: Some(status),
            api_error_code: None,
            api_error_message: Some(api_error_message),
            retryable,
        }),
    }
}

// ---------------------------------------------------------------------------
// HTTP 클라이언트
// ---------------------------------------------------------------------------

/// 네이버 카페 게시판(메뉴) 목록 조회 HTTP 클라이언트.
///
/// 테스트에서는 [`CafeMenuClient::with_base_url`]로 wiremock 등의 목 서버를 주입한다.
pub struct CafeMenuClient {
    base_url: String,
    http: reqwest::Client,
}

impl CafeMenuClient {
    /// 기본 URL(`https://{MENU_API_HOST}`)을 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(format!("https://{}", MENU_API_HOST))
    }

    /// 주입된 `base_url`을 사용하는 클라이언트를 생성한다.
    ///
    /// 테스트에서 wiremock 서버 URL을 주입하는 데 사용한다.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// 카페의 게시판 목록을 조회하고 반환한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러 메시지나 로그에 절대 노출하지 않는다.
    ///
    /// # 실패 처리
    /// - Transport 오류 → `MENU_TRANSPORT_ERROR`
    /// - non-2xx → `MENU_HTTP_ERROR` (api_error_code/message/trace_id 파싱 시도)
    /// - 2xx + `{"error":{...}}` 형태 → `MENU_API_ERROR`
    /// - 2xx + 파싱 불가 → `MENU_PARSE_ERROR`
    pub async fn fetch_menus(
        &self,
        cafe_id: &str,
        cookie_header: Option<&str>,
    ) -> Result<Vec<Menu>, MenuError> {
        let path = menu_list_path(cafe_id);
        let url = format!("{}{}", self.base_url, path);

        let mut req = self
            .http
            .get(&url)
            .header("Accept", "application/json")
            .header("User-Agent", BROWSER_USER_AGENT);

        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(cookie) = cookie_header {
            req = req.header("Cookie", cookie);
        }

        let response = req.send().await.map_err(|e| {
            let retryable = e.is_timeout() || e.is_connect();
            ErrorEnvelope {
                trace_id: String::new(),
                code: "MENU_TRANSPORT_ERROR".to_string(),
                message: format!("HTTP 전송 오류가 발생했습니다: {}", e),
                error_data: Some(NaverCafeCommonErrorData {
                    target: None,
                    http_status: None,
                    api_error_code: None,
                    api_error_message: None,
                    retryable,
                }),
            }
        })?;

        let status = response.status();
        let status_code = status.as_u16();
        let raw_body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(make_http_error(status_code, raw_body));
        }

        // 2xx 응답 — 성공 형태 먼저 시도
        if let Ok(envelope) = serde_json::from_str::<ResultEnvelope<Vec<Menu>>>(&raw_body) {
            return Ok(envelope.result);
        }

        // 2xx이지만 {"error":{...}} 형태인 경우 (200-with-error 케이스)
        if let Some(error_body) = NaverApiErrorBody::parse(&raw_body) {
            let trace_id = error_body
                .error
                .more
                .as_ref()
                .and_then(|m| m.request_id.clone())
                .unwrap_or_default();
            return Err(ErrorEnvelope {
                trace_id,
                code: "MENU_API_ERROR".to_string(),
                message: "게시판 목록 조회 API가 오류를 반환했습니다.".to_string(),
                error_data: Some(NaverCafeCommonErrorData {
                    target: None,
                    http_status: Some(status_code),
                    api_error_code: Some(error_body.error.error_code),
                    api_error_message: Some(error_body.error.message),
                    retryable: false,
                }),
            });
        }

        // 파싱 불가 폴백
        let api_error_message = truncate_body(raw_body);
        Err(ErrorEnvelope {
            trace_id: String::new(),
            code: "MENU_PARSE_ERROR".to_string(),
            message: "게시판 목록 응답을 파싱하지 못했습니다.".to_string(),
            error_data: Some(NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status_code),
                api_error_code: None,
                api_error_message: Some(api_error_message),
                retryable: false,
            }),
        })
    }

    /// 카페의 일반 게시판 목록만 반환한다 — `fetch_menus`에 `general_writable_boards` 필터를 적용한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러 메시지나 로그에 절대 노출하지 않는다.
    pub async fn fetch_general_writable_boards(
        &self,
        cafe_id: &str,
        cookie_header: Option<&str>,
    ) -> Result<Vec<Menu>, MenuError> {
        let menus = self.fetch_menus(cafe_id, cookie_header).await?;
        Ok(general_writable_boards(&menus)
            .into_iter()
            .cloned()
            .collect())
    }
}

impl Default for CafeMenuClient {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{header, header_exists, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn cafe_id() -> &'static str {
        "31732304"
    }

    fn expected_path() -> String {
        menu_list_path(cafe_id())
    }

    // 실측 캡처된 실제 픽스처 응답
    const REAL_FIXTURE: &str = include_str!("fixtures/menu_list_success.json");

    // ------------------------------------------------------------------
    // 성공 케이스 — 실제 픽스처 응답
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_menus_success_returns_menu_list() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = CafeMenuClient::with_base_url(server.uri());
        let menus = client
            .fetch_menus(cafe_id(), None)
            .await
            .expect("성공 응답이어야 함");

        assert_eq!(menus.len(), 1, "게시판 1개가 반환되어야 함");
        let menu = &menus[0];
        assert_eq!(menu.menu_id, 1);
        assert_eq!(menu.menu_name, "자유게시판");
        assert_eq!(menu.board_type, "L");
    }

    // ------------------------------------------------------------------
    // 요청 헤더 검증
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_menus_sends_browser_user_agent() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(expected_path()))
            .and(header_exists("User-Agent"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = CafeMenuClient::with_base_url(server.uri());
        client
            .fetch_menus(cafe_id(), None)
            .await
            .expect("성공 응답이어야 함");

        assert!(
            BROWSER_USER_AGENT.contains("Chrome/"),
            "BROWSER_USER_AGENT는 Chrome/을 포함해야 함: {}",
            BROWSER_USER_AGENT
        );
    }

    #[tokio::test]
    async fn fetch_menus_sends_cookie_header_when_provided() {
        let server = MockServer::start().await;

        // 테스트용 가짜 쿠키 값 (실제 쿠키 아님)
        let fake_cookie = "NID_AUT=FAKE_TEST_VALUE; NID_SES=FAKE_SES_VALUE";

        Mock::given(method("GET"))
            .and(path(expected_path()))
            .and(header("Cookie", fake_cookie))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = CafeMenuClient::with_base_url(server.uri());
        client
            .fetch_menus(cafe_id(), Some(fake_cookie))
            .await
            .expect("성공 응답이어야 함");
    }

    // ------------------------------------------------------------------
    // fetch_general_writable_boards 필터 케이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_general_writable_boards_filters_correctly() {
        let server = MockServer::start().await;

        // 다양한 게시판 유형 혼합 응답
        let mixed_response = json!({
            "result": [
                // 유효한 일반 게시판
                {
                    "cafeId": 31732304_u64,
                    "menuId": 1_u64,
                    "menuName": "자유게시판",
                    "menuType": "B",
                    "boardType": "L",
                    "writable": true,
                    "hidden": false,
                    "separatorMenuType": false
                },
                // 구분선 메뉴 — 제외되어야 함
                {
                    "cafeId": 31732304_u64,
                    "menuId": 2_u64,
                    "menuName": "구분선",
                    "menuType": "B",
                    "boardType": "L",
                    "writable": true,
                    "hidden": false,
                    "separatorMenuType": true
                },
                // 숨김 게시판 — 제외되어야 함
                {
                    "cafeId": 31732304_u64,
                    "menuId": 3_u64,
                    "menuName": "숨김게시판",
                    "menuType": "B",
                    "boardType": "L",
                    "writable": true,
                    "hidden": true,
                    "separatorMenuType": false
                },
                // 글쓰기 불가 게시판 — 제외되어야 함
                {
                    "cafeId": 31732304_u64,
                    "menuId": 4_u64,
                    "menuName": "읽기전용",
                    "menuType": "B",
                    "boardType": "L",
                    "writable": false,
                    "hidden": false,
                    "separatorMenuType": false
                },
                // 마켓 게시판 (menuType != "B") — 제외되어야 함
                {
                    "cafeId": 31732304_u64,
                    "menuId": 5_u64,
                    "menuName": "중고마켓",
                    "menuType": "M",
                    "boardType": "L",
                    "writable": true,
                    "hidden": false,
                    "separatorMenuType": false
                },
                // 두 번째 유효한 일반 게시판
                {
                    "cafeId": 31732304_u64,
                    "menuId": 6_u64,
                    "menuName": "공지사항",
                    "menuType": "B",
                    "boardType": "L",
                    "writable": true,
                    "hidden": false,
                    "separatorMenuType": false
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(200).set_body_json(mixed_response))
            .mount(&server)
            .await;

        let client = CafeMenuClient::with_base_url(server.uri());
        let boards = client
            .fetch_general_writable_boards(cafe_id(), None)
            .await
            .expect("성공 응답이어야 함");

        assert_eq!(boards.len(), 2, "일반 게시판 2개만 반환되어야 함");
        assert_eq!(boards[0].menu_id, 1);
        assert_eq!(boards[1].menu_id, 6);
    }

    // ------------------------------------------------------------------
    // non-2xx 오류 — 실측 캡처된 실제 오류 스키마
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_menus_500_real_error_body_returns_menu_http_error() {
        let server = MockServer::start().await;

        let real_error_body = r#"{"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"cf4ee2db355d4584b6e0add8f8743048"}}}"#;

        Mock::given(method("GET"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(500).set_body_string(real_error_body))
            .mount(&server)
            .await;

        let client = CafeMenuClient::with_base_url(server.uri());
        let err = client
            .fetch_menus(cafe_id(), None)
            .await
            .expect_err("500은 Err여야 함");

        assert_eq!(err.code, "MENU_HTTP_ERROR", "오류 코드가 틀림");
        assert_eq!(
            err.trace_id, "cf4ee2db355d4584b6e0add8f8743048",
            "requestId가 trace_id에 매핑되어야 함"
        );

        let error_data = err.error_data.expect("errorData가 없음");
        assert_eq!(error_data.http_status, Some(500));
        assert!(error_data.retryable, "500은 재시도 가능이어야 함");
        assert_eq!(error_data.api_error_code.as_deref(), Some("10404"));
        assert_eq!(
            error_data.api_error_message.as_deref(),
            Some("Page Not Found")
        );
    }

    // ------------------------------------------------------------------
    // 200 + {"error":{...}} — 200-with-error 케이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_menus_200_with_error_body_returns_menu_api_error() {
        let server = MockServer::start().await;

        let error_body = r#"{"error":{"errorCode":"40004","message":"카페를 찾을 수 없습니다"}}"#;

        Mock::given(method("GET"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string(error_body))
            .mount(&server)
            .await;

        let client = CafeMenuClient::with_base_url(server.uri());
        let err = client
            .fetch_menus(cafe_id(), None)
            .await
            .expect_err("200-with-error는 Err여야 함");

        assert_eq!(err.code, "MENU_API_ERROR", "오류 코드가 틀림");

        let error_data = err.error_data.expect("errorData가 없음");
        assert_eq!(
            error_data.api_error_code.as_deref(),
            Some("40004"),
            "errorCode가 api_error_code에 매핑되어야 함"
        );
    }
}
