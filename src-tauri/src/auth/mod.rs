mod accounts;
mod adb;
mod chrome;
pub mod config;
mod error;
mod login;
mod login_flow;
mod paths;
mod queue;
mod types;
mod util;

use tauri::{AppHandle, Runtime};

pub use accounts::{read_account_cookies, save_accounts_file};
pub use adb::probe_adb_connection;
// 게시(forum)에서도 로그인과 같은 Chrome 런처를 재사용해, 디버그 포트 Chrome을 앱이 직접 띄운다.
pub(crate) use chrome::launch as launch_debug_chrome;
pub use error::OrchestratorError;
pub use paths::{app_data_root, paths_for_root};
pub use queue::{enqueue_accounts, get_queue_status, QueueState};
pub use types::{Account, QueueJob, QueueJobStatus, QueueStatus, RuntimePaths};

use accounts::{has_valid_account_cookies, load_accounts_file};
use adb::{assert_adb_device, toggle_airplane_mode};
use paths::ensure_runtime_dirs;

/// 런타임 환경을 초기화하고 필요한 디렉토리와 도구들을 준비한다.
pub async fn bootstrap_runtime() -> Result<RuntimePaths, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    ensure_runtime_dirs(&paths)?;
    Ok(paths)
}

// `_app`은 sidecar 시절 shell 실행에 쓰였으나, CDP 로그인으로 전환하며 더는 쓰이지 않는다.
// 큐 워커가 `AppHandle<R>`를 넘기므로(IPC 테스트의 MockRuntime 포함) 제네릭 시그니처는
// 유지하되, 본문은 CDP 로그인을 직접 호출하므로 핸들은 사용하지 않는다.
async fn process_account<R: Runtime>(
    _app: &AppHandle<R>,
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
        // IP가 바뀐 뒤 네트워크가 안정될 시간을 주고 나서 Chrome을 띄운다(사수 권고).
        // 직전 계정의 Chrome은 직전 login() 반환 시 ChromeHandle Drop에서 kill+wait로
        // 이미 완전히 종료되며, 그 사실이 "[CHROME] ✓ ... 완전 종료 확인" 로그로 남는다.
        eprintln!(
            "[LOGIN] IP 변경 확인 — {}초 안정화 대기 후 Chrome 실행",
            config::ADB_SETTLE_AFTER_ROTATE_SECS
        );
        tokio::time::sleep(std::time::Duration::from_secs(
            config::ADB_SETTLE_AFTER_ROTATE_SECS,
        ))
        .await;
    }

    // CDP 로그인은 Chrome을 띄워 동기적으로 동작하므로 blocking 스레드에서 실행한다.
    tauri::async_runtime::spawn_blocking(move || login::login(&paths, &account, headless))
        .await
        .map_err(|error| OrchestratorError::CommandFailed(format!("로그인 스레드 오류: {error}")))?
}
