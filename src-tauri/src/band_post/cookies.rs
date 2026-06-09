//! band 세션 쿠키를 HTTP `Cookie` 헤더 문자열로 변환한다.
//!
//! 로그인([`band_auth`](crate::band_auth))이 저장한 `cookies-band/{id}.json`
//! (Playwright storage-state 형태)에서 band.us 도메인 쿠키만 골라
//! `name=value; name=value` 헤더 값을 만든다.
//!
//! # 보안
//! 반환 문자열은 사용자 인증 자격 증명이다. 로그·에러·`Debug`에 절대 포함하지 않는다.

use serde_json::Value;

use crate::auth::{app_data_root, OrchestratorError};

use super::util::safe_file_stem;

/// band 쿠키 저장 디렉토리 이름(band_auth와 동일 위치를 읽는다).
const DIR_COOKIES_BAND: &str = "cookies-band";

/// 계정의 저장된 band 쿠키 파일을 읽어 `Cookie` 헤더 문자열을 만든다.
///
/// 파일이 없거나 band 쿠키가 없으면 `Ok(None)`. 로그인([`band_auth`](crate::band_auth))이
/// `cookies-band/{id}.json`에 저장한 것을 소비한다.
pub fn load_band_cookie_header(account_id: &str) -> Result<Option<String>, OrchestratorError> {
    let path = app_data_root()?
        .join(DIR_COOKIES_BAND)
        .join(format!("{}.json", safe_file_stem(account_id)));
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let value: Value = serde_json::from_str(&text)?;
    Ok(band_cookie_header(&value))
}

/// storage-state JSON에서 band.us 도메인 쿠키만 추출해 `Cookie` 헤더 값을 만든다.
///
/// `domain` 필드가 `band.us`를 포함하는 쿠키만 포함하며, 유효한 쿠키가 없으면
/// `None`을 반환한다.
pub fn band_cookie_header(value: &Value) -> Option<String> {
    let cookies = value.get("cookies")?.as_array()?;

    let pairs: Vec<String> = cookies
        .iter()
        .filter_map(|cookie| {
            let domain = cookie.get("domain")?.as_str()?;
            if !domain.contains("band.us") {
                return None;
            }
            let name = cookie.get("name")?.as_str()?;
            let val = cookie.get("value")?.as_str()?;
            Some(format!("{name}={val}"))
        })
        .collect();

    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn includes_band_cookies_excludes_others() {
        let state = json!({
            "cookies": [
                {"name": "BUC", "value": "FAKE_BUC", "domain": ".band.us"},
                {"name": "SECRET_KEY_COOKIE", "value": "FAKE", "domain": "auth.band.us"},
                {"name": "GA", "value": "should_not_appear", "domain": ".google.com"}
            ]
        });
        let header = band_cookie_header(&state).expect("쿠키 헤더 생성");
        assert!(header.contains("BUC=FAKE_BUC"));
        assert!(header.contains("SECRET_KEY_COOKIE=FAKE"));
        assert!(!header.contains("should_not_appear"));
        assert!(!header.contains("GA="));
    }

    #[test]
    fn joins_pairs_with_semicolon() {
        let state = json!({
            "cookies": [
                {"name": "A", "value": "1", "domain": ".band.us"},
                {"name": "B", "value": "2", "domain": "www.band.us"}
            ]
        });
        let header = band_cookie_header(&state).unwrap();
        assert_eq!(header, "A=1; B=2");
    }

    #[test]
    fn none_when_no_band_cookies() {
        let state = json!({ "cookies": [
            {"name": "X", "value": "y", "domain": ".example.com"}
        ]});
        assert!(band_cookie_header(&state).is_none());
    }

    #[test]
    fn none_for_empty_or_missing() {
        assert!(band_cookie_header(&json!({ "cookies": [] })).is_none());
        assert!(band_cookie_header(&json!({})).is_none());
    }
}
