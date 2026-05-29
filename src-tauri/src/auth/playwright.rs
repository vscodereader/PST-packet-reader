use std::fs;

use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use super::{
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

    let input = serde_json::json!({
        "accountId": account.id,
        "id": account.id,
        "password": account.password,
        "cookiesPath": cookies_path,
        "headless": headless,
        "chromePath": config::chrome_path(),
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

    if !cookies_path.exists() {
        return Err(OrchestratorError::CommandFailed(
            "Playwright login did not write a cookie file".to_string(),
        ));
    }

    Ok(())
}
