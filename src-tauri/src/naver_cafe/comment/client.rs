//! 네이버 카페 댓글/대댓글 등록 HTTP 클라이언트.
//!
//! reqwest로 실제 HTTP POST 요청을 전송한다. 글 작성 클라이언트
//! ([`post::CafeHttpClient`](crate::naver_cafe::post::CafeHttpClient))의 댓글 버전이며
//! 다음이 다르다:
//! - 호스트가 `apis.naver.com`, 본문이 `application/x-www-form-urlencoded`.
//! - 실패 응답 스키마가 글 작성과 다르다(`{"errorCode","reason","more":{...}}`,
//!   `{"error":{...}}` 래퍼 없음) — 댓글 전용 파서
//!   [`CommentApiFailure`](super::parser::CommentApiFailure)를 사용한다.
//!
//! 테스트에서는 [`CafeCommentClient::with_base_url`]로 wiremock 서버를 주입할 수 있다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을
//! 로그, 에러 메시지, `Debug` 출력에 절대 포함하지 않는다.

use super::{
    error::{CommentError, CommentErrorData},
    models::{CommentRequest, ReplyRequest},
    parser::{parse_comment_result, CommentApiFailure, CommentResult},
    request_builder::{
        build_comment_form, build_reply_form, comment_headers, comment_post_path,
        comment_reply_path, encode_form, API_HOST,
    },
    service::CODE_FORM_BUILD_FAILED,
};
use crate::naver_cafe::{
    error::{ErrorEnvelope, NaverCafeCommonErrorData},
    post::BROWSER_USER_AGENT,
};

// ---------------------------------------------------------------------------
// 오류 코드 상수
// ---------------------------------------------------------------------------

/// HTTP 전송 오류 코드 — 연결 실패, 타임아웃 등 transport 계층 오류.
pub const CODE_HTTP_TRANSPORT_ERROR: &str = "HTTP_TRANSPORT_ERROR";

/// non-2xx 응답 오류 코드.
pub const CODE_COMMENT_HTTP_ERROR: &str = "COMMENT_HTTP_ERROR";

/// 2xx이지만 성공 형태로 파싱 불가능한 경우의 오류 코드.
pub const CODE_COMMENT_PARSE_ERROR: &str = "COMMENT_PARSE_ERROR";

/// 응답 바디 최대 보존 길이(바이트). 초과 시 잘라내고 주석을 추가한다.
const RAW_BODY_MAX_LEN: usize = 2000;

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

/// 응답 바디 텍스트를 최대 `RAW_BODY_MAX_LEN` 바이트로 잘라낸다.
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

/// [`CommentErrorData`]를 조립한다.
fn error_data(
    article_id: &str,
    ref_comment_id: Option<&str>,
    cafe: NaverCafeCommonErrorData,
) -> CommentErrorData {
    CommentErrorData {
        cafe,
        article_id: Some(article_id.to_string()),
        ref_comment_id: ref_comment_id.map(|s| s.to_string()),
        validation_errors: vec![],
    }
}

/// 폼 인코딩 실패를 [`CommentError`]로 변환한다.
fn form_build_error(
    article_id: &str,
    ref_comment_id: Option<&str>,
    err: serde_urlencoded::ser::Error,
) -> CommentError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_FORM_BUILD_FAILED.to_string(),
        message: format!("댓글 폼 인코딩에 실패했습니다: {err}"),
        error_data: Some(error_data(
            article_id,
            ref_comment_id,
            NaverCafeCommonErrorData {
                target: None,
                http_status: None,
                api_error_code: None,
                api_error_message: None,
                retryable: false,
            },
        )),
    }
}

