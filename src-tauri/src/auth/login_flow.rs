//! CDP 로그인 시퀀스. 시스템 Chrome에 attach한 `CdpClient`로 네이버 로그인 폼을
//! 실제 키 이벤트(`Input.dispatchKeyEvent`)로 채우고, 결과를 분류한다.
//!
//! ⚠️ `Runtime.evaluate`로 `input.value=`를 직접 설정하지 않는다. 네이버 로그인은
//! keydown을 후킹해 암호화 페이로드를 만들기 때문에 값만 꽂으면 암호화가 깨진다.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::naver_automation::{AutomationError, CdpClient};

const LOGIN_URL: &str = "https://nid.naver.com/nidlogin.login?mode=form&url=https://www.naver.com/";
// headless: 챌린지가 보이면 곧장 headed로 승격해야 하므로 짧게 기다린다.
const HEADLESS_TIMEOUT: Duration = Duration::from_secs(40);
// headed: 사용자가 캡차/2차 인증을 직접 푸는 동안(사수 요구: 창 띄우고 시간 지나면
// 타임아웃) 성공 또는 타임아웃까지 기다린다.
const HEADED_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// 챌린지(추가 인증) 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChallengeKind {
    Captcha,
    Otp,
    Device,
}

/// 로그인 결과.
pub(crate) enum LoginOutcome {
    Ok { cookies: Vec<Value> },
    ChallengeRequired { kind: ChallengeKind },
    BadCredentials,
    Error(String),
}

/// 페이지 판정 신호(분류 입력). 분류 로직을 브라우저에서 분리해 단위 테스트한다.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PageSignals {
    pub logged_in: bool,
    pub captcha: bool,
    pub otp: bool,
    pub device: bool,
    pub bad_credentials: bool,
    pub blocked: bool,
}

/// 진행 중/확정 신호.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    Pending,
    Success,
    Challenge(ChallengeKind),
    BadCredentials,
    Blocked,
}

/// 폴링 한 스텝의 판정 결과. 루프는 이 값을 실제 동작(반환/대기)으로 옮긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopDecision {
    Success,
    PromoteChallenge(ChallengeKind),
    ConfirmedBad,
    ConfirmedBlocked,
    KeepWaiting(Option<Signal>),
}

/// 직전 음성 신호(`last_negative`)와 현재 신호로 이번 폴링의 동작을 결정한다(순수 함수).
///
/// 핵심: BadCredentials/Blocked는 **2회 연속**일 때만 확정한다. 클릭 직후 잠깐 떴다
/// 사라지는 `#err_common`이나 네비게이션 과도기에 폼이 사라진 상태를 영구 실패로 latch하지
/// 않기 위함이다. Success/Pending/headed-Challenge는 음성 누적을 초기화한다.
fn decide_loop_step(
    last_negative: Option<Signal>,
    signal: Signal,
    wait_for_human: bool,
) -> LoopDecision {
    match signal {
        Signal::Success => LoopDecision::Success,
        Signal::Challenge(kind) => {
            if wait_for_human {
                LoopDecision::KeepWaiting(None)
            } else {
                LoopDecision::PromoteChallenge(kind)
            }
        }
        Signal::BadCredentials => {
            if last_negative == Some(Signal::BadCredentials) {
                LoopDecision::ConfirmedBad
            } else {
                LoopDecision::KeepWaiting(Some(Signal::BadCredentials))
            }
        }
        Signal::Blocked => {
            if wait_for_human {
                LoopDecision::KeepWaiting(last_negative)
            } else if last_negative == Some(Signal::Blocked) {
                LoopDecision::ConfirmedBlocked
            } else {
                LoopDecision::KeepWaiting(Some(Signal::Blocked))
            }
        }
        Signal::Pending => LoopDecision::KeepWaiting(None),
    }
}

