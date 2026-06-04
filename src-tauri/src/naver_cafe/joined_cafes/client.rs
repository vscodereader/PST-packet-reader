//! 가입 카페 목록 조회 HTTP 클라이언트.
//!
//! `apis.naver.com/cafe-home-web/cafe-home/v1/config/join-cafes/groups`("내 카페
//! 관리 > 가입 카페" 화면 API)를 `page=1`부터 `pageInfo.lastPage`까지 순회해
//! 현재 로그인 계정의 가입 카페를 모두 모은다. 테스트에서는
//! [`JoinedCafesClient::with_base_url`]로 wiremock 서버를 주입한다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을
//! 로그, 에러 메시지, `Debug` 출력에 절대 포함하지 않는다.

use crate::naver_cafe::error::{http_error_envelope, ErrorEnvelope, NaverCafeCommonErrorData};
use crate::naver_cafe::joined_cafes::models::{JoinCafesEnvelope, JoinedCafe, JoinedCafesError};
use crate::naver_cafe::post::BROWSER_USER_AGENT;
use crate::naver_cafe::response::{truncate_body, NaverApiErrorBody};

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

/// 가입 카페 목록 API 호스트.
pub const JOINED_CAFES_HOST: &str = "apis.naver.com";

/// 페이지 순회 안전 상한(무한 루프 방지).
const MAX_PAGES: u32 = 50;

// ---------------------------------------------------------------------------
// 경로 헬퍼
// ---------------------------------------------------------------------------

/// 가입 카페 목록 API 경로(`page` 쿼리 포함)를 반환한다.
pub fn join_cafes_path(page: u32) -> String {
    format!(
        "/cafe-home-web/cafe-home/v1/config/join-cafes/groups/?page={}",
        page
    )
}

/// non-2xx 응답 시 오류를 생성한다(쿠키/세션 값은 절대 포함하지 않는다).
fn make_http_error(status: u16, raw_body: String) -> JoinedCafesError {
    http_error_envelope(
        status,
        raw_body,
        "JOINED_CAFES_HTTP_ERROR",
        "가입 카페 목록 조회 요청이 실패했습니다.",
    )
}

// ---------------------------------------------------------------------------
// HTTP 클라이언트
// ---------------------------------------------------------------------------

/// 가입 카페 목록 조회 HTTP 클라이언트.
pub struct JoinedCafesClient {
    base_url: String,
    http: reqwest::Client,
}

