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
///
/// `manual_captcha`가 true이면(보류 계정 재로그인) 캡차가 떠도 사용자가 직접 풀도록 창을
/// 성공까지 열어둔다. false이면(첫 로그인) 캡차는 grace 없이 즉시 실패(보류)로 떨어진다.
pub(crate) fn login(
    paths: &RuntimePaths,
    account: &Account,
    headless: bool,
    manual_captcha: bool,
) -> Result<LoginResolution, OrchestratorError> {
    // [진단·임시] PSTMACRO_LOGIN_MANUAL 이 설정되면 자동 타이핑을 끄고(run_inner에서) 사용자가 직접
    // 입력하게 둔다. 사용자가 창을 보고 입력해야 하므로 headless 를 강제로 끈다(headed 고정).
    let headless = headless && std::env::var("PSTMACRO_LOGIN_MANUAL").is_err();
    let (outcome, trace) = attempt(&account.id, &account.password, headless, manual_captcha)?;

    // headless에서 챌린지가 나오면 headed로 승격해 사용자가 직접 해결하도록 재실행.
    let (outcome, trace) = match outcome {
        LoginOutcome::ChallengeRequired { .. } if headless => {
            attempt(&account.id, &account.password, false, manual_captcha)?
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
    manual_captcha: bool,
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

    // headed(=!headless)면 사용자가 캡차/2차 인증을 직접 풀 동안 기다린다. manual_captcha는
    // 보류 계정 재로그인일 때만 true라, 캡차 직접 입력을 창을 열어둔 채 기다린다.
    let (outcome, trace) = login_flow::run(&mut client, id, pw, !headless, manual_captcha);

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
                // 쿠키는 저장됐는데 유효 세션이 없는 예기치 못한 오류(Error) — 알림 "자세히 보기"용
                // 백트레이스를 붙여 추적 가능하게 한다(graceful 실패도 trace 보존).
                Ok(LoginResolution::failure_with_trace(
                    AccountStatus::Error,
                    "저장된 쿠키에 유효한 네이버 세션이 없습니다.",
                    Some(crate::util::backtrace_string()),
                ))
            }
        }
        other => Ok(resolve_non_ok(other, trace)),
    }
}