/// 페이지 신호를 로그인 진행/결과 신호로 분류한다(순수 함수).
pub(crate) fn classify(signals: &PageSignals) -> Signal {
    if signals.logged_in {
        Signal::Success
    } else if signals.captcha {
        Signal::Challenge(ChallengeKind::Captcha)
    } else if signals.otp {
        Signal::Challenge(ChallengeKind::Otp)
    } else if signals.device {
        Signal::Challenge(ChallengeKind::Device)
    } else if signals.bad_credentials {
        Signal::BadCredentials
    } else if signals.blocked {
        Signal::Blocked
    } else {
        Signal::Pending
    }
}

/// 로그인을 수행하고 결과를 분류해 반환한다.
///
/// `wait_for_human`이 true이면(headed) 캡차/2차 인증이 떠도 즉시 포기하지 않고,
/// 사용자가 열린 Chrome 창에서 직접 푸는 동안 성공(쿠키) 또는 타임아웃까지 기다린다.
/// false이면(headless) 챌린지를 만나는 즉시 `ChallengeRequired`로 반환해 호출자가
/// headed로 승격하도록 한다.
pub(crate) fn run(
    client: &mut CdpClient,
    id: &str,
    pw: &str,
    wait_for_human: bool,
) -> LoginOutcome {
    match run_inner(client, id, pw, wait_for_human) {
        Ok(outcome) => outcome,
        Err(error) => LoginOutcome::Error(error.to_string()),
    }
}

/// 자격증명이 로그인 시도에 충분한지(둘 다 비어있지 않은지) 검사한다(순수 함수).
/// `type_into`는 빈 문자열에 대해 "0글자를 성공적으로 입력"으로 `Ok(true)`를 돌려주므로,
/// 빈 자격증명이 그대로 로그인 버튼 클릭까지 진행되는 것을 막으려면 시도 전에 걸러야 한다.
pub(crate) fn credentials_present(id: &str, pw: &str) -> bool {
    !id.trim().is_empty() && !pw.is_empty()
}

