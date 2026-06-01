//! 네이버 카페 게시글 등록 HTTP 클라이언트.
//!
//! reqwest를 사용해 실제 HTTP POST 요청을 전송한다.
//! 테스트에서는 [`CafeHttpClient::with_base_url`]로 wiremock 서버를 주입할 수 있다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을
//! 로그, 에러 메시지, `Debug` 출력에 절대 포함하지 않는다.

use super::{
    error::{PostError, PostErrorData},
    parser::{parse_article_register, ArticleRegisterResult},
    request_builder::{article_post_headers, article_post_path, ArticleWriteBody, API_HOST},
};
use crate::naver_cafe::{
    error::{ErrorEnvelope, NaverCafeCommonErrorData},
    response::NaverApiErrorBody,
};

// ---------------------------------------------------------------------------
// 오류 코드 상수
// ---------------------------------------------------------------------------

/// HTTP 전송 오류 코드 — 연결 실패, 타임아웃 등 transport 계층 오류.
pub const CODE_HTTP_TRANSPORT_ERROR: &str = "HTTP_TRANSPORT_ERROR";

/// non-2xx 응답 오류 코드 — 실제 응답 바디를 `api_error_message`에 담아
/// 현재 추정된 실패 스키마를 실제 데이터로 확인할 수 있도록 한다.
pub const CODE_REGISTER_HTTP_ERROR: &str = "REGISTER_HTTP_ERROR";

/// 2xx이지만 성공 형태로 파싱 불가능한 경우의 오류 코드 — 원본 바디를
/// `api_error_message`에 담아 실제 응답 스키마를 확인하는 데 사용한다.
pub const CODE_REGISTER_PARSE_ERROR: &str = "REGISTER_PARSE_ERROR";

/// 세션 쿠키 없음/만료 오류 코드.
pub const CODE_SESSION_INVALID: &str = "SESSION_INVALID";

/// 응답 바디 최대 보존 길이(바이트). 초과 시 잘라내고 주석을 추가한다.
const RAW_BODY_MAX_LEN: usize = 2000;

// ---------------------------------------------------------------------------
// 쿠키 헬퍼
// ---------------------------------------------------------------------------

/// Playwright storage-state JSON에서 네이버 도메인 쿠키만 추출해
/// `name=value; name=value` 형식의 Cookie 헤더 값을 생성한다.
///
/// # 보안 주의
/// 반환 문자열은 사용자의 인증 자격 증명이다. 로그·에러·`Debug` 출력에
/// 절대 포함하면 안 된다.
///
/// `domain` 필드가 `"naver"`를 포함하는 쿠키만 포함하며,
/// 유효한 쿠키가 없으면 `None`을 반환한다.
pub fn cookie_header_from_storage_state(value: &serde_json::Value) -> Option<String> {
    let cookies = value.get("cookies")?.as_array()?;

    let pairs: Vec<String> = cookies
        .iter()
        .filter_map(|cookie| {
            let domain = cookie.get("domain")?.as_str()?;
            if !domain.contains("naver") {
                return None;
            }
            let name = cookie.get("name")?.as_str()?;
            let val = cookie.get("value")?.as_str()?;
            Some(format!("{}={}", name, val))
        })
        .collect();

    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
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
        // 문자 경계에서 안전하게 잘라낸다
        let cutoff = raw
            .char_indices()
            .take_while(|(i, _)| *i < RAW_BODY_MAX_LEN)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(RAW_BODY_MAX_LEN);
        format!("{} [truncated]", &raw[..cutoff])
    }
}

