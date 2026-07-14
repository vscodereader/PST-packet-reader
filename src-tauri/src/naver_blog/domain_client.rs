//! 네이버 블로그 도메인(블로그명) 확인/추천 HTTP 클라이언트.
//!
//! 블로그명 중복확인은 **봇탐지 토큰이 필요 없는 단순 GET 조회**라 CDP 없이 HTTP로 처리한다
//! (블로그 생성·글 발행은 ncpt 봇탐지 tokenId가 필요해 CDP 브라우저 경로로 간다). 저장된 네이버
//! 세션 쿠키를 실어 `section.blog.naver.com/blogdomain/*` 을 조회한다. 패킷(2026-07-13) 확인:
//! `GET /blogdomain/BlogDomainDuplicateCheck.naver?domainId={명}` → `{"result":true}`(사용가능)
//! / `{"result":false}`(사용중). 추천은 `RecommendBlogDomainList.naver?domainId={명}`.
//!
//! 카페/댓글 클라이언트와 동일하게 base_url을 분리 보관해 실서버/wiremock을 함께 쓴다.

use serde_json::Value;

use super::error::BlogError;

/// 블로그 도메인 조회 호스트.
const SECTION_BLOG_HOST: &str = "https://section.blog.naver.com";

/// 블로그명 확인/추천 HTTP 클라이언트.
pub struct BlogDomainClient {
    base: String,
    http: reqwest::Client,
}

impl Default for BlogDomainClient {
    fn default() -> Self {
        Self::new()
    }
}