fn run_inner(
    client: &mut CdpClient,
    id: &str,
    pw: &str,
    wait_for_human: bool,
) -> Result<LoginOutcome, AutomationError> {
    // 빈/공백 자격증명이면 브라우저 폼을 건드리지 않고 즉시 입력 실패로 중단한다(기존
    // 사이드카도 빈 자격증명이면 브라우저를 띄우지 않았다). BadCredentials로 두면 "비번
    // 틀림"으로 오분류되므로, 사용자 입력 누락을 알리는 명확한 Error로 반환한다.
    if !credentials_present(id, pw) {
        return Ok(LoginOutcome::Error(
            "아이디 또는 비밀번호가 비어 있어 로그인을 시도하지 않았습니다.".to_owned(),
        ));
    }

    client.navigate(LOGIN_URL)?;
    // navigate가 readyState까지 기다려도, 로그인 폼이 렌더되고 네이버의 keydown 암호화
    // 핸들러가 붙기 전에 타이핑하면 글자가 필드에 들어가지 않는다. 폼이 준비될 때까지 기다린다.
    if !wait_for_login_form(client) {
        return Ok(LoginOutcome::Error(
            "로그인 폼(#id/#pw)을 찾지 못했습니다.".to_owned(),
        ));
    }

    // 사람처럼 천천히, 폼 스크립트가 자리잡을 시간을 두고 입력한다:
    // 로그인 폼이 뜬 뒤 2초 대기 → 아이디 입력 → 1초 대기 → 비밀번호 입력.
    // 3회 재시도 후에도 필드가 비어 있으면(일시적 렌더/타이밍 문제) type_into가 false를
    // 돌려준다. 빈/부분 자격증명으로 로그인 버튼을 누르면 결과가 #err_common/타임아웃으로
    // 분류돼 일시적 타이핑 실패가 영구 BadCredentials/Error로 둔갑하므로, 클릭하지 않고
    // 명확한 입력 실패로 중단한다.
    sleep(Duration::from_secs(2));
    if !type_into(client, "#id", id)? {
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(아이디 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(Duration::from_secs(1));
    if !type_into(client, "#pw", pw)? {
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(비밀번호 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }

    // 로그인 버튼 클릭(값 주입이 아니라 클릭이므로 evaluate 사용 가능).
    client.evaluate(
        "(()=>{const b=document.querySelector('#log\\\\.login')||\
         document.querySelector('button[type=submit]');if(b){b.click();return true;}return false;})()",
    )?;

    let timeout = if wait_for_human {
        HEADED_TIMEOUT
    } else {
        HEADLESS_TIMEOUT
    };
    // 클릭 직후 네비게이션이 정리될 시간을 준다. 이 settle 없이 곧장 읽으면, 클릭 직후
    // 잠깐 렌더된 #err_common이나 네비게이션 중간에 폼이 사라진 과도기 상태를 — 실제로는
    // 성공 중인 로그인인데도 — 실패로 latch한다.
    sleep(POLL_INTERVAL);

    // 음성 신호(BadCredentials/Blocked)는 한 번 보였다고 바로 확정하지 않고, 2회 연속
    // 폴링에서 지속될 때만 확정한다(과도기 깜빡임 latch 방지).
    let mut last_negative: Option<Signal> = None;
    let deadline = Instant::now() + timeout;
    loop {
        // 인증 성공 직후 뜨는 "새 기기 등록" 페이지면 "등록 안함"을 눌러 마무리한다
        // (설계 5단계: browser_flow의 기존 로직 재사용). 없으면 무시한다.
        let _ = client.click_device_dontsave_if_present(Duration::from_millis(300));

        let signals = read_signals(client)?;
        match decide_loop_step(last_negative, classify(&signals), wait_for_human) {
            LoopDecision::Success => {
                let cookies = collect_naver_cookies(client)?;
                return Ok(LoginOutcome::Ok { cookies });
            }
            LoopDecision::PromoteChallenge(kind) => {
                return Ok(LoginOutcome::ChallengeRequired { kind });
            }
            LoopDecision::ConfirmedBad => return Ok(LoginOutcome::BadCredentials),
            LoopDecision::ConfirmedBlocked => {
                return Ok(LoginOutcome::Error(
                    "로그인 접근이 차단되었습니다.".to_owned(),
                ));
            }
            LoopDecision::KeepWaiting(next) => last_negative = next,
        }

        if Instant::now() >= deadline {
            // headed에서 시간 내 인증을 끝내지 못한 경우를 포함한다(사수 요구: 타임아웃).
            let url = client.current_url().unwrap_or_default();
            return Ok(LoginOutcome::Error(format!(
                "로그인 시간이 초과되었습니다(캡차/2차 인증 미완료). 마지막 페이지: {url}"
            )));
        }
        sleep(POLL_INTERVAL);
    }
}

// 로그인 폼(#id/#pw)이 나타나고 입력 가능해질 때까지 기다린다. 폼 스크립트가 자리잡도록
// 주는 안정화 대기(2초)는 호출부(run_inner)에서 아이디 입력 직전에 둔다.
fn wait_for_login_form(client: &mut CdpClient) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ready = client
            .evaluate_bool("!!document.querySelector('#id') && !!document.querySelector('#pw')")
            .unwrap_or(false);
        if ready {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(250));
    }
}

// 선택자에 포커스한 뒤 한 글자씩 실제 키 이벤트로 입력한다(keydown 후킹 암호화 대응).
// 입력 후 필드 값 길이를 확인해, 비어 있으면(타이밍/렌더 문제로 헛친 경우) 최대 3회 재시도한다.
// 채워졌으면 `Ok(true)`, 3회 후에도 비어 있으면 `Ok(false)`를 반환해 호출자가 판단하게 한다.
fn type_into(client: &mut CdpClient, selector: &str, text: &str) -> Result<bool, AutomationError> {
    let expected = text.chars().count();

    for _ in 0..3 {
        // 기존 값 비우고 포커스(재시도 시 중복 입력 방지). 셀렉터는 고정 안전 문자열(#id/#pw).
        let focus = format!(
            "(()=>{{const el=document.querySelector('{selector}');\
             if(el){{el.value='';el.focus();return true;}}return false;}})()"
        );
        client.evaluate(&focus)?;

        for ch in text.chars() {
            let s = ch.to_string();
            client.call(
                "Input.dispatchKeyEvent",
                json!({ "type": "keyDown", "text": s, "key": s }),
            )?;
            client.call(
                "Input.dispatchKeyEvent",
                json!({ "type": "keyUp", "key": s }),
            )?;
        }

        let got = client
            .evaluate(&format!(
                "(()=>{{const el=document.querySelector('{selector}');\
                 return el&&el.value?el.value.length:0;}})()"
            ))?
            .as_u64()
            .unwrap_or(0) as usize;
        if got >= expected {
            return Ok(true);
        }
        sleep(Duration::from_millis(500));
    }
    // 3회 후에도 채우지 못함 — 호출자가 빈 자격증명으로 진행하지 않도록 false를 알린다.
    Ok(false)
}

// 셀렉터에 해당하는 "화면에 보이는" 요소가 있는지 확인한다. `offsetParent`가 null이면
// 숨겨진 요소이므로(예: 항상 DOM에 존재하는 Caps Lock 경고) false로 본다.
fn visible_exists(client: &mut CdpClient, selector: &str) -> bool {
    let expr = format!(
        "(()=>{{const e=document.querySelector('{selector}');\
         return !!(e&&e.offsetParent!==null);}})()"
    );
    client.evaluate_bool(&expr).unwrap_or(false)
}

// 현재 페이지에서 로그인 성공/챌린지/실패 신호를 읽는다.
fn read_signals(client: &mut CdpClient) -> Result<PageSignals, AutomationError> {
    let cookies = collect_naver_cookies(client)?;
    let logged_in = has_session_cookies(&cookies);

    let captcha = visible_exists(client, "#captchaDiv, #captcha, img#captchaimg");
    let otp = visible_exists(client, "#otp, input[name=otp], #cellphoneCertify");
    let current_url = client.current_url().unwrap_or_default();
    // 낯선 기기 추가 인증 페이지(기기 등록 확인). 성공 후의 "등록안함" 다이얼로그와 달리
    // 쿠키가 아직 없는 상태에서 사용자 조작을 요구한다.
    let device = current_url.contains("deviceConfirm") || current_url.contains("deviceCheck");
    // 실제 로그인 오류는 `#err_common`이 "보이는" 상태로 텍스트를 가진다. `.error_message`는
    // "Caps Lock is on." 경고가 항상 숨은 채 DOM에 존재하므로 단순 존재 검사는 오판한다.
    let bad_credentials = client
        .evaluate_bool(
            "(()=>{const e=document.querySelector('#err_common');\
             return !!(e&&e.offsetParent!==null&&(e.textContent||'').trim().length>0);})()",
        )
        .unwrap_or(false);
    let blocked = {
        let on_login = current_url.contains("nid.naver.com");
        let has_form = client
            .evaluate_bool("!!document.querySelector('form#frmNIDLogin, #id')")
            .unwrap_or(false);
        on_login && !has_form && !logged_in
    };

    Ok(PageSignals {
        logged_in,
        captcha,
        otp,
        device,
        bad_credentials,
        blocked,
    })
}

// Network.getCookies로 .naver.com 쿠키를 수거한다.
fn collect_naver_cookies(client: &mut CdpClient) -> Result<Vec<Value>, AutomationError> {
    let result = client.call(
        "Network.getCookies",
        json!({ "urls": ["https://www.naver.com", "https://nid.naver.com"] }),
    )?;
    let cookies = result
        .get("cookies")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| {
            c.get("domain")
                .and_then(Value::as_str)
                .is_some_and(|d| d.contains("naver.com"))
        })
        .collect();
    Ok(cookies)
}

