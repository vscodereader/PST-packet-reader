//! 네이버 블로그 댓글 등록 HTTP 클라이언트(#271).
//!
//! 블로그는 **댓글 전용**이다. 저장된 네이버 쿠키로 블로그 글(`https://blog.naver.com/{blogId}/{logNo}`)
//! 에 댓글을 단다. 3단계로 진행한다(패킷 분석으로 확정된 API — 재발견하지 말 것):
//!   1. PostView.naver HTML에서 숫자 `groupId`를 추출한다.
//!   2. cbox web_naver_token API로 `cbox_token`을 받는다.
//!   3. cbox web_naver_create API로 댓글을 등록한다.
//!
//! reqwest로 실제 HTTP 요청을 전송한다. 테스트에서는 [`BlogCommentClient::with_base_urls`]로
//! wiremock 서버를 주입할 수 있다. 카페 댓글 클라이언트(`naver_cafe::comment`)의 블로그 버전이다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 모듈은 쿠키 값을 로그, 에러 메시지,
//! `Debug` 출력에 절대 포함하지 않는다.

use super::error::BlogError;
use super::headers::{blog_cbox_headers, blog_document_headers};
use crate::naver_cafe::post::BROWSER_USER_AGENT;

/// 블로그 본문/cbox API 호스트.
const BLOG_HOST: &str = "https://blog.naver.com";
const CBOX_HOST: &str = "https://apis.naver.com";

/// 댓글 1건 등록 성공 결과. 등록된 댓글 번호와 본문을 보존한다(완료 로그 표시용).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlogCommentResult {
    pub comment_no: String,
    pub contents: String,
}

/// 네이버 블로그 댓글 등록 HTTP 클라이언트. base_url을 분리 보관해 실서버/wiremock을 함께 쓴다.
pub struct BlogCommentClient {
    blog_base: String,
    cbox_base: String,
    http: reqwest::Client,
}