/// non-2xx 응답 시 `PostError`를 생성한다.
///
/// 실측 캡처된 실패 스키마(`{"error":{"errorCode","message","more":{"requestId"}}}`)로
/// 파싱을 시도한다:
/// - 파싱 성공: `api_error_code`, `api_error_message`, `trace_id`(requestId)를 채운다.
/// - 파싱 실패: 원본 바디(최대 2000자)를 `api_error_message`에 담는 폴백을 유지한다.
///
/// 쿠키/세션 값은 절대 이 오류에 포함되지 않는다.
fn make_non_2xx_error(
    code: &str,
    message: String,
    status: u16,
    raw_body: String,
    retryable: bool,
) -> PostError {
    // 실측 캡처된 스키마로 파싱 시도
    if let Some(error_body) = NaverApiErrorBody::parse(&raw_body) {
        let trace_id = error_body
            .error
            .more
            .as_ref()
            .and_then(|m| m.request_id.clone())
            .unwrap_or_default();
        return ErrorEnvelope {
            trace_id,
            code: code.to_string(),
            message,
            error_data: Some(PostErrorData {
                cafe: NaverCafeCommonErrorData {
                    target: None,
                    http_status: Some(status),
                    api_error_code: Some(error_body.error.error_code),
                    api_error_message: Some(error_body.error.message),
                    retryable,
                },
                menu_id: None,
                subject: None,
                validation_errors: vec![],
            }),
        };
    }

    // 알 수 없는 형태 폴백: 원본 바디를 그대로 보존
    let api_error_message = truncate_body(raw_body);
    ErrorEnvelope {
        trace_id: String::new(),
        code: code.to_string(),
        message,
        error_data: Some(PostErrorData {
            cafe: NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status),
                api_error_code: None,
                api_error_message: Some(api_error_message),
                retryable,
            },
            menu_id: None,
            subject: None,
            validation_errors: vec![],
        }),
    }
}

/// 2xx이지만 파싱 실패 시 `PostError`를 생성한다.
///
/// 원본 바디(최대 2000자)를 `api_error_message`에 담아 실제 응답 스키마 확인에 사용한다.
///
/// 쿠키/세션 값은 절대 이 오류에 포함되지 않는다.
fn make_parse_error(
    code: &str,
    message: String,
    status: u16,
    raw_body: String,
) -> PostError {
    let api_error_message = truncate_body(raw_body);
    ErrorEnvelope {
        trace_id: String::new(),
        code: code.to_string(),
        message,
        error_data: Some(PostErrorData {
            cafe: NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status),
                api_error_code: None,
                api_error_message: Some(api_error_message),
                retryable: false,
            },
            menu_id: None,
            subject: None,
            validation_errors: vec![],
        }),
    }
}

// ---------------------------------------------------------------------------
// HTTP 클라이언트
// ---------------------------------------------------------------------------

/// 네이버 카페 게시글 등록 HTTP 클라이언트.
///
/// 테스트에서는 [`CafeHttpClient::with_base_url`]로 wiremock 등의 목 서버를 주입한다.
pub struct CafeHttpClient {
    base_url: String,
    http: reqwest::Client,
}

impl CafeHttpClient {
    /// 기본 URL(`https://{API_HOST}`)을 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(format!("https://{}", API_HOST))
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

    /// 게시글 등록 요청을 전송하고 결과를 반환한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러 메시지나 로그에 절대 노출하지 않는다.
    ///
    /// # 실패 시 원본 바디 캡처
    /// non-2xx 응답 또는 2xx이지만 파싱 실패 시, `PostError.error_data.cafe.api_error_message`에
    /// 원본 응답 바디(최대 2000자)를 담는다. 이를 통해 현재 추정된 실패 응답
    /// 스키마를 실제 데이터로 검증할 수 있다.
    pub async fn post_article(
        &self,
        cafe_id: &str,
        menu_id: u64,
        body: &ArticleWriteBody,
        cookie_header: Option<&str>,
    ) -> Result<ArticleRegisterResult, PostError> {
        let path = article_post_path(cafe_id, menu_id);
        let url = format!("{}{}", self.base_url, path);

        // 헤더 설정 — Origin / Referer / Content-Type
        let mut req = self.http.post(&url);
        for (name, value) in article_post_headers(cafe_id) {
            req = req.header(&name, &value);
        }

        // Cookie 헤더 설정 — 값은 절대 로그에 기록하지 않는다
        if let Some(cookie) = cookie_header {
            req = req.header("Cookie", cookie);
        }

        // JSON 바디 전송
        let response = req.json(body).send().await.map_err(|e| {
            let retryable = e.is_timeout() || e.is_connect();
            ErrorEnvelope {
                trace_id: String::new(),
                code: CODE_HTTP_TRANSPORT_ERROR.to_string(),
                message: format!("HTTP 전송 오류가 발생했습니다: {}", e),
                error_data: Some(PostErrorData {
                    cafe: NaverCafeCommonErrorData {
                        target: None,
                        http_status: None,
                        api_error_code: None,
                        api_error_message: None,
                        retryable,
                    },
                    menu_id: None,
                    subject: None,
                    validation_errors: vec![],
                }),
            }
        })?;

        let status = response.status();
        let status_code = status.as_u16();

        // 응답 바디 읽기
        let raw_body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            // non-2xx: 실측 캡처된 스키마로 파싱 시도, 실패 시 원본 바디 폴백
            let retryable = status_code >= 500;
            return Err(make_non_2xx_error(
                CODE_REGISTER_HTTP_ERROR,
                "게시글 등록 요청이 실패했습니다. 오류 응답은 errorData.cafe.apiErrorMessage를 확인하세요."
                    .to_string(),
                status_code,
                raw_body,
                retryable,
            ));
        }

