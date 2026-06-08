//! band 로그인 오케스트레이터. fresh Chrome을 띄워 CDP로 로그인하고, 성공 시 band
//! 쿠키 파일을 저장한다(네이버 `auth/login.rs` 미러). band는 캡차/2차 인증 챌린지가 없어
//! headless→headed 승격 루프가 불필요하므로 단순화했다.

use std::path::Path;

use serde_json::json;

use crate::auth::{launch_debug_chrome, Account, OrchestratorError};
use crate::naver_automation::CdpClient;

use super::{
    cookies::has_valid_band_cookie_file,
    login_flow::{self, BandLoginOutcome},
    paths::{band_cookie_file_path, ensure_band_cookies_dir},
    util::now_secs,
};

/// 한 계정을 band에 로그인하고 쿠키를 저장한다.
pub(crate) fn login(
    cookies_dir: &Path,
    account: &Account,
    headless: bool,
) -> Result<(), OrchestratorError> {
    let outcome = attempt(&account.id, &account.password, headless)?;
    finalize(cookies_dir, account, outcome)
}

// Chrome을 띄워 attach하고 로그인 시퀀스를 1회 수행한다.
fn attempt(id: &str, pw: &str, headless: bool) -> Result<BandLoginOutcome, OrchestratorError> {
    let handle = launch_debug_chrome(headless)?;
    let mut client = CdpClient::connect_to_existing_chrome("127.0.0.1", handle.port)
        .map_err(|error| OrchestratorError::CommandFailed(error.to_string()))?;
    // 로그인은 Runtime.enable 을 켜지 않는다(CDP 탐지 누출 방지). Page 도메인만 활성화.
    client
        .enable_page_only()
        .map_err(|error| OrchestratorError::CommandFailed(error.to_string()))?;

    // headed(=!headless)면 사용자가 직접 개입할 수 있도록 더 오래 기다린다.
    let outcome = login_flow::run(&mut client, id, pw, !headless);

    drop(client);
    drop(handle); // ChromeHandle Drop이 프로세스/임시 프로필을 정리한다.
    Ok(outcome)
}

// 결과에 따라 쿠키를 저장하거나 명확한 오류를 반환한다.
fn finalize(
    cookies_dir: &Path,
    account: &Account,
    outcome: BandLoginOutcome,
) -> Result<(), OrchestratorError> {
    match outcome {
        BandLoginOutcome::Ok { cookies } => {
            ensure_band_cookies_dir(cookies_dir)?;
            let path = band_cookie_file_path(cookies_dir, &account.id);
            let payload = json!({
                "accountId": account.id,
                "savedAt": now_secs(),
                "cookies": cookies,
            });
            std::fs::write(&path, serde_json::to_string_pretty(&payload)?)?;

            if has_valid_band_cookie_file(&path)? {
                Ok(())
            } else {
                Err(OrchestratorError::CommandFailed(
                    "저장된 쿠키에 유효한 band 세션(band_session)이 없습니다.".to_owned(),
                ))
            }
        }
        BandLoginOutcome::BadCredentials => Err(OrchestratorError::CommandFailed(
            "이메일 또는 비밀번호가 올바르지 않습니다.".to_owned(),
        )),
        BandLoginOutcome::Blocked => Err(OrchestratorError::CommandFailed(
            "로그인 접근이 차단되었습니다(계정 상태 확인 필요).".to_owned(),
        )),
        BandLoginOutcome::Error(message) => Err(OrchestratorError::CommandFailed(message)),
    }
}
