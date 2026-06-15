//! band.us 이메일 로그인 모듈(네이버 `auth/` 미러). 로그인 "장소"만 band.us로 바뀐다.
//!
//! 네이버 로그인과 완전히 병행하는 별도 모듈로, 기존 `auth/` 동작을 건드리지 않는다.
//! 계정 목록은 네이버와 동일한 `accounts.json`에서 읽고(같은 계정에 band 비밀번호를 둠),
//! band 세션 쿠키만 `cookies-band/`로 분리 저장한다.

mod cookies;
mod login;
mod login_flow;
mod outcome;
mod paths;
mod queue;
mod util;

use tauri::{AppHandle, Runtime};

use crate::auth::outcome::LoginResolution;
use crate::auth::{
    app_data_root, assert_adb_device, config, paths_for_root, toggle_airplane_mode, Account,
    OrchestratorError,
};

pub use queue::{enqueue_band_accounts, get_band_queue_status, BandQueueState};

use cookies::{account_band_cookie_status, BandCookieStatus};
use paths::{band_cookies_dir_for_app_data, ensure_band_cookies_dir};

// 네이버 `accounts.json`에서 계정 목록을 읽는다. `auth::load_accounts_file`은 비공개이므로
// band 모듈이 공개 경로 헬퍼(`paths_for_root`/`app_data_root`)로 같은 파일을 직접 읽는다.
fn load_accounts() -> Result<Vec<Account>, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    if !paths.accounts_file.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&paths.accounts_file)?;
    Ok(serde_json::from_str(&text)?)
}

/// 한 band 계정을 처리한다(네이버 `process_account` 미러).
///
/// `_app`은 큐 워커가 넘기는 핸들로, CDP 로그인은 직접 호출하므로 본문에서는 쓰지 않는다.
pub(crate) async fn process_band_account<R: Runtime>(
    _app: &AppHandle<R>,
    account_id: &str,
    headless: bool,
    use_adb: bool,
    force: bool,
) -> Result<LoginResolution, OrchestratorError> {
    let accounts = load_accounts()?;
    let account = accounts
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| OrchestratorError::AccountNotFound(account_id.to_string()))?;

    // 유효한 band 쿠키가 있으면 재로그인을 건너뛴다(이미 로그인 = active). 단 명시적
    // 재로그인(force)이면 로컬 쿠키가 유효해 보여도 단락하지 않고 실제 로그인으로 새
    // 비밀번호를 검증한다 — 바뀐/죽은 자격증명을 잡는다(네이버 #132와 동일).
    if !force && account_band_cookie_status(account_id)? == BandCookieStatus::Valid {
        return Ok(LoginResolution::active());
    }

    let cookies_dir = band_cookies_dir_for_app_data()?;
    ensure_band_cookies_dir(&cookies_dir)?;

    if use_adb {
        assert_adb_device().await?;
        toggle_airplane_mode().await?;
        // IP가 바뀐 뒤 네트워크가 안정될 시간을 주고 나서 Chrome을 띄운다(네이버 미러).
        tracing::info!(
            "[BAND] IP 변경 확인 — {}초 안정화 대기 후 Chrome 실행",
            config::ADB_SETTLE_AFTER_ROTATE_SECS
        );
        tokio::time::sleep(std::time::Duration::from_secs(
            config::ADB_SETTLE_AFTER_ROTATE_SECS,
        ))
        .await;
    }

    // CDP 로그인은 Chrome을 띄워 동기적으로 동작하므로 blocking 스레드에서 실행한다.
    tauri::async_runtime::spawn_blocking(move || login::login(&cookies_dir, &account, headless))
        .await
        .map_err(|error| OrchestratorError::CommandFailed(format!("로그인 스레드 오류: {error}")))?
}