/// reqwest 전송 오류를 [`CommentError`]로 변환한다. 쿠키 값은 포함하지 않는다.
fn transport_error(
    article_id: &str,
    ref_comment_id: Option<&str>,
    err: reqwest::Error,
) -> CommentError {
    let retryable = err.is_timeout() || err.is_connect();
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_HTTP_TRANSPORT_ERROR.to_string(),
        message: format!("HTTP 전송 오류가 발생했습니다: {err}"),
        error_data: Some(error_data(
            article_id,
            ref_comment_id,
            NaverCafeCommonErrorData {
                target: None,
                http_status: None,
                api_error_code: None,
                api_error_message: None,
                retryable,
            },
        )),
    }
}

/// non-2xx 응답 시 [`CommentError`]를 생성한다.
///
/// 실측 댓글 실패 스키마([`CommentApiFailure`])로 파싱을 시도한다:
/// - 성공: `api_error_code`(errorCode), `api_error_message`(reason)을 채운다.
/// - 실패: 원본 바디(최대 2000자)를 `api_error_message`에 담는 폴백.
///
/// 쿠키/세션 값은 절대 포함되지 않는다.
fn make_non_2xx_error(
    status: u16,
    raw_body: String,
    retryable: bool,
    article_id: &str,
    ref_comment_id: Option<&str>,
) -> CommentError {
    if let Some(failure) = CommentApiFailure::parse(&raw_body) {
        return ErrorEnvelope {
            trace_id: String::new(),
            code: CODE_COMMENT_HTTP_ERROR.to_string(),
            message: "댓글 등록 요청이 실패했습니다. 상세는 errorData.cafe를 확인하세요."
                .to_string(),
            error_data: Some(error_data(
                article_id,
                ref_comment_id,
                NaverCafeCommonErrorData {
                    target: None,
                    http_status: Some(status),
                    api_error_code: Some(failure.error_code),
                    api_error_message: Some(failure.reason),
                    retryable,
                },
            )),
        };
    }

    // 알 수 없는 형태 폴백: 원본 바디 보존
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_COMMENT_HTTP_ERROR.to_string(),
        message: "댓글 등록 요청이 실패했습니다. 응답 원문은 errorData.cafe.apiErrorMessage를 확인하세요."
            .to_string(),
        error_data: Some(error_data(
            article_id,
            ref_comment_id,
            NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status),
                api_error_code: None,
                api_error_message: Some(truncate_body(raw_body)),
                retryable,
            },
        )),
    }
}

/// 2xx이지만 파싱 실패 시 [`CommentError`]를 생성한다. 원본 바디를 보존한다.
fn make_parse_error(
    status: u16,
    raw_body: String,
    article_id: &str,
    ref_comment_id: Option<&str>,
) -> CommentError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_COMMENT_PARSE_ERROR.to_string(),
        message: "댓글 등록 응답을 파싱하지 못했습니다. 원문은 errorData.cafe.apiErrorMessage를 확인하세요."
            .to_string(),
        error_data: Some(error_data(
            article_id,
            ref_comment_id,
            NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status),
                api_error_code: None,
                api_error_message: Some(truncate_body(raw_body)),
                retryable: false,
            },
        )),
    }
}

// ---------------------------------------------------------------------------
// HTTP 클라이언트
// ---------------------------------------------------------------------------

/// 네이버 카페 댓글/대댓글 등록 HTTP 클라이언트.
///
/// 테스트에서는 [`CafeCommentClient::with_base_url`]로 wiremock 등의 목 서버를 주입한다.
pub struct CafeCommentClient {
    base_url: String,
    http: reqwest::Client,
}

