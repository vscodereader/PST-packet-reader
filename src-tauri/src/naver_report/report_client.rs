//! 신고 사유 상수 + `/api/report` 바디 빌더(순수·테스트) + 저장 쿠키를 주입한 reqwest HTTP.
//!
//! 조회(by-item/profile)·제출(report)은 전부 순수 Rust HTTP다(설계서 §4). 쿠키는
//! `read_account_cookies`가 준 storageState JSON에서 naver.com 도메인만 뽑아 Cookie 헤더로 만든다
//! (`packet_client`의 host-scoped 규칙과 동일한 last-wins 정책 미러).

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Value};

/// 신고 사유(설계서 §2.4, service=FIN 실측 7개). `code`는 `reportReasonCode`로 전송된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportReason {
    /// 신고 사유 코드(예: `AA01`).
    pub code: &'static str,
    /// 사용자 표시 문구.
    pub label: &'static str,
}

/// 종목토론방(service=FIN) 신고 사유 7개(설계서 §2.4 실측). UI 라디오·검증에 그대로 쓴다.
pub const REPORT_REASONS: [ReportReason; 7] = [
    ReportReason {
        code: "AA01",
        label: "혐오/차별적/생명경시/욕설 표현입니다",
    },
    ReportReason {
        code: "AA29",
        label: "스팸홍보/도배입니다",
    },
    ReportReason {
        code: "AA14",
        label: "음란물입니다",
    },
    ReportReason {
        code: "AA68",
        label: "불법정보를 포함하고 있습니다",
    },
    ReportReason {
        code: "AA33",
        label: "청소년에게 유해한 내용입니다",
    },
    ReportReason {
        code: "AA24",
        label: "개인정보가 노출되었습니다",
    },
    ReportReason {
        code: "AB28",
        label: "불쾌한 표현이 있습니다",
    },
];

/// 사유 코드가 실측 7개 중 하나인지. 커맨드가 프론트 입력을 방어적으로 검증하는 데 쓴다.
pub fn is_valid_reason_code(code: &str) -> bool {
    REPORT_REASONS.iter().any(|reason| reason.code == code)
}

/// `POST /api/report`의 JSON 바디를 만든다(순수 함수 — 설계서 §2.3 실측 필드를 그대로 재현).
/// contentId/encryptedUserId/reasonCode/token만 가변이고 나머지는 실측 고정값이다.
pub fn build_report_body(
    content_id: &str,
    encrypted_user_id: &str,
    reason_code: &str,
    ncaptcha_token_id: &str,
) -> Value {
    json!({
        "serviceCode": "FIN",
        "virtualServiceCode": "FIN",
        "contentTypeCode": "AC01",
        "contentId": content_id,
        "contentWriterId": "",
        "contentWriterIdEncrypted": encrypted_user_id,
        "reportCountryCode": "",
        "reportLanguageCode": "",
        "environment": "pc",
        "reportReasonCode": reason_code,
        "addInformation": {},
        "ncaptchaTokenId": ncaptcha_token_id,
        "reportSystemId": ""
    })
}

/// naver.com 도메인 로그인 쿠키를 주입한 신고 전용 HTTP 클라이언트. `read_account_cookies`가 준
/// storageState JSON을 소비한다. by-item/profile GET과 report POST가 이 하나를 공유한다.
pub struct ReportHttp {
    client: reqwest::blocking::Client,
    /// (name, value) 쌍 — naver.com 도메인 쿠키만. Cookie 헤더로 직렬화된다.
    cookies: Vec<(String, String)>,
}

/// srp2 신고센터 API 오리진(설계서 §2.1).
const SRP2_REPORT_URL: &str = "https://srp2.naver.com/api/report";
/// 데스크톱 크롬 UA — Chrome 없이도 네이버 JSON API가 정상 응답하도록 카페 경로와 동일 UA 재사용.
const BROWSER_USER_AGENT: &str = crate::naver_cafe::post::client::BROWSER_USER_AGENT;

