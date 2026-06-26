//! 네이버 클립 댓글용 프로필 보장(#클립). 클립은 네이버 로그인만으로는 댓글을 못 달고, 처음에
//! "프로필 생성"을 한 번 해야 한다(사용자: 별다른 입력 없이 버튼만 누르면 됨). 자동화에서는 게시
//! 직전에 계정마다 이 단계를 보장한다. (패킷 분석으로 확정 — 재발견하지 말 것):
//!   1. `GET creatorhub-api/api/v1.0/clip/profiles`(헤더 x-creator-hub-sid: clip) →
//!      `header.code==0`이면 이미 프로필 있음(스킵), `-2102`면 없음(생성 진행).
//!   2. 없으면 `POST clip.naver.com/api/graphql`의 `NaverProfile`로 기본 nickname/profileImageUrl을 받고,
//!   3. `SignUp` mutation으로 프로필을 만든다(성공 = `__typename=="SignUpSucceed"`).
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 인증 자격 증명이다. 이 모듈은 쿠키 값을 로그/에러/Debug에 절대 포함하지 않는다.

use super::error::ClipError;
use super::headers::{clip_graphql_headers, creatorhub_headers};
use crate::naver_cafe::post::BROWSER_USER_AGENT;

const CLIP_HOST: &str = "https://clip.naver.com";
const CREATORHUB_HOST: &str = "https://creatorhub-api.naver.com";
/// graphql 회원가입 컨텍스트 Referer.
const SIGNUP_REFERER: &str = "https://clip.naver.com/signup?version=light";

/// 네이버 클립 프로필 보장 클라이언트. base_url을 분리 보관해 실서버/wiremock을 함께 쓴다.
pub struct ClipProfileClient {
    clip_base: String,
    creatorhub_base: String,
    http: reqwest::Client,
}

