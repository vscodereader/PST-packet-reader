//! 네이버 클립 댓글용 프로필 보장(#클립). 클립은 네이버 로그인만으로는 댓글을 못 달고, 처음에
//! "프로필 생성"을 한 번 해야 한다(사용자: 별다른 입력 없이 버튼만 누르면 됨). 자동화에서는 게시
//! 직전에 계정마다 이 단계를 보장한다. (패킷 분석으로 확정 2026-07-15 `네이버 클립 프로필.pcapng` — 재발견 말 것):
//!   1. `GET creatorhub-api/api/v1.0/clip/profiles`(헤더 x-creator-hub-sid: clip) →
//!      `header.code==0`이면 이미 프로필 있음(스킵), `-2102`면 없음(생성 진행).
//!   2. 없으면 `GET creatorhub-api/api/v6.0/clip/profiles/naver-profile`로 기본 nickname/profileImageUrl을 받고,
//!   3. `POST creatorhub-api/api/v5.0/clip/profiles`(본문 `{clipId,nickname,profileImageUrl}`)로 만든다
//!      (성공 = `header.code==0`, 본인인증 미완료 = `-7020`).
//!
//! ⚠️ 예전엔 `POST clip.naver.com/api/graphql`(NaverProfile/SignUp)을 썼으나 네이버가 그 엔드포인트를
//!    폐기해 **404**가 난다(실측). 위 creatorhub REST가 현재 유일한 경로다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 인증 자격 증명이다. 이 모듈은 쿠키 값을 로그/에러/Debug에 절대 포함하지 않는다.

use super::error::ClipError;
use super::headers::{creatorhub_headers, creatorhub_signup_headers};
use crate::naver_cafe::post::BROWSER_USER_AGENT;

const CREATORHUB_HOST: &str = "https://creatorhub-api.naver.com";

/// 네이버 클립 프로필 보장 클라이언트. creatorhub base_url을 보관해 실서버/wiremock을 함께 쓴다.
/// (프로필 조회·생성이 전부 creatorhub REST로 옮겨져 clip.naver.com base는 더는 필요 없다.)
pub struct ClipProfileClient {
    creatorhub_base: String,
    http: reqwest::Client,
}

