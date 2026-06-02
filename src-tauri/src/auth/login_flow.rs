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

fn run_inner(
    client: &mut CdpClient,
    id: &str,
    pw: &str,
    wait_for_human: bool,
) -> Result<LoginOutcome, AutomationError> {
    client.navigate(LOGIN_URL)?;
    // navigate가 readyState까지 기다려도, 로그인 폼이 렌더되고 네이버의 keydown 암호화
    // 핸들러가 붙기 전에 타이핑하면 글자가 필드에 들어가지 않는다. 폼이 준비될 때까지 기다린다.
    if !wait_for_login_form(client) {
        return Ok(LoginOutcome::Error(
            "로그인 폼(#id/#pw)을 찾지 못했습니다.".to_owned(),
        ));
    }

    type_into(client, "#id", id)?;
    type_into(client, "#pw", pw)?;

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
    let deadline = Instant::now() + timeout;
    loop {
        // 인증 성공 직후 뜨는 "새 기기 등록" 페이지면 "등록 안함"을 눌러 마무리한다
        // (설계 5단계: browser_flow의 기존 로직 재사용). 없으면 무시한다.
        let _ = client.click_device_dontsave_if_present(Duration::from_millis(300));

        let signals = read_signals(client)?;
        match classify(&signals) {
            Signal::Success => {
                let cookies = collect_naver_cookies(client)?;
                return Ok(LoginOutcome::Ok { cookies });
            }
            Signal::Challenge(kind) => {
                // headed: 사용자가 직접 푸는 중이므로 성공/타임아웃까지 계속 기다린다.
                // headless: 즉시 반환해 호출자가 headed로 승격하게 한다.
                if !wait_for_human {
                    return Ok(LoginOutcome::ChallengeRequired { kind });
                }
            }
            Signal::BadCredentials => return Ok(LoginOutcome::BadCredentials),
            Signal::Blocked => {
                // headless에서만 즉시 차단으로 본다. headed에서는 기기등록/인증 중간 페이지를
                // 차단으로 오판하지 않도록, 사람이 진행하는 동안 타임아웃까지 기다린다.
                if !wait_for_human {
                    return Ok(LoginOutcome::Error(
                        "로그인 접근이 차단되었습니다.".to_owned(),
                    ));
                }
            }
            Signal::Pending => {}
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

// 로그인 폼(#id/#pw)이 나타나고 입력 가능해질 때까지 기다린 뒤, 폼 스크립트가 자리잡도록
// 잠깐 안정화 시간을 준다. 이 대기 없이 곧장 타이핑하면 자동 입력이 빈 화면에 헛쳐진다.
fn wait_for_login_form(client: &mut CdpClient) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ready = client
            .evaluate_bool("!!document.querySelector('#id') && !!document.querySelector('#pw')")
            .unwrap_or(false);
        if ready {
            sleep(Duration::from_millis(1500));
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
fn type_into(client: &mut CdpClient, selector: &str, text: &str) -> Result<(), AutomationError> {
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
            return Ok(());
        }
        sleep(Duration::from_millis(500));
    }
    // 3회 후에도 비면 그대로 진행한다(headed면 사용자가 직접 입력해 마무리할 수 있다).
    Ok(())
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
