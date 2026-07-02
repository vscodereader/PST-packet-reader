//! 네이버 클립 댓글 등록 HTTP 클라이언트(#클립).
//!
//! 클립은 **댓글 전용**이다. 저장된 네이버 쿠키로 클립 영상/게시물에 댓글을 단다. 댓글 API는
//! 블로그와 같은 cbox지만 `ticket=clip&pool=cbox8`이고, **objectId가 곧 미디어 HEX id**라
//! 블로그처럼 groupId를 따로 추출할 필요가 없다(패킷 분석으로 확정 — 재발견하지 말 것):
//!   1. cbox web_naver_token API로 `cbox_token`을 받는다.
//!   2. cbox web_naver_create API로 댓글을 등록한다.
//!
//! reqwest로 실제 HTTP를 전송한다. 테스트는 [`ClipCommentClient::with_base_url`]로 wiremock을 주입한다.
//! 카페/블로그 댓글 클라이언트의 클립 버전이다(공용 reqwest 클라이언트 재사용).
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 인증 자격 증명이다. 이 모듈은 쿠키 값을 로그/에러/Debug에 절대 포함하지 않는다.

use super::error::ClipError;
use super::headers::{clip_cbox_headers, CLIP_ORIGIN};
use crate::naver_cafe::post::BROWSER_USER_AGENT;

/// cbox API 호스트.
const CBOX_HOST: &str = "https://apis.naver.com";

/// 댓글 1건 등록 성공 결과(완료 로그 표시용).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipCommentResult {
    pub comment_no: String,
    pub contents: String,
}

/// 네이버 클립 댓글 등록 HTTP 클라이언트.
pub struct ClipCommentClient {
    cbox_base: String,
    http: reqwest::Client,
}