impl ClipProfileClient {
    /// 실서버 호스트를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(CREATORHUB_HOST)
    }

    /// 주입된 creatorhub base_url을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(creatorhub_base: impl Into<String>) -> Self {
        Self {
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

    /// `GET creatorhub v6.0/clip/profiles/naver-profile`로 기본 nickname/profileImageUrl을 받는다(실측).
    /// 응답: `{"header":{"code":0},"body":{"nickname":..,"profileImageUrl":..}}`.
    pub async fn fetch_naver_profile(
        &self,
        cookie: Option<&str>,
    ) -> Result<(String, String), ClipError> {
        let url = format!(
            "{}/api/v6.0/clip/profiles/naver-profile",
            self.creatorhub_base
        );
        let raw = self.get_signup(&url, cookie).await?;
        let json: serde_json::Value = serde_json::from_str(json_slice(&raw)).map_err(|_| {
            ClipError::new("클립 naver-profile 응답을 해석하지 못했습니다(형식 변경)")
        })?;
        let body = json.get("body");
        let nickname = body
            .and_then(|b| b.get("nickname"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let profile_image_url = body
            .and_then(|b| b.get("profileImageUrl"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        Ok((nickname, profile_image_url))
    }

    /// `POST creatorhub v5.0/clip/profiles`로 프로필을 만든다(실측). 본문 `{clipId,nickname,profileImageUrl}`,
    /// 성공 = `header.code==0`. 본인인증 미완료 = `-7020`.
    pub async fn sign_up(
        &self,
        clip_id: &str,
        nickname: &str,
        profile_image_url: &str,
        cookie: Option<&str>,
    ) -> Result<(), ClipError> {
        let url = format!("{}/api/v5.0/clip/profiles", self.creatorhub_base);
        let body = serde_json::json!({
            "clipId": clip_id,
            "nickname": nickname,
            "profileImageUrl": profile_image_url,
        })
        .to_string();
        let raw = self.post_signup_json(&url, &body, cookie).await?;
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
        match parse_profile_result(&raw) {
            ProfileResult::Succeed => Ok(()),
            // -7020 = 본인인증(실명·연령확인) 미완료 계정(실측). 네이버 정책상 코드로 우회 불가하므로,
            // 사용자에게 본인인증을 먼저 하라고 명확히 안내한다(입력 문제 아님).
            ProfileResult::IdentityRequired => Err(ClipError::new(
                "네이버 클립 댓글은 본인인증(실명·연령확인)이 완료된 계정만 가능합니다. 이 계정은 \
                 본인인증이 안 돼 있어요 — 네이버 클립에 직접 로그인해 본인인증을 먼저 완료한 뒤 \
                 다시 시도하세요(code=-7020)."
                    .to_string(),
            )),
            ProfileResult::Error { code, message } => Err(ClipError::new(format!(
                "클립 프로필 생성에 실패했습니다(code={code}, {message}). {input_diag} {}",
                response_diagnostic(&raw)
            ))),
            ProfileResult::Unknown => Err(ClipError::new(format!(
                "클립 프로필 생성 응답을 해석하지 못했습니다(형식 변경). {input_diag} {}",
                response_diagnostic(&raw)
            ))),
        }
    }

    /// creatorhub GET 공통(signup 컨텍스트 헤더·쿠키·UA). 상태코드는 무시하고 본문을 돌려준다
    /// (creatorhub는 비-2xx에도 `header.code`로 사유를 준다 — 호출부가 판정).
    async fn get_signup(&self, url: &str, cookie: Option<&str>) -> Result<String, ClipError> {
        let mut req = self.http.get(url).header("User-Agent", BROWSER_USER_AGENT);
        for (name, value) in creatorhub_signup_headers() {
            req = req.header(name, value);
        }
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        Self::send_text(req).await
    }

    /// creatorhub POST 공통(JSON 본문, signup 컨텍스트 헤더·쿠키·UA).
    async fn post_signup_json(
        &self,
        url: &str,
        body: &str,
        cookie: Option<&str>,
    ) -> Result<String, ClipError> {
        let mut req = self
            .http
            .post(url)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Content-Type", "application/json");
        for (name, value) in creatorhub_signup_headers() {
            req = req.header(name, value);
        }
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        Self::send_text(req.body(body.to_owned())).await
    }

    /// 요청을 보내고 본문 텍스트만 받는다. 상태코드는 판정하지 않는다(creatorhub 규약).
    async fn send_text(req: reqwest::RequestBuilder) -> Result<String, ClipError> {
        let response = req.send().await.map_err(|e| {
            ClipError::new(crate::transport_error_message!(
                "HTTP 전송 오류가 발생했습니다",
                e
            ))
        })?;
        response
            .text()
            .await
            .map_err(|e| ClipError::new(format!("응답 본문 읽기 오류: {e}")))
    }
}

impl Default for ClipProfileClient {
    fn default() -> Self {
        Self::new()
    }
}

/// 프로필 생성 응답 종류(creatorhub `header.code` 기반).
#[derive(Debug, PartialEq, Eq)]
enum ProfileResult {
    Succeed,
    /// 본인인증(실명·연령) 미완료(code=-7020, 실측). 코드로 우회 불가 — 사용자 안내.
    IdentityRequired,
    Error { code: String, message: String },
    Unknown,
}

/// creatorhub 프로필 생성 응답을 판정한다(순수). `header.code==0`=성공, `-7020`(또는 메시지에
/// 본인인증/실명)=본인인증 미완료, 그 외 코드=에러. header가 없으면 Unknown(형식변경/HTML 404 등).
fn parse_profile_result(body: &str) -> ProfileResult {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(json_slice(body)) else {
        return ProfileResult::Unknown;
    };
    let Some(header) = json.get("header") else {
        return ProfileResult::Unknown;
    };
    let Some(code) = header.get("code").and_then(|c| c.as_i64()) else {
        return ProfileResult::Unknown;
    };
    let message = header
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    match code {
        0 => ProfileResult::Succeed,
        -7020 => ProfileResult::IdentityRequired,
        _ if message.contains("본인인증") || message.contains("실명") => {
            ProfileResult::IdentityRequired
        }
        other => ProfileResult::Error {
            code: other.to_string(),
            message,
        },
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
        matchers::{header, method, path},
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
    fn parse_profile_result_distinguishes_outcomes() {
        // 실측 성공 응답 모양(header.code==0 + body.profileId).
        assert_eq!(
            parse_profile_result(
                r#"{"header":{"code":0,"message":""},"body":{"profileId":"P","clipId":"c"}}"#
            ),
            ProfileResult::Succeed
        );
        assert_eq!(
            parse_profile_result(r#"{"header":{"code":-7020,"message":null}}"#),
            ProfileResult::IdentityRequired
        );
        assert_eq!(
            parse_profile_result(r#"{"header":{"code":409,"message":"이미 사용중"}}"#),
            ProfileResult::Error {
                code: "409".into(),
                message: "이미 사용중".into()
            }
        );
        assert_eq!(parse_profile_result("garbage"), ProfileResult::Unknown);
        // header 없는 HTML 404 등 → Unknown.
        assert_eq!(
            parse_profile_result("<html>404</html>"),
            ProfileResult::Unknown
        );
    }

    #[test]
    fn generate_clip_id_is_10_alnum_starts_letter() {
        let id = generate_clip_id();
        assert_eq!(id.chars().count(), 10, "실측 성공값과 동일하게 10자");
        assert!(
            id.chars().next().unwrap().is_ascii_alphabetic(),
            "첫 글자는 영문자"
        );
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
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"header":{"code":0,"message":""},"body":{"profileId":"P"}}"#,
                ),
            )
            .mount(&creatorhub)
            .await;
        let client = ClipProfileClient::with_base_url(creatorhub.uri());
        client.ensure_profile(None).await.expect("이미 있으면 스킵");
    }

    #[tokio::test]
    async fn ensure_profile_creates_when_missing() {
        // 실측 흐름: v1.0 없음(-2102) → v6.0 naver-profile로 닉/이미지 → v5.0 POST로 생성.
        let creatorhub = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1.0/clip/profiles"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"header":{"code":-2102,"message":"없음"}}"#),
            )
            .mount(&creatorhub)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v6.0/clip/profiles/naver-profile"))
            .and(header("x-creator-hub-sid", "clip"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"header":{"code":0},"body":{"nickname":"닉네임","profileImageUrl":"http://img"}}"#,
            ))
            .mount(&creatorhub)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v5.0/clip/profiles"))
            .and(header("x-creator-hub-sid", "clip"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"header":{"code":0},"body":{"profileId":"P","clipId":"c1"}}"#,
            ))
            .mount(&creatorhub)
            .await;
        let client = ClipProfileClient::with_base_url(creatorhub.uri());
        client.ensure_profile(None).await.expect("생성 성공");
    }

    #[tokio::test]
    async fn sign_up_error_surfaces_message_and_trace() {
        let creatorhub = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v5.0/clip/profiles"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"header":{"code":409,"message":"이미 사용중"}}"#,
            ))
            .mount(&creatorhub)
            .await;
        let client = ClipProfileClient::with_base_url(creatorhub.uri());
        let err = client
            .sign_up("cabc", "닉네임", "http://img", None)
            .await
            .expect_err("에러 코드는 실패");
        assert!(err.message().contains("이미 사용중"));
        assert!(err.trace().contains("at "));
    }

    #[tokio::test]
    async fn sign_up_7020_maps_to_identity_verification_message() {
        // -7020 = 본인인증 미완료 → code 대신 본인인증 안내 메시지로 바꾼다.
        let creatorhub = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v5.0/clip/profiles"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"header":{"code":-7020,"message":null}}"#,
            ))
            .mount(&creatorhub)
            .await;
        let client = ClipProfileClient::with_base_url(creatorhub.uri());
        let err = client
            .sign_up("iodsx8sl11", "iodsx8sl11", "http://img", None)
            .await
            .expect_err("-7020은 실패");
        assert!(err.message().contains("본인인증"));
        assert!(err.message().contains("-7020"));
    }
}
