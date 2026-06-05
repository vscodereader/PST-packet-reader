//! 로그인 오케스트레이터. fresh Chrome을 띄워 CDP로 로그인하고, headless에서
//! 챌린지가 나오면 headed로 승격 재실행한다. 성공 시 쿠키 파일을 저장한다.

use serde_json::json;

use crate::naver_automation::CdpClient;

use super::{
    accounts::has_valid_cookie_file,
    chrome,
    error::OrchestratorError,
    login_flow::{self, LoginOutcome},
    types::{Account, RuntimePaths},
    util::{now_secs, safe_file_stem},
};

/// 한 계정을 로그인하고 쿠키를 저장한다(승격 루프 포함).
pub(crate) fn login(
    paths: &RuntimePaths,
    account: &Account,
    headless: bool,
) -> Result<(), OrchestratorError> {
    let outcome = attempt(&account.id, &account.password, headless)?;

    // headless에서 챌린지가 나오면 headed로 승격해 사용자가 직접 해결하도록 재실행.
    let outcome = match outcome {
        LoginOutcome::ChallengeRequired { .. } if headless => {
            attempt(&account.id, &account.password, false)?
        }
        other => other,
    };

    finalize(paths, account, outcome)
}

// Chrome을 띄워 attach하고 로그인 시퀀스를 1회 수행한다.
fn attempt(id: &str, pw: &str, headless: bool) -> Result<LoginOutcome, OrchestratorError> {
    let handle = chrome::launch(headless)?;
    let mut client = CdpClient::connect_to_existing_chrome("127.0.0.1", handle.port)
        .map_err(|error| OrchestratorError::CommandFailed(error.to_string()))?;
    // 로그인은 Runtime.enable 을 켜지 않는다(CDP 탐지 누출 방지). Page 도메인만 활성화.
    client
        .enable_page_only()
        .map_err(|error| OrchestratorError::CommandFailed(error.to_string()))?;

    // headed(=!headless)면 사용자가 캡차/2차 인증을 직접 풀 동안 기다린다.
    let outcome = login_flow::run(&mut client, id, pw, !headless);

    drop(client);
    drop(handle); // ChromeHandle Drop이 프로세스/임시 프로필을 정리한다.
    Ok(outcome)
}

// 결과에 따라 쿠키를 저장하거나 명확한 오류를 반환한다.
fn finalize(
    paths: &RuntimePaths,
    account: &Account,
    outcome: LoginOutcome,
) -> Result<(), OrchestratorError> {
    match outcome {
        LoginOutcome::Ok { cookies } => {
            let path = paths
                .cookies_dir
                .join(format!("{}.json", safe_file_stem(&account.id)));
            let payload = json!({
                "accountId": account.id,
                "savedAt": now_secs(),
                "cookies": cookies,
            });
            std::fs::write(&path, serde_json::to_string_pretty(&payload)?)?;

            if has_valid_cookie_file(&path)? {
                Ok(())
            } else {
                Err(OrchestratorError::CommandFailed(
                    "저장된 쿠키에 유효한 네이버 세션이 없습니다.".to_owned(),
                ))
            }
        }
        LoginOutcome::ChallengeRequired { kind } => Err(OrchestratorError::CommandFailed(format!(
            "추가 인증이 필요합니다({kind:?}). 열린 Chrome 창에서 직접 완료한 뒤 다시 실행하세요."
        ))),
        LoginOutcome::BadCredentials => Err(OrchestratorError::CommandFailed(
            "아이디 또는 비밀번호가 올바르지 않습니다.".to_owned(),
        )),
        LoginOutcome::Error(message) => Err(OrchestratorError::CommandFailed(message)),
    }
}
