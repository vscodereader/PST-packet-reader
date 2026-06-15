//! band 로그인 오케스트레이터. fresh Chrome을 띄워 CDP로 로그인하고, 성공 시 band
//! 쿠키 파일을 저장한다(네이버 `auth/login.rs` 미러). band는 캡차/2차 인증 챌린지가 없어
//! headless→headed 승격 루프가 불필요하므로 단순화했다.

use std::path::Path;

use serde_json::json;

use crate::auth::outcome::LoginResolution;
use crate::auth::{launch_debug_chrome, Account, OrchestratorError};
use crate::ipc::accounts::AccountStatus;
use crate::naver_automation::CdpClient;

use super::{
    cookies::has_valid_band_cookie_file,
    login_flow::{self, BandLoginOutcome},
    outcome::resolve_band_failure,
    paths::{band_cookie_file_path, ensure_band_cookies_dir},
    util::now_secs,
};

/// 한 계정을 band에 로그인하고 쿠키를 저장한다. 성공/실패를 세분 상태(`LoginResolution`)로
/// 돌려줘 호출부가 계정 status(active/badCredentials/blocked/error)에 반영하게 한다.
pub(crate) fn login(
    cookies_dir: &Path,
    account: &Account,
    headless: bool,
) -> Result<LoginResolution, OrchestratorError> {
    let (outcome, trace) = attempt(&account.id, &account.password, headless)?;
    finalize(cookies_dir, account, outcome, trace)
}

// Chrome을 띄워 attach하고 로그인 시퀀스를 1회 수행한다. 실패 시 사용자 메시지와 함께
// "자세히 보기"용 trace(위치+백트레이스)도 돌려준다(#210, 네이버 미러).
fn attempt(
    id: &str,
    pw: &str,
    headless: bool,
) -> Result<(BandLoginOutcome, Option<String>), OrchestratorError> {
    let handle = launch_debug_chrome(headless)?;
    // CDP 연결/Page 활성화 실패는 AutomationError(백트레이스 보유)다. 인프라 Err로 뭉개지
    // 않고 메시지/trace를 보존해 로그인 실패(Error)로 흘린다(#210).
    let mut client = match CdpClient::connect_to_existing_chrome("127.0.0.1", handle.port) {
        Ok(client) => client,
        Err(error) => {
            return Ok((
                BandLoginOutcome::Error(error.message().to_owned()),
                Some(error.trace()),
            ))
        }
    };
    // 로그인은 Runtime.enable 을 켜지 않는다(CDP 탐지 누출 방지). Page 도메인만 활성화.
    if let Err(error) = client.enable_page_only() {
        return Ok((
            BandLoginOutcome::Error(error.message().to_owned()),
            Some(error.trace()),
        ));
    }

    // headed(=!headless)면 사용자가 직접 개입할 수 있도록 더 오래 기다린다.
    let (outcome, trace) = login_flow::run(&mut client, id, pw, !headless);

    drop(client);
    drop(handle); // ChromeHandle Drop이 프로세스/임시 프로필을 정리한다.
    Ok((outcome, trace))
}

// 결과를 세분 상태로 해석한다. 쿠키 저장 IO 오류만 인프라 Err(`?`)로 올리고, 로그인 결과
// (성공/비번오류/차단/오류)는 `LoginResolution`으로 돌려준다(네이버 `auth/queue.rs` 패턴).
fn finalize(
    cookies_dir: &Path,
    account: &Account,
    outcome: BandLoginOutcome,
    trace: Option<String>,
) -> Result<LoginResolution, OrchestratorError> {
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
                Ok(LoginResolution::active())
            } else {
                // 쿠키는 저장됐지만 유효 세션이 없다 — 인프라 오류가 아닌 로그인 실패(error)로 본다.
                Ok(LoginResolution::failure(
                    AccountStatus::Error,
                    "저장된 쿠키에 유효한 band 세션(band_session)이 없습니다.",
                ))
            }
        }
        // 실패 계열(비번오류/차단/오류)은 순수 매핑으로 세분 상태를 만든다. CDP 실패의
        // trace는 "자세히 보기"용으로 보존한다(#210).
        other => Ok(resolve_band_failure(&other, trace)),
    }
}
