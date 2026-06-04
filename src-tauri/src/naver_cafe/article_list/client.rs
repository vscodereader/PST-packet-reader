//! 카페 게시글 목록(최신글/인기글) 조회 HTTP 클라이언트.
//!
//! reqwest를 사용해 `apis.naver.com`에서 카페의 게시글 목록을 조회한다.
//! 정렬 기준([`SortBy`])으로 최신글/인기글을 구분한다. 테스트에서는
//! [`ArticleListClient::with_base_url`]로 wiremock 서버를 주입할 수 있다.
//!
//! ⚠️ 미확인 엔드포인트: 게시글 목록 API의 실제 패킷 캡처가 없어
//! ([[#96]] 구현 시점) 경로/파라미터를 형제 모듈(menu)과 네이버 카페
//! 공개 article-list API 관례로 추정했다. 실제 응답으로 검증 후 확정할 것.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을
//! 로그, 에러 메시지, `Debug` 출력에 절대 포함하지 않는다.

use crate::naver_cafe::article_list::models::{ArticleListError, ArticleListResponse, SortBy};
use crate::naver_cafe::article_list::parser::parse_article_list_body;
use crate::naver_cafe::error::{http_error_envelope, ErrorEnvelope, NaverCafeCommonErrorData};
use crate::naver_cafe::post::BROWSER_USER_AGENT;

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

/// 게시글 목록 API 호스트.
pub const ARTICLE_LIST_API_HOST: &str = "apis.naver.com";

/// 한 페이지당 게시글 수(기본값).
const DEFAULT_PAGE_SIZE: u32 = 20;

// ---------------------------------------------------------------------------
// 경로 헬퍼
// ---------------------------------------------------------------------------

/// 게시글 목록 API 경로(쿼리 포함)를 반환한다.
///
/// ⚠️ 미확인: 경로/파라미터는 추정값이다(모듈 doc 참조). `menuId=0`은
/// 전체 게시판을 의미하는 관례를 따른다.
pub fn article_list_path(cafe_id: &str, sort_by: SortBy, page: u32) -> String {
    format!(
        "/cafe-web/cafe-articleapi/v2.1/cafes/{}/menus/0/articles?page={}&pageSize={}&sortBy={}",
        cafe_id,
        page,
        DEFAULT_PAGE_SIZE,
        sort_by.as_query_value()
    )
}

/// non-2xx 응답 시 `ArticleListError`를 생성한다(쿠키/세션 값은 절대 포함하지 않는다).
fn make_http_error(status: u16, raw_body: String) -> ArticleListError {
    http_error_envelope(
        status,
        raw_body,
        "ARTICLE_LIST_HTTP_ERROR",
        "게시글 목록 조회 요청이 실패했습니다.",
    )
}

// ---------------------------------------------------------------------------
// HTTP 클라이언트
// ---------------------------------------------------------------------------

/// 카페 게시글 목록(최신글/인기글) 조회 HTTP 클라이언트.
///
/// 테스트에서는 [`ArticleListClient::with_base_url`]로 wiremock 등의 목 서버를 주입한다.
pub struct ArticleListClient {
    base_url: String,
    http: reqwest::Client,
}