impl ReportHttp {
    /// storageState JSON(저장 쿠키)으로 클라이언트를 만든다. naver.com 도메인 쿠키만 로드하고,
    /// 세션 쿠키(NID_AUT/NID_SES)가 없으면 로그인 만료로 보고 명시적으로 실패한다(설계서 §2.5).
    pub fn from_storage_state(storage: &Value) -> Result<Self, String> {
        let mut cookies: Vec<(String, String)> = Vec::new();
        if let Some(arr) = storage.get("cookies").and_then(Value::as_array) {
            for cookie in arr {
                let (Some(domain), Some(name), Some(value)) = (
                    cookie.get("domain").and_then(Value::as_str),
                    cookie.get("name").and_then(Value::as_str),
                    cookie.get("value").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if domain.contains("naver.com") {
                    cookies.push((name.to_owned(), value.to_owned()));
                }
            }
        }
        let has = |name: &str| cookies.iter().any(|(n, _)| n == name);
        if !has("NID_AUT") || !has("NID_SES") {
            return Err(
                "저장된 로그인 쿠키에 네이버 세션(NID_AUT/NID_SES)이 없습니다. 계정을 다시 로그인하세요."
                    .to_owned(),
            );
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|error| format!("신고 HTTP 클라이언트 생성 실패: {error}"))?;
        Ok(Self { client, cookies })
    }

    /// 로드된 쿠키를 Cookie 헤더 문자열로 만든다(같은 이름은 last-wins, 이름순 정렬로 안정화).
    fn cookie_header(&self) -> String {
        let mut by_name: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, value) in &self.cookies {
            by_name.insert(name.as_str(), value.as_str());
        }
        by_name
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// stock.naver.com JSON API를 쿠키 달아 GET하고 JSON으로 파싱한다(by-item/profile 공용).
    pub fn get_json(&self, url: &str) -> Result<Value, String> {
        let response = self
            .client
            .get(url)
            .header("accept", "application/json, text/plain, */*")
            .header("referer", "https://stock.naver.com/")
            .header("user-agent", BROWSER_USER_AGENT)
            .header("cookie", self.cookie_header())
            .send()
            .map_err(|error| format!("GET 전송 실패: {error}"))?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|error| format!("GET 본문 읽기 실패: {error}"))?;
        if !status.is_success() {
            return Err(format!("GET 응답 실패(status={status}): {text}"));
        }
        serde_json::from_str(&text).map_err(|error| format!("GET JSON 파싱 실패: {error}"))
    }

    /// 완성된 신고 바디를 `POST /api/report`로 제출한다. `{"success":true}`면 Ok, 아니면 원문 오류.
    pub fn submit_report(&self, body: &Value) -> Result<(), String> {
        let response = self
            .client
            .post(SRP2_REPORT_URL)
            .header("content-type", "application/json")
            .header("x-requested-with", "XMLHttpRequest")
            .header("origin", "https://srp2.naver.com")
            .header("referer", "https://srp2.naver.com/report")
            .header("user-agent", BROWSER_USER_AGENT)
            .header("cookie", self.cookie_header())
            .body(body.to_string())
            .send()
            .map_err(|error| format!("report POST 전송 실패: {error}"))?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|error| format!("report 본문 읽기 실패: {error}"))?;
        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if parsed.get("success").and_then(Value::as_bool) == Some(true) {
            return Ok(());
        }
        Err(format!("신고 실패(status={status}): {text}"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn report_reasons_are_the_seven_measured_codes() {
        let codes: Vec<&str> = REPORT_REASONS.iter().map(|r| r.code).collect();
        assert_eq!(
            codes,
            ["AA01", "AA29", "AA14", "AA68", "AA33", "AA24", "AB28"]
        );
        // 라벨이 비어 있지 않고 유효성 검사가 코드와 일치한다.
        assert!(REPORT_REASONS.iter().all(|r| !r.label.is_empty()));
        assert!(is_valid_reason_code("AA01"));
        assert!(is_valid_reason_code("AB28"));
        assert!(!is_valid_reason_code("ZZ99"));
        assert!(!is_valid_reason_code(""));
    }

    #[test]
    fn build_report_body_matches_measured_packet_shape() {
        let body = build_report_body(
            "FIN_001;item;board;425406371",
            "vAOdyIfnpaqA=",
            "AA29",
            "token-xyz",
        );
        assert_eq!(body["serviceCode"], "FIN");
        assert_eq!(body["virtualServiceCode"], "FIN");
        assert_eq!(body["contentTypeCode"], "AC01");
        assert_eq!(body["contentId"], "FIN_001;item;board;425406371");
        assert_eq!(body["contentWriterId"], "");
        assert_eq!(body["contentWriterIdEncrypted"], "vAOdyIfnpaqA=");
        assert_eq!(body["reportCountryCode"], "");
        assert_eq!(body["reportLanguageCode"], "");
        assert_eq!(body["environment"], "pc");
        assert_eq!(body["reportReasonCode"], "AA29");
        assert_eq!(body["addInformation"], json!({}));
        assert_eq!(body["ncaptchaTokenId"], "token-xyz");
        assert_eq!(body["reportSystemId"], "");
        // 실측 바디는 정확히 13개 키를 가진다(누락·초과 방지).
        assert_eq!(body.as_object().unwrap().len(), 13);
    }

    #[test]
    fn from_storage_state_requires_naver_session_cookies() {
        let no_session = json!({
            "cookies": [ { "domain": ".naver.com", "name": "BUC", "value": "x" } ]
        });
        assert!(ReportHttp::from_storage_state(&no_session).is_err());

        let ok = json!({
            "cookies": [
                { "domain": ".naver.com", "name": "NID_AUT", "value": "a" },
                { "domain": ".naver.com", "name": "NID_SES", "value": "s" },
                { "domain": "example.com", "name": "OTHER", "value": "z" }
            ]
        });
        let http = ReportHttp::from_storage_state(&ok).unwrap();
        // 비-naver 도메인 쿠키는 버려지고, naver 쿠키만 Cookie 헤더에 들어간다.
        let header = http.cookie_header();
        assert!(header.contains("NID_AUT=a"));
        assert!(header.contains("NID_SES=s"));
        assert!(!header.contains("OTHER"));
    }
}
