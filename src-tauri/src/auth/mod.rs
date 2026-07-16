mod accounts;
mod adb;
mod chrome;
pub mod config;
mod error;
mod login;
pub(crate) mod login_flow;
// band_auth가 로그인 결과 매핑(`LoginResolution`)을 재사용하도록 크레이트 내부에 공개한다.
pub(crate) mod outcome;
mod paths;
mod types;
mod ua;
mod util;

use tauri::{AppHandle, Runtime};

pub use accounts::{
    account_cookie_expiry, clear_account_cookies, read_account_cookies,
    read_account_cookies_unchecked, save_accounts_file,
};
pub use adb::probe_adb_connection;
// 게시(forum)에서도 로그인과 같은 Chrome 런처를 재사용해, 디버그 포트 Chrome을 앱이 직접 띄운다.
pub(crate) use chrome::force_kill_tree;
pub(crate) use chrome::launch as launch_debug_chrome;
// 신고(naver_report) 토큰 브라우저가 계정당 크롬 핸들을 struct 필드로 보관하려면 타입 이름이 필요하다.
pub(crate) use chrome::ChromeHandle;
// 잔존(고아) Chrome 개수 조회 — UI가 "실행 중 크롬 N개"를 작업관리자 없이 보여주는 데 쓴다.
pub(crate) use chrome::running_chrome_count;
pub use error::OrchestratorError;
pub use paths::{app_data_root, paths_for_root};
pub use types::{Account, RuntimePaths};
// 로그용 ID 마스킹 헬퍼를 다른 모듈(예: discussion_batch)에서도 쓸 수 있게 재노출.
pub(crate) use util::mask_id;
// 수동추가(사람이 직접 로그인) 결과 — IPC 커맨드가 계정 행을 만들 때 쓴다.
pub(crate) use login::ManualAddResult;

use accounts::{has_valid_account_cookies, load_accounts_file};
// band_auth가 ADB IP 회전을 재사용하도록 재노출한다(동작 무변경 — 기존 private import의
// 가시성만 crate 범위로 넓힌다). process_account도 이 경로로 동일하게 호출한다.
pub(crate) use adb::{assert_adb_device, fetch_external_ip, toggle_airplane_mode, IpRotation};
use paths::ensure_runtime_dirs;

/// 런타임 환경을 초기화하고 필요한 디렉토리와 도구들을 준비한다.
pub async fn bootstrap_runtime() -> Result<RuntimePaths, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    ensure_runtime_dirs(&paths)?;
    Ok(paths)
}

// `app`은 보류(OnHold) 계정 재로그인 판정(manual_captcha)을 위해 IPC 계정 상태를 조회하는 데
// 쓴다. 큐 워커가 `AppHandle<R>`를 넘기므로(IPC 테스트의 MockRuntime 포함) 제네릭 시그니처를
// 유지한다.
pub(crate) async fn process_account<R: Runtime>(
    app: &AppHandle<R>,
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
        // ADB는 '있으면 IP를 회전, 없으면 현재 IP로 그대로 진행'하는 선택 기능이다(사수 지시).
        // 폰이 안 붙어 있어도 로그인 자체는 되게 해야 하므로, 디바이스가 없으면 하드 에러로
        // 계정 전체를 중단하지 않고 IP 회전/체크만 건너뛴다. 연결 확인은 부작용 없는
        // probe_adb_connection으로 한다(assert_adb_device는 없을 때 에러를 던진다).
        if probe_adb_connection().await.is_ok() {
            // 로그인: 폰 인터넷 끊김 + IP 실제 변경을 확인하며 진행 → 로그인도 새 IP로 수행.
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
        } else {
            tracing::info!(
                "[LOGIN] ADB 디바이스 없음 — IP 회전/체크 생략, 현재 IP로 진행(사수 지시)"
            );
        }
    }

    // 보류(OnHold) 계정의 재로그인이면 캡차를 사용자가 직접 풀도록 창을 열어둔다(manual_captcha).
    // 계정의 현재 상태는 IPC 계정 스토어에서 loginId로 찾는다. 스토어가 없으면(테스트 등) false.
    // 첫 로그인(상태 != OnHold)이면 false라, 캡차가 떠도 grace 없이 즉시 보류로 떨어진다.
    use tauri::Manager;
    let manual_captcha = app
        .try_state::<crate::store::JsonStore<crate::ipc::accounts::Account>>()
        .map(|store| {
            store.snapshot().iter().any(|a| {
                a.login_id == account_id && a.status == crate::ipc::accounts::AccountStatus::OnHold
            })
        })
        .unwrap_or(false);

    // CDP 로그인은 Chrome을 띄워 동기적으로 동작하므로 blocking 스레드에서 실행한다.
    tauri::async_runtime::spawn_blocking(move || {
        login::login(&paths, &account, headless, manual_captcha)
    })
    .await
    .map_err(|error| OrchestratorError::CommandFailed(format!("로그인 스레드 오류: {error}")))?
}

/// 수동추가(사람이 직접 로그인). headed Chrome을 띄워 사용자가 직접 로그인하게 하고, 성공하면
/// 자동로그인과 동일하게 쿠키를 저장한 뒤 사람이 친 아이디/비밀번호를 돌려준다. 취소/타임아웃이면
/// `Ok(None)`. Chrome을 띄워 동기적으로 기다리므로 blocking 스레드에서 실행한다.
pub(crate) async fn manual_add_account() -> Result<Option<ManualAddResult>, OrchestratorError> {
    let paths = paths_for_root(app_data_root()?);
    ensure_runtime_dirs(&paths)?;

    // ADB가 연결돼 있으면 수동추가 로그인창을 띄우기 **전에 IP를 한 번 회전**한다(사용자 요청).
    // 일반 로그인(§79)과 동일한 '있으면 회전, 없으면 현재 IP로 진행' 규칙을 그대로 재사용한다 —
    // 폰이 안 붙어 있으면 IP 회전만 건너뛰고 기존과 똑같이 그대로 창을 띄운다(그 이후는 전부 동일).
    if probe_adb_connection().await.is_ok() {
        toggle_airplane_mode().await?;
        tracing::info!(
            "[수동추가] IP 변경 확인 — {}초 안정화 대기 후 로그인창 실행",
            config::ADB_SETTLE_AFTER_ROTATE_SECS
        );
        tokio::time::sleep(std::time::Duration::from_secs(
            config::ADB_SETTLE_AFTER_ROTATE_SECS,
        ))
        .await;
    } else {
        tracing::info!("[수동추가] ADB 디바이스 없음 — IP 회전 생략, 현재 IP로 진행");
    }

    tauri::async_runtime::spawn_blocking(move || login::manual_add(&paths))
        .await
        .map_err(|error| {
            OrchestratorError::CommandFailed(format!("수동추가 스레드 오류: {error}"))
        })?
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
