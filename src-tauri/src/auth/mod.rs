mod accounts;
mod adb;
mod chrome;
pub mod config;
mod error;
mod login;
mod login_flow;
// band_auth가 로그인 결과 매핑(`LoginResolution`)을 재사용하도록 크레이트 내부에 공개한다.
pub(crate) mod outcome;
mod paths;
mod types;
mod util;

use tauri::{AppHandle, Runtime};

pub use accounts::{read_account_cookies, read_account_cookies_unchecked, save_accounts_file};
pub use adb::probe_adb_connection;
// 게시(forum)에서도 로그인과 같은 Chrome 런처를 재사용해, 디버그 포트 Chrome을 앱이 직접 띄운다.
pub(crate) use chrome::launch as launch_debug_chrome;
pub use error::OrchestratorError;
pub use paths::{app_data_root, paths_for_root};
pub use types::{Account, RuntimePaths};
// 로그용 ID 마스킹 헬퍼를 다른 모듈(예: discussion_batch)에서도 쓸 수 있게 재노출.
pub(crate) use util::mask_id;

use accounts::{has_valid_account_cookies, load_accounts_file};
// band_auth가 ADB IP 회전을 재사용하도록 재노출한다(동작 무변경 — 기존 private import의
// 가시성만 crate 범위로 넓힌다). process_account도 이 경로로 동일하게 호출한다.
pub(crate) use adb::{assert_adb_device, fetch_external_ip, toggle_airplane_mode};
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
pub(crate) async fn process_account<R: Runtime>(
    _app: &AppHandle<R>,
    account_id: &str,
    headless: bool,
    use_adb: bool,
    force: bool,
) -> Result<outcome::LoginResolution, OrchestratorError> {
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

    // 로컬 쿠키가 유효해 보여도, 명시적 재로그인(force)이면 단락하지 않고 실제 로그인을
    // 수행해 새 쿠키로 덮어쓴다. 로컬 검증은 서버측에서 죽은 세션을 가려낼 수 없기 때문
    // (세션 쿠키는 항상 "안 만료"로 통과)이며, 이 단락이 그대로면 죽은 쿠키가 영원히
    // 남는다(이슈 #132).
    if should_skip_login(force, has_valid_account_cookies(&paths, account_id)?) {
        // 로컬 쿠키가 유효해 단락 — 이미 로그인된 상태로 간주한다.
        return Ok(outcome::LoginResolution::active());
    }

    if use_adb {
        assert_adb_device().await?;
        toggle_airplane_mode().await?;
        // IP가 바뀐 뒤 네트워크가 안정될 시간을 주고 나서 Chrome을 띄운다(사수 권고).
        // 직전 계정의 Chrome은 직전 login() 반환 시 ChromeHandle Drop에서 kill+wait로
        // 이미 완전히 종료되며, 그 사실이 "[CHROME] ✓ ... 완전 종료 확인" 로그로 남는다.
        tracing::info!(
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

// 로컬 쿠키가 유효해 보일 때 실제 로그인을 건너뛸지(단락) 판정한다. 단, 명시적 재로그인
// (force)이면 로컬 캐시를 무시하고 절대 건너뛰지 않는다(이슈 #132).
fn should_skip_login(force: bool, has_valid_cookies: bool) -> bool {
    !force && has_valid_cookies
}

#[cfg(test)]
mod tests {
    use super::should_skip_login;

    #[test]
    fn force_never_skips_even_with_valid_cookies() {
        // 죽었지만 로컬 검증만 통과하는 쿠키를 새 값으로 덮어쓰려면, force일 때는
        // 유효해 보여도 건너뛰지 않고 실제 로그인을 해야 한다.
        assert!(!should_skip_login(true, true));
        assert!(!should_skip_login(true, false));
    }

    #[test]
    fn non_force_skips_only_when_cookies_look_valid() {
        // 비강제(예: 전체 실행)는 기존 단락을 유지해 살아있는 세션을 재로그인하지 않는다.
        assert!(should_skip_login(false, true));
        // 쿠키가 없거나 만료면 비강제여도 로그인해야 한다(건너뛰지 않음).
        assert!(!should_skip_login(false, false));
    }
}
