//! 네이버 클립 창작자 미디어 목록 조회 HTTP 클라이언트(#클립, "최신 N개" 모드용).
//!
//! 블로그 [`crate::naver_blog::post_list`]의 클립 버전이다. 사용자가 창작자 핸들 링크
//! (`https://clip.naver.com/@<handle>`)를 넣으면 그 창작자의 최신 미디어(영상/게시물)를 N개
//! 모아 각 미디어 id(cbox objectId)로 댓글을 단다. (패킷 분석으로 확정된 API — 재발견하지 말 것):
//!   1. `GET /@<handle>` HTML에서 `"clipId":"<handle>","profileId":"<PID>"`로 profileId를 얻는다.
//!   2. `POST /api/graphql`의 `ContentsQuery`로 `data.contents.edges[].node.id`(미디어 HEX)를 모은다.
//!      `mediaType`은 ALL/VIDEO(사용자 탭 ?tab=all / ?tab=video 대응), 페이징은 endCursor.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 인증 자격 증명이다. 이 모듈은 쿠키 값을 로그/에러/Debug에 절대 포함하지 않는다.

use super::error::ClipError;
use super::headers::{clip_document_headers, clip_graphql_headers};
use crate::naver_cafe::post::BROWSER_USER_AGENT;

const CLIP_HOST: &str = "https://clip.naver.com";

/// 한 페이지당 조회할 미디어 수. 페이징 종료 판단에도 쓴다.
const PAGE_SIZE: u32 = 18;
/// 페이징 상한(연속 조회로 의심받지 않게). 페이지당 18개이므로 최대 약 180개.
const MAX_PAGES: u32 = 10;

/// 클립 미디어 1건. 댓글 대상 식별은 `media_id`(cbox objectId), `title`은 표시 전용.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipMedia {
    /// 미디어 HEX id(cbox objectId). 예: "4957392FFA60CCD90328B915120ECAA4C9CF".
    pub media_id: String,
    /// 제목(표시 전용).
    pub title: String,
}

/// 클립 미디어 종류 필터. 사용자 탭(전체 ?tab=all / 영상 ?tab=video)에 대응한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipMediaType {
    All,
    Video,
}

impl ClipMediaType {
    /// graphql `mediaType` 입력값.
    fn as_str(self) -> &'static str {
        match self {
            ClipMediaType::All => "ALL",
            ClipMediaType::Video => "VIDEO",
        }
    }
}

/// 네이버 클립 미디어 목록 조회 클라이언트. base_url을 분리 보관해 실서버/wiremock을 함께 쓴다.
pub struct ClipListClient {
    base_url: String,
    http: reqwest::Client,
}

