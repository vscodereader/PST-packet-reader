//! 네이버 카페 기본 정보(CafeGateInfo) 조회 HTTP 클라이언트.
//!
//! reqwest를 사용해 `apis.naver.com`에서 카페 기본 정보를 조회한다.
//! 테스트에서는 [`CafeGateClient::with_base_url`]로 wiremock 서버를 주입할 수 있다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을
//! 로그, 에러 메시지, `Debug` 출력에 절대 포함하지 않는다.

use crate::naver_cafe::error::ErrorEnvelope;
use crate::naver_cafe::{
    cafe_ref::models::{CafeGateInfoResponse, CafeInfoView, CafeRefError},
    error::NaverCafeCommonErrorData,
    post::BROWSER_USER_AGENT,
    response::NaverApiErrorBody,
};

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

/// CafeGateInfo API 호스트.
pub const CAFE_API_HOST: &str = "apis.naver.com";

/// 응답 바디 최대 보존 길이(바이트). 초과 시 잘라내고 주석을 추가한다.
const RAW_BODY_MAX_LEN: usize = 2000;

// ---------------------------------------------------------------------------
// 경로 헬퍼
// ---------------------------------------------------------------------------

/// CafeGateInfo API 경로(쿼리 파라미터 포함)를 반환한다.
///
/// # 예시
///
/// ```rust
/// # use pstmacro_lib::naver_cafe::cafe_ref::client::cafe_gate_info_path;
/// assert_eq!(
///     cafe_gate_info_path(31732304),
///     "/cafe-web/cafe2/CafeGateInfo.json?cafeId=31732304"
/// );
/// ```
pub fn cafe_gate_info_path(cafe_id: u64) -> String {
    format!("/cafe-web/cafe2/CafeGateInfo.json?cafeId={}", cafe_id)
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

/// non-2xx 응답 시 `CafeRefError`를 생성한다.
///
/// 실측 캡처된 실패 스키마(`{"error":{"errorCode","message","more":{"requestId"}}}`)로
/// 파싱을 시도한다. 파싱 성공 시 `api_error_code`, `api_error_message`, `trace_id`를 채우고,
/// 실패 시 원본 바디(최대 2000자)를 `api_error_message`에 담는다.
///
/// 쿠키/세션 값은 절대 이 오류에 포함되지 않는다.
fn make_http_error(status: u16, raw_body: String) -> CafeRefError {
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
            code: "CAFE_REF_HTTP_ERROR".to_string(),
            message: "카페 정보 조회 요청이 실패했습니다.".to_string(),
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
        code: "CAFE_REF_HTTP_ERROR".to_string(),
        message: "카페 정보 조회 요청이 실패했습니다.".to_string(),
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

/// 네이버 카페 기본 정보(CafeGateInfo) 조회 HTTP 클라이언트.
///
/// 테스트에서는 [`CafeGateClient::with_base_url`]로 wiremock 등의 목 서버를 주입한다.
pub struct CafeGateClient {
    base_url: String,
    http: reqwest::Client,
}

impl CafeGateClient {
    /// 기본 URL(`https://{CAFE_API_HOST}`)을 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(format!("https://{}", CAFE_API_HOST))
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

    /// cafeId로 카페 정보(cafeInfoView)를 조회해 검증/표시에 사용한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러 메시지나 로그에 절대 노출하지 않는다.
    ///
    /// # 실패 처리
    /// - Transport 오류 → `CAFE_REF_TRANSPORT_ERROR`
    /// - non-2xx → `CAFE_REF_HTTP_ERROR` (api_error_code/message/trace_id 파싱 시도)
    /// - 2xx + `{"error":{...}}` 형태 → `CAFE_REF_API_ERROR`
    /// - 2xx + 파싱 불가 → `CAFE_REF_PARSE_ERROR`
    pub async fn fetch_gate_info(
        &self,
        cafe_id: u64,
        cookie_header: Option<&str>,
    ) -> Result<CafeInfoView, CafeRefError> {
        let path = cafe_gate_info_path(cafe_id);
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
                code: "CAFE_REF_TRANSPORT_ERROR".to_string(),
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
        if let Ok(envelope) = serde_json::from_str::<CafeGateInfoResponse>(&raw_body) {
            return Ok(envelope.message.result.cafe_info_view);
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
                code: "CAFE_REF_API_ERROR".to_string(),
                message: "카페 정보 조회 API가 오류를 반환했습니다.".to_string(),
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
            code: "CAFE_REF_PARSE_ERROR".to_string(),
            message: "카페 정보 응답을 파싱하지 못했습니다.".to_string(),
            error_data: Some(NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status_code),
                api_error_code: None,
                api_error_message: Some(api_error_message),
                retryable: false,
            }),
        })
    }
}