impl ClipCommentClient {
    /// 실서버 호스트를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(CBOX_HOST)
    }

    /// 주입된 cbox base_url을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(cbox_base: impl Into<String>) -> Self {
        Self {
            cbox_base: cbox_base.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// 1단계: cbox web_naver_token API로 `cbox_token`을 받는다(ticket=clip, pool=cbox8).
    pub async fn fetch_cbox_token(
        &self,
        object_id: &str,
        profile_id: &str,
        cookie: Option<&str>,
    ) -> Result<String, ClipError> {
        let url = format!(
            "{}/commentBox/cbox/web_naver_token_json.json?ticket=clip&templateId=default&pool=cbox8&_cv=&lang=ko&pageType=more&country=&objectId={}&categoryId=&pageSize=20&indexSize=10&groupId=&listType=OBJECT&userType=",
            self.cbox_base, object_id
        );
        let referer = contents_url(profile_id, object_id);
        let body = self
            .get_text(&url, &clip_cbox_headers(&referer), cookie)
            .await?;
        parse_cbox_token(&body)
            .ok_or_else(|| ClipError::new("클립 댓글 토큰(cbox_token)을 받지 못했습니다"))
    }

    /// 2단계: cbox web_naver_create API로 댓글을 등록한다.
    pub async fn post_clip_comment(
        &self,
        object_id: &str,
        profile_id: &str,
        token: &str,
        contents: &str,
        cookie: Option<&str>,
    ) -> Result<ClipCommentResult, ClipError> {
        let url = format!(
            "{}/commentBox/cbox/web_naver_create_json.json?ticket=clip&templateId=default&pool=cbox8&_cv=",
            self.cbox_base
        );
        let referer = contents_url(profile_id, object_id);
        // form 파라미터(패킷 확정). groupId는 빈 값, objectId가 곧 미디어 id, clientType=web-mobile.
        let form: Vec<(&str, &str)> = vec![
            ("lang", "ko"),
            ("pageType", "more"),
            ("country", ""),
            ("objectId", object_id),
            ("categoryId", ""),
            ("pageSize", "20"),
            ("indexSize", "10"),
            ("groupId", ""),
            ("listType", "OBJECT"),
            ("clientType", "web-mobile"),
            ("objectUrl", &referer),
            ("contents", contents),
            ("userType", ""),
            ("pick", "false"),
            ("manager", "false"),
            ("score", "0"),
            ("likeItId", ""),
            ("sort", "NEW"),
            ("secret", "false"),
            ("refresh", "true"),
            ("replyNotificationSet", "OFF"),
            ("validateBanWords", "true"),
            ("invalidateCleanbotAlert", "false"),
            ("cbox_token", token),
        ];
        let body = serde_urlencoded::to_string(&form)
            .map_err(|e| ClipError::new(format!("댓글 폼 인코딩에 실패했습니다: {e}")))?;

        let mut req = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", BROWSER_USER_AGENT);
        for (name, value) in clip_cbox_headers(&referer) {
            // Content-Type은 위에서 이미 지정했으므로 중복 추가하지 않는다.
            if name == "Content-Type" {
                continue;
            }
            req = req.header(name, value);
        }
        // 보안: Cookie 헤더 값은 로그에 기록하지 않는다.
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let _ = CLIP_ORIGIN; // Origin은 clip_cbox_headers가 채운다(상수 사용 표시).

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
        {
            // [사용자·사수 지시: 성공/실패 전부 네이버 원문] 클립 API 실제 응답 status+body 그대로.
            let snippet: String = raw.chars().take(800).collect();
            tracing::info!(status = status.as_u16(), body = %snippet, "[CLIP] 실제 API 응답 — 네이버 원문");
        }
        if !status.is_success() {
            return Err(ClipError::new(format!(
                "클립 댓글 등록 요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        parse_create_result(&raw).ok_or_else(|| {
            ClipError::new(format!(
                "클립 댓글 등록에 실패했습니다(네이버가 성공을 반환하지 않음). {}",
                create_failure_detail(&raw)
            ))
        })
    }

    /// 고수준 진입점: 토큰 → 댓글 등록을 차례로 수행한다.
    pub async fn create_comment(
        &self,
        object_id: &str,
        profile_id: &str,
        contents: &str,
        cookie: Option<&str>,
    ) -> Result<ClipCommentResult, ClipError> {
        let token = self.fetch_cbox_token(object_id, profile_id, cookie).await?;
        self.post_clip_comment(object_id, profile_id, &token, contents, cookie)
            .await
    }

    /// 공통 GET — 위장 헤더·쿠키·브라우저 UA를 실어 본문 텍스트를 받는다.
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
        {
            // [사용자·사수 지시: 성공/실패 전부 네이버 원문] 클립 API 실제 응답 status+body 그대로.
            let snippet: String = raw.chars().take(800).collect();
            tracing::info!(status = status.as_u16(), body = %snippet, "[CLIP] 실제 API 응답 — 네이버 원문");
        }
        if !status.is_success() {
            return Err(ClipError::new(format!(
                "요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        Ok(raw)
    }
}

impl Default for ClipCommentClient {
    fn default() -> Self {
        Self::new()
    }
}

/// cbox objectUrl/Referer로 쓰는 클립 contents URL. recId는 JSON을 한 번 URL 인코딩한 값이다
/// (패킷 확정 형태). objectUrl은 cbox 메타라 정확도가 중요하진 않지만 실측 형태를 그대로 만든다.
fn contents_url(profile_id: &str, object_id: &str) -> String {
    let rec_id = format!("{{\"targetProfileId\":\"{profile_id}\",\"open\":true}}");
    let rec_id_enc = urlencoding::encode(&rec_id);
    format!(
        "{CLIP_ORIGIN}/contents?recType=CLIP_PC&recId={rec_id_enc}&mediaType=ALL&serviceType=CLIP&seedMediaId={object_id}"
    )
}

/// web_naver_token 응답에서 `result.cbox_token`을 읽는다(success도 검증).
fn parse_cbox_token(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(json_slice(body)).ok()?;
    if json.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    json.get("result")?
        .get("cbox_token")?
        .as_str()
        .map(|s| s.to_owned())
}

/// web_naver_create 응답에서 등록된 댓글을 읽는다. success && code="1000"이고 `result.comment`가
/// 있을 때만 성공으로 본다. 클립은 `result.comment`(단수)에 새 댓글이 온다(블로그는 commentList).
fn parse_create_result(body: &str) -> Option<ClipCommentResult> {
    let json: serde_json::Value = serde_json::from_str(json_slice(body)).ok()?;
    if json.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    if json.get("code").and_then(|v| v.as_str()) != Some("1000") {
        return None;
    }
    let comment = json.get("result")?.get("comment")?;
    // commentNo는 숫자 또는 문자열로 올 수 있다.
    let comment_no = comment
        .get("commentNo")
        .map(value_to_string)
        .filter(|s| !s.is_empty())?;
    let contents = comment
        .get("contents")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    Some(ClipCommentResult {
        comment_no,
        contents,
    })
}

/// JSON 값(숫자/문자열)을 문자열로. 그 외 타입은 빈 문자열.
fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// create 실패 응답에서 진단 문자열(success/code/message)을 만든다. JSON이 아니면 스니펫 폴백.
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
        Err(_) => {
            let snippet: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
            let snippet: String = snippet.chars().take(160).collect();
            format!("응답길이={}, 앞부분=\"{}\"", body.len(), snippet)
        }
    }
}

/// 응답 앞에 JSONP/가드 접두가 붙는 경우를 대비해 첫 `{`부터의 슬라이스를 돌려준다.
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
        matchers::{header, header_exists, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    fn token_json(token: &str) -> serde_json::Value {
        serde_json::json!({"success": true, "code": "1000", "result": {"cbox_token": token}})
    }

    fn create_success_json(no: &str, contents: &str) -> serde_json::Value {
        serde_json::json!({
            "success": true, "code": "1000",
            "result": {"comment": {"commentNo": no, "contents": contents}}
        })
    }

    #[test]
    fn contents_url_encodes_recid_json() {
        let u = contents_url("PID123", "HEXABC");
        assert!(u.contains("seedMediaId=HEXABC"));
        assert!(u.contains("recType=CLIP_PC"));
        // recId JSON이 URL 인코딩돼 targetProfileId가 들어간다.
        assert!(u.contains("targetProfileId") || u.contains("targetProfileId".replace(':', "%3A").as_str()));
        assert!(u.contains("PID123"));
    }

    #[test]
    fn parse_cbox_token_reads_result_token() {
        assert_eq!(
            parse_cbox_token(&token_json("TK").to_string()),
            Some("TK".to_owned())
        );
        let fail = serde_json::json!({"success": false, "result": {"cbox_token": "x"}});
        assert_eq!(parse_cbox_token(&fail.to_string()), None);
    }

    #[test]
    fn parse_create_result_reads_comment_singular() {
        // 클립은 result.comment(단수). 숫자 commentNo도 처리.
        let ok = serde_json::json!({
            "success": true, "code": "1000",
            "result": {"comment": {"commentNo": 897388624352379164i64, "contents": "안녕"}}
        })
        .to_string();
        let r = parse_create_result(&ok).expect("성공이어야 함");
        assert_eq!(r.comment_no, "897388624352379164");
        assert_eq!(r.contents, "안녕");
        // code != 1000 → None.
        let bad = serde_json::json!({"success": true, "code": "9999"});
        assert_eq!(parse_create_result(&bad.to_string()), None);
    }

    #[test]
    fn create_failure_detail_surfaces_code_message() {
        let body = serde_json::json!({"success": false, "code": "4090", "message": "도배 제한"}).to_string();
        let d = create_failure_detail(&body);
        assert!(d.contains("code=4090") && d.contains("도배 제한"));
    }

    #[tokio::test]
    async fn create_comment_chains_token_then_create() {
        let cbox = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .and(query_param("ticket", "clip"))
            .and(query_param("pool", "cbox8"))
            .and(header_exists("Referer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("TK")))
            .mount(&cbox)
            .await;
        Mock::given(method("POST"))
            .and(path("/commentBox/cbox/web_naver_create_json.json"))
            .and(header("Origin", "https://clip.naver.com"))
            .respond_with(ResponseTemplate::new(200).set_body_json(create_success_json("7", "c")))
            .mount(&cbox)
            .await;
        let client = ClipCommentClient::with_base_url(cbox.uri());
        let r = client
            .create_comment("HEX", "PID", "c", None)
            .await
            .expect("성공이어야 함");
        assert_eq!(r.comment_no, "7");
    }

    #[tokio::test]
    async fn create_comment_sends_cookie_when_provided() {
        let cbox = MockServer::start().await;
        let fake = "NID_AUT=FAKE; NID_SES=FAKE";
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .and(header_exists("Cookie"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("T")))
            .mount(&cbox)
            .await;
        Mock::given(method("POST"))
            .and(path("/commentBox/cbox/web_naver_create_json.json"))
            .and(header_exists("Cookie"))
            .respond_with(ResponseTemplate::new(200).set_body_json(create_success_json("1", "c")))
            .mount(&cbox)
            .await;
        let client = ClipCommentClient::with_base_url(cbox.uri());
        client
            .create_comment("HEX", "PID", "c", Some(fake))
            .await
            .expect("쿠키 존재 시 성공");
    }

    #[tokio::test]
    async fn create_comment_failure_carries_trace_and_detail() {
        let cbox = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/commentBox/cbox/web_naver_token_json.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_json("T")))
            .mount(&cbox)
            .await;
        Mock::given(method("POST"))
            .and(path("/commentBox/cbox/web_naver_create_json.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"success": false, "code": "4000", "message": "막힘"})),
            )
            .mount(&cbox)
            .await;
        let client = ClipCommentClient::with_base_url(cbox.uri());
        let err = client
            .create_comment("HEX", "PID", "c", None)
            .await
            .expect_err("실패여야 함");
        assert!(err.trace().contains("at "));
        assert!(err.message().contains("code=4000"));
    }
}
