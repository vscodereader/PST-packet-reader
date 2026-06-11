//! band 세션 쿠키 검증(네이버 `auth/accounts.rs` 쿠키부 미러, `band_session` 기준).
//!
//! band 로그인 성공 판정은 쿠키 `band_session`(domain contains `band.us`) 존재 + 미만료다.
//! (`BUC`는 네이버 쿠키이며 band은 발급하지 않는다 — 패킷 캡처로 확인.)
//! 만료 검사 로직은 네이버와 동일(`cookie_is_unexpired`).

use serde_json::Value;
use std::{fs, path::Path};

use crate::auth::{app_data_root, OrchestratorError};

use super::{
    paths::{band_cookie_file_path, band_cookies_dir},
    util::now_secs,
};

const REQUIRED_BAND_COOKIE_NAMES: [&str; 1] = ["band_session"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BandCookieStatus {
    Missing,
    Valid,
    Expired,
}

/// 계정의 저장된 band 쿠키를 읽어온다(만료 시 None).
///
/// 게시(forum) 등 band 쿠키를 소비하는 IPC는 후속 단계에서 배선되므로, 그때까지
/// 미사용 경고를 막는다(네이버 `read_account_cookies`의 band 대응).
#[allow(dead_code)]
pub(crate) fn read_band_account_cookies(
    account_id: &str,
) -> Result<Option<Value>, OrchestratorError> {
    let dir = band_cookies_dir(app_data_root()?);
    let path = band_cookie_file_path(&dir, account_id);
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let value = serde_json::from_str(&text)?;
    if has_valid_band_session_cookies(&value, now_secs()) {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

/// 앱 데이터 루트 기준 계정의 band 쿠키 상태를 조회한다.
pub(crate) fn account_band_cookie_status(
    account_id: &str,
) -> Result<BandCookieStatus, OrchestratorError> {
    let dir = band_cookies_dir(app_data_root()?);
    let path = band_cookie_file_path(&dir, account_id);
    if !path.exists() {
        return Ok(BandCookieStatus::Missing);
    }

    let text = fs::read_to_string(path)?;
    cookie_status_from_text(&text)
}

/// 저장된 band 쿠키 파일이 유효한 세션을 담고 있는지 확인한다.
pub(crate) fn has_valid_band_cookie_file(path: &Path) -> Result<bool, OrchestratorError> {
    if !path.exists() {
        return Ok(false);
    }

    let text = fs::read_to_string(path)?;
    let value = serde_json::from_str(&text)?;
    Ok(has_valid_band_session_cookies(&value, now_secs()))
}

fn cookie_status_from_text(text: &str) -> Result<BandCookieStatus, OrchestratorError> {
    let value = serde_json::from_str(text)?;
    Ok(cookie_status_from_value(&value, now_secs()))
}

fn has_valid_band_session_cookies(value: &Value, now_secs: u64) -> bool {
    cookie_status_from_value(value, now_secs) == BandCookieStatus::Valid
}

fn cookie_status_from_value(value: &Value, now_secs: u64) -> BandCookieStatus {
    let Some(cookies) = value.get("cookies").and_then(Value::as_array) else {
        return BandCookieStatus::Missing;
    };

    let mut saw_expired_required_cookie = false;

    for required_name in REQUIRED_BAND_COOKIE_NAMES {
        let matching_cookies: Vec<&Value> = cookies
            .iter()
            .filter(|cookie| is_required_band_cookie(cookie, required_name))
            .collect();
        if matching_cookies.is_empty() {
            return BandCookieStatus::Missing;
        }

        if !matching_cookies
            .iter()
            .any(|cookie| cookie_is_unexpired(cookie, now_secs))
        {
            saw_expired_required_cookie = true;
        }
    }

    if saw_expired_required_cookie {
        BandCookieStatus::Expired
    } else {
        BandCookieStatus::Valid
    }
}

fn is_required_band_cookie(cookie: &Value, required_name: &str) -> bool {
    let name_matches = cookie
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| name == required_name);
    let domain_matches = cookie
        .get("domain")
        .and_then(Value::as_str)
        .is_some_and(|domain| domain.contains("band.us"));
    name_matches && domain_matches
}

fn cookie_is_unexpired(cookie: &Value, now_secs: u64) -> bool {
    match cookie.get("expires").and_then(Value::as_f64) {
        Some(expires) if expires > 0.0 => expires > now_secs as f64,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_band_session_requires_session_cookie() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "band_session", "domain": ".band.us", "expires": now + 3600}
            ]
        });

        assert!(has_valid_band_session_cookies(&value, now));
        assert_eq!(
            cookie_status_from_value(&value, now),
            BandCookieStatus::Valid
        );
    }

    #[test]
    fn missing_buc_cookie_is_status_missing() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "OTHER", "domain": ".band.us", "expires": now + 3600}
            ]
        });

        assert!(!has_valid_band_session_cookies(&value, now));
        assert_eq!(
            cookie_status_from_value(&value, now),
            BandCookieStatus::Missing
        );
    }

    #[test]
    fn expired_buc_cookie_is_status_expired() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "band_session", "domain": ".band.us", "expires": now - 1}
            ]
        });

        assert!(!has_valid_band_session_cookies(&value, now));
        assert_eq!(
            cookie_status_from_value(&value, now),
            BandCookieStatus::Expired
        );
    }

    #[test]
    fn non_band_domain_buc_is_ignored() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "band_session", "domain": "example.com", "expires": now + 3600}
            ]
        });

        assert!(!has_valid_band_session_cookies(&value, now));
        assert_eq!(
            cookie_status_from_value(&value, now),
            BandCookieStatus::Missing
        );
    }

    #[test]
    fn missing_cookies_array_is_status_missing() {
        assert_eq!(
            cookie_status_from_value(&serde_json::json!({}), 0),
            BandCookieStatus::Missing
        );
    }

    #[test]
    fn session_cookie_without_positive_expiry_is_unexpired() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "band_session", "domain": ".band.us", "expires": -1}
            ]
        });

        // expires <= 0 또는 누락은 세션 쿠키로 간주 → 만료되지 않음.
        assert_eq!(
            cookie_status_from_value(&value, now),
            BandCookieStatus::Valid
        );
    }

    #[test]
    fn cookie_file_status_reports_missing_then_valid_from_disk() {
        let temp = tempfile::tempdir().unwrap();
        let now = now_secs();

        let valid = serde_json::json!({
            "cookies": [
                {"name": "band_session", "domain": ".band.us", "expires": now + 3600}
            ]
        });
        let cookie_path = temp.path().join("id1.json");

        // 파일이 없으면 유효하지 않다.
        assert!(!has_valid_band_cookie_file(&cookie_path).unwrap());

        fs::write(&cookie_path, valid.to_string()).unwrap();
        assert!(has_valid_band_cookie_file(&cookie_path).unwrap());
    }
}