/// 수거한 쿠키에 `NID_AUT`·`NID_SES`가 모두 있는지 확인한다(순수 함수).
pub(crate) fn has_session_cookies(cookies: &[Value]) -> bool {
    let names: Vec<&str> = cookies
        .iter()
        .filter_map(|c| c.get("name").and_then(Value::as_str))
        .collect();
    names.contains(&"NID_AUT") && names.contains(&"NID_SES")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_prioritizes_success() {
        let s = PageSignals {
            logged_in: true,
            captcha: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), Signal::Success);
    }

    #[test]
    fn classify_detects_challenge_and_errors() {
        assert_eq!(
            classify(&PageSignals {
                captcha: true,
                ..Default::default()
            }),
            Signal::Challenge(ChallengeKind::Captcha)
        );
        assert_eq!(
            classify(&PageSignals {
                otp: true,
                ..Default::default()
            }),
            Signal::Challenge(ChallengeKind::Otp)
        );
        assert_eq!(
            classify(&PageSignals {
                bad_credentials: true,
                ..Default::default()
            }),
            Signal::BadCredentials
        );
        assert_eq!(
            classify(&PageSignals {
                blocked: true,
                ..Default::default()
            }),
            Signal::Blocked
        );
        assert_eq!(classify(&PageSignals::default()), Signal::Pending);
    }

    #[test]
    fn credentials_present_rejects_empty_or_whitespace() {
        assert!(credentials_present("user", "pw"));
        assert!(!credentials_present("", "pw"));
        assert!(!credentials_present("   ", "pw"));
        assert!(!credentials_present("user", ""));
    }

    // --- decide_loop_step: 클릭 직후 과도기 신호를 영구 실패로 latch하지 않는지(2회 확정) ---

    #[test]
    fn loop_success_returns_even_after_negative() {
        assert_eq!(
            decide_loop_step(None, Signal::Success, false),
            LoopDecision::Success
        );
        // 직전에 음성 신호가 누적돼 있었어도 성공이면 성공으로 끝낸다.
        assert_eq!(
            decide_loop_step(Some(Signal::BadCredentials), Signal::Success, false),
            LoopDecision::Success
        );
    }

    #[test]
    fn loop_bad_credentials_needs_two_consecutive_polls() {
        // 첫 히트는 확정하지 않고 대기(과도기 깜빡임일 수 있으므로).
        assert_eq!(
            decide_loop_step(None, Signal::BadCredentials, false),
            LoopDecision::KeepWaiting(Some(Signal::BadCredentials))
        );
        // 2회 연속이면 확정.
        assert_eq!(
            decide_loop_step(Some(Signal::BadCredentials), Signal::BadCredentials, false),
            LoopDecision::ConfirmedBad
        );
    }

    #[test]
    fn loop_pending_resets_negative_so_transient_does_not_latch() {
        assert_eq!(
            decide_loop_step(Some(Signal::BadCredentials), Signal::Pending, false),
            LoopDecision::KeepWaiting(None)
        );
    }

    #[test]
    fn loop_blocked_confirms_only_in_headless_over_two_polls() {
        assert_eq!(
            decide_loop_step(None, Signal::Blocked, false),
            LoopDecision::KeepWaiting(Some(Signal::Blocked))
        );
        assert_eq!(
            decide_loop_step(Some(Signal::Blocked), Signal::Blocked, false),
            LoopDecision::ConfirmedBlocked
        );
    }

    #[test]
    fn loop_blocked_in_headed_keeps_waiting_for_user() {
        // headed에서는 기기등록/인증 중간 페이지를 차단으로 단정하지 않는다.
        assert_eq!(
            decide_loop_step(None, Signal::Blocked, true),
            LoopDecision::KeepWaiting(None)
        );
    }

    #[test]
    fn loop_challenge_promotes_in_headless_but_waits_in_headed() {
        assert_eq!(
            decide_loop_step(None, Signal::Challenge(ChallengeKind::Captcha), false),
            LoopDecision::PromoteChallenge(ChallengeKind::Captcha)
        );
        assert_eq!(
            decide_loop_step(None, Signal::Challenge(ChallengeKind::Otp), true),
            LoopDecision::KeepWaiting(None)
        );
    }

    #[test]
    fn session_cookie_detection() {
        let cookies = vec![
            json!({ "name": "NID_AUT", "value": "a" }),
            json!({ "name": "NID_SES", "value": "b" }),
        ];
        assert!(has_session_cookies(&cookies));
        assert!(!has_session_cookies(&[json!({ "name": "NID_AUT" })]));
        assert!(!has_session_cookies(&[]));
    }
}
