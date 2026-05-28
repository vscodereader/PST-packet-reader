use serde_json::Value;
use std::fs;

use super::{
    error::OrchestratorError,
    paths::{app_data_root, ensure_runtime_dirs, paths_for_root},
    types::{Account, RuntimePaths},
    util::safe_file_stem,
};

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
    let path = paths
        .cookies_dir
        .join(format!("{}.json", safe_file_stem(account_id)));
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&text)?))
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
}
