//! Vanity 슬러그 → 숫자 cafeId 해석 (카페 홈 HTML 스크레이핑).
//!
//! `cafe.naver.com/<slug>` 형태의 vanity URL은 숫자 cafeId를 직접 담고 있지 않다.
//! 카페 홈 페이지 HTML을 받아 `var g_sClubId = "31732304";` 또는 `clubid=31732304`
//! 패턴에서 숫자 id를 파싱한다 — 별도 JSON API가 아니라 홈 HTML에 박혀 있다(실측 확인).
//!
//! `apis.naver.com`을 쓰는 [`super::client`](CafeGateInfo)와 달리 이 모듈은
//! `cafe.naver.com` 호스트에 GET 요청을 보내고 **HTML 텍스트**를 파싱한다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을
//! 로그, 에러 메시지, `Debug` 출력에 절대 포함하지 않는다.

use crate::naver_cafe::cafe_ref::models::CafeRefError;
use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};
use crate::naver_cafe::post::BROWSER_USER_AGENT;

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

/// 카페 홈 페이지 호스트.
pub const CAFE_HOME_HOST: &str = "cafe.naver.com";

/// `g_sClubId`/`clubid` 마커 뒤에서 숫자를 찾을 때 허용하는 최대 비-숫자 거리(문자).
///
/// `g_sClubId = "31732304"`처럼 마커 직후 곧바로 숫자가 나오는 경우만 허용하고,
/// 멀리 떨어진 무관한 숫자를 잘못 집는 것을 막는다.
const MARKER_DIGIT_WINDOW: usize = 16;

/// 오류 응답 바디 최대 보존 길이(바이트).
const RAW_BODY_MAX_LEN: usize = 2000;

// ---------------------------------------------------------------------------
// 경로 헬퍼
// ---------------------------------------------------------------------------

/// 카페 홈 페이지 경로(`/<slug>`)를 반환한다.
pub fn cafe_home_path(slug: &str) -> String {
    format!("/{}", slug)
}

// ---------------------------------------------------------------------------
// HTML 파싱
// ---------------------------------------------------------------------------

/// 카페 홈 HTML에서 숫자 cafeId(clubId)를 파싱한다.
///
/// 다음 순서로 탐색한다:
/// 1. `g_sClubId` 마커 직후의 숫자 (예: `var g_sClubId = "31732304";`)
/// 2. `clubid` 마커 직후의 숫자 (예: `clubid=31732304`, 대소문자 무관)
///
/// 어느 패턴도 찾지 못하면 `None`을 반환한다.
pub fn parse_club_id_from_html(html: &str) -> Option<u64> {
    if let Some(id) = digits_near_marker(html, "g_sClubId", MARKER_DIGIT_WINDOW) {
        return Some(id);
    }
    // clubid 는 대소문자 변형(ClubId, clubId 등)이 있을 수 있어 소문자로 비교한다.
    let lower = html.to_ascii_lowercase();
    digits_near_marker(&lower, "clubid", MARKER_DIGIT_WINDOW)
}

/// `marker` 등장 위치 이후 `window` 문자 이내에서 시작하는 첫 숫자 런을 u64로 파싱한다.
///
/// 마커 직후 `window`개 문자 안에 숫자가 없으면 `None`을 반환한다(무관한 먼 숫자 방지).
fn digits_near_marker(haystack: &str, marker: &str, window: usize) -> Option<u64> {
    let idx = haystack.find(marker)?;
    let rest = &haystack[idx + marker.len()..];

    let mut buf = String::new();
    for (count, ch) in rest.chars().enumerate() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else if !buf.is_empty() {
            break; // 숫자 런 종료
        } else if count >= window {
            return None; // window 안에 숫자 없음
        }
    }

    if buf.is_empty() {
        None
    } else {
        buf.parse::<u64>().ok()
    }
}

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

fn http_error(code: &str, message: String, status: Option<u16>, retryable: bool) -> CafeRefError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: code.to_string(),
        message,
        error_data: Some(NaverCafeCommonErrorData {
            target: None,
            http_status: status,
            api_error_code: None,
            api_error_message: None,
            retryable,
        }),
    }
}

