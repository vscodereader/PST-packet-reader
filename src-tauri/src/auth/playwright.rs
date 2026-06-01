use std::{fs, path::Path, time::Duration};

use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;
use tokio::time::{sleep, Instant};

use super::{
    accounts::has_valid_cookie_file,
    config,
    error::OrchestratorError,
    types::{Account, RuntimePaths},
    util::safe_file_stem,
};

pub async fn run_playwright_login(
    app: &AppHandle,
    paths: &RuntimePaths,
    account: &Account,
    headless: bool,
) -> Result<(), OrchestratorError> {
    let input_path = paths
        .root
        .join(format!("login-{}.json", safe_file_stem(&account.id)));
    let cookies_path = paths
        .cookies_dir
        .join(format!("{}.json", safe_file_stem(&account.id)));

    let chrome = config::chrome_path().map_err(OrchestratorError::CommandFailed)?;

    let input = serde_json::json!({
        "accountId": account.id,
        "id": account.id,
        "password": account.password,
        "cookiesPath": cookies_path,
        "headless": headless,
        "chromePath": chrome,
    });
    fs::write(&input_path, serde_json::to_string_pretty(&input)?)?;

    let output = app
        .shell()
        .sidecar("naver-login")
        .map_err(|e| OrchestratorError::CommandFailed(e.to_string()))?
        .arg(input_path.to_string_lossy().as_ref())
        .output()
        .await
        .map_err(|e| OrchestratorError::CommandFailed(format!("sidecar spawn failed: {e}")))?;

    let _ = fs::remove_file(&input_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(OrchestratorError::CommandFailed(format!(
            "Playwright login failed\nstderr: {}\nstdout: {}",
            stderr.trim(),
            stdout.trim(),
        )));
    }

    wait_for_valid_cookie_file(&cookies_path).await?;

    Ok(())
}

async fn wait_for_valid_cookie_file(path: &Path) -> Result<(), OrchestratorError> {
    let timeout = Duration::from_secs(config::COOKIE_WRITE_TIMEOUT_SECS);
    let interval = Duration::from_millis(config::COOKIE_WRITE_POLL_INTERVAL_MS);
    let deadline = Instant::now() + timeout;

    loop {
        if has_valid_cookie_file(path).unwrap_or(false) {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(OrchestratorError::CommandFailed(format!(
                "Playwright login did not write valid cookies within {} seconds",
                config::COOKIE_WRITE_TIMEOUT_SECS
            )));
        }

        sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_returns_immediately_for_valid_cookie_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cookies.json");
        let cookie = serde_json::json!({
            "cookies": [
                {"name": "NID_AUT", "domain": ".naver.com", "expires": 9_999_999_999u64},
                {"name": "NID_SES", "domain": ".naver.com", "expires": 9_999_999_999u64}
            ]
        });
        fs::write(&path, cookie.to_string()).unwrap();

        // 유효한 쿠키가 이미 있으면 첫 폴링에서 즉시 Ok.
        wait_for_valid_cookie_file(&path).await.unwrap();
    }
}
