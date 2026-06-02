mod accounts;
mod adb;
pub mod config;
mod error;
mod paths;
mod playwright;
mod queue;
mod types;
mod util;

use tauri::{AppHandle, Runtime};

pub use accounts::{read_account_cookies, read_account_cookies_unchecked, save_accounts_file};
pub use error::OrchestratorError;
pub use paths::{app_data_root, paths_for_root};
pub use queue::{enqueue_accounts, get_queue_status, QueueState};
pub use types::{Account, QueueJob, QueueJobStatus, QueueStatus, RuntimePaths};

use accounts::{has_valid_account_cookies, load_accounts_file};
use adb::{assert_adb_device, toggle_airplane_mode};
use paths::ensure_runtime_dirs;
use playwright::run_playwright_login;

/// 런타임 환경을 초기화하고 필요한 디렉토리와 도구들을 준비한다.
pub async fn bootstrap_runtime() -> Result<RuntimePaths, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    ensure_runtime_dirs(&paths)?;
    Ok(paths)
}

async fn process_account<R: Runtime>(
    app: &AppHandle<R>,
    account_id: &str,
    headless: bool,
    use_adb: bool,
) -> Result<(), OrchestratorError> {
    let paths = if use_adb {
        bootstrap_runtime().await?
    } else {
        let p = paths_for_root(app_data_root()?);
        ensure_runtime_dirs(&p)?;
        p
    };

    let accounts = load_accounts_file(&paths)?;
    let account = accounts
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| OrchestratorError::AccountNotFound(account_id.to_string()))?;

    if has_valid_account_cookies(&paths, account_id)? {
        return Ok(());
    }

    if use_adb {
        assert_adb_device().await?;
        toggle_airplane_mode().await?;
    }
    run_playwright_login(app, &paths, &account, headless).await
}