impl JoinedCafesClient {
    /// 기본 URL(`https://{JOINED_CAFES_HOST}`)을 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(format!("https://{}", JOINED_CAFES_HOST))
    }

    /// 주입된 `base_url`을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// 현재 로그인 계정의 가입 카페를 모든 페이지에 걸쳐 조회한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러/로그에 절대 노출하지 않는다.
    ///
    /// # 실패 처리
    /// - Transport 오류 → `JOINED_CAFES_TRANSPORT_ERROR`
    /// - non-2xx → `JOINED_CAFES_HTTP_ERROR`
    /// - 2xx + `{"error":{...}}` → `JOINED_CAFES_API_ERROR`
    /// - 2xx + 파싱 불가 → `JOINED_CAFES_PARSE_ERROR`
    pub async fn fetch_joined_cafes(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<Vec<JoinedCafe>, JoinedCafesError> {
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let (mut cafes, last_page) = self.fetch_page(page, cookie_header).await?;
            tracing::debug!(
                page,
                fetched = cafes.len(),
                last_page,
                "가입 카페 페이지 조회"
            );
            all.append(&mut cafes);

            if last_page || page >= MAX_PAGES {
                if !last_page {
                    tracing::warn!(
                        max_pages = MAX_PAGES,
                        "가입 카페 페이지 상한 도달 — 목록이 잘렸을 수 있음"
                    );
                }
                break;
            }
            page += 1;
        }
        tracing::info!(count = all.len(), "가입 카페 목록 조회 완료");
        Ok(all)
    }

    /// 단일 페이지를 조회해 `(카페 목록, 마지막 페이지 여부)`를 반환한다.
    async fn fetch_page(
        &self,
        page: u32,
        cookie_header: Option<&str>,
    ) -> Result<(Vec<JoinedCafe>, bool), JoinedCafesError> {
        let url = format!("{}{}", self.base_url, join_cafes_path(page));

        let mut req = self
            .http
            .get(&url)
            .header("Accept", "application/json, text/plain, */*")
            .header("x-cafe-product", "pc")
            .header("Origin", "https://section.cafe.naver.com")
            .header(
                "Referer",
                "https://section.cafe.naver.com/ca-fe/home/manage-my-cafe/join",
            )
            .header("User-Agent", BROWSER_USER_AGENT);

        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(cookie) = cookie_header {
            req = req.header("Cookie", cookie);
        }

        let response = req.send().await.map_err(|e| {
            let retryable = e.is_timeout() || e.is_connect();
            tracing::warn!(error = %e, "가입 카페 조회 전송 오류");
            ErrorEnvelope {
                trace_id: String::new(),
                code: "JOINED_CAFES_TRANSPORT_ERROR".to_string(),
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
            tracing::warn!(status = status_code, "가입 카페 조회 HTTP 오류");
            return Err(make_http_error(status_code, raw_body));
        }

        // 2xx — 성공 봉투 먼저 시도 (content-type 무시: 일부 경로가 text/html이어도 바디는 JSON).
        if let Ok(envelope) = serde_json::from_str::<JoinCafesEnvelope>(&raw_body) {
            let last_page = envelope.is_last_page();
            return Ok((envelope.into_cafes(), last_page));
        }

        // 2xx인데 {"error":{...}} 형태 (200-with-error)
        if let Some(error_body) = NaverApiErrorBody::parse(&raw_body) {
            let trace_id = error_body
                .error
                .more
                .as_ref()
                .and_then(|m| m.request_id.clone())
                .unwrap_or_default();
            tracing::warn!(api_error_code = %error_body.error.error_code, "가입 카페 조회 API 오류");
            return Err(ErrorEnvelope {
                trace_id,
                code: "JOINED_CAFES_API_ERROR".to_string(),
                message: "가입 카페 목록 API가 오류를 반환했습니다.".to_string(),
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
        tracing::warn!(status = status_code, "가입 카페 응답 파싱 실패");
        Err(ErrorEnvelope {
            trace_id: String::new(),
            code: "JOINED_CAFES_PARSE_ERROR".to_string(),
            message: "가입 카페 목록 응답을 파싱하지 못했습니다.".to_string(),
            error_data: Some(NaverCafeCommonErrorData {
                target: None,
                http_status: Some(status_code),
                api_error_code: None,
                api_error_message: Some(truncate_body(raw_body)),
                retryable: false,
            }),
        })
    }
}

impl Default for JoinedCafesClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const REAL_FIXTURE: &str = include_str!("fixtures/join_cafes_groups_success.json");

    fn page2_empty_last() -> &'static str {
        // page=2 응답: 빈 그룹 + lastPage=true (페이지 루프 종료 검증용)
        r#"{"message":{"status":"200","error":{"code":"","msg":""},"result":{"groups":[],"pageInfo":{"page":2,"perPage":15,"totalCount":3,"lastPage":true}}}}"#
    }

    fn page1_not_last() -> String {
        // REAL_FIXTURE에서 lastPage만 false로 바꿔 다음 페이지를 강제한다.
        REAL_FIXTURE.replace("\"lastPage\": true", "\"lastPage\": false")
    }

    #[tokio::test]
    async fn fetch_joined_cafes_success_returns_flattened_cafes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/cafe-home-web/cafe-home/v1/config/join-cafes/groups/",
            ))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = JoinedCafesClient::with_base_url(server.uri());
        let cafes = client.fetch_joined_cafes(None).await.expect("성공해야 함");

        assert_eq!(cafes.len(), 3);
        assert_eq!(cafes[0].cafe_id, 31732304);
        assert_eq!(cafes[0].cafe_url, "bluegrayoc3uc");
        assert!(cafes[0].managing_cafe);
    }

    #[tokio::test]
    async fn fetch_joined_cafes_sends_required_headers_and_cookie() {
        let server = MockServer::start().await;
        let fake_cookie = "NID_AUT=FAKE; NID_SES=FAKE_SES";
        Mock::given(method("GET"))
            .and(path(
                "/cafe-home-web/cafe-home/v1/config/join-cafes/groups/",
            ))
            .and(header("x-cafe-product", "pc"))
            .and(header("Cookie", fake_cookie))
            .respond_with(ResponseTemplate::new(200).set_body_string(REAL_FIXTURE))
            .mount(&server)
            .await;

        let client = JoinedCafesClient::with_base_url(server.uri());
        client
            .fetch_joined_cafes(Some(fake_cookie))
            .await
            .expect("헤더/쿠키 매칭 성공해야 함");
    }

    #[tokio::test]
    async fn fetch_joined_cafes_follows_pagination_until_last_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/cafe-home-web/cafe-home/v1/config/join-cafes/groups/",
            ))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page1_not_last()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/cafe-home-web/cafe-home/v1/config/join-cafes/groups/",
            ))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page2_empty_last()))
            .mount(&server)
            .await;

        let client = JoinedCafesClient::with_base_url(server.uri());
        let cafes = client.fetch_joined_cafes(None).await.expect("성공해야 함");
        // page1(3건) + page2(0건) = 3건
        assert_eq!(cafes.len(), 3);
    }

    #[tokio::test]
    async fn fetch_joined_cafes_maps_404_to_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let client = JoinedCafesClient::with_base_url(server.uri());
        let err = client
            .fetch_joined_cafes(None)
            .await
            .expect_err("404는 Err여야 함");
        assert_eq!(err.code, "JOINED_CAFES_HTTP_ERROR");
        assert_eq!(err.error_data.unwrap().http_status, Some(404));
    }

    #[tokio::test]
    async fn fetch_joined_cafes_maps_unparseable_2xx_to_parse_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .mount(&server)
            .await;

        let client = JoinedCafesClient::with_base_url(server.uri());
        let err = client
            .fetch_joined_cafes(None)
            .await
            .expect_err("파싱불가는 Err여야 함");
        assert_eq!(err.code, "JOINED_CAFES_PARSE_ERROR");
    }

    #[tokio::test]
    async fn fetch_joined_cafes_transport_error_when_server_down() {
        // 존재하지 않는 포트로 전송 → 연결 오류
        let client = JoinedCafesClient::with_base_url("http://127.0.0.1:1");
        let err = client
            .fetch_joined_cafes(None)
            .await
            .expect_err("전송오류는 Err여야 함");
        assert_eq!(err.code, "JOINED_CAFES_TRANSPORT_ERROR");
    }
}