impl ArticleListClient {
    /// 기본 URL(`https://{ARTICLE_LIST_API_HOST}`)을 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(format!("https://{}", ARTICLE_LIST_API_HOST))
    }

    /// 주입된 `base_url`을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// 카페의 게시글 목록을 정렬 기준에 따라 조회한다(첫 페이지).
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러 메시지나 로그에 절대 노출하지 않는다.
    ///
    /// # 실패 처리
    /// - Transport 오류 → `ARTICLE_LIST_TRANSPORT_ERROR`
    /// - non-2xx → `ARTICLE_LIST_HTTP_ERROR` (api_error_code/message/trace_id 파싱 시도)
    /// - 2xx + `{"error":{...}}` 형태 → `ARTICLE_LIST_API_ERROR`
    /// - 2xx + 파싱 불가 → `ARTICLE_LIST_PARSE_ERROR`
    pub async fn fetch_article_list(
        &self,
        cafe_id: &str,
        sort_by: SortBy,
        cookie_header: Option<&str>,
    ) -> Result<ArticleListResponse, ArticleListError> {
        let path = article_list_path(cafe_id, sort_by, 1);
        let url = format!("{}{}", self.base_url, path);

        let mut req = self
            .http
            .get(&url)
            .header("Accept", "application/json, text/plain, */*")
            .header("User-Agent", BROWSER_USER_AGENT);

        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(cookie) = cookie_header {
            req = req.header("Cookie", cookie);
        }

        let response = req.send().await.map_err(|e| {
            let retryable = e.is_timeout() || e.is_connect();
            tracing::warn!(error = %e, "게시글 목록 조회 전송 오류");
            ErrorEnvelope {
                trace_id: String::new(),
                code: "ARTICLE_LIST_TRANSPORT_ERROR".to_string(),
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
        // 본문 읽기 실패는 일시적 네트워크 오류일 수 있다 — 빈 문자열로 삼키면
        // 재시도 불가한 PARSE_ERROR로 둔갑하므로 전송 오류로 보존한다.
        let raw_body = response.text().await.map_err(|e| {
            let retryable = e.is_timeout() || e.is_connect();
            tracing::warn!(error = %e, "게시글 목록 응답 본문 읽기 오류");
            ErrorEnvelope {
                trace_id: String::new(),
                code: "ARTICLE_LIST_TRANSPORT_ERROR".to_string(),
                message: format!("HTTP 응답 본문을 읽지 못했습니다: {}", e),
                error_data: Some(NaverCafeCommonErrorData {
                    target: None,
                    http_status: Some(status_code),
                    api_error_code: None,
                    api_error_message: None,
                    retryable,
                }),
            }
        })?;

        if !status.is_success() {
            tracing::warn!(status = status_code, "게시글 목록 조회 HTTP 오류");
            return Err(make_http_error(status_code, raw_body));
        }

        let response = parse_article_list_body(status_code, raw_body)?;
        tracing::debug!(
            count = response.articles.len(),
            sort_by = sort_by.as_query_value(),
            "게시글 목록 조회 완료"
        );
        Ok(response)
    }
}

impl Default for ArticleListClient {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, header_exists, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const ASSUMED_FIXTURE: &str = include_str!("fixtures/article_list_success.assumed.json");

    fn cafe_id() -> &'static str {
        "31732304"
    }

    fn expected_path() -> &'static str {
        "/cafe-web/cafe-articleapi/v2.1/cafes/31732304/menus/0/articles"
    }

    // ------------------------------------------------------------------
    // 성공 케이스 — 추정 fixture 응답
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_article_list_success_returns_articles() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(expected_path()))
            .and(query_param("sortBy", "TIME"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ASSUMED_FIXTURE))
            .mount(&server)
            .await;

        let client = ArticleListClient::with_base_url(server.uri());
        let response = client
            .fetch_article_list(cafe_id(), SortBy::Latest, None)
            .await
            .expect("성공 응답이어야 함");

        assert_eq!(response.articles.len(), 2, "게시글 2건이 반환되어야 함");
        assert!(response.last_page);
        assert_eq!(response.articles[0].article_id, 1024);
        assert_eq!(response.articles[0].subject, "오늘의 공지사항입니다");
    }

    // ------------------------------------------------------------------
    // 정렬 기준이 sortBy 쿼리로 매핑되는지 검증 (인기글)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_article_list_popular_sends_like_sort_query() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(expected_path()))
            .and(query_param("sortBy", "LIKE"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ASSUMED_FIXTURE))
            .mount(&server)
            .await;

        let client = ArticleListClient::with_base_url(server.uri());
        client
            .fetch_article_list(cafe_id(), SortBy::Popular, None)
            .await
            .expect("인기글 정렬 매칭 성공해야 함");
    }

    // ------------------------------------------------------------------
    // 요청 헤더/쿠키 검증
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_article_list_sends_required_headers_and_cookie() {
        let server = MockServer::start().await;
        // 테스트용 가짜 쿠키 값 (실제 쿠키 아님)
        let fake_cookie = "NID_AUT=FAKE_TEST_VALUE; NID_SES=FAKE_SES_VALUE";
        Mock::given(method("GET"))
            .and(path(expected_path()))
            .and(header_exists("User-Agent"))
            .and(header("Cookie", fake_cookie))
            .respond_with(ResponseTemplate::new(200).set_body_string(ASSUMED_FIXTURE))
            .mount(&server)
            .await;

        let client = ArticleListClient::with_base_url(server.uri());
        client
            .fetch_article_list(cafe_id(), SortBy::Latest, Some(fake_cookie))
            .await
            .expect("헤더/쿠키 매칭 성공해야 함");
    }

    // ------------------------------------------------------------------
    // non-2xx 오류 — 실측 캡처된 실제 오류 스키마
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_article_list_500_real_error_body_returns_http_error() {
        let server = MockServer::start().await;
        let real_error_body = r#"{"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"cf4ee2db355d4584b6e0add8f8743048"}}}"#;
        Mock::given(method("GET"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(500).set_body_string(real_error_body))
            .mount(&server)
            .await;

        let client = ArticleListClient::with_base_url(server.uri());
        let err = client
            .fetch_article_list(cafe_id(), SortBy::Latest, None)
            .await
            .expect_err("500은 Err여야 함");

        assert_eq!(err.code, "ARTICLE_LIST_HTTP_ERROR");
        assert_eq!(err.trace_id, "cf4ee2db355d4584b6e0add8f8743048");
        let data = err.error_data.expect("errorData가 없음");
        assert_eq!(data.http_status, Some(500));
        assert!(data.retryable, "500은 재시도 가능이어야 함");
        assert_eq!(data.api_error_code.as_deref(), Some("10404"));
    }

    // ------------------------------------------------------------------
    // 200 + {"error":{...}} — 200-with-error 케이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_article_list_200_with_error_body_returns_api_error() {
        let server = MockServer::start().await;
        let error_body = r#"{"error":{"errorCode":"40004","message":"카페를 찾을 수 없습니다"}}"#;
        Mock::given(method("GET"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string(error_body))
            .mount(&server)
            .await;

        let client = ArticleListClient::with_base_url(server.uri());
        let err = client
            .fetch_article_list(cafe_id(), SortBy::Latest, None)
            .await
            .expect_err("200-with-error는 Err여야 함");

        assert_eq!(err.code, "ARTICLE_LIST_API_ERROR");
        let data = err.error_data.expect("errorData가 없음");
        assert_eq!(data.api_error_code.as_deref(), Some("40004"));
    }

    // ------------------------------------------------------------------
    // 2xx + 파싱 불가 — 파싱 실패 케이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_article_list_unparseable_2xx_returns_parse_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(expected_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .mount(&server)
            .await;

        let client = ArticleListClient::with_base_url(server.uri());
        let err = client
            .fetch_article_list(cafe_id(), SortBy::Latest, None)
            .await
            .expect_err("파싱불가는 Err여야 함");
        assert_eq!(err.code, "ARTICLE_LIST_PARSE_ERROR");
    }

    // ------------------------------------------------------------------
    // 전송 오류 — 서버 다운
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_article_list_transport_error_when_server_down() {
        // 존재하지 않는 포트로 전송 → 연결 오류
        let client = ArticleListClient::with_base_url("http://127.0.0.1:1");
        let err = client
            .fetch_article_list(cafe_id(), SortBy::Latest, None)
            .await
            .expect_err("전송오류는 Err여야 함");
        assert_eq!(err.code, "ARTICLE_LIST_TRANSPORT_ERROR");
    }
}
