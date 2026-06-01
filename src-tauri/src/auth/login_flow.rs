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
const LOGIN_TIMEOUT: Duration = Duration::from_secs(40);
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
pub(crate) fn run(client: &mut CdpClient, id: &str, pw: &str) -> LoginOutcome {
    match run_inner(client, id, pw) {
        Ok(outcome) => outcome,
        Err(error) => LoginOutcome::Error(error.to_string()),
    }
}

fn run_inner(client: &mut CdpClient, id: &str, pw: &str) -> Result<LoginOutcome, AutomationError> {
    client.navigate(LOGIN_URL)?;

    type_into(client, "#id", id)?;
    type_into(client, "#pw", pw)?;

    // 로그인 버튼 클릭(값 주입이 아니라 클릭이므로 evaluate 사용 가능).
    client.evaluate(
        "(()=>{const b=document.querySelector('#log\\\\.login')||\
         document.querySelector('button[type=submit]');if(b){b.click();return true;}return false;})()",
    )?;

    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        let signals = read_signals(client)?;
        match classify(&signals) {
            Signal::Success => {
                let cookies = collect_naver_cookies(client)?;
                return Ok(LoginOutcome::Ok { cookies });
            }
            Signal::Challenge(kind) => return Ok(LoginOutcome::ChallengeRequired { kind }),
            Signal::BadCredentials => return Ok(LoginOutcome::BadCredentials),
            Signal::Blocked => {
                return Ok(LoginOutcome::Error(
                    "로그인 접근이 차단되었습니다.".to_owned(),
                ))
            }
            Signal::Pending => {}
        }

        if Instant::now() >= deadline {
            return Ok(LoginOutcome::Error(
                "로그인 시간이 초과되었습니다. 캡차/2차 인증이 필요할 수 있습니다.".to_owned(),
            ));
        }
        sleep(POLL_INTERVAL);
    }
}

// 선택자에 포커스한 뒤 한 글자씩 실제 키 이벤트로 입력한다(keydown 후킹 암호화 대응).
fn type_into(client: &mut CdpClient, selector: &str, text: &str) -> Result<(), AutomationError> {
    let focus = format!(
        "(()=>{{const el=document.querySelector('{selector}');if(el){{el.focus();return true;}}return false;}})()"
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
    Ok(())
}

// 현재 페이지에서 로그인 성공/챌린지/실패 신호를 읽는다.
fn read_signals(client: &mut CdpClient) -> Result<PageSignals, AutomationError> {
    let cookies = collect_naver_cookies(client)?;
    let logged_in = has_session_cookies(&cookies);

    let captcha = client
        .evaluate_bool("!!document.querySelector('#captchaDiv, #captcha, img#captchaimg')")
        .unwrap_or(false);
    let otp = client
        .evaluate_bool("!!document.querySelector('#otp, input[name=otp], #cellphoneCertify')")
        .unwrap_or(false);
    let current_url = client.current_url().unwrap_or_default();
    // 낯선 기기 추가 인증 페이지(기기 등록 확인). 성공 후의 "등록안함" 다이얼로그와 달리
    // 쿠키가 아직 없는 상태에서 사용자 조작을 요구한다.
    let device = current_url.contains("deviceConfirm") || current_url.contains("deviceCheck");
    let bad_credentials = client
        .evaluate_bool("!!document.querySelector('#err_common, .error_message')")
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
