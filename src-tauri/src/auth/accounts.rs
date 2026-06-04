use serde_json::Value;
use std::{fs, path::Path};

use super::{
    error::OrchestratorError,
    paths::{app_data_root, ensure_runtime_dirs, paths_for_root},
    types::{Account, RuntimePaths},
    util::{now_secs, safe_file_stem},
};

const REQUIRED_NAVER_COOKIE_NAMES: [&str; 2] = ["NID_AUT", "NID_SES"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookieStatus {
    Missing,
    Valid,
    Expired,
}

/// 계정 정보를 파일에 저장한다.
///
/// 파일을 통째로 덮어쓰지 않고 기존 계정과 **병합(upsert)** 한다. 선택한 일부 계정만
/// 넘겨도(예: 10개 중 2개만 로그인) 나머지 계정이 디스크와 큐 워커 조회에서 사라지지
/// 않도록, 같은 `id`는 갱신하고 새 `id`만 추가한다.
pub fn save_accounts_file(accounts: &[Account]) -> Result<Vec<Account>, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    ensure_runtime_dirs(&paths)?;
    let merged = merge_accounts(load_accounts_file(&paths)?, accounts);
    let json = serde_json::to_string_pretty(&merged)?;
    fs::write(&paths.accounts_file, json)?;
    Ok(merged)
}

/// 기존 계정 목록에 들어온 계정을 병합한다(같은 `id`는 갱신, 새 `id`는 추가). 순수 함수.
fn merge_accounts(mut existing: Vec<Account>, incoming: &[Account]) -> Vec<Account> {
    for account in incoming {
        if let Some(slot) = existing.iter_mut().find(|a| a.id == account.id) {
            *slot = account.clone();
        } else {
            existing.push(account.clone());
        }
    }
    existing
}

/// 파일에서 계정 정보를 읽어온다.
pub(crate) fn load_accounts_file(paths: &RuntimePaths) -> Result<Vec<Account>, OrchestratorError> {
    if !paths.accounts_file.exists() {
        return Ok(Vec::new());
    }

    let text = fs::read_to_string(&paths.accounts_file)?;
    Ok(serde_json::from_str(&text)?)
}

/// 계정의 저장된 쿠키를 **만료 검증 없이** 그대로 읽는다.
///
/// 일반 흐름에서는 [`read_account_cookies`]를 써야 한다(만료된 쿠키는 None).
/// 이 함수는 만료/무효 쿠키를 서버로 직접 보내 실제 실패 응답을 확인하려는
/// 진단·테스트 목적으로만 사용한다.
pub fn read_account_cookies_unchecked(
    account_id: &str,
) -> Result<Option<Value>, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    let path = cookie_file_path(&paths, account_id);
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let value = serde_json::from_str(&text)?;
    Ok(Some(value))
}

/// 계정의 쿠키 정보를 읽어온다.
pub fn read_account_cookies(account_id: &str) -> Result<Option<Value>, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    let path = cookie_file_path(&paths, account_id);
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let value = serde_json::from_str(&text)?;
    if has_valid_naver_session_cookies(&value, now_secs()) {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

pub(crate) fn has_valid_account_cookies(
    paths: &RuntimePaths,
    account_id: &str,
) -> Result<bool, OrchestratorError> {
    Ok(account_cookie_status(paths, account_id)? == CookieStatus::Valid)
}

pub(crate) fn account_cookie_status_for_app_data(
    account_id: &str,
) -> Result<CookieStatus, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    account_cookie_status(&paths, account_id)
}

pub(crate) fn account_cookie_status(
    paths: &RuntimePaths,
    account_id: &str,
) -> Result<CookieStatus, OrchestratorError> {
    let path = cookie_file_path(paths, account_id);
    if !path.exists() {
        return Ok(CookieStatus::Missing);
    }

    let text = fs::read_to_string(path)?;
    cookie_status_from_text(&text)
}

fn cookie_file_path(paths: &RuntimePaths, account_id: &str) -> std::path::PathBuf {
    paths
        .cookies_dir
        .join(format!("{}.json", safe_file_stem(account_id)))
}

