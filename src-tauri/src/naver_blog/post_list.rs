//! 네이버 블로그 글 목록(최신 N개) 조회 HTTP 클라이언트(#279).
//!
//! "최신 N개 글에 댓글" 모드용이다. 카페 최신글 목록([`crate::naver_cafe::article_list`])의
//! 블로그 버전으로, `PostTitleListAsync.naver`(패킷 분석으로 확정된 API — 재발견하지 말 것)를
//! 페이징하며 최신 글의 `logNo`를 모은다:
//!   `GET /PostTitleListAsync.naver?blogId={blogId}&viewdate=&currentPage={page}
//!        &categoryNo={cat}&parentCategoryNo=&countPerPage={n}`
//! 응답은 JSON(gzip은 reqwest가 푼다)이며 `postList`는 **최신 글 우선**(1페이지=최신)으로 온다:
//!   `{"resultCode":"S","totalCount":"54","countPerPage":"5","postList":[{"logNo":"…","title":"…"}, …]}`
//! `totalCount`(문자열)로 카테고리의 전체 글 수를 알 수 있어, 전부 페이징하지 않고 N개만 모은다.
//! `title`은 퍼센트 인코딩된 UTF-8이라 디코드해 보존한다(표시용이며 댓글 대상 식별은 `logNo`).
//!
//! reqwest로 실제 HTTP 요청을 전송한다. 테스트에서는 [`BlogPostListClient::with_base_url`]로
//! wiremock 서버를 주입한다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을 로그, 에러 메시지,
//! `Debug` 출력에 절대 포함하지 않는다.

use super::error::BlogError;
use crate::naver_cafe::post::BROWSER_USER_AGENT;

/// 블로그 글 목록 API 호스트.
const BLOG_HOST: &str = "https://blog.naver.com";

/// 한 페이지당 조회할 글 수(요청 `countPerPage`). 페이징 종료 판단에도 쓴다.
const PAGE_SIZE: u32 = 30;

/// 페이징 시 조회할 최대 페이지 수(연속 조회로 의심받지 않게 둔 상한). 페이지당 30개이므로
/// 최대 약 300개까지 모은다.
const MAX_PAGES: u32 = 10;

/// 블로그 글 1건(최신 N개 댓글 대상). 댓글 대상 식별은 `log_no`, `title`은 표시 전용이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlogPost {
    /// 글 번호(숫자 문자열). 댓글 대상이 되는 값(예: "224320957761").
    pub log_no: String,
    /// 글 제목(퍼센트 디코드된 UTF-8). 표시 전용.
    pub title: String,
}

/// 블로그 글 목록 조회 결과 — 모은 글(최신 우선)과 카테고리 전체 글 수.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlogPostList {
    /// 최신 우선으로 모은 글(요청한 count까지).
    pub posts: Vec<BlogPost>,
    /// 카테고리의 전체 글 수(`totalCount`). 모자란 글 개수("글이 없습니다") 판단에 쓴다.
    pub total_count: u32,
}

/// 네이버 블로그 글 목록 조회 HTTP 클라이언트. base_url을 분리 보관해 실서버/wiremock을 함께 쓴다.
pub struct BlogPostListClient {
    base_url: String,
    http: reqwest::Client,
}

