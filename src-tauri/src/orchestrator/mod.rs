mod accounts;
mod adb;
mod error;
mod paths;
mod playwright;
mod queue;
mod scrcpy;
mod types;
mod util;

use tauri::AppHandle;

pub use accounts::{read_account_cookies, save_accounts_file};
pub use error::OrchestratorError;
pub use paths::{app_data_root, paths_for_root};
pub use queue::{enqueue_accounts, get_queue_status, QueueState};
pub use types::{Account, QueueJob, QueueJobStatus, QueueStatus, RuntimePaths};

use accounts::load_accounts_file;
use adb::{assert_adb_device, toggle_airplane_mode};
use paths::ensure_runtime_dirs;
use playwright::{run_playwright_login, run_playwright_login_without_app};
use scrcpy::ensure_adb;

pub async fn bootstrap_runtime() -> Result<RuntimePaths, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    ensure_runtime_dirs(&paths)?;
    ensure_adb(&paths).await?;
    Ok(paths)
}

async fn process_account(
    app: &AppHandle,
    account_id: &str,
    headless: bool,
) -> Result<(), OrchestratorError> {
    let paths = bootstrap_runtime().await?;
    let accounts = load_accounts_file(&paths)?;
    let account = accounts
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| OrchestratorError::AccountNotFound(account_id.to_string()))?;

    assert_adb_device(&paths.adb_path).await?;
    toggle_airplane_mode(&paths.adb_path).await?;
    run_playwright_login(app, &paths, &account, headless).await
}

pub async fn refresh_account_cookie(
    account: Account,
    headless: bool,
) -> Result<RuntimePaths, OrchestratorError> {
    let paths = bootstrap_runtime().await?;
    assert_adb_device(&paths.adb_path).await?;
    toggle_airplane_mode(&paths.adb_path).await?;
    run_playwright_login_without_app(&paths, &account, headless).await?;
    Ok(paths)
}
