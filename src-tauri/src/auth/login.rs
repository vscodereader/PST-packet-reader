//! 로그인 오케스트레이터. fresh Chrome을 띄워 CDP로 로그인하고, headless에서
//! 챌린지가 나오면 headed로 승격 재실행한다. 성공 시 쿠키 파일을 저장한다.

use serde_json::json;

use crate::ipc::accounts::AccountStatus;
use crate::naver_automation::CdpClient;

use super::{
    accounts::has_valid_cookie_file,
    chrome,
    error::OrchestratorError,
    login_flow::{self, LoginOutcome},
    outcome::{resolve_non_ok, LoginResolution},
    types::{Account, RuntimePaths},
    util::{now_secs, safe_file_stem},
};

/// 한 계정을 로그인하고 쿠키를 저장한다(승격 루프 포함). 결과는 계정 상태/안내로 해석해
/// [`LoginResolution`]으로 돌려준다 — 실패 계열(비번오류/인증/차단)도 `Err`로 뭉개지 않고
/// 세분화된 상태를 보존한다.
pub(crate) fn login(
    paths: &RuntimePaths,
    account: &Account,
    headless: bool,
) -> Result<LoginResolution, OrchestratorError> {
    let (outcome, trace) = attempt(&account.id, &account.password, headless)?;

    // headless에서 챌린지가 나오면 headed로 승격해 사용자가 직접 해결하도록 재실행.
    let (outcome, trace) = match outcome {
        LoginOutcome::ChallengeRequired { .. } if headless => {
            attempt(&account.id, &account.password, false)?
        }
        other => (other, trace),
    };

    finalize(paths, account, outcome, trace)
}

// Chrome을 띄워 attach하고 로그인 시퀀스를 1회 수행한다. 실패 시 사용자 메시지와 함께
// "자세히 보기"용 trace(위치+백트레이스)도 돌려준다(#210).
fn attempt(
    id: &str,
    pw: &str,
    headless: bool,
) -> Result<(LoginOutcome, Option<String>), OrchestratorError> {
    let handle = chrome::launch(headless)?;
    // CDP 연결/Page 활성화 실패는 AutomationError(백트레이스 보유)다. 인프라 Err로 뭉개
    // 백트레이스를 잃지 않도록, 메시지를 사용자 사유로·trace를 "자세히 보기"로 보존해
    // 로그인 실패(Error)로 흘린다(#199 게시 trace와 동일 철학).
    let mut client = match CdpClient::connect_to_existing_chrome("127.0.0.1", handle.port) {
        Ok(client) => client,
        Err(error) => {
            return Ok((
                LoginOutcome::Error(error.message().to_owned()),
                Some(error.trace()),
            ))
        }
    };
    // 로그인은 Runtime.enable 을 켜지 않는다(CDP 탐지 누출 방지). Page 도메인만 활성화.
    if let Err(error) = client.enable_page_only() {
        return Ok((
            LoginOutcome::Error(error.message().to_owned()),
            Some(error.trace()),
        ));
    }

    // headed(=!headless)면 사용자가 캡차/2차 인증을 직접 풀 동안 기다린다.
    let (outcome, trace) = login_flow::run(&mut client, id, pw, !headless);

    drop(client);
    drop(handle); // ChromeHandle Drop이 프로세스/임시 프로필을 정리한다.
    Ok((outcome, trace))
}

// 결과를 해석한다: 성공이면 쿠키를 저장하고, 그 외(인증필요/비번오류/차단/오류)는
// 세분화된 [`LoginResolution`]으로 보존한다. `Err`은 진짜 인프라 오류(파일 쓰기/쿠키 검증
// IO 실패)에만 쓴다 — 로그인 결과 자체는 `Ok(LoginResolution)`로 흐른다.
fn finalize(
    paths: &RuntimePaths,
    account: &Account,
    outcome: LoginOutcome,
    trace: Option<String>,
) -> Result<LoginResolution, OrchestratorError> {
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
                Ok(LoginResolution::active())
            } else {
                Ok(LoginResolution::failure(
                    AccountStatus::Error,
                    "저장된 쿠키에 유효한 네이버 세션이 없습니다.",
                ))
            }
        }
        other => Ok(resolve_non_ok(other, trace)),
    }
}