impl BlogPostListClient {
    /// 실서버 호스트(`blog.naver.com`)를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(BLOG_HOST)
    }

    /// 주입된 `base_url`을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            // 카페와 공용 전역 reqwest 클라이언트를 재사용해 keep-alive 커넥션을 공유한다.
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// `PostTitleListAsync.naver` 한 페이지를 조회해 (글 목록, totalCount)로 파싱한다.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을 에러/로그에 노출하지 않는다.
    async fn fetch_page(
        &self,
        blog_id: &str,
        category_no: u32,
        page: u32,
        cookie: Option<&str>,
    ) -> Result<(Vec<BlogPost>, u32), BlogError> {
        let url = format!(
            "{}/PostTitleListAsync.naver?blogId={}&viewdate=&currentPage={}&categoryNo={}&parentCategoryNo=&countPerPage={}",
            self.base_url, blog_id, page, category_no, PAGE_SIZE
        );
        let mut req = self.http.get(&url).header("User-Agent", BROWSER_USER_AGENT);
        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let response = req
            .send()
            .await
            .map_err(|e| BlogError::new(format!("HTTP 전송 오류가 발생했습니다: {e}")))?;
        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| BlogError::new(format!("응답 본문 읽기 오류: {e}")))?;
        if !status.is_success() {
            return Err(BlogError::new(format!(
                "블로그 글 목록 조회 요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        parse_post_list(&raw)
            .ok_or_else(|| BlogError::new("블로그 글 목록을 해석하지 못했습니다(응답 형식 변경)"))
    }

    /// 최신 글을 `count`개 모일 때까지 페이지를 이어 조회한다(최신 우선, 1페이지=최신).
    ///
    /// 한 페이지가 가득 차지 않거나(PAGE_SIZE 미만) `totalCount`까지 다 모으면 멈추고, 안전상
    /// [`MAX_PAGES`]까지만 조회한다. 첫 페이지 조회가 실패하면 그 오류를 그대로 반환하고, 둘째
    /// 페이지 이후의 실패는 지금까지 모은 글로 진행한다(부분 수집). `total_count`는 첫 페이지의
    /// 값을 보존한다(모자란 글 개수 판단용).
    ///
    /// # 쿠키 보안
    /// `cookie`는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
    pub async fn fetch_latest_posts(
        &self,
        blog_id: &str,
        category_no: u32,
        count: usize,
        cookie: Option<&str>,
    ) -> Result<BlogPostList, BlogError> {
        let mut posts: Vec<BlogPost> = Vec::new();
        let mut total_count: u32 = 0;
        for page in 1..=MAX_PAGES {
            let (fetched, total) = match self
                .fetch_page(blog_id, category_no, page, cookie)
                .await
            {
                Ok(v) => v,
                // 첫 페이지 실패는 댓글 대상이 0개가 되므로 오류로 알린다. 이후 페이지 실패는
                // 부분 수집으로 진행한다(조용히 멈춘다).
                Err(e) if page == 1 => return Err(e),
                Err(_) => break,
            };
            if page == 1 {
                total_count = total;
            }
            let got = fetched.len();
            posts.extend(fetched);
            // 충분히 모았거나, 전체 글 수에 도달했거나, 한 페이지가 가득 차지 않으면(마지막 페이지) 멈춘다.
            if posts.len() >= count
                || posts.len() as u32 >= total_count
                || got < PAGE_SIZE as usize
            {
                break;
            }
        }
        posts.truncate(count);
        Ok(BlogPostList { posts, total_count })
    }
}

impl Default for BlogPostListClient {
    fn default() -> Self {
        Self::new()
    }
}

/// `PostTitleListAsync.naver` 응답 본문을 (글 목록, totalCount)로 파싱한다(순수).
///
/// `totalCount`/`countPerPage`는 문자열로 오고, `title`은 퍼센트 인코딩된 UTF-8이라 디코드한다.
/// JSONP/가드 접두가 붙는 경우를 대비해 첫 `{`부터 파싱한다. `resultCode`가 "S"가 아니어도
/// `postList`만 있으면 그대로 읽는다(에러는 호출부의 HTTP/None 처리에 맡긴다).
fn parse_post_list(body: &str) -> Option<(Vec<BlogPost>, u32)> {
    // 네이버 응답의 `pagingHtml` 등에는 JSON 표준상 무효인 `\'`(백슬래시+작은따옴표) 이스케이프가
    // 섞여 온다. 브라우저/jQuery는 느슨해 통과하지만 serde_json은 엄격해 거부하므로(→ "응답 형식
    // 변경"), 우리가 쓰는 logNo/totalCount엔 영향 없는 `\'`를 `'`로 정리한 뒤 파싱한다.
    let cleaned = json_slice(body).replace("\\'", "'");
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;
    let total_count = json
        .get("totalCount")
        .and_then(parse_count_field)
        .unwrap_or(0);
    let list = json.get("postList")?.as_array()?;
    let posts = list
        .iter()
        .filter_map(|entry| {
            let log_no = entry.get("logNo")?.as_str()?.trim().to_owned();
            if log_no.is_empty() {
                return None;
            }
            let title = entry
                .get("title")
                .and_then(|v| v.as_str())
                .map(decode_title)
                .unwrap_or_default();
            Some(BlogPost { log_no, title })
        })
        .collect();
    Some((posts, total_count))
}

/// `totalCount`/`countPerPage`는 문자열("54") 또는 숫자(54)로 올 수 있어 둘 다 받는다.
fn parse_count_field(v: &serde_json::Value) -> Option<u32> {
    if let Some(s) = v.as_str() {
        return s.trim().parse().ok();
    }
    v.as_u64().map(|n| n as u32)
}

/// 퍼센트 인코딩된 UTF-8 제목을 디코드한다. 디코드 실패(불완전 인코딩)면 원문을 그대로 둔다.
fn decode_title(raw: &str) -> String {
    match urlencoding::decode(raw) {
        Ok(decoded) => decoded.into_owned(),
        Err(_) => raw.to_owned(),
    }
}

/// 응답 앞에 JSONP/가드 접두가 붙는 경우를 대비해 첫 `{`부터의 슬라이스를 돌려준다.
fn json_slice(body: &str) -> &str {
    match body.find('{') {
        Some(i) => &body[i..],
        None => body,
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

    fn blog_id() -> &'static str {
        "press02"
    }

    fn list_path() -> &'static str {
        "/PostTitleListAsync.naver"
    }

    /// 응답 본문을 logNo 목록으로 합성한다(페이징 테스트용). title은 퍼센트 인코딩된 형태.
    fn list_body(total: u32, log_nos: &[u64]) -> String {
        let items: Vec<String> = log_nos
            .iter()
            .map(|n| {
                format!(
                    r#"{{"logNo":"{n}","title":"%EA%B8%80{n}","categoryNo":"1","commentCount":"0","addDate":"2026. 6. 19.","openType":"2"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"resultCode":"S","resultMessage":"","totalCount":"{total}","countPerPage":"30","postList":[{}]}}"#,
            items.join(",")
        )
    }

    // ------------------------------------------------------------------
    // 순수 파서 단위 테스트
    // ------------------------------------------------------------------

    #[test]
    fn parse_post_list_reads_lognos_and_total() {
        let body = list_body(54, &[224320957761, 224320957762]);
        let (posts, total) = parse_post_list(&body).expect("파싱 성공해야 함");
        assert_eq!(total, 54, "totalCount(문자열)를 숫자로 읽어야 함");
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].log_no, "224320957761");
    }

    #[test]
    fn parse_post_list_tolerates_invalid_backslash_quote_escape() {
        // 실측 회귀(#280 후속): 네이버 pagingHtml에 JSON 무효 escape `\'`가 섞여 와도 파싱돼야 한다.
        let body = r#"{"totalCount":"3","postList":[{"logNo":"224320957761","title":"%EA%B8%80"}],"pagingHtml":"<div class=\'blog2_paginate\'><strong class=\'blind\'>페이지</strong></div>"}"#;
        let (posts, total) = parse_post_list(body).expect("무효 escape가 있어도 파싱 성공해야 함");
        assert_eq!(total, 3);
        assert_eq!(posts[0].log_no, "224320957761");
    }

    #[test]
    fn parse_post_list_decodes_percent_encoded_title() {
        // "%ED%85%8C%EC%8A%A4%ED%8A%B8" == "테스트".
        let body = r#"{"totalCount":"1","postList":[{"logNo":"1","title":"%ED%85%8C%EC%8A%A4%ED%8A%B8"}]}"#;
        let (posts, _) = parse_post_list(body).expect("파싱 성공해야 함");
        assert_eq!(posts[0].title, "테스트", "퍼센트 인코딩 제목을 디코드해야 함");
    }

    #[test]
    fn parse_post_list_skips_entries_without_logno() {
        let body = r#"{"totalCount":"2","postList":[{"title":"x"},{"logNo":"7","title":"y"}]}"#;
        let (posts, _) = parse_post_list(body).expect("파싱 성공해야 함");
        assert_eq!(posts.len(), 1, "logNo 없는 항목은 제외");
        assert_eq!(posts[0].log_no, "7");
    }

    #[test]
    fn parse_post_list_none_when_no_postlist() {
        assert!(parse_post_list("<html>not json</html>").is_none());
        assert!(parse_post_list(r#"{"resultCode":"E"}"#).is_none());
    }

    // ------------------------------------------------------------------
    // 성공 — 한 페이지로 충분(count <= 페이지 글 수)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_latest_returns_newest_first_up_to_count() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(list_path()))
            .and(query_param("blogId", blog_id()))
            .and(query_param("currentPage", "1"))
            .and(query_param("countPerPage", "30"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(list_body(54, &[10, 9, 8, 7, 6])),
            )
            .mount(&server)
            .await;

        let client = BlogPostListClient::with_base_url(server.uri());
        let result = client
            .fetch_latest_posts(blog_id(), 0, 3, None)
            .await
            .expect("성공 응답이어야 함");
        assert_eq!(result.total_count, 54);
        assert_eq!(result.posts.len(), 3, "count(3)만큼만 반환");
        // 최신 우선(1페이지 = 최신) — 응답 순서를 그대로 보존한다.
        assert_eq!(result.posts[0].log_no, "10");
        assert_eq!(result.posts[2].log_no, "8");
    }

    // ------------------------------------------------------------------
    // 페이징 — count가 한 페이지(30)를 넘으면 다음 페이지를 이어 조회
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_latest_pages_until_count_is_reached() {
        let server = MockServer::start().await;
        let page1: Vec<u64> = (1..=30).collect();
        let page2: Vec<u64> = (31..=35).collect();
        Mock::given(method("GET"))
            .and(path(list_path()))
            .and(query_param("currentPage", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body(60, &page1)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(list_path()))
            .and(query_param("currentPage", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body(60, &page2)))
            .mount(&server)
            .await;

        let client = BlogPostListClient::with_base_url(server.uri());
        let result = client
            .fetch_latest_posts(blog_id(), 0, 35, None)
            .await
            .expect("페이징 조회 성공해야 함");
        assert_eq!(result.posts.len(), 35, "두 페이지를 합쳐 35개여야 함");
        assert_eq!(result.posts[0].log_no, "1");
        assert_eq!(result.posts[34].log_no, "35");
    }

    // ------------------------------------------------------------------
    // totalCount로 조기 종료 — 전체 글 수가 count보다 적으면 있는 만큼만
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_latest_stops_at_total_count() {
        // totalCount=3인데 count=10을 요청 → 있는 3개만(page=2를 요청하지 않음).
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(list_path()))
            .and(query_param("currentPage", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body(3, &[3, 2, 1])))
            .mount(&server)
            .await;

        let client = BlogPostListClient::with_base_url(server.uri());
        let result = client
            .fetch_latest_posts(blog_id(), 0, 10, None)
            .await
            .expect("조회 성공해야 함");
        assert_eq!(result.total_count, 3);
        assert_eq!(result.posts.len(), 3, "있는 만큼(3개)만 반환");
    }

    // ------------------------------------------------------------------
    // 빈 블로그 — postList가 빈 배열
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_latest_tolerates_empty_blog() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(list_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body(0, &[])))
            .mount(&server)
            .await;

        let client = BlogPostListClient::with_base_url(server.uri());
        let result = client
            .fetch_latest_posts(blog_id(), 0, 5, None)
            .await
            .expect("빈 블로그도 성공(목록만 비어 있음)");
        assert_eq!(result.total_count, 0);
        assert!(result.posts.is_empty());
    }

    // ------------------------------------------------------------------
    // 쿠키/UA/categoryNo 전송 검증
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_sends_cookie_user_agent_and_category() {
        let server = MockServer::start().await;
        // 테스트용 가짜 쿠키 값(실제 인증 값 아님).
        let fake_cookie = "NID_AUT=FAKE_TEST_VALUE; NID_SES=FAKE_SES_VALUE";
        Mock::given(method("GET"))
            .and(path(list_path()))
            .and(query_param("categoryNo", "7"))
            .and(header_exists("User-Agent"))
            .and(header("Cookie", fake_cookie))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body(1, &[1])))
            .mount(&server)
            .await;

        let client = BlogPostListClient::with_base_url(server.uri());
        client
            .fetch_latest_posts(blog_id(), 7, 1, Some(fake_cookie))
            .await
            .expect("헤더/쿠키/categoryNo 매칭 성공해야 함");
    }

    // ------------------------------------------------------------------
    // non-2xx 오류 + 백트레이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_http_error_carries_backtrace() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(list_path()))
            .respond_with(ResponseTemplate::new(500).set_body_string("error"))
            .mount(&server)
            .await;

        let client = BlogPostListClient::with_base_url(server.uri());
        let err = client
            .fetch_latest_posts(blog_id(), 0, 3, None)
            .await
            .expect_err("500은 Err여야 함");
        assert!(err.message().contains("HTTP status 500"));
        // 자세히 보기 trace는 앵커(at …)를 항상 포함한다(#199).
        assert!(err.trace().contains("at "));
    }

    // ------------------------------------------------------------------
    // 파싱 불가 2xx
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_unparseable_2xx_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(list_path()))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .mount(&server)
            .await;

        let client = BlogPostListClient::with_base_url(server.uri());
        let err = client
            .fetch_latest_posts(blog_id(), 0, 3, None)
            .await
            .expect_err("파싱불가는 Err여야 함");
        assert!(err.message().contains("해석"));
    }

    // ------------------------------------------------------------------
    // 전송 오류 — 서버 다운
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_transport_error_when_server_down() {
        let client = BlogPostListClient::with_base_url("http://127.0.0.1:1");
        let err = client
            .fetch_latest_posts(blog_id(), 0, 3, None)
            .await
            .expect_err("전송오류는 Err여야 함");
        assert!(err.message().contains("전송 오류"));
    }
}