impl BlogDomainClient {
    /// 실서버 호스트를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(SECTION_BLOG_HOST)
    }

    /// 주입된 base_url을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// 블로그명 사용 가능 여부를 조회한다. `true`=사용 가능, `false`=이미 사용 중.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn check_availability(
        &self,
        domain_id: &str,
        cookie: Option<&str>,
    ) -> Result<bool, BlogError> {
        let url = format!("{}/blogdomain/BlogDomainDuplicateCheck.naver", self.base);
        let text = self.get_json(&url, domain_id, cookie).await?;
        parse_result_bool(&text).ok_or_else(|| {
            BlogError::new(format!(
                "블로그명 확인 응답을 해석하지 못했습니다: {}",
                snippet(&text)
            ))
        })
    }

    /// 블로그가 없는 계정에 블로그를 자동 생성한다(도메인 등록). 성공 시 `true`.
    ///
    /// 패킷(2026-07-13) 확정: `POST section.blog.naver.com/blogdomain/BlogDomainRegistration.naver`
    /// (application/x-www-form-urlencoded) body `domainId={loginId}&naverId={loginId}&tokenId={생성값}`
    /// → `{"result":true}`(성공). `tokenId`는 클라이언트가 만드는 32바이트 값이라 발행과 동일하게
    /// [`write_client::generate_token_id`]로 생성한다. 실패면 상태·본문 원문 로그를 남기고 에러로 알린다.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn register(
        &self,
        domain_id: &str,
        naver_id: &str,
        cookie: Option<&str>,
    ) -> Result<bool, BlogError> {
        let url = format!("{}/blogdomain/BlogDomainRegistration.naver", self.base);
        let token_id = super::write_client::generate_token_id();
        let form = build_registration_form(domain_id, naver_id, &token_id);
        let mut req = self
            .http
            .post(&url)
            .header("User-Agent", crate::naver_cafe::post::BROWSER_USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", SECTION_BLOG_HOST)
            .header("Referer", "https://section.blog.naver.com/BlogHome.naver")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form);
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BlogError::new(format!("블로그 생성 요청 실패: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BlogError::new(format!("블로그 생성 응답 읽기 실패: {e}")))?;
        // 게시 API 원문 로그 원칙: 상태/본문을 필터 없이 남긴다(쿠키는 본문에 없어 안전).
        tracing::info!(
            "[BLOG] 블로그 생성 응답 — status={} body={}",
            status.as_u16(),
            snippet(&text)
        );
        match parse_result_bool(&text) {
            Some(true) => Ok(true),
            _ => Err(BlogError::new(format!(
                "블로그 자동 생성에 실패했습니다. status={} 응답={}",
                status.as_u16(),
                snippet(&text)
            ))),
        }
    }

    /// 사용 중일 때 대체 블로그명 추천 목록을 조회한다(best-effort — 응답이 비면 빈 목록).
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn recommend(
        &self,
        domain_id: &str,
        cookie: Option<&str>,
    ) -> Result<Vec<String>, BlogError> {
        let url = format!("{}/blogdomain/RecommendBlogDomainList.naver", self.base);
        let text = self.get_json(&url, domain_id, cookie).await?;
        Ok(parse_recommend_list(&text))
    }

    /// `domainId` 쿼리를 붙여 GET하고 본문 텍스트를 돌려준다. 위장 헤더(same-site XHR)를 싣는다.
    async fn get_json(
        &self,
        url: &str,
        domain_id: &str,
        cookie: Option<&str>,
    ) -> Result<String, BlogError> {
        let mut req = self
            .http
            .get(url)
            .query(&[("domainId", domain_id)])
            .header("User-Agent", crate::naver_cafe::post::BROWSER_USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", "https://section.blog.naver.com/BlogHome.naver")
            .header("sec-fetch-site", "same-origin")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-dest", "empty");
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BlogError::new(format!("블로그명 확인 요청 실패: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BlogError::new(format!("블로그명 확인 응답 읽기 실패: {e}")))?;
        // 게시 API 원문 로그 원칙: 상태/본문을 필터 없이 남긴다(쿠키는 본문에 없어 안전).
        tracing::info!(
            "[BLOG] 블로그명 확인 응답 — status={} body={}",
            status.as_u16(),
            snippet(&text)
        );
        Ok(text)
    }
}

/// `{"result":true}` / `{"result":false}` (또는 문자열 "true"/"false")에서 bool을 뽑는다(순수 함수).
fn parse_result_bool(text: &str) -> Option<bool> {
    let value: Value = serde_json::from_str(text).ok()?;
    let result = value.get("result")?;
    match result {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// 추천 응답에서 블로그명 문자열 목록을 best-effort로 추출한다(순수 함수). 배열이 `["a","b"]`든
/// `[{"domainId":"a"},...]`든, 최상위/`result`/`recommendList` 배열에서 문자열·`domainId`를 모은다.
fn parse_recommend_list(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let arr = value
        .as_array()
        .or_else(|| value.get("result").and_then(Value::as_array))
        .or_else(|| value.get("recommendList").and_then(Value::as_array))
        .or_else(|| value.get("domainList").and_then(Value::as_array));
    let Some(arr) = arr else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| match item {
            Value::String(s) => Some(s.clone()),
            Value::Object(_) => item
                .get("domainId")
                .or_else(|| item.get("domain"))
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .collect()
}

/// 블로그 생성(BlogDomainRegistration) 폼 필드를 만든다(순수 함수). `domainId`/`naverId`/`tokenId` 3개.
/// reqwest `.form()`이 url-encoding을 담당하므로 값만 그대로 담는다.
fn build_registration_form(
    domain_id: &str,
    naver_id: &str,
    token_id: &str,
) -> [(&'static str, String); 3] {
    [
        ("domainId", domain_id.to_string()),
        ("naverId", naver_id.to_string()),
        ("tokenId", token_id.to_string()),
    ]
}

/// 로그·에러용 응답 앞부분 스니펫(최대 200자).
fn snippet(text: &str) -> String {
    text.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parses_result_bool_from_json_bool() {
        assert_eq!(parse_result_bool(r#"{"result":true}"#), Some(true));
        assert_eq!(parse_result_bool(r#"{"result":false}"#), Some(false));
    }

    #[test]
    fn parses_result_bool_from_json_string() {
        assert_eq!(parse_result_bool(r#"{"result":"true"}"#), Some(true));
        assert_eq!(parse_result_bool(r#"{"result":"false"}"#), Some(false));
    }

    #[test]
    fn parse_result_bool_none_on_garbage() {
        assert_eq!(parse_result_bool("<html>bot</html>"), None);
        assert_eq!(parse_result_bool(r#"{"other":1}"#), None);
    }

    #[test]
    fn parse_recommend_handles_string_and_object_arrays() {
        assert_eq!(
            parse_recommend_list(r#"["nblog1","nblog2"]"#),
            vec!["nblog1", "nblog2"]
        );
        assert_eq!(
            parse_recommend_list(r#"{"result":[{"domainId":"nblog3"},{"domainId":"nblog4"}]}"#),
            vec!["nblog3", "nblog4"]
        );
        assert!(parse_recommend_list("nonsense").is_empty());
    }

    #[tokio::test]
    async fn check_availability_true_when_result_true() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/blogdomain/BlogDomainDuplicateCheck.naver"))
            .and(query_param("domainId", "nblog4test"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"result":true}"#))
            .mount(&server)
            .await;
        let client = BlogDomainClient::with_base_url(server.uri());
        assert!(client.check_availability("nblog4test", None).await.unwrap());
    }

    #[tokio::test]
    async fn check_availability_false_when_taken() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/blogdomain/BlogDomainDuplicateCheck.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"result":false}"#))
            .mount(&server)
            .await;
        let client = BlogDomainClient::with_base_url(server.uri());
        assert!(!client.check_availability("william", None).await.unwrap());
    }

    #[tokio::test]
    async fn check_availability_sends_cookie_when_provided() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/blogdomain/BlogDomainDuplicateCheck.naver"))
            .and(wiremock::matchers::header("Cookie", "NID_SES=abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"result":true}"#))
            .mount(&server)
            .await;
        let client = BlogDomainClient::with_base_url(server.uri());
        assert!(client
            .check_availability("nblog4test", Some("NID_SES=abc"))
            .await
            .unwrap());
    }

    #[test]
    fn build_registration_form_has_three_fields() {
        let form = build_registration_form("choisw0404", "choisw0404", "TOK-123");
        assert_eq!(form[0], ("domainId", "choisw0404".to_string()));
        assert_eq!(form[1], ("naverId", "choisw0404".to_string()));
        assert_eq!(form[2], ("tokenId", "TOK-123".to_string()));
    }

    #[tokio::test]
    async fn register_true_on_result_true() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/blogdomain/BlogDomainRegistration.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"result":true}"#))
            .mount(&server)
            .await;
        let client = BlogDomainClient::with_base_url(server.uri());
        assert!(client
            .register("choisw0404", "choisw0404", Some("NID_SES=abc"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn register_errors_when_result_false() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/blogdomain/BlogDomainRegistration.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"result":false}"#))
            .mount(&server)
            .await;
        let client = BlogDomainClient::with_base_url(server.uri());
        assert!(client.register("x", "x", None).await.is_err());
    }

    #[tokio::test]
    async fn register_errors_on_bot_html() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/blogdomain/BlogDomainRegistration.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>bot</html>"))
            .mount(&server)
            .await;
        let client = BlogDomainClient::with_base_url(server.uri());
        assert!(client.register("x", "x", None).await.is_err());
    }

    #[tokio::test]
    async fn check_availability_errors_on_bot_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/blogdomain/BlogDomainDuplicateCheck.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>bot block</html>"))
            .mount(&server)
            .await;
        let client = BlogDomainClient::with_base_url(server.uri());
        assert!(client.check_availability("x", None).await.is_err());
    }
}