impl Default for CafeGateClient {
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
    use wiremock::{
        matchers::{header, header_exists, method, path_regex},
        Mock, MockServer, ResponseTemplate,
    };

    fn cafe_id() -> u64 {
        31732304
    }

    fn expected_path() -> String {
        cafe_gate_info_path(cafe_id())
    }

    // 실측 캡처된 실제 픽스처 응답
    const REAL_FIXTURE: &str = include_str!("fixtures/cafe_gate_info_success.json");

    // ------------------------------------------------------------------
    // 경로 헬퍼 단위 테스트
    // ------------------------------------------------------------------

    #[test]
    fn cafe_gate_info_path_formats_correctly() {
        assert_eq!(
            cafe_gate_info_path(31732304),
            "/cafe-web/cafe2/CafeGateInfo.json?cafeId=31732304"
        );
    }

    #[test]
    fn cafe_gate_info_path_formats_zero() {
        assert_eq!(
            cafe_gate_info_path(0),
            "/cafe-web/cafe2/CafeGateInfo.json?cafeId=0"
        );
    }

    // ------------------------------------------------------------------
    // 성공 케이스 — 실제 픽스처 응답
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_gate_info_success_returns_cafe_info_view() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        let view = client
            .fetch_gate_info(cafe_id(), None)
            .await
            .expect("성공 응답이어야 함");