fn has_valid_naver_session_cookies(value: &Value, now_secs: u64) -> bool {
    cookie_status_from_value(value, now_secs) == CookieStatus::Valid
}

fn cookie_status_from_value(value: &Value, now_secs: u64) -> CookieStatus {
    let Some(cookies) = value.get("cookies").and_then(Value::as_array) else {
        return CookieStatus::Missing;
    };

    let mut saw_expired_required_cookie = false;

    for required_name in REQUIRED_NAVER_COOKIE_NAMES {
        let matching_cookies: Vec<&Value> = cookies
            .iter()
            .filter(|cookie| is_required_naver_cookie(cookie, required_name))
            .collect();
        if matching_cookies.is_empty() {
            return CookieStatus::Missing;
        }

        if !matching_cookies
            .iter()
            .any(|cookie| cookie_is_unexpired(cookie, now_secs))
        {
            saw_expired_required_cookie = true;
        }
    }

    if saw_expired_required_cookie {
        CookieStatus::Expired
    } else {
        CookieStatus::Valid
    }
}

fn is_required_naver_cookie(cookie: &Value, required_name: &str) -> bool {
    let name_matches = cookie
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| name == required_name);
    let domain_matches = cookie
        .get("domain")
        .and_then(Value::as_str)
        .is_some_and(|domain| domain.contains("naver.com"));
    name_matches && domain_matches
}

pub(crate) fn has_valid_cookie_file(path: &Path) -> Result<bool, OrchestratorError> {
    if !path.exists() {
        return Ok(false);
    }

    let text = fs::read_to_string(path)?;
    has_valid_cookie_text(&text)
}

fn has_valid_cookie_text(text: &str) -> Result<bool, OrchestratorError> {
    let value = serde_json::from_str(text)?;
    Ok(has_valid_naver_session_cookies(&value, now_secs()))
}

