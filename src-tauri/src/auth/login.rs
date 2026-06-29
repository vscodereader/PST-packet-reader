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
            let contents = serde_json::to_string_pretty(&payload)?;
            write_cookie_file_resilient(&path, &contents)?;

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

/// IO 실패에 **경로·동작·OS 코드**를 담은 진단 가능한 오류를 만든다(#디스크IO 후속). 바닥
/// `io: <메시지>`만으론 어느 파일에서 무슨 동작이 왜 실패했는지 알 수 없어, 외부 오류(500/403)와
/// 구분도 안 됐다. 이걸로 "디스크 접근 실패 — 경로/원인 확인"이 메시지에 드러나게 한다.
fn disk_io_error(op: &str, path: &std::path::Path, e: &std::io::Error) -> OrchestratorError {
    OrchestratorError::CommandFailed(format!(
        "{op} 실패 [{}]: {e} (kind={:?}, os={:?})",
        path.display(),
        e.kind(),
        e.raw_os_error()
    ))
}

/// 쿠키 파일을 견고하게 저장한다(#디스크IO 후속, 통제 가능한 IO 실패를 "되게" 만든다).
/// ⒜ 부모 디렉터리 보장(네이버 finalize엔 그동안 직전 보장이 없어 밴드와 비대칭이었다),
/// ⒝ 같은 디렉터리에 임시파일로 쓰고 rename으로 교체(원자적 — 부분기록/손상 방지, 최종 파일
///    점유 창 축소), ⒞ 백신/인덱서의 **일시적 파일 잠금**(Windows 공유위반 os error 32·권한 거부)
///    에만 짧게 백오프 재시도. 디스크 풀 등 영구 오류는 재시도하지 않고 진단 메시지로 올린다.
fn write_cookie_file_resilient(
    path: &std::path::Path,
    contents: &str,
) -> Result<(), OrchestratorError> {
    use std::io::ErrorKind;
    // ⒜ 디렉터리 보장.
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| disk_io_error("쿠키 디렉터리 생성", dir, &e))?;
    }
    // 일시적(재시도 가치 있는) 잠금: 권한 거부 또는 Windows 공유위반(os error 32).
    let is_transient =
        |e: &std::io::Error| e.kind() == ErrorKind::PermissionDenied || e.raw_os_error() == Some(32);

    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);

    const ATTEMPTS: u32 = 4;
    let mut last: Option<std::io::Error> = None;
    for attempt in 0..ATTEMPTS {
        // ⒝ 임시파일 쓰기 → rename 교체(원자적).
        match std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, path)) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt + 1 < ATTEMPTS && is_transient(&e) {
                    std::thread::sleep(std::time::Duration::from_millis(
                        150 * (attempt as u64 + 1),
                    ));
                    last = Some(e);
                    continue;
                }
                let _ = std::fs::remove_file(&tmp); // 실패한 임시파일 흔적 제거(best-effort).
                return Err(disk_io_error("쿠키 저장", path, &e));
            }
        }
    }
    // 모든 시도가 일시적 오류로 소진된 경우.
    Err(disk_io_error(
        "쿠키 저장(재시도 소진)",
        path,
        &last.unwrap_or_else(|| std::io::Error::new(ErrorKind::Other, "unknown")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resilient_write_creates_parent_dir_and_writes_atomically() {
        // ⒜ 부모 디렉터리가 없어도 만들어 쓰고, 내용이 정확히 저장되는지.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("user.json");
        write_cookie_file_resilient(&path, "{\"a\":1}").expect("write ok");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":1}");
        // 임시파일은 남지 않는다(rename으로 교체).
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        assert!(!std::path::PathBuf::from(tmp).exists(), "임시파일이 남으면 안 됨");
    }

    #[test]
    fn resilient_write_overwrites_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("user.json");
        write_cookie_file_resilient(&path, "old").expect("first write");
        write_cookie_file_resilient(&path, "new").expect("overwrite");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn disk_io_error_includes_path_and_os_context() {
        let e = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = disk_io_error("쿠키 저장", std::path::Path::new("/x/user.json"), &e);
        let msg = err.to_string();
        assert!(msg.contains("/x/user.json"), "경로 포함");
        assert!(msg.contains("PermissionDenied"), "원인 kind 포함");
    }
}