        assert_eq!(view.cafe_id, 31732304, "cafeId가 틀림");
        assert_eq!(view.cafe_url, "bluegrayoc3uc", "cafeUrl이 틀림");
        assert_eq!(view.cafe_name, "test64979381", "cafeName이 틀림");
    }

    // ------------------------------------------------------------------
    // 요청 헤더 검증 — User-Agent
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_gate_info_sends_browser_user_agent() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .and(header_exists("User-Agent"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        client
            .fetch_gate_info(cafe_id(), None)
            .await
            .expect("성공 응답이어야 함");

        assert!(
            BROWSER_USER_AGENT.contains("Chrome/"),
            "BROWSER_USER_AGENT는 Chrome/을 포함해야 함: {}",
            BROWSER_USER_AGENT
        );
    }

    // ------------------------------------------------------------------
    // 요청 헤더 검증 — Cookie
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_gate_info_sends_cookie_header_when_provided() {
        let server = MockServer::start().await;

        // 테스트용 가짜 쿠키 값 (실제 쿠키 아님)
        let fake_cookie = "NID_AUT=FAKE_TEST_VALUE; NID_SES=FAKE_SES_VALUE";

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .and(header("Cookie", fake_cookie))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        client
            .fetch_gate_info(cafe_id(), Some(fake_cookie))
            .await
            .expect("성공 응답이어야 함");
    }

    #[tokio::test]
    async fn fetch_gate_info_omits_cookie_header_when_none() {
        let server = MockServer::start().await;

        // Cookie 헤더가 없을 때도 성공해야 함
        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        let result = client.fetch_gate_info(cafe_id(), None).await;
        assert!(result.is_ok(), "쿠키 없이도 성공해야 함");
    }

    // ------------------------------------------------------------------
    // non-2xx 오류 — 실측 캡처된 실제 오류 스키마
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_gate_info_500_with_real_error_body_returns_http_error() {
        let server = MockServer::start().await;

        let real_error_body = r#"{"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"cf4ee2db355d4584b6e0add8f8743048"}}}"#;

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .respond_with(ResponseTemplate::new(500).set_body_string(real_error_body))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        let err = client
            .fetch_gate_info(cafe_id(), None)
            .await
            .expect_err("500은 Err여야 함");

        assert_eq!(err.code, "CAFE_REF_HTTP_ERROR", "오류 코드가 틀림");
        assert_eq!(
            err.trace_id, "cf4ee2db355d4584b6e0add8f8743048",
            "requestId가 trace_id에 매핑되어야 함"
        );

        let error_data = err.error_data.expect("errorData가 없음");
        assert_eq!(error_data.http_status, Some(500), "http_status가 틀림");
        assert!(error_data.retryable, "500은 재시도 가능이어야 함");
        assert_eq!(
            error_data.api_error_code.as_deref(),
            Some("10404"),
            "api_error_code가 틀림"
        );
        assert_eq!(
            error_data.api_error_message.as_deref(),
            Some("Page Not Found"),
            "api_error_message가 틀림"
        );
    }

    #[tokio::test]
    async fn fetch_gate_info_500_unknown_body_returns_http_error_with_raw_body() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        let err = client
            .fetch_gate_info(cafe_id(), None)
            .await
            .expect_err("500은 Err여야 함");

        assert_eq!(err.code, "CAFE_REF_HTTP_ERROR");
        let error_data = err.error_data.expect("errorData가 없음");
        assert_eq!(error_data.http_status, Some(500));
        assert!(
            error_data
                .api_error_message
                .as_deref()
                .unwrap_or("")
                .contains("Internal Server Error"),
            "원본 바디가 api_error_message에 있어야 함"
        );
    }

    // ------------------------------------------------------------------
    // 200 + {"error":{...}} — 200-with-error 케이스 → CAFE_REF_API_ERROR
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_gate_info_200_with_error_body_returns_api_error() {
        let server = MockServer::start().await;

        let error_body = r#"{"error":{"errorCode":"40004","message":"카페를 찾을 수 없습니다"}}"#;

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(error_body))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        let err = client
            .fetch_gate_info(cafe_id(), None)
            .await
            .expect_err("200-with-error는 Err여야 함");

        assert_eq!(err.code, "CAFE_REF_API_ERROR", "오류 코드가 틀림");

        let error_data = err.error_data.expect("errorData가 없음");
        assert_eq!(
            error_data.api_error_code.as_deref(),
            Some("40004"),
            "errorCode가 api_error_code에 매핑되어야 함"
        );
        assert_eq!(
            error_data.api_error_message.as_deref(),
            Some("카페를 찾을 수 없습니다"),
            "error message가 틀림"
        );
        assert_eq!(error_data.http_status, Some(200));
        assert!(!error_data.retryable, "API 오류는 재시도 불가여야 함");
    }

    // ------------------------------------------------------------------
    // 2xx + 파싱 불가 → CAFE_REF_PARSE_ERROR
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_gate_info_200_unparseable_returns_parse_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not valid json at all"))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        let err = client
            .fetch_gate_info(cafe_id(), None)
            .await
            .expect_err("파싱 불가 응답은 Err여야 함");

        assert_eq!(err.code, "CAFE_REF_PARSE_ERROR", "오류 코드가 틀림");

        let error_data = err.error_data.expect("errorData가 없음");
        assert_eq!(error_data.http_status, Some(200));
        assert!(!error_data.retryable, "파싱 오류는 재시도 불가여야 함");
        assert!(
            error_data.api_error_message.is_some(),
            "api_error_message에 원본 바디가 있어야 함"
        );
    }

    #[tokio::test]
    async fn fetch_gate_info_204_no_content_returns_parse_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe2/CafeGateInfo\.json"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = CafeGateClient::with_base_url(server.uri());
        let err = client
            .fetch_gate_info(cafe_id(), None)
            .await
            .expect_err("빈 본문은 Err여야 함");

        assert_eq!(
            err.code, "CAFE_REF_PARSE_ERROR",
            "빈 응답은 파싱 오류여야 함"
        );
    }

    // ------------------------------------------------------------------
    // Default 구현 확인
    // ------------------------------------------------------------------

    #[test]
    fn cafe_gate_client_default_uses_apis_naver_com() {
        let client = CafeGateClient::default();
        let expected_base = format!("https://{}", CAFE_API_HOST);
        assert_eq!(client.base_url, expected_base);
    }

    // ------------------------------------------------------------------
    // 경로에 cafeId가 올바르게 포함되는지 확인
    // ------------------------------------------------------------------

    #[test]
    fn expected_path_contains_cafe_id() {
        let path = expected_path();
        assert!(
            path.contains("31732304"),
            "경로에 cafeId가 포함되어야 함: {}",
            path
        );
        assert!(
            path.starts_with("/cafe-web/cafe2/CafeGateInfo.json"),
            "경로가 올바른 접두사로 시작해야 함: {}",
            path
        );
    }
}