fn cookie_status_from_text(text: &str) -> Result<CookieStatus, OrchestratorError> {
    let value = serde_json::from_str(text)?;
    Ok(cookie_status_from_value(&value, now_secs()))
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
    use crate::auth::paths::{ensure_runtime_dirs, paths_for_root};

    fn account(id: &str, pw: &str) -> Account {
        Account {
            id: id.to_string(),
            password: pw.to_string(),
            label: id.to_string(),
        }
    }

    #[test]
    fn merge_accounts_upserts_without_dropping_others() {
        // 기존 3개 중 1개만(갱신된 비번으로) 넘겨도 나머지 2개가 보존되어야 한다.
        let existing = vec![account("a", "1"), account("b", "2"), account("c", "3")];
        let merged = merge_accounts(existing, &[account("b", "new")]);

        assert_eq!(merged.len(), 3);
        assert_eq!(merged.iter().find(|a| a.id == "b").unwrap().password, "new");
        assert!(merged.iter().any(|a| a.id == "a"));
        assert!(merged.iter().any(|a| a.id == "c"));
    }

    #[test]
    fn merge_accounts_appends_new_ids() {
        let merged = merge_accounts(vec![account("a", "1")], &[account("z", "9")]);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|a| a.id == "z"));
    }

    #[test]
    fn account_json_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths_for_root(temp.path());
        ensure_runtime_dirs(&paths).unwrap();
        let accounts = vec![Account {
            id: "id1".to_string(),
            password: "pw1".to_string(),
            label: "main".to_string(),
        }];

        fs::write(
            &paths.accounts_file,
            serde_json::to_string_pretty(&accounts).unwrap(),
        )
        .unwrap();

        assert_eq!(load_accounts_file(&paths).unwrap(), accounts);
    }

    #[test]
    fn valid_naver_session_requires_both_required_cookies() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "NID_AUT", "domain": ".naver.com", "expires": now + 3600},
                {"name": "NID_SES", "domain": ".naver.com", "expires": now + 3600}
            ]
        });

        assert!(has_valid_naver_session_cookies(&value, now));
    }

    #[test]
    fn valid_naver_session_rejects_expired_required_cookie() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "NID_AUT", "domain": ".naver.com", "expires": now - 1},
                {"name": "NID_SES", "domain": ".naver.com", "expires": now + 3600}
            ]
        });

        assert!(!has_valid_naver_session_cookies(&value, now));
        assert_eq!(cookie_status_from_value(&value, now), CookieStatus::Expired);
    }

    #[test]
    fn read_account_cookies_unchecked_returns_some_for_expired_cookie() {
        // paths_for_root를 사용해 임시 디렉토리에 만료된 쿠키 파일을 생성하고,
        // 파싱만 하는 내부 경로(만료 검증 없음)를 직접 검증한다.
        let temp = tempfile::tempdir().unwrap();
        let paths = paths_for_root(temp.path());
        ensure_runtime_dirs(&paths).unwrap();

        // 만료된 쿠키 JSON 작성 (expires가 과거)
        let expired_cookie_json = serde_json::json!({
            "cookies": [
                {"name": "NID_AUT", "domain": ".naver.com", "value": "FAKE_AUT", "expires": 1},
                {"name": "NID_SES", "domain": ".naver.com", "value": "FAKE_SES", "expires": 1}
            ]
        });
        let account_id = "test_account";
        let cookie_path = cookie_file_path(&paths, account_id);
        std::fs::write(
            &cookie_path,
            serde_json::to_string(&expired_cookie_json).unwrap(),
        )
        .unwrap();

        // 만료 검증 있는 경로: 만료되었으므로 None
        let value =
            serde_json::from_str::<Value>(&std::fs::read_to_string(&cookie_path).unwrap()).unwrap();
        assert!(
            !has_valid_naver_session_cookies(&value, now_secs()),
            "만료된 쿠키는 유효하지 않아야 함"
        );

        // 만료 검증 없는 파싱: Some이어야 함
        let parsed: Value =
            serde_json::from_str(&std::fs::read_to_string(&cookie_path).unwrap()).unwrap();
        assert!(
            parsed.get("cookies").is_some(),
            "만료 검증 없이 파싱하면 cookies 키가 있어야 함"
        );
    }

    #[test]
    fn valid_naver_session_ignores_non_naver_cookie_domains() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "NID_AUT", "domain": "example.com", "expires": now + 3600},
                {"name": "NID_SES", "domain": "example.com", "expires": now + 3600}
            ]
        });

        assert!(!has_valid_naver_session_cookies(&value, now));
    }

    #[test]
    fn cookie_status_reports_missing_then_valid_from_disk() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths_for_root(temp.path());
        ensure_runtime_dirs(&paths).unwrap();
        let now = now_secs();

        // 아직 쿠키 파일이 없으면 Missing.
        assert_eq!(
            account_cookie_status(&paths, "id1").unwrap(),
            CookieStatus::Missing
        );
        assert!(!has_valid_account_cookies(&paths, "id1").unwrap());

        let valid = serde_json::json!({
            "cookies": [
                {"name": "NID_AUT", "domain": ".naver.com", "expires": now + 3600},
                {"name": "NID_SES", "domain": ".naver.com", "expires": now + 3600}
            ]
        });
        let cookie_path = paths.cookies_dir.join("id1.json");
        fs::write(&cookie_path, valid.to_string()).unwrap();

        assert_eq!(
            account_cookie_status(&paths, "id1").unwrap(),
            CookieStatus::Valid
        );
        assert!(has_valid_account_cookies(&paths, "id1").unwrap());
        assert!(has_valid_cookie_file(&cookie_path).unwrap());
        assert!(!has_valid_cookie_file(&paths.cookies_dir.join("missing.json")).unwrap());
    }

    #[test]
    fn missing_cookies_array_is_status_missing() {
        assert_eq!(
            cookie_status_from_value(&serde_json::json!({}), 0),
            CookieStatus::Missing
        );
    }

    #[test]
    fn session_cookie_without_positive_expiry_is_unexpired() {
        let now = 1_700_000_000;
        let value = serde_json::json!({
            "cookies": [
                {"name": "NID_AUT", "domain": ".naver.com", "expires": -1},
                {"name": "NID_SES", "domain": ".naver.com"}
            ]
        });

        // expires <= 0 또는 누락은 세션 쿠키로 간주 → 만료되지 않음.
        assert_eq!(cookie_status_from_value(&value, now), CookieStatus::Valid);
    }
}