impl ClipListClient {
    /// 실서버 호스트(`clip.naver.com`)를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(CLIP_HOST)
    }

    /// 주입된 base_url을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// `@<handle>` 페이지에서 창작자 profileId를 얻는다. 페이지 HTML에 `"clipId":"<handle>"` 뒤로
    /// `"profileId":"<PID>"`가 들어 있다(gzip은 reqwest가 푼다).
    pub async fn resolve_profile_id(
        &self,
        handle: &str,
        cookie: Option<&str>,
    ) -> Result<String, ClipError> {
        let handle = handle.trim_start_matches('@');
        let url = format!("{}/@{}", self.base_url, handle);
        let html = self
            .get_text(&url, &clip_document_headers(), cookie)
            .await?;
        parse_profile_id(&html, handle).ok_or_else(|| {
            ClipError::new(format!(
                "클립 창작자(@{handle})의 profileId를 찾지 못했습니다(없는 계정이거나 페이지 형식 변경). {}",
                response_diagnostic(&html)
            ))
        })
    }

    /// 창작자 profileId의 최신 미디어를 `count`개 모일 때까지 graphql ContentsQuery로 페이징한다
    /// (최신 우선). `media_type`은 전체/영상. 한 페이지가 가득 차지 않거나 hasNextPage=false면 멈춘다.
    pub async fn fetch_latest_clips(
        &self,
        profile_id: &str,
        media_type: ClipMediaType,
        count: usize,
        cookie: Option<&str>,
    ) -> Result<Vec<ClipMedia>, ClipError> {
        let mut clips: Vec<ClipMedia> = Vec::new();
        let mut after: Option<String> = None;
        for page in 1..=MAX_PAGES {
            let (mut fetched, page_info) = match self
                .fetch_page(profile_id, media_type, after.as_deref(), cookie)
                .await
            {
                Ok(v) => v,
                // 첫 페이지 실패는 대상이 0개가 되므로 오류로 알린다. 이후 페이지 실패는 부분 수집.
                Err(e) if page == 1 => return Err(e),
                Err(_) => break,
            };
            let got = fetched.len();
            clips.append(&mut fetched);
            let has_next = page_info.as_ref().map(|p| p.has_next).unwrap_or(false);
            after = page_info.and_then(|p| p.end_cursor);
            if clips.len() >= count || !has_next || got < PAGE_SIZE as usize || after.is_none() {
                break;
            }
        }
        clips.truncate(count);
        Ok(clips)
    }

    /// ContentsQuery 한 페이지를 조회해 (미디어 목록, 페이지 정보)로 파싱한다.
    async fn fetch_page(
        &self,
        profile_id: &str,
        media_type: ClipMediaType,
        after: Option<&str>,
        cookie: Option<&str>,
    ) -> Result<(Vec<ClipMedia>, Option<PageInfo>), ClipError> {
        let url = format!("{}/api/graphql", self.base_url);
        let body = build_contents_query(profile_id, media_type, PAGE_SIZE, after);
        let referer = format!("{}/@{}", self.base_url, profile_id);
        let mut req = self
            .http
            .post(&url)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Content-Type", "application/json");
        for (name, value) in clip_graphql_headers(&referer) {
            if name == "Content-Type" {
                continue;
            }
            req = req.header(name, value);
        }
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let response = req.body(body).send().await.map_err(|e| {
            ClipError::new(crate::transport_error_message!(
                "HTTP 전송 오류가 발생했습니다",
                e
            ))
        })?;
        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| ClipError::new(format!("응답 본문 읽기 오류: {e}")))?;
        if !status.is_success() {
            return Err(ClipError::new(format!(
                "클립 목록 조회 요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        parse_contents(&raw)
            .ok_or_else(|| ClipError::new("클립 목록을 해석하지 못했습니다(응답 형식 변경)"))
    }

    /// 공통 GET — 위장 헤더·쿠키·브라우저 UA로 본문 텍스트를 받는다.
    async fn get_text(
        &self,
        url: &str,
        headers: &[(&str, String)],
        cookie: Option<&str>,
    ) -> Result<String, ClipError> {
        let mut req = self.http.get(url).header("User-Agent", BROWSER_USER_AGENT);
        for (name, value) in headers {
            req = req.header(*name, value);
        }
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let response = req.send().await.map_err(|e| {
            ClipError::new(crate::transport_error_message!(
                "HTTP 전송 오류가 발생했습니다",
                e
            ))
        })?;
        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| ClipError::new(format!("응답 본문 읽기 오류: {e}")))?;
        if !status.is_success() {
            return Err(ClipError::new(format!(
                "요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        Ok(raw)
    }
}

impl Default for ClipListClient {
    fn default() -> Self {
        Self::new()
    }
}

/// graphql 페이지 정보(다음 페이지 유무 + 커서).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PageInfo {
    has_next: bool,
    end_cursor: Option<String>,
}

/// ContentsQuery 요청 본문(JSON)을 만든다. recId는 JSON을 문자열로 한 번 감싼 값이다(패킷 형태).
/// 필요한 필드만 고른 최소 쿼리 — graphql 서버는 인라인 쿼리의 임의 필드 선택을 허용한다.
fn build_contents_query(
    profile_id: &str,
    media_type: ClipMediaType,
    first: u32,
    after: Option<&str>,
) -> String {
    const QUERY: &str = "query ContentsQuery($input: ContentsInput!, $first: Int, $after: String, $reverse: Boolean = false) {\n  contents(input: $input, first: $first, after: $after, reverse: $reverse) {\n    edges { node { id mediaId mediaType title status publishedTime } }\n    pageInfo { hasNextPage endCursor }\n  }\n}";
    let rec_id = format!("{{\"targetProfileId\":\"{profile_id}\",\"open\":true}}");
    let mut variables = serde_json::json!({
        "reverse": false,
        "first": first,
        "input": {
            "recType": "CLIP_PC",
            "mediaType": media_type.as_str(),
            "recId": rec_id,
        }
    });
    if let Some(cursor) = after {
        variables["after"] = serde_json::Value::String(cursor.to_owned());
    }
    serde_json::json!({
        "operationName": "ContentsQuery",
        "variables": variables,
        "extensions": {},
        "query": QUERY,
    })
    .to_string()
}

/// ContentsQuery 응답에서 (미디어 목록, 페이지 정보)를 파싱한다(순수). `node.id`가 cbox objectId.
fn parse_contents(body: &str) -> Option<(Vec<ClipMedia>, Option<PageInfo>)> {
    let json: serde_json::Value = serde_json::from_str(json_slice(body)).ok()?;
    let contents = json.get("data")?.get("contents")?;
    let edges = contents.get("edges")?.as_array()?;
    let clips = edges
        .iter()
        .filter_map(|edge| {
            let node = edge.get("node")?;
            let media_id = node
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())?;
            let title = node
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            Some(ClipMedia { media_id, title })
        })
        .collect();
    let page_info = contents.get("pageInfo").map(|p| PageInfo {
        has_next: p
            .get("hasNextPage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        end_cursor: p
            .get("endCursor")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned()),
    });
    Some((clips, page_info))
}

/// `@<handle>` 페이지 HTML에서 profileId를 뽑는다. `"clipId":"<handle>"` 출현 뒤 가장 가까운
/// `"profileId":"<값>"`을 읽는다. 핸들 매칭으로 다른 사용자의 profileId 오인을 막는다(순수 함수).
fn parse_profile_id(html: &str, handle: &str) -> Option<String> {
    let needle = format!("\"clipId\":\"{handle}\"");
    let from = html.find(&needle)?;
    let after = &html[from..];
    let key = "\"profileId\":\"";
    let pos = after.find(key)? + key.len();
    let value: String = after[pos..].chars().take_while(|&c| c != '"').collect();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// 진단 문자열(응답 길이 + 공백 정리 스니펫 최대 160자). 본문엔 쿠키가 없다.
fn response_diagnostic(body: &str) -> String {
    let snippet: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let snippet: String = snippet.chars().take(160).collect();
    format!("응답길이={}, 앞부분=\"{}\"", body.len(), snippet)
}

/// 응답 앞에 가드 접두가 붙는 경우를 대비해 첫 `{`부터의 슬라이스를 돌려준다.
fn json_slice(body: &str) -> &str {
    match body.find('{') {
        Some(i) => &body[i..],
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{header, header_exists, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn contents_body(ids: &[&str], has_next: bool, end_cursor: &str) -> String {
        let edges: Vec<String> = ids
            .iter()
            .map(|id| format!(r#"{{"node":{{"id":"{id}","mediaId":"{id}","title":"클립 {id}","mediaType":"SHORT_FORM","status":"SERVICE"}}}}"#))
            .collect();
        format!(
            r#"{{"data":{{"contents":{{"edges":[{}],"pageInfo":{{"hasNextPage":{},"endCursor":"{}"}}}}}}}}"#,
            edges.join(","),
            has_next,
            end_cursor
        )
    }

    #[test]
    fn parse_profile_id_matches_handle() {
        let html = r#"...{"__typename":"User","id":"dMcs7AzzIF0228MoaRdS","clipId":"dongzzi_chef","profileId":"dMcs7AzzIF0228MoaRdS","nickname":"동찌"}..."#;
        assert_eq!(
            parse_profile_id(html, "dongzzi_chef"),
            Some("dMcs7AzzIF0228MoaRdS".to_owned())
        );
        // 다른 핸들이면 못 찾는다.
        assert_eq!(parse_profile_id(html, "someone_else"), None);
    }

    #[test]
    fn parse_contents_reads_ids_and_pageinfo() {
        let body = contents_body(&["AAA111", "BBB222"], true, "CUR1");
        let (clips, pi) = parse_contents(&body).expect("파싱 성공");
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].media_id, "AAA111");
        assert_eq!(clips[0].title, "클립 AAA111");
        let pi = pi.expect("pageInfo");
        assert!(pi.has_next);
        assert_eq!(pi.end_cursor.as_deref(), Some("CUR1"));
    }

    #[test]
    fn build_contents_query_embeds_profile_and_mediatype() {
        let b = build_contents_query("PID9", ClipMediaType::Video, 18, Some("C2"));
        assert!(b.contains("\"operationName\":\"ContentsQuery\""));
        assert!(b.contains("VIDEO"));
        assert!(b.contains("PID9"));
        assert!(b.contains("\"after\":\"C2\""));
    }

    #[tokio::test]
    async fn resolve_profile_id_fetches_handle_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/@dongzzi_chef"))
            .and(header_exists("User-Agent"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"<html>{"clipId":"dongzzi_chef","profileId":"PID_X"}</html>"#,
                ),
            )
            .mount(&server)
            .await;
        let client = ClipListClient::with_base_url(server.uri());
        let pid = client
            .resolve_profile_id("@dongzzi_chef", None)
            .await
            .expect("성공");
        assert_eq!(pid, "PID_X");
    }

    #[tokio::test]
    async fn fetch_latest_returns_up_to_count_newest_first() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(contents_body(
                &["M1", "M2", "M3", "M4", "M5"],
                false,
                "",
            )))
            .mount(&server)
            .await;
        let client = ClipListClient::with_base_url(server.uri());
        let clips = client
            .fetch_latest_clips("PID", ClipMediaType::All, 3, None)
            .await
            .expect("성공");
        assert_eq!(clips.len(), 3);
        assert_eq!(clips[0].media_id, "M1");
        assert_eq!(clips[2].media_id, "M3");
    }

    #[tokio::test]
    async fn fetch_latest_first_page_error_is_returned() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .respond_with(ResponseTemplate::new(500).set_body_string("err"))
            .mount(&server)
            .await;
        let client = ClipListClient::with_base_url(server.uri());
        let err = client
            .fetch_latest_clips("PID", ClipMediaType::All, 3, None)
            .await
            .expect_err("500은 Err");
        assert!(err.message().contains("HTTP status 500"));
        assert!(err.trace().contains("at "));
    }
}
