use std::{env, fs, path::PathBuf, process::Command};

use tauri::{AppHandle, Manager};

use super::{
    config,
    error::OrchestratorError,
    types::{Account, RuntimePaths},
    util::safe_file_stem,
};

/// Playwright를 사용하여 로그인 스크립트를 실행한다 (앱 핸들 포함).
pub async fn run_playwright_login(
    app: &AppHandle,
    paths: &RuntimePaths,
    account: &Account,
    headless: bool,
) -> Result<(), OrchestratorError> {
    let script = locate_login_script(Some(app))?;
    run_login_script(script, paths, account, headless).await
}

/// Playwright를 사용하여 로그인 스크립트를 실행한다 (앱 핸들 없이).
pub async fn run_playwright_login_without_app(
    paths: &RuntimePaths,
    account: &Account,
    headless: bool,
) -> Result<(), OrchestratorError> {
    let script = locate_login_script(None)?;
    run_login_script(script, paths, account, headless).await
}

async fn run_login_script(
    script: PathBuf,
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
        "cdpPort": config::find_free_port(),
    });
    fs::write(&input_path, serde_json::to_string_pretty(&input)?)?;

    let status = Command::new("node")
        .arg("--experimental-strip-types")
        .arg(script)
        .arg(&input_path)
        .status()?;
    let _ = fs::remove_file(&input_path);

    if !status.success() {
        return Err(OrchestratorError::CommandFailed(format!(
            "Playwright login exited with status {}",
            status
        )));
    }
    if !cookies_path.exists() {
        return Err(OrchestratorError::CommandFailed(
            "Playwright login did not write a cookie file".to_string(),
        ));
    }

    Ok(())
}

fn locate_login_script(app: Option<&AppHandle>) -> Result<PathBuf, OrchestratorError> {
    if let Ok(path) = env::var(config::LOGIN_SCRIPT_ENV) {
        return Ok(PathBuf::from(path));
    }

    if let Some(app) = app {
        if let Ok(path) = app.path().resolve(
            config::LOGIN_SCRIPT_PATH,
            tauri::path::BaseDirectory::Resource,
        ) {
            if path.exists() {
                return Ok(path);
            }
        }
        if let Ok(path) = app
            .path()
            .resolve(config::LOGIN_SCRIPT_FILENAME, tauri::path::BaseDirectory::Resource)
        {
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("src")
        .join("features")
        .join("playwright")
        .join(config::LOGIN_SCRIPT_FILENAME))
}