impl ClipProfileClient {
    /// 실서버 호스트를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_urls(CLIP_HOST, CREATORHUB_HOST)
    }

    /// 주입된 base_url들을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_urls(
        clip_base: impl Into<String>,
        creatorhub_base: impl Into<String>,
    ) -> Self {
        Self {
            clip_base: clip_base.into(),
            creatorhub_base: creatorhub_base.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// 클립 댓글 프로필을 보장한다. 이미 있으면 즉시 반환, 없으면 생성한다.
    pub async fn ensure_profile(&self, cookie: Option<&str>) -> Result<(), ClipError> {
        if self.profile_exists(cookie).await? {
            return Ok(());
        }
        let (nickname, profile_image_url) = self.fetch_naver_profile(cookie).await?;
        let clip_id = generate_clip_id();
        // 네이버 클립은 **숫자만/빈/너무 짧은 닉네임**을 거부한다(-7020, 실측: nickname="040" 실패).
        // 영문자가 포함된 안전한 값으로 보정한다(없으면 clipId 사용 — 영숫자라 항상 유효).
        let nickname = safe_nickname(&nickname, &clip_id);
        self.sign_up(&clip_id, &nickname, &profile_image_url, cookie)
            .await
    }

    /// `creatorhub/api/v1.0/clip/profiles`로 프로필 존재 여부를 본다. `header.code==0`=있음.
    pub async fn profile_exists(&self, cookie: Option<&str>) -> Result<bool, ClipError> {
        let url = format!("{}/api/v1.0/clip/profiles", self.creatorhub_base);
        let mut req = self.http.get(&url).header("User-Agent", BROWSER_USER_AGENT);
        for (name, value) in creatorhub_headers() {
            req = req.header(name, value);
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
        // 404/권한 등으로 비-2xx여도 본문에 header.code가 있으면 그걸로 판정한다(없음=-2102).
        let code = parse_header_code(&raw);
        match code {
            Some(0) => Ok(true),
            Some(_) => Ok(false),
            None => {
                if status.is_success() {
                    // 형식이 바뀐 2xx — 보수적으로 "없음"으로 보고 생성 단계로 보낸다.
                    Ok(false)
                } else {
                    Err(ClipError::new(format!(
                        "클립 프로필 조회가 실패했습니다(HTTP status {})",
                        status.as_u16()
                    )))
                }
            }
        }
    }

    /// `NaverProfile` graphql로 기본 nickname/profileImageUrl을 받는다.
    pub async fn fetch_naver_profile(
        &self,
        cookie: Option<&str>,
    ) -> Result<(String, String), ClipError> {
        const QUERY: &str = "query NaverProfile {\n  naverProfile {\n    nickname\n    profileImageUrl\n    __typename\n  }\n}";
        let body = serde_json::json!({
            "operationName": "NaverProfile",
            "variables": {},
            "extensions": {"clientLibrary": {"name": "@apollo/client", "version": "4.1.9"}},
            "query": QUERY,
        })
        .to_string();
        let raw = self.post_graphql(&body, cookie).await?;
        let json: serde_json::Value =
            serde_json::from_str(json_slice(&raw)).map_err(|_| {
                ClipError::new("클립 NaverProfile 응답을 해석하지 못했습니다(형식 변경)")
            })?;
        let np = json.get("data").and_then(|d| d.get("naverProfile"));
        let nickname = np
            .and_then(|n| n.get("nickname"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let profile_image_url = np
            .and_then(|n| n.get("profileImageUrl"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        Ok((nickname, profile_image_url))
    }

    /// `SignUp` mutation으로 프로필을 만든다. 성공 = `data.signUp.__typename=="SignUpSucceed"`.
    pub async fn sign_up(
        &self,
        clip_id: &str,
        nickname: &str,
        profile_image_url: &str,
        cookie: Option<&str>,
    ) -> Result<(), ClipError> {
        const QUERY: &str = "mutation SignUp($input: SignUpInput!) {\n  signUp(input: $input) {\n    __typename\n    ... on SignUpSucceed {\n      user {\n        id\n        clipId\n        profileId\n        __typename\n      }\n      __typename\n    }\n    ... on CommonError {\n      message\n      code\n      __typename\n    }\n  }\n}";
        let body = serde_json::json!({
            "operationName": "SignUp",
            "variables": {"input": {
                "clipId": clip_id,
                "nickname": nickname,
                "profileImageUrl": profile_image_url,
            }},
            // 실측 브라우저와 동일하게 Apollo clientLibrary 확장을 싣는다(빈 {}와 차이 제거).
            "extensions": {"clientLibrary": {"name": "@apollo/client", "version": "4.1.9"}},
            "query": QUERY,
        })
        .to_string();
        let raw = self.post_graphql(&body, cookie).await?;
        // 진단: 실패 시 보낸 입력값(clipId/nickname/이미지유무)과 원본 응답을 에러에 남긴다 — 어떤
        // 입력이 -7020을 유발하는지 사후 식별용(쿠키는 없음). nickname은 그대로 노출(자격 증명 아님).
        let input_diag = format!(
            "입력[clipId={clip_id}, nickname=\"{nickname}\", img={}]",
            if profile_image_url.is_empty() {
                "(없음)"
            } else {
                "있음"
            }
        );
        match parse_sign_up(&raw) {
            SignUpResult::Succeed => Ok(()),
            // -7020 = 본인인증(실명·연령확인) 미완료 계정(실측 확인). 네이버 정책상 코드로 우회
            // 불가하므로, 사용자에게 본인인증을 먼저 하라고 명확히 안내한다(입력 문제 아님).
            SignUpResult::CommonError { code, .. } if code == "-7020" => Err(ClipError::new(
                "네이버 클립 댓글은 본인인증(실명·연령확인)이 완료된 계정만 가능합니다. 이 계정은 \
                 본인인증이 안 돼 있어요 — 네이버 클립에 직접 로그인해 본인인증을 먼저 완료한 뒤 \
                 다시 시도하세요(code=-7020)."
                    .to_string(),
            )),
            SignUpResult::CommonError { code, message } => Err(ClipError::new(format!(
                "클립 프로필 생성에 실패했습니다(code={code}, {message}). {input_diag} {}",
                response_diagnostic(&raw)
            ))),
            SignUpResult::Unknown => Err(ClipError::new(format!(
                "클립 프로필 생성 응답을 해석하지 못했습니다(형식 변경). {input_diag} {}",
                response_diagnostic(&raw)
            ))),
        }
    }

    /// graphql POST 공통(JSON 본문, Referer=signup, 위장 헤더·쿠키·UA).
    async fn post_graphql(&self, body: &str, cookie: Option<&str>) -> Result<String, ClipError> {
        let url = format!("{}/api/graphql", self.clip_base);
        let mut req = self
            .http
            .post(&url)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Content-Type", "application/json");
        for (name, value) in clip_graphql_headers(SIGNUP_REFERER) {
            if name == "Content-Type" {
                continue;
            }
            req = req.header(name, value);
        }
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let response = req.body(body.to_owned()).send().await.map_err(|e| {
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
                "클립 graphql 요청이 실패했습니다(HTTP status {})",
                status.as_u16()
            )));
        }
        Ok(raw)
    }
}

impl Default for ClipProfileClient {
    fn default() -> Self {
        Self::new()
    }
}

/// SignUp 응답 종류.
#[derive(Debug, PartialEq, Eq)]
enum SignUpResult {
    Succeed,
    CommonError { code: String, message: String },
    Unknown,
}

/// `data.signUp.__typename`으로 SignUp 결과를 판정한다(순수).
fn parse_sign_up(body: &str) -> SignUpResult {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(json_slice(body)) else {
        return SignUpResult::Unknown;
    };
    let Some(sign_up) = json.get("data").and_then(|d| d.get("signUp")) else {
        return SignUpResult::Unknown;
    };
    match sign_up.get("__typename").and_then(|v| v.as_str()) {
        Some("SignUpSucceed") => SignUpResult::Succeed,
        Some("CommonError") => SignUpResult::CommonError {
            code: sign_up
                .get("code")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            message: sign_up
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
        },
        _ => SignUpResult::Unknown,
    }
}

/// creatorhub 응답에서 `header.code`(정수)를 읽는다(순수). 없으면 None.
fn parse_header_code(body: &str) -> Option<i64> {
    let json: serde_json::Value = serde_json::from_str(json_slice(body)).ok()?;
    json.get("header")?.get("code")?.as_i64()
}

/// 클라이언트가 정하는 임시 clipId(서버가 나중에 변경 허용). 시간 기반 base36 영숫자 **10자**
/// (실측 성공값 "uizpj525n7"과 동일 길이), 항상 영문자로 시작(핸들 규칙 안전). 전역 유일성이
/// 필요하지만 충돌 확률은 낮고, 충돌 시 SignUp이 CommonError로 알려준다.
fn generate_clip_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut n = nanos;
    let alnum = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let letters = b"abcdefghijklmnopqrstuvwxyz";
    // 첫 글자는 영문자.
    let mut s = String::new();
    s.push(letters[(n % 26) as usize] as char);
    n /= 26;
    // 나머지 9자는 영숫자. nanos가 다 떨어지면(이론상) 'x'로 채워 항상 10자를 보장한다.
    while s.chars().count() < 10 {
        let c = if n > 0 {
            let ch = alnum[(n % 36) as usize];
            n /= 36;
            ch
        } else {
            b'x'
        };
        s.push(c as char);
    }
    s.chars().take(10).collect()
}

/// SignUp 닉네임을 보정한다(순수). 네이버 클립은 빈/숫자만/너무 짧은 닉네임을 거부(-7020)하므로,
/// **영문자가 1개 이상 + 2자 이상**일 때만 원본을 쓰고, 아니면 `fallback`(clipId 등 영숫자)을 쓴다.
fn safe_nickname(raw: &str, fallback: &str) -> String {
    let t = raw.trim();
    let ok = t.chars().count() >= 2 && t.chars().any(|c| c.is_alphabetic());
    if ok {
        t.to_string()
    } else {
        fallback.to_string()
    }
}

/// 진단 문자열(응답 길이 + 스니펫). 본문엔 쿠키가 없다.
fn response_diagnostic(body: &str) -> String {
    let snippet: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let snippet: String = snippet.chars().take(160).collect();
    format!("응답길이={}, 앞부분=\"{}\"", body.len(), snippet)
}

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
        matchers::{body_string_contains, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn parse_header_code_reads_code() {
        assert_eq!(
            parse_header_code(r#"{"header":{"code":0,"message":""}}"#),
            Some(0)
        );
        assert_eq!(
            parse_header_code(r#"{"header":{"code":-2102,"message":"없음"}}"#),
            Some(-2102)
        );
        assert_eq!(parse_header_code("not json"), None);
    }

    #[test]
    fn parse_sign_up_distinguishes_outcomes() {
        assert_eq!(
            parse_sign_up(r#"{"data":{"signUp":{"__typename":"SignUpSucceed","user":{"profileId":"P"}}}}"#),
            SignUpResult::Succeed
        );
        assert_eq!(
            parse_sign_up(r#"{"data":{"signUp":{"__typename":"CommonError","code":409,"message":"중복"}}}"#),
            SignUpResult::CommonError { code: "409".into(), message: "중복".into() }
        );
        assert_eq!(parse_sign_up("garbage"), SignUpResult::Unknown);
    }

    #[test]
    fn generate_clip_id_is_10_alnum_starts_letter() {
        let id = generate_clip_id();
        assert_eq!(id.chars().count(), 10, "실측 성공값과 동일하게 10자");
        assert!(id.chars().next().unwrap().is_ascii_alphabetic(), "첫 글자는 영문자");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn safe_nickname_replaces_digit_only_or_empty() {
        // 숫자만(-7020 유발)·빈·1자 → fallback. 영문 포함 2자+ → 원본.
        assert_eq!(safe_nickname("040", "ab12cd34ef"), "ab12cd34ef");
        assert_eq!(safe_nickname("", "ab12cd34ef"), "ab12cd34ef");
        assert_eq!(safe_nickname("a", "ab12cd34ef"), "ab12cd34ef");
        assert_eq!(safe_nickname("NULL", "fb"), "NULL");
        assert_eq!(safe_nickname("동찌", "fb"), "동찌"); // 한글도 alphabetic → 유지
    }

    #[tokio::test]
    async fn ensure_profile_skips_when_exists() {
        let creatorhub = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1.0/clip/profiles"))
            .and(header("x-creator-hub-sid", "clip"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"header":{"code":0,"message":""},"body":{"profileId":"P"}}"#))
            .mount(&creatorhub)
            .await;
        // clip_base는 안 쓰이지만 형식상 주입.
        let client = ClipProfileClient::with_base_urls("http://unused", creatorhub.uri());
        client.ensure_profile(None).await.expect("이미 있으면 스킵");
    }

    #[tokio::test]
    async fn ensure_profile_creates_when_missing() {
        let clip = MockServer::start().await;
        let creatorhub = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1.0/clip/profiles"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"header":{"code":-2102,"message":"없음"}}"#))
            .mount(&creatorhub)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .and(body_string_contains("NaverProfile"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data":{"naverProfile":{"nickname":"닉","profileImageUrl":"http://img"}}}"#))
            .mount(&clip)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .and(body_string_contains("SignUp"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data":{"signUp":{"__typename":"SignUpSucceed","user":{"id":"U","clipId":"c1","profileId":"P"}}}}"#))
            .mount(&clip)
            .await;
        let client = ClipProfileClient::with_base_urls(clip.uri(), creatorhub.uri());
        client.ensure_profile(None).await.expect("생성 성공");
    }

    #[tokio::test]
    async fn sign_up_common_error_surfaces_message_and_trace() {
        let clip = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data":{"signUp":{"__typename":"CommonError","code":409,"message":"이미 사용중"}}}"#))
            .mount(&clip)
            .await;
        let client = ClipProfileClient::with_base_urls(clip.uri(), "http://unused");
        let err = client
            .sign_up("cabc", "닉", "http://img", None)
            .await
            .expect_err("CommonError는 실패");
        assert!(err.message().contains("이미 사용중"));
        assert!(err.trace().contains("at "));
    }

    #[tokio::test]
    async fn sign_up_7020_maps_to_identity_verification_message() {
        // -7020 = 본인인증 미완료 → code 대신 본인인증 안내 메시지로 바꾼다.
        let clip = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"data":{"signUp":{"__typename":"CommonError","message":null,"code":-7020}}}"#,
            ))
            .mount(&clip)
            .await;
        let client = ClipProfileClient::with_base_urls(clip.uri(), "http://unused");
        let err = client
            .sign_up("iodsx8sl11", "iodsx8sl11", "http://img", None)
            .await
            .expect_err("-7020은 실패");
        assert!(err.message().contains("본인인증"));
        assert!(err.message().contains("-7020"));
    }
}