impl CafeCommentClient {
    /// 기본 URL(`https://{API_HOST}`)을 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(format!("https://{API_HOST}"))
    }

    /// 주입된 `base_url`을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// 일반 댓글을 등록한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러 메시지나 로그에 절대 노출하지 않는다.
    pub async fn post_comment(
        &self,
        request: &CommentRequest,
        cookie_header: Option<&str>,
    ) -> Result<CommentResult, CommentError> {
        let body = encode_form(&build_comment_form(request))
            .map_err(|e| form_build_error(&request.article_id, None, e))?;
        self.send(
            comment_post_path(),
            &request.cafe_id,
            &request.article_id,
            None,
            cookie_header,
            body,
        )
        .await
    }

    /// 대댓글(답글)을 등록한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header` 값은 에러/로그에 절대 노출되지 않는다.
    pub async fn post_reply(
        &self,
        request: &ReplyRequest,
        cookie_header: Option<&str>,
    ) -> Result<CommentResult, CommentError> {
        let body = encode_form(&build_reply_form(request)).map_err(|e| {
            form_build_error(&request.article_id, Some(&request.ref_comment_id), e)
        })?;
        self.send(
            comment_reply_path(),
            &request.cafe_id,
            &request.article_id,
            Some(&request.ref_comment_id),
            cookie_header,
            body,
        )
        .await
    }

    /// 공통 전송 경로 — form 바디를 POST하고 응답을 [`CommentResult`]로 파싱한다.
    async fn send(
        &self,
        path: &str,
        cafe_id: &str,
        article_id: &str,
        ref_comment_id: Option<&str>,
        cookie_header: Option<&str>,
        form_body: String,
    ) -> Result<CommentResult, CommentError> {
        let url = format!("{}{}", self.base_url, path);

        let mut req = self.http.post(&url);
        for (name, value) in comment_headers(cafe_id, article_id) {
            req = req.header(&name, &value);
        }
        // reqwest 기본값 대신 브라우저 User-Agent를 사용한다.
        req = req.header("User-Agent", BROWSER_USER_AGENT);
        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(cookie) = cookie_header {
            req = req.header("Cookie", cookie);
        }

        let response = req
            .body(form_body)
            .send()
            .await
            .map_err(|e| transport_error(article_id, ref_comment_id, e))?;

        let status = response.status();
        let status_code = status.as_u16();
        let raw_body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            let retryable = status_code >= 500;
            return Err(make_non_2xx_error(
                status_code,
                raw_body,
                retryable,
                article_id,
                ref_comment_id,
            ));
        }

        parse_comment_result(&raw_body)
            .map_err(|_| make_parse_error(status_code, raw_body, article_id, ref_comment_id))
    }
}

impl Default for CafeCommentClient {
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

    fn comment_req() -> CommentRequest {
        CommentRequest {
            cafe_id: cafe_id().to_string(),
            article_id: "9".to_string(),
            content: "안녕하세요".to_string(),
            sticker_id: None,
        }
    }

    fn reply_req() -> ReplyRequest {
        ReplyRequest {
            cafe_id: cafe_id().to_string(),
            article_id: "9".to_string(),
            content: "답글입니다".to_string(),
            sticker_id: None,
            ref_comment_id: "62628988".to_string(),
        }
    }

    // 실측 캡처된 댓글 실패 응답(없는 게시글, HTTP 404).
    const REAL_FAILURE: &str = r#"{"errorCode":"4003","reason":"삭제되었거나 존재하지 않는 게시글입니다.","more":{"cafeUrl":"bluegrayoc3uc","cafeName":"test64979381","pcCafeName":"test64979381","cafeId":31732304}}"#;