// ---------------------------------------------------------------------------
// HTTP 클라이언트
// ---------------------------------------------------------------------------

/// 카페 홈 페이지를 받아 vanity 슬러그를 숫자 cafeId로 해석하는 HTTP 클라이언트.
///
/// 테스트에서는 [`CafeHomeClient::with_base_url`]로 wiremock 등의 목 서버를 주입한다.
pub struct CafeHomeClient {
    base_url: String,
    http: reqwest::Client,
}

impl CafeHomeClient {
    /// 기본 URL(`https://{CAFE_HOME_HOST}`)을 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(format!("https://{}", CAFE_HOME_HOST))
    }

    /// 주입된 `base_url`을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// vanity 슬러그(`bluegrayoc3uc` 등)를 숫자 cafeId로 해석한다.
    ///
    /// `GET {base_url}/{slug}` 로 카페 홈 HTML을 받아 [`parse_club_id_from_html`]로
    /// clubId를 추출한다.
    ///
    /// # 쿠키 보안
    /// `cookie_header`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을
    /// 에러 메시지나 로그에 절대 노출하지 않는다.
    ///
    /// # 실패 처리
    /// - Transport 오류 → `CAFE_HOME_TRANSPORT_ERROR`
    /// - non-2xx → `CAFE_HOME_HTTP_ERROR`
    /// - 2xx이지만 HTML에서 clubId를 찾지 못함 → `CAFE_HOME_PARSE_ERROR`
    pub async fn resolve_slug(
        &self,
        slug: &str,
        cookie_header: Option<&str>,
    ) -> Result<u64, CafeRefError> {
        let url = format!("{}{}", self.base_url, cafe_home_path(slug));

        let mut req = self
            .http
            .get(&url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("User-Agent", BROWSER_USER_AGENT);

        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(cookie) = cookie_header {
            req = req.header("Cookie", cookie);
        }

        let response = req.send().await.map_err(|e| {
            let retryable = e.is_timeout() || e.is_connect();
            http_error(
                "CAFE_HOME_TRANSPORT_ERROR",
                format!("HTTP 전송 오류가 발생했습니다: {}", e),
                None,
                retryable,
            )
        })?;

        let status = response.status();
        let status_code = status.as_u16();
        let raw_body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(http_error(
                "CAFE_HOME_HTTP_ERROR",
                "카페 홈 페이지 요청이 실패했습니다.".to_string(),
                Some(status_code),
                status_code >= 500,
            ));
        }

        parse_club_id_from_html(&raw_body).ok_or_else(|| ErrorEnvelope {
            trace_id: String::new(),
            code: "CAFE_HOME_PARSE_ERROR".to_string(),
            message: "카페 홈 HTML에서 cafeId(clubId)를 찾지 못했습니다.".to_string(),
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

impl Default for CafeHomeClient {
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
        matchers::{header, header_exists, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    // ------------------------------------------------------------------
    // parse_club_id_from_html — 패턴별 단위 테스트
    // ------------------------------------------------------------------

    #[test]
    fn parses_g_s_club_id_double_quoted() {
        let html = r#"<script>var g_sClubId = "31732304"; var x = 1;</script>"#;
        assert_eq!(parse_club_id_from_html(html), Some(31732304));
    }

    #[test]
    fn parses_g_s_club_id_with_extra_spacing() {
        let html = r#"g_sClubId    =    "42";"#;
        assert_eq!(parse_club_id_from_html(html), Some(42));
    }

    #[test]
    fn parses_clubid_query_param_form() {
        let html = r#"<a href="/ArticleList.nhn?clubid=31732304&menuid=1">목록</a>"#;
        assert_eq!(parse_club_id_from_html(html), Some(31732304));
    }

    #[test]
    fn g_s_club_id_takes_precedence_over_clubid() {
        // 두 패턴이 모두 있으면 g_sClubId 우선
        let html = r#"clubid=99999 ... var g_sClubId = "31732304";"#;
        assert_eq!(parse_club_id_from_html(html), Some(31732304));
    }

    #[test]
    fn case_insensitive_clubid_marker() {
        let html = r#"<meta data-ClubId="555">"#;
        assert_eq!(parse_club_id_from_html(html), Some(555));
    }

    #[test]
    fn returns_none_when_no_marker() {
        let html = r#"<html><body>카페가 없습니다</body></html>"#;
        assert_eq!(parse_club_id_from_html(html), None);
    }

    #[test]
    fn returns_none_when_marker_has_no_nearby_digits() {
        // g_sClubId 직후 window 안에 숫자가 없음
        let html = r#"var g_sClubId = "";  // 한참 뒤에야 12345 등장"#;
        assert_eq!(parse_club_id_from_html(html), None);
    }

    #[test]
    fn parses_realistic_inline_script() {
        let html = r#"
            <script type="text/javascript">
                var g_sClubId = "31732304";
                var g_sCafeUrlOnly = "bluegrayoc3uc";
            </script>
        "#;
        assert_eq!(parse_club_id_from_html(html), Some(31732304));
    }

    // ------------------------------------------------------------------
    // 경로 헬퍼
    // ------------------------------------------------------------------

    #[test]
    fn cafe_home_path_formats_slug() {
        assert_eq!(cafe_home_path("bluegrayoc3uc"), "/bluegrayoc3uc");
    }

    // ------------------------------------------------------------------
    // resolve_slug — 성공
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn resolve_slug_success_returns_club_id() {
        let server = MockServer::start().await;

        let html = r#"<script>var g_sClubId = "31732304";</script>"#;

        Mock::given(method("GET"))
            .and(path("/bluegrayoc3uc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html))
            .mount(&server)
            .await;

        let client = CafeHomeClient::with_base_url(server.uri());
        let id = client
            .resolve_slug("bluegrayoc3uc", None)
            .await
            .expect("성공해야 함");
        assert_eq!(id, 31732304);
    }

    #[tokio::test]
    async fn resolve_slug_sends_browser_user_agent() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/myclub"))
            .and(header_exists("User-Agent"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"var g_sClubId = "7";"#),
            )
            .mount(&server)
            .await;

        let client = CafeHomeClient::with_base_url(server.uri());
        client.resolve_slug("myclub", None).await.expect("성공해야 함");
    }

    #[tokio::test]
    async fn resolve_slug_sends_cookie_header_when_provided() {
        let server = MockServer::start().await;

        // 테스트용 가짜 쿠키 (실제 쿠키 아님)
        let fake_cookie = "NID_AUT=FAKE_TEST; NID_SES=FAKE_SES";

        Mock::given(method("GET"))
            .and(path("/myclub"))
            .and(header("Cookie", fake_cookie))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"g_sClubId = "7";"#),
            )
            .mount(&server)
            .await;

        let client = CafeHomeClient::with_base_url(server.uri());
        client
            .resolve_slug("myclub", Some(fake_cookie))
            .await
            .expect("성공해야 함");
    }

    // ------------------------------------------------------------------
    // resolve_slug — 실패
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn resolve_slug_404_returns_http_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
            .mount(&server)
            .await;

        let client = CafeHomeClient::with_base_url(server.uri());
        let err = client
            .resolve_slug("missing", None)
            .await
            .expect_err("404는 Err여야 함");

        assert_eq!(err.code, "CAFE_HOME_HTTP_ERROR");
        assert_eq!(err.error_data.unwrap().http_status, Some(404));
    }

    #[tokio::test]
    async fn resolve_slug_200_without_club_id_returns_parse_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/noclub"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("<html>clubId 없음</html>"),
            )
            .mount(&server)
            .await;

        let client = CafeHomeClient::with_base_url(server.uri());
        let err = client
            .resolve_slug("noclub", None)
            .await
            .expect_err("clubId 없으면 Err여야 함");

        assert_eq!(err.code, "CAFE_HOME_PARSE_ERROR");
    }
}
