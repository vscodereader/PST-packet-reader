use serde_json::Value;
use std::fs;

use super::{
    error::OrchestratorError,
    paths::{app_data_root, ensure_runtime_dirs, paths_for_root},
    types::{Account, RuntimePaths},
    util::{now_secs, safe_file_stem},
};

const REQUIRED_NAVER_COOKIE_NAMES: [&str; 2] = ["NID_AUT", "NID_SES"];

/// 계정 정보를 파일에 저장한다.
pub fn save_accounts_file(accounts: &[Account]) -> Result<Vec<Account>, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    ensure_runtime_dirs(&paths)?;
    let json = serde_json::to_string_pretty(accounts)?;
    fs::write(&paths.accounts_file, json)?;
    Ok(accounts.to_vec())
}

/// 파일에서 계정 정보를 읽어온다.
pub(crate) fn load_accounts_file(paths: &RuntimePaths) -> Result<Vec<Account>, OrchestratorError> {
    if !paths.accounts_file.exists() {
        return Ok(Vec::new());
    }

    let text = fs::read_to_string(&paths.accounts_file)?;
    Ok(serde_json::from_str(&text)?)
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
    let path = cookie_file_path(paths, account_id);
    if !path.exists() {
        return Ok(false);
    }

    let text = fs::read_to_string(path)?;
    let value = serde_json::from_str(&text)?;
    Ok(has_valid_naver_session_cookies(&value, now_secs()))
}

fn cookie_file_path(paths: &RuntimePaths, account_id: &str) -> std::path::PathBuf {
    paths
        .cookies_dir
        .join(format!("{}.json", safe_file_stem(account_id)))
}

fn has_valid_naver_session_cookies(value: &Value, now_secs: u64) -> bool {
    let Some(cookies) = value.get("cookies").and_then(Value::as_array) else {
        return false;
    };

    REQUIRED_NAVER_COOKIE_NAMES.iter().all(|required_name| {
        cookies.iter().any(|cookie| {
            let name_matches = cookie
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name == *required_name);
            let domain_matches = cookie
                .get("domain")
                .and_then(Value::as_str)
                .is_some_and(|domain| domain.contains("naver.com"));
            name_matches && domain_matches && cookie_is_unexpired(cookie, now_secs)
        })
    })
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
}