        // 2xx: 성공 형태로 파싱 시도
        parse_article_register(&raw_body).map_err(|_| {
            // 2xx이지만 파싱 실패: 원본 바디를 보존해 실제 스키마 확인에 사용
            make_parse_error(
                CODE_REGISTER_PARSE_ERROR,
                "게시글 등록 응답을 파싱하지 못했습니다. 실제 응답 형태는 errorData.cafe.apiErrorMessage를 확인하세요."
                    .to_string(),
                status_code,
                raw_body,
            )
        })
    }
}

impl Default for CafeHttpClient {
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

    // ------------------------------------------------------------------
    // 헬퍼
    // ------------------------------------------------------------------

    fn cafe_id() -> &'static str {
        "31732304"
    }

    fn menu_id() -> u64 {
        1
    }

    fn dummy_body() -> ArticleWriteBody {
        use crate::naver_cafe::post::{
            models::PostRequest, request_builder::build_article_write_body_with_content,
            smart_editor::SequentialIdProvider,
        };
        let req = PostRequest {
            cafe_id: cafe_id().to_string(),
            menu_id: menu_id(),
            subject: "테스트".to_string(),
            body_text: "본문".to_string(),
            tag_list: vec![],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        };
        let mut ids = SequentialIdProvider::new();
        build_article_write_body_with_content(&req, &mut ids).expect("body 생성 실패")
    }

    fn expected_path() -> String {
        format!("/editor/v2.0/cafes/{}/menus/{}/articles", cafe_id(), menu_id())
    }

    // ------------------------------------------------------------------
    // 성공 케이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_article_success_returns_register_result() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {
                    "cafeId": 31732304_u64,
                    "articleId": 7_u64,
                    "menuId": 1_u64
                }
            })))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        let result = client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect("성공 응답이어야 함");

        assert_eq!(result.cafe_id, 31732304);
        assert_eq!(result.article_id, 7);
        assert_eq!(result.menu_id, 1);
    }

    // ------------------------------------------------------------------
    // 요청 형태 검증
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_article_sends_correct_content_type_and_origin() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .and(header("Content-Type", "application/json"))
            .and(header("Origin", "https://cafe.naver.com"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "cafeId": 31732304_u64, "articleId": 1_u64, "menuId": 1_u64 }
            })))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect("성공 응답이어야 함");

        // mock이 응답했다면 헤더 조건이 충족된 것
    }

    #[tokio::test]
    async fn post_article_referer_contains_menus_0() {
        let server = MockServer::start().await;

        let expected_referer = format!(
            "https://cafe.naver.com/ca-fe/cafes/{}/menus/0/articles/write",
            cafe_id()
        );

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .and(header("Referer", expected_referer.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "cafeId": 31732304_u64, "articleId": 1_u64, "menuId": 1_u64 }
            })))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect("성공 응답이어야 함");
    }

    #[tokio::test]
    async fn post_article_sends_cookie_header_when_provided() {
        let server = MockServer::start().await;

        // 테스트용 가짜 쿠키 값 사용 (실제 쿠키 아님)
        let fake_cookie = "NID_AUT=FAKE_VALUE_FOR_TEST; NID_SES=FAKE_SES_FOR_TEST";

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .and(header_exists("Cookie"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "cafeId": 31732304_u64, "articleId": 1_u64, "menuId": 1_u64 }
            })))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        client
            .post_article(cafe_id(), menu_id(), &dummy_body(), Some(fake_cookie))
            .await
            .expect("성공 응답이어야 함");
    }

    #[tokio::test]
    async fn post_article_cookie_header_contains_nid_aut() {
        let server = MockServer::start().await;

        // 테스트용 가짜 쿠키 값 — NID_AUT= 접두어가 포함되어야 함을 검증
        let fake_cookie = "NID_AUT=FAKE_VALUE_FOR_TEST; NID_SES=FAKE_SES_FOR_TEST";

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .and(header("Cookie", fake_cookie))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "cafeId": 31732304_u64, "articleId": 1_u64, "menuId": 1_u64 }
            })))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        client
            .post_article(cafe_id(), menu_id(), &dummy_body(), Some(fake_cookie))
            .await
            .expect("성공 응답이어야 함");
    }

    // ------------------------------------------------------------------
    // 500 실패 — 실측 캡처된 실제 오류 스키마
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_article_500_real_error_body_parses_error_code_message_trace_id() {
        let server = MockServer::start().await;

        let real_body = r#"{"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"cf4ee2db355d4584b6e0add8f8743048"}}}"#;

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(500).set_body_string(real_body))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        let err = client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect_err("500은 Err여야 함");

        assert_eq!(err.code, CODE_REGISTER_HTTP_ERROR, "오류 코드가 틀림");
        assert_eq!(
            err.trace_id, "cf4ee2db355d4584b6e0add8f8743048",
            "requestId가 trace_id에 매핑되어야 함"
        );

        let error_data = err.error_data.expect("errorData가 없음");
        let cafe = &error_data.cafe;

        assert_eq!(cafe.http_status, Some(500), "HTTP 상태가 500이어야 함");
        assert!(cafe.retryable, "500은 재시도 가능이어야 함");
        assert_eq!(
            cafe.api_error_code.as_deref(),
            Some("10404"),
            "errorCode가 api_error_code에 매핑되어야 함"
        );
        assert_eq!(
            cafe.api_error_message.as_deref(),
            Some("Page Not Found"),
            "message가 api_error_message에 매핑되어야 함"
        );
    }

    // ------------------------------------------------------------------
    // 403 실패 — 알 수 없는 형태 폴백
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_article_non_2xx_unknown_shape_falls_back_to_raw_body() {
        let server = MockServer::start().await;

        let raw_body = r#"{"some":"unknown error shape"}"#;

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(403).set_body_string(raw_body))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        let err = client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect_err("403은 Err여야 함");

        assert_eq!(err.code, CODE_REGISTER_HTTP_ERROR, "오류 코드가 틀림");

        let error_data = err.error_data.expect("errorData가 없음");
        let cafe = &error_data.cafe;

        assert_eq!(cafe.http_status, Some(403), "HTTP 상태가 403이어야 함");
        assert!(!cafe.retryable, "403은 재시도 불가여야 함");
        assert!(
            cafe.api_error_code.is_none(),
            "알 수 없는 형태는 api_error_code가 None이어야 함"
        );

        let captured_body = cafe
            .api_error_message
            .as_deref()
            .expect("api_error_message가 없음");
        assert!(
            captured_body.contains(r#"{"some":"unknown error shape"}"#),
            "원본 바디가 폴백으로 캡처되어야 함: {}",
            captured_body
        );
    }

    // 기존 테스트명 유지를 위한 별칭 테스트
    #[tokio::test]
    async fn post_article_403_returns_http_error_with_raw_body() {
        let server = MockServer::start().await;

        let raw_body = r#"{"other":"unknown"}"#;

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(403).set_body_string(raw_body))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        let err = client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect_err("403은 Err여야 함");

        assert_eq!(err.code, CODE_REGISTER_HTTP_ERROR, "오류 코드가 틀림");
        let cafe = &err.error_data.expect("errorData가 없음").cafe;
        assert_eq!(cafe.http_status, Some(403));
        assert!(!cafe.retryable, "403은 재시도 불가여야 함");
    }

    // ------------------------------------------------------------------
    // 500 실패 — retryable
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_article_500_is_retryable() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        let err = client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect_err("500은 Err여야 함");

        let cafe = &err.error_data.expect("errorData가 없음").cafe;
        assert!(cafe.retryable, "500은 재시도 가능이어야 함");
        assert_eq!(cafe.http_status, Some(500));
    }

    // ------------------------------------------------------------------
    // 2xx 파싱 실패
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_article_200_unparseable_returns_parse_error() {
        let server = MockServer::start().await;

        let raw_body = r#"{"unexpected":true}"#;

        Mock::given(method("POST"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string(raw_body))
            .mount(&server)
            .await;

        let client = CafeHttpClient::with_base_url(server.uri());
        let err = client
            .post_article(cafe_id(), menu_id(), &dummy_body(), None)
            .await
            .expect_err("파싱 불가 응답은 Err여야 함");

        assert_eq!(err.code, CODE_REGISTER_PARSE_ERROR, "오류 코드가 틀림");

        let captured_body = err
            .error_data
            .expect("errorData가 없음")
            .cafe
            .api_error_message
            .expect("api_error_message가 없음");
        assert!(
            captured_body.contains(r#"{"unexpected":true}"#),
            "원본 바디가 캡처되어야 함: {}",
            captured_body
        );
    }

    // ------------------------------------------------------------------
    // cookie_header_from_storage_state 단위 테스트
    // ------------------------------------------------------------------

    #[test]
    fn cookie_header_includes_naver_cookies_and_excludes_others() {
        // 테스트용 가짜 쿠키 값 — 실제 인증 값 아님
        let storage_state = json!({
            "cookies": [
                {
                    "name": "NID_AUT",
                    "value": "FAKE_NID_AUT_FOR_TEST",
                    "domain": ".naver.com",
                    "expires": 9999999999_u64
                },
                {
                    "name": "NID_SES",
                    "value": "FAKE_NID_SES_FOR_TEST",
                    "domain": ".naver.com",
                    "expires": 9999999999_u64
                },
                {
                    "name": "GOOGLE_COOKIE",
                    "value": "should_not_appear",
                    "domain": ".google.com",
                    "expires": 9999999999_u64
                }
            ]
        });

        let header = cookie_header_from_storage_state(&storage_state)
            .expect("쿠키 헤더가 생성되어야 함");

        // 네이버 쿠키 포함 여부
        assert!(
            header.contains("NID_AUT=FAKE_NID_AUT_FOR_TEST"),
            "NID_AUT가 포함되어야 함: {}",
            header
        );
        assert!(
            header.contains("NID_SES=FAKE_NID_SES_FOR_TEST"),
            "NID_SES가 포함되어야 함: {}",
            header
        );

        // 비-네이버 쿠키 제외 여부
        assert!(
            !header.contains("GOOGLE_COOKIE"),
            "GOOGLE_COOKIE가 포함되면 안 됨: {}",
            header
        );
        assert!(
            !header.contains("should_not_appear"),
            "google 쿠키 값이 포함되면 안 됨: {}",
            header
        );
    }

    #[test]
    fn cookie_header_returns_none_when_no_naver_cookies() {
        let storage_state = json!({
            "cookies": [
                {
                    "name": "SOME_COOKIE",
                    "value": "some_value",
                    "domain": ".example.com",
                    "expires": 9999999999_u64
                }
            ]
        });

        let header = cookie_header_from_storage_state(&storage_state);
        assert!(header.is_none(), "네이버 쿠키 없으면 None이어야 함");
    }

    #[test]
    fn cookie_header_returns_none_for_empty_cookies() {
        let storage_state = json!({ "cookies": [] });
        let header = cookie_header_from_storage_state(&storage_state);
        assert!(header.is_none(), "빈 쿠키 배열이면 None이어야 함");
    }

    #[test]
    fn cookie_header_returns_none_for_missing_cookies_key() {
        let storage_state = json!({});
        let header = cookie_header_from_storage_state(&storage_state);
        assert!(header.is_none(), "cookies 키가 없으면 None이어야 함");
    }

    // ------------------------------------------------------------------
    // truncate_body 단위 테스트
    // ------------------------------------------------------------------

    #[test]
    fn truncate_body_short_string_unchanged() {
        let s = "short string".to_string();
        let result = truncate_body(s.clone());
        assert_eq!(result, s);
    }

    #[test]
    fn truncate_body_long_string_is_truncated() {
        let s = "x".repeat(RAW_BODY_MAX_LEN + 100);
        let result = truncate_body(s);
        assert!(
            result.contains("[truncated]"),
            "[truncated] 주석이 있어야 함"
        );
        assert!(
            result.len() <= RAW_BODY_MAX_LEN + 50,
            "결과가 너무 길면 안 됨"
        );
    }
}