    // ------------------------------------------------------------------
    // 성공 케이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_comment_success_returns_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(comment_post_path()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"commentId": 62628988_u64, "refCommentId": 62628988_u64})),
            )
            .mount(&server)
            .await;

        let client = CafeCommentClient::with_base_url(server.uri());
        let result = client
            .post_comment(&comment_req(), None)
            .await
            .expect("성공 응답이어야 함");

        assert_eq!(result.comment_id, 62628988);
        assert_eq!(result.ref_comment_id, 62628988);
        assert!(!result.is_reply(), "원댓글은 is_reply=false");
    }

    #[tokio::test]
    async fn post_reply_success_returns_distinct_ids() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(comment_reply_path()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"commentId": 62628990_u64, "refCommentId": 62628988_u64})),
            )
            .mount(&server)
            .await;

        let client = CafeCommentClient::with_base_url(server.uri());
        let result = client
            .post_reply(&reply_req(), None)
            .await
            .expect("성공 응답이어야 함");

        assert_eq!(result.comment_id, 62628990);
        assert_eq!(result.ref_comment_id, 62628988);
        assert!(result.is_reply(), "대댓글은 is_reply=true");
    }

    // ------------------------------------------------------------------
    // 요청 형태 검증
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_comment_sends_form_content_type_and_origin() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(comment_post_path()))
            .and(header("Content-Type", "application/x-www-form-urlencoded"))
            .and(header("Origin", "https://cafe.naver.com"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"commentId": 1_u64, "refCommentId": 1_u64})),
            )
            .mount(&server)
            .await;

        let client = CafeCommentClient::with_base_url(server.uri());
        client
            .post_comment(&comment_req(), None)
            .await
            .expect("헤더 조건 충족 시 성공해야 함");
    }

    #[tokio::test]
    async fn post_comment_sends_cookie_when_provided() {
        let server = MockServer::start().await;
        // 테스트용 가짜 쿠키 (실제 인증 값 아님)
        let fake_cookie = "NID_AUT=FAKE_FOR_TEST; NID_SES=FAKE_FOR_TEST";
        Mock::given(method("POST"))
            .and(path(comment_post_path()))
            .and(header_exists("Cookie"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"commentId": 1_u64, "refCommentId": 1_u64})),
            )
            .mount(&server)
            .await;

        let client = CafeCommentClient::with_base_url(server.uri());
        client
            .post_comment(&comment_req(), Some(fake_cookie))
            .await
            .expect("Cookie 헤더 존재 시 성공해야 함");
    }

    // ------------------------------------------------------------------
    // 실패 케이스 — 실측 스키마 (errorCode/reason)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_comment_404_real_failure_parses_error_code_and_reason() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(comment_post_path()))
            .respond_with(ResponseTemplate::new(404).set_body_string(REAL_FAILURE))
            .mount(&server)
            .await;

        let client = CafeCommentClient::with_base_url(server.uri());
        let err = client
            .post_comment(&comment_req(), None)
            .await
            .expect_err("404는 Err여야 함");

        assert_eq!(err.code, CODE_COMMENT_HTTP_ERROR);
        let data = err.error_data.expect("errorData가 없음");
        assert_eq!(data.article_id.as_deref(), Some("9"));
        let cafe = &data.cafe;
        assert_eq!(cafe.http_status, Some(404));
        assert!(!cafe.retryable, "404는 재시도 불가");
        assert_eq!(cafe.api_error_code.as_deref(), Some("4003"));
        assert_eq!(
            cafe.api_error_message.as_deref(),
            Some("삭제되었거나 존재하지 않는 게시글입니다.")
        );
    }

    #[tokio::test]
    async fn post_comment_500_is_retryable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(comment_post_path()))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let client = CafeCommentClient::with_base_url(server.uri());
        let err = client
            .post_comment(&comment_req(), None)
            .await
            .expect_err("500은 Err여야 함");

        let cafe = &err.error_data.expect("errorData가 없음").cafe;
        assert!(cafe.retryable, "500은 재시도 가능");
        assert_eq!(cafe.http_status, Some(500));
    }

    // ------------------------------------------------------------------
    // 2xx 파싱 실패
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_comment_200_unparseable_returns_parse_error() {
        let server = MockServer::start().await;
        let raw = r#"{"unexpected":true}"#;
        Mock::given(method("POST"))
            .and(path(comment_post_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string(raw))
            .mount(&server)
            .await;

        let client = CafeCommentClient::with_base_url(server.uri());
        let err = client
            .post_comment(&comment_req(), None)
            .await
            .expect_err("파싱 불가 응답은 Err여야 함");

        assert_eq!(err.code, CODE_COMMENT_PARSE_ERROR);
        let captured = err
            .error_data
            .expect("errorData가 없음")
            .cafe
            .api_error_message
            .expect("api_error_message가 없음");
        assert!(captured.contains(r#"{"unexpected":true}"#), "원본 바디 보존: {captured}");
    }
}