impl BlogCommentClient {
    /// 실서버 호스트를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_urls(BLOG_HOST, CBOX_HOST)
    }

    /// 주입된 base_url들을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_urls(blog_base: impl Into<String>, cbox_base: impl Into<String>) -> Self {
        Self {
            blog_base: blog_base.into(),
            cbox_base: cbox_base.into(),
            // 카페와 공용 전역 reqwest 클라이언트를 재사용해 keep-alive 커넥션을 공유한다.
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// 1단계: PostView.naver HTML을 받아 숫자 `groupId`를 추출한다.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자의 인증 자격 증명이며, 이 함수는 해당 값을 에러/로그에 노출하지 않는다.
    pub async fn resolve_group_id(
        &self,
        blog_id: &str,
        log_no: &str,
        cookie: Option<&str>,
    ) -> Result<String, BlogError> {
        let url = format!(
            "{}/PostView.naver?blogId={}&logNo={}",
            self.blog_base, blog_id, log_no
        );
        // 위장 헤더(#312): 로그인 세션인데 Referer·sec-fetch 등이 없으면 네이버가 봇 인터스티셜
        // (200이지만 blogNo 없는 페이지)을 돌려줘 groupId를 못 찾는다. Referer는 같은 오리진의
        // 블로그 홈으로 둬 sec-fetch-site: same-origin과 일관되게 한다.
        let referer = format!("{}/{}", self.blog_base, blog_id);
        let html = self
            .get_text(&url, &blog_document_headers(&referer), cookie)
            .await?;
        parse_group_id(&html).ok_or_else(|| {
            // 진단(#312): 또 막히면 어떤 응답이 왔는지 보이도록 길이 + 앞부분 스니펫을 남긴다.
            // 본문(HTML)에는 쿠키가 없으므로 자격 증명 노출 위험이 없다.
            BlogError::new(format!(
                "블로그 글에서 groupId를 찾지 못했습니다(삭제·비공개 글이거나 네이버 봇차단 응답일 수 있어요). {}",
                response_diagnostic(&html)
            ))
        })
    }

    /// 2단계: cbox web_naver_token API로 `cbox_token`을 받는다.
    pub async fn fetch_cbox_token(
        &self,
        blog_id: &str,
        log_no: &str,
        group_id: &str,
        cookie: Option<&str>,
    ) -> Result<String, BlogError> {
        let object_id = object_id(group_id, log_no);
        let url = format!(
            "{}/commentBox/cbox/web_naver_token_json.json?ticket=blog&templateId=default&pool=blogid&_cv=&lang=ko&pageType=default&country=&objectId={}&categoryId=&pageSize=50&indexSize=10&groupId={}&listType=OBJECT&userType=",
            self.cbox_base, object_id, group_id
        );
        // cbox는 apis.naver.com(=same-site)이라 Referer를 글 PostView 페이지로 둔다(#312).
        let referer = format!(
            "{}/PostView.naver?blogId={}&logNo={}",
            BLOG_HOST, blog_id, log_no
        );
        let body = self
            .get_text(&url, &blog_cbox_headers(&referer), cookie)
            .await?;
        parse_cbox_token(&body)
            .ok_or_else(|| BlogError::new("블로그 댓글 토큰(cbox_token)을 받지 못했습니다"))
    }

    /// 3단계: cbox web_naver_create API로 댓글을 등록한다.
    pub async fn post_blog_comment(
        &self,
        blog_id: &str,
        log_no: &str,
        group_id: &str,
        token: &str,
        contents: &str,
        cookie: Option<&str>,
    ) -> Result<BlogCommentResult, BlogError> {
        let url = format!(
            "{}/commentBox/cbox/web_naver_create_json.json?ticket=blog&templateId=default&pool=blogid&_cv=",
            self.cbox_base
        );
        let object_url = format!("{}/{}/{}", BLOG_HOST, blog_id, log_no);
        let referer = format!(
            "{}/PostView.naver?blogId={}&logNo={}",
            BLOG_HOST, blog_id, log_no
        );
        let object_id = object_id(group_id, log_no);
        let like_it_id = format!("{}_{}", blog_id, log_no);

        // form 파라미터(확정된 API). 쿠키는 헤더로만 보낸다.
        let form: Vec<(&str, &str)> = vec![
            ("lang", "ko"),
            ("pageType", "default"),
            ("country", ""),
            ("objectId", &object_id),
            ("categoryId", ""),
            ("pageSize", "50"),
            ("indexSize", "10"),
            ("groupId", group_id),
            ("listType", "OBJECT"),
            ("clientType", "web-pc"),
            ("objectUrl", &object_url),
            ("contents", contents),
            ("userType", ""),
            ("pick", "false"),
            ("manager", "false"),
            ("score", "0"),
            ("likeItId", &like_it_id),
            ("sort", "NEW"),
            ("secret", "false"),
            ("refresh", "true"),
            ("imageCount", "0"),
            ("commentType", "txt"),
            ("validateBanWords", "true"),
            ("invalidateCleanbotAlert", "false"),
            ("cbox_token", token),
        ];
        let body = serde_urlencoded::to_string(&form)
            .map_err(|e| BlogError::new(format!("댓글 폼 인코딩에 실패했습니다: {e}")))?;

        let mut req = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", BROWSER_USER_AGENT);
        // 위장 헤더(#312): cbox 쓰기도 읽기와 동일한 same-site 지문을 실어 봇차단을 피한다.
        // Referer만 이 글의 PostView로 따로 둔다(blog_cbox_headers가 Origin/sec-fetch/accept-language 채움).
        for (name, value) in blog_cbox_headers(&referer) {
            req = req.header(name, value);
        }
        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }

        let response = req.body(body).send().await.map_err(|e| {
            BlogError::new(crate::transport_error_message!(
                "HTTP 전송 오류가 발생했습니다",
                e
            ))
        })?;
        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| BlogError::new(format!("응답 본문 읽기 오류: {e}")))?;
        {
            // [사용자·사수 지시: 성공/실패 전부 네이버 원문] 블로그 API 실제 응답 status+body 그대로.
            let snippet: String = raw.chars().take(800).collect();
            tracing::info!(status = status.as_u16(), body = %snippet, "[BLOG] 실제 API 응답 — 네이버 원문");
        }
        if !status.is_success() {
            return Err(BlogError::new(format!(
                "블로그 댓글 등록 요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        parse_create_result(&raw).ok_or_else(|| {
            // 진단(#312-후속): 네이버가 success/code=1000을 안 준 진짜 사유(code·message)를 노출한다.
            // 금칙어·중복·이웃공개·스팸차단 등을 사후 구분할 수 있다. 본문엔 쿠키가 없다.
            BlogError::new(format!(
                "블로그 댓글 등록에 실패했습니다(네이버가 성공을 반환하지 않음). {}",
                create_failure_detail(&raw)
            ))
        })
    }

    /// 고수준 진입점: 3단계(groupId 해석 → 토큰 → 댓글 등록)를 차례로 수행한다.
    ///
    /// # 쿠키 보안
    /// `cookie`는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
    pub async fn create_comment(
        &self,
        blog_id: &str,
        log_no: &str,
        contents: &str,
        cookie: Option<&str>,
    ) -> Result<BlogCommentResult, BlogError> {
        let group_id = self.resolve_group_id(blog_id, log_no, cookie).await?;
        let token = self
            .fetch_cbox_token(blog_id, log_no, &group_id, cookie)
            .await?;
        self.post_blog_comment(blog_id, log_no, &group_id, &token, contents, cookie)
            .await
    }

    /// 공통 GET — 위장 헤더 세트(#312)·쿠키·브라우저 User-Agent를 실어 본문 텍스트를 받는다.
    /// `headers`는 [`blog_document_headers`]/[`blog_cbox_headers`]가 만든 (이름, 값) 목록이다.
    async fn get_text(
        &self,
        url: &str,
        headers: &[(&str, String)],
        cookie: Option<&str>,
    ) -> Result<String, BlogError> {
        let mut req = self.http.get(url).header("User-Agent", BROWSER_USER_AGENT);
        for (name, value) in headers {
            req = req.header(*name, value);
        }
        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let response = req.send().await.map_err(|e| {
            BlogError::new(crate::transport_error_message!(
                "HTTP 전송 오류가 발생했습니다",
                e
            ))
        })?;
        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| BlogError::new(format!("응답 본문 읽기 오류: {e}")))?;
        {
            // [사용자·사수 지시: 성공/실패 전부 네이버 원문] 블로그 API 실제 응답 status+body 그대로.
            let snippet: String = raw.chars().take(800).collect();
            tracing::info!(status = status.as_u16(), body = %snippet, "[BLOG] 실제 API 응답 — 네이버 원문");
        }
        if !status.is_success() {
            return Err(BlogError::new(format!(
                "요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        Ok(raw)
    }
}

impl Default for BlogCommentClient {
    fn default() -> Self {
        Self::new()
    }
}

/// cbox objectId 규약: `{groupId}_201_{logNo}`.
fn object_id(group_id: &str, log_no: &str) -> String {
    format!("{}_201_{}", group_id, log_no)
}

/// 진단 문자열(#312): 응답 길이 + 공백 정리한 앞부분 스니펫(최대 160자). groupId 파싱 실패 시
/// "어떤 페이지가 왔는지"를 trace로 보여 봇차단/리다이렉트/빈응답을 구분하게 한다(순수 함수).
/// 본문(HTML/JSON)에는 쿠키가 없어 자격 증명 노출 위험이 없다.
fn response_diagnostic(body: &str) -> String {
    let snippet: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let snippet: String = snippet.chars().take(160).collect();
    format!("응답길이={}, 앞부분=\"{}\"", body.len(), snippet)
}

/// PostView.naver HTML에서 cbox `groupId`(= 그 블로그의 숫자 ID)를 뽑는다.
///
/// 네이버 블로그 cbox의 `groupId`는 블로그의 `blogNo`(숫자)와 **같다**. **현행 PostView HTML에는
/// `groupId` 토큰이 아예 없고 `blogNo = '165657723'` 형태로만 들어 있다**(2026-06 패킷·실제 HTML
/// 확인 — `groupId`만 찾던 옛 코드가 항상 실패해 댓글이 안 달리던 근본 원인). 그래서 `groupId`
/// (구버전 호환)와 `blogNo`(현행) 두 토큰을 차례로 찾아, 먼저 숫자가 잡히는 쪽을 쓴다.
fn parse_group_id(html: &str) -> Option<String> {
    // groupId(구버전) → blogNo(현행) 순. cbox groupId == blogNo 라 어느 쪽이든 같은 값이다.
    ["groupId", "blogNo"]
        .into_iter()
        .find_map(|token| find_number_after_token(html, token))
}

/// HTML에서 `token`의 **모든 출현**을 훑어, 토큰 바로 뒤(값과의 구분자 `= : " ' 공백 \ \t \n \r`
/// 만 건너뛴)에 숫자가 오는 첫 출현의 숫자열을 돌려준다(순수 함수). 숫자가 없는 출현
/// (`groupIdList`·`blogNoCount` 등)은 다음 출현으로 넘어간다 — 블로그 스킨/HTML 변형 대응.
fn find_number_after_token(html: &str, token: &str) -> Option<String> {
    let mut from = 0;
    while let Some(rel) = html[from..].find(token) {
        let after_idx = from + rel + token.len();
        from = after_idx; // 다음 탐색은 이 토큰 뒤부터(무한 루프 방지 + 다음 출현 검사)
                          // 토큰과 값 사이의 구분자만 건너뛴다 — 임의의 먼 숫자로 점프하지 않는다.
        let after_sep = html[after_idx..]
            .trim_start_matches([' ', '=', ':', '"', '\'', '\\', '\t', '\n', '\r']);
        let digits: String = after_sep
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return Some(digits);
        }
    }
    None
}

/// web_naver_token 응답에서 `result.cbox_token`을 읽는다. success/code도 함께 검증한다.
fn parse_cbox_token(body: &str) -> Option<String> {
    // 네이버 cbox 응답은 JSONP/접두 문자가 붙기도 하므로, 첫 `{`부터 JSON으로 파싱한다.
    let json: serde_json::Value = serde_json::from_str(json_slice(body)).ok()?;
    if json.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    json.get("result")?
        .get("cbox_token")?
        .as_str()
        .map(|s| s.to_owned())
}

/// web_naver_create 응답에서 등록된 댓글(commentNo/contents)을 읽는다. success=true && code="1000"
/// 이고 commentList에 1건 이상 있을 때만 성공으로 본다.
fn parse_create_result(body: &str) -> Option<BlogCommentResult> {
    let json: serde_json::Value = serde_json::from_str(json_slice(body)).ok()?;
    if json.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    if json.get("code").and_then(|v| v.as_str()) != Some("1000") {
        return None;
    }
    let comment = json
        .get("result")?
        .get("commentList")?
        .as_array()?
        .first()?;
    let comment_no = comment.get("commentNo")?.as_str()?.to_owned();
    let contents = comment
        .get("contents")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    Some(BlogCommentResult {
        comment_no,
        contents,
    })
}

/// web_naver_create 실패 응답에서 진단 문자열을 만든다(#312-후속). `success`/`code`/`message`를
/// 뽑아 거절 사유를 드러내고, JSON 파싱이 안 되면 [`response_diagnostic`] 스니펫으로 떨어진다.
/// 본문(JSON)에는 쿠키가 없어 자격 증명 노출 위험이 없다(순수 함수).
fn create_failure_detail(body: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(json_slice(body)) {
        Ok(json) => {
            let success = json.get("success").and_then(|v| v.as_bool());
            let code = json.get("code").and_then(|v| v.as_str());
            let message = json.get("message").and_then(|v| v.as_str());
            format!(
                "success={}, code={}, message={}",
                success.map(|b| b.to_string()).unwrap_or_else(|| "?".into()),
                code.unwrap_or("?"),
                message.unwrap_or("?")
            )
        }
        // JSON이 아니면(봇차단 HTML 등) 길이+스니펫으로 보여준다.
        Err(_) => response_diagnostic(body),
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
    use super::*;
    use wiremock::{
        matchers::{header, header_exists, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    fn post_view_html(group_id: &str) -> String {
        // 실측 형태를 흉내낸 HTML: groupId가 대입식과 따옴표 두 곳에 나온다.
        format!("<html><script>var groupId={group_id};\nvar foo='{group_id}';</script></html>")
    }

    fn token_json(token: &str) -> serde_json::Value {
        serde_json::json!({
            "success": true,
            "code": "1000",
            "result": { "cbox_token": token }
        })
    }

    fn create_success_json(no: &str, contents: &str) -> serde_json::Value {
        serde_json::json!({
            "success": true,
            "code": "1000",
            "message": "요청을 성공적으로 처리하였습니다.",
            "result": { "commentList": [ { "commentNo": no, "contents": contents } ] }
        })
    }

    // ------------------------------------------------------------------
    // 순수 파서 단위 테스트
    // ------------------------------------------------------------------

    #[test]
    fn parse_group_id_extracts_first_digits_after_token() {
        assert_eq!(
            parse_group_id("x groupId=144945128; y"),
            Some("144945128".to_owned())
        );
        assert_eq!(
            parse_group_id("var g = '144945128';"),
            None,
            "groupId 토큰이 없으면 None"
        );
        assert_eq!(
            parse_group_id("groupId : '987654';"),
            Some("987654".to_owned())
        );
        assert_eq!(parse_group_id("no token here"), None);
        // #271 후속: 첫 출현(groupIdList 등)에 숫자가 없어도 다음 출현의 실제 숫자를 찾는다.
        assert_eq!(
            parse_group_id(r#"var groupIdList=[]; x "groupId":144945128, y"#),
            Some("144945128".to_owned())
        );
        // JSON 따옴표 값 형도 잡는다.
        assert_eq!(
            parse_group_id(r#"{"groupId":"998877"}"#),
            Some("998877".to_owned())
        );
        // 토큰은 있으나 끝내 숫자가 없으면 None.
        assert_eq!(parse_group_id(r#"{"groupId":null,"groupIdList":[]}"#), None);
        // 현행 네이버 HTML(2026-06 패킷·실제 PostView 확인): groupId 토큰이 없고 blogNo로 들어 있다.
        // cbox groupId == blogNo 라 blogNo에서 뽑는다 — 댓글이 안 달리던 근본 버그의 수정.
        assert_eq!(
            parse_group_id("var blogNo = '165657723'; // groupId 토큰 없음"),
            Some("165657723".to_owned()),
            "blogNo(따옴표) 형태에서 groupId(=blogNo) 추출"
        );
        assert_eq!(
            parse_group_id("blogNo = 165657723"),
            Some("165657723".to_owned()),
            "blogNo(따옴표 없음) 형태"
        );
        // 숫자 없는 blogNo 출현(blogNoCount 등)은 건너뛰고 실제 blogNo를 찾는다.
        assert_eq!(
            parse_group_id("blogNoCount: 3, blogNo:'777'"),
            Some("777".to_owned())
        );
        // groupId가 있으면 그쪽 우선(구버전 호환), blogNo는 폴백.
        assert_eq!(
            parse_group_id("groupId=111; blogNo='222'"),
            Some("111".to_owned()),
            "groupId 우선, blogNo 폴백"
        );
    }

    #[test]
    fn parse_cbox_token_reads_result_token() {
        let body = token_json("TKN-123").to_string();
        assert_eq!(parse_cbox_token(&body), Some("TKN-123".to_owned()));
        // success=false면 None.
        let fail = serde_json::json!({"success": false, "result": {"cbox_token": "x"}});
        assert_eq!(parse_cbox_token(&fail.to_string()), None);
    }

    #[test]
    fn parse_create_result_requires_success_and_1000() {
        let ok = create_success_json("99", "안녕").to_string();
        let r = parse_create_result(&ok).expect("성공이어야 함");
        assert_eq!(r.comment_no, "99");
        assert_eq!(r.contents, "안녕");
        // code != 1000 → None.
        let bad = serde_json::json!({"success": true, "code": "9999"});
        assert_eq!(parse_create_result(&bad.to_string()), None);
        // success=false → None.
        let bad2 = serde_json::json!({"success": false, "code": "1000"});
        assert_eq!(parse_create_result(&bad2.to_string()), None);
    }

    // ------------------------------------------------------------------
    // 성공 통합 케이스(3단계)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn create_comment_success_chains_three_steps() {
        let blog = MockServer::start().await;
        let cbox = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostView.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(post_view_html("144945128")))
            .mount(&blog)
            .await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("TKN")))
            .mount(&cbox)
            .await;
        Mock::given(method("POST"))
            .and(path("/commentBox/cbox/web_naver_create_json.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(create_success_json("7", "댓글내용")),
            )
            .mount(&cbox)
            .await;

        let client = BlogCommentClient::with_base_urls(blog.uri(), cbox.uri());
        let result = client
            .create_comment("press02", "224311392458", "댓글내용", None)
            .await
            .expect("성공이어야 함");
        assert_eq!(result.comment_no, "7");
        assert_eq!(result.contents, "댓글내용");
    }

    #[tokio::test]
    async fn resolve_group_id_parses_html() {
        let blog = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostView.naver"))
            .and(query_param("blogId", "cho41004"))
            .and(query_param("logNo", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_string(post_view_html("55")))
            .mount(&blog)
            .await;
        let client = BlogCommentClient::with_base_urls(blog.uri(), "http://unused");
        let gid = client
            .resolve_group_id("cho41004", "100", None)
            .await
            .expect("성공이어야 함");
        assert_eq!(gid, "55");
    }

    #[tokio::test]
    async fn fetch_cbox_token_reads_token() {
        let cbox = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("ABC")))
            .mount(&cbox)
            .await;
        let client = BlogCommentClient::with_base_urls("http://unused", cbox.uri());
        let token = client
            .fetch_cbox_token("b", "l", "144945128", None)
            .await
            .expect("성공이어야 함");
        assert_eq!(token, "ABC");
    }

    // ------------------------------------------------------------------
    // 쿠키 전송 / 미전송
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn create_comment_sends_cookie_when_provided() {
        let blog = MockServer::start().await;
        let cbox = MockServer::start().await;
        // 테스트용 가짜 쿠키(실제 인증 값 아님).
        let fake_cookie = "NID_AUT=FAKE_FOR_TEST; NID_SES=FAKE_FOR_TEST";
        Mock::given(method("GET"))
            .and(path("/PostView.naver"))
            .and(header_exists("Cookie"))
            .respond_with(ResponseTemplate::new(200).set_body_string(post_view_html("1")))
            .mount(&blog)
            .await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .and(header_exists("Cookie"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("T")))
            .mount(&cbox)
            .await;
        Mock::given(method("POST"))
            .and(path("/commentBox/cbox/web_naver_create_json.json"))
            .and(header_exists("Cookie"))
            .and(header("Origin", "https://blog.naver.com"))
            .respond_with(ResponseTemplate::new(200).set_body_json(create_success_json("1", "c")))
            .mount(&cbox)
            .await;

        let client = BlogCommentClient::with_base_urls(blog.uri(), cbox.uri());
        client
            .create_comment("b", "l", "c", Some(fake_cookie))
            .await
            .expect("Cookie 헤더 존재 시 성공해야 함");
    }

    #[tokio::test]
    async fn create_comment_works_without_cookie() {
        let blog = MockServer::start().await;
        let cbox = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostView.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(post_view_html("2")))
            .mount(&blog)
            .await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("T")))
            .mount(&cbox)
            .await;
        Mock::given(method("POST"))
            .and(path("/commentBox/cbox/web_naver_create_json.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(create_success_json("3", "c")))
            .mount(&cbox)
            .await;
        let client = BlogCommentClient::with_base_urls(blog.uri(), cbox.uri());
        client
            .create_comment("b", "l", "c", None)
            .await
            .expect("쿠키 없이도 성공(목 서버는 인증을 강제하지 않음)");
    }

    // ------------------------------------------------------------------
    // 실패 + 백트레이스
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn create_comment_failure_carries_backtrace() {
        let blog = MockServer::start().await;
        let cbox = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostView.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(post_view_html("9")))
            .mount(&blog)
            .await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("T")))
            .mount(&cbox)
            .await;
        // 네이버가 실패(success=false)를 반환.
        Mock::given(method("POST"))
            .and(path("/commentBox/cbox/web_naver_create_json.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"success": false, "code": "4000"})),
            )
            .mount(&cbox)
            .await;
        let client = BlogCommentClient::with_base_urls(blog.uri(), cbox.uri());
        let err = client
            .create_comment("b", "l", "c", None)
            .await
            .expect_err("실패여야 함");
        assert!(!err.message().is_empty());
        // 자세히 보기 trace는 앵커(at …)를 항상 포함한다(#199).
        assert!(err.trace().contains("at "));
    }

    #[tokio::test]
    async fn resolve_group_id_missing_token_fails_with_trace_and_diagnostic() {
        let blog = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostView.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>no group here</html>"))
            .mount(&blog)
            .await;
        let client = BlogCommentClient::with_base_urls(blog.uri(), "http://unused");
        let err = client
            .resolve_group_id("b", "l", None)
            .await
            .expect_err("groupId 없으면 실패");
        assert!(err.message().contains("groupId"));
        // 진단(#312): 응답 길이·스니펫이 메시지에 실려 봇차단/빈응답을 사후 구분할 수 있다.
        assert!(err.message().contains("응답길이="));
        assert!(err.message().contains("no group here"));
        assert!(err.trace().contains("at "));
    }

    // #312: PostView GET이 위장 헤더(Referer + sec-fetch)를 실어야 네이버 봇차단을 피한다.
    #[tokio::test]
    async fn resolve_group_id_sends_disguise_headers() {
        let blog = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostView.naver"))
            .and(header_exists("Referer"))
            .and(header("sec-fetch-mode", "navigate"))
            .and(header("sec-fetch-dest", "document"))
            .respond_with(ResponseTemplate::new(200).set_body_string(post_view_html("77")))
            .mount(&blog)
            .await;
        let client = BlogCommentClient::with_base_urls(blog.uri(), "http://unused");
        let gid = client
            .resolve_group_id("b", "l", None)
            .await
            .expect("위장 헤더가 실려야 성공");
        assert_eq!(gid, "77");
    }

    // #312: cbox 토큰 GET은 same-site 지문(Origin/Referer/cors)을 실어야 한다.
    #[tokio::test]
    async fn fetch_cbox_token_sends_cors_disguise_headers() {
        let cbox = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .and(header("Origin", "https://blog.naver.com"))
            .and(header("sec-fetch-mode", "cors"))
            .and(header_exists("Referer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("OK")))
            .mount(&cbox)
            .await;
        let client = BlogCommentClient::with_base_urls("http://unused", cbox.uri());
        let token = client
            .fetch_cbox_token("b", "l", "1", None)
            .await
            .expect("cors 위장 헤더가 실려야 성공");
        assert_eq!(token, "OK");
    }

    #[test]
    fn create_failure_detail_surfaces_code_and_message() {
        let body = serde_json::json!({
            "success": false, "code": "4090",
            "message": "이미 등록된 댓글입니다."
        })
        .to_string();
        let d = create_failure_detail(&body);
        assert!(d.contains("success=false"));
        assert!(d.contains("code=4090"));
        assert!(d.contains("이미 등록된 댓글입니다."));
    }

    #[test]
    fn create_failure_detail_falls_back_to_snippet_for_non_json() {
        let d = create_failure_detail("<html>로그인이 필요합니다</html>");
        assert!(d.contains("응답길이="));
        assert!(d.contains("로그인이 필요합니다"));
    }

    #[test]
    fn response_diagnostic_reports_length_and_collapsed_snippet() {
        let d = response_diagnostic("  <html>\n  hello   world  </html>  ");
        assert!(d.contains("응답길이="));
        assert!(d.contains("<html> hello world </html>"));
    }
}
