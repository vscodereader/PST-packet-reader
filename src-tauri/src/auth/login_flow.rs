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

// 봇탐지(ncaptcha/wtm) 완화용 스텔스 스크립트. 페이지 스크립트보다 먼저 모든 새 문서에서
// 실행되어 CDP 제어 흔적인 `navigator.webdriver` 를 일반 크롬과 동일한 값으로 맞춘다.
//
// 네이버 안티봇 번들(wtm.pstatic.net)의 검사는
//   getWebdriver(){ return void 0!==navigator.webdriver ? Boolean(navigator.webdriver).toString() : "" }
// 형태다. 일반(비자동화) 크롬은 `navigator.webdriver === false` 라 "false"를 보고한다. 따라서
// `undefined`(필드 없음)로 두면 오히려 일반 크롬과 달라지므로, 정확히 `false`로 맞춘다.
// Chrome 실행 플래그 `--disable-blink-features=AutomationControlled` 가 headed에선 네이티브로
// false를 주지만, 헤드리스에선 webdriver가 노출될 수 있어 JS로 한 번 더 false로 덮는다.
const STEALTH_INIT_JS: &str = "Object.defineProperty(navigator,'webdriver',{get:()=>false});";

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

    // navigate 전에 스텔스 스크립트를 등록해, 로그인 폼이 로드되며 실행되는 ncaptcha JS가
    // navigator.webdriver 를 읽기 전에 가려지도록 한다. CDP 호출이 실패해도 로그인 자체는
    // 진행해야 하므로 best-effort(let _)로 둔다(Page 도메인 미활성 등 환경 차이 흡수).
    let _ = client.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": STEALTH_INIT_JS }),
    );

    client.navigate(LOGIN_URL)?;
    // navigate가 readyState까지 기다려도, 로그인 폼이 렌더되고 네이버의 keydown 암호화
    // 핸들러가 붙기 전에 타이핑하면 글자가 필드에 들어가지 않는다. 폼이 준비될 때까지 기다린다.
    if !wait_for_login_form(client) {
        return Ok(LoginOutcome::Error(
            "로그인 폼(#id/#pw)을 찾지 못했습니다.".to_owned(),
        ));
    }

    // 위 wait_for_login_form이 "폼 완전 로딩"을 확인(로그)한 뒤에만 여기 도달한다. 곧장
    // 아이디 입력 → 2초 대기 → 비밀번호 입력 → 2초 대기 → 로그인 클릭.
    // 3회 재시도 후에도 필드가 비어 있으면(일시적 렌더/타이밍 문제) type_into가 false를
    // 돌려준다. 빈/부분 자격증명으로 로그인 버튼을 누르면 결과가 #err_common/타임아웃으로
    // 분류돼 일시적 타이핑 실패가 영구 BadCredentials/Error로 둔갑하므로, 클릭하지 않고
    // 명확한 입력 실패로 중단한다.
    if !type_into(client, "#id", id)? {
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(아이디 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(Duration::from_secs(2));
    if !type_into(client, "#pw", pw)? {
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(비밀번호 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }

    // 비밀번호 입력 후 2초 기다렸다가 로그인 버튼을 누른다(사람처럼 천천히).
    sleep(Duration::from_secs(2));
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

// 로그인 폼이 "완전히" 로딩될 때까지 기다린다: 페이지 로딩 완료(readyState=complete) +
// #id/#pw가 화면에 보이고 입력 가능(disabled 아님) + 로그인 버튼 존재. 이게 다 충족돼야
// 네이버 페이지 스크립트(키 입력 암호화 핸들러 포함)가 자리잡은 것으로 본다. 진행 상황을
// stderr로 출력해 콘솔에서 "폼이 완전히 로딩됐는지"를 확인할 수 있게 한다.
fn wait_for_login_form(client: &mut CdpClient) -> bool {
    eprintln!("[LOGIN] 로그인 폼 로딩 대기 중...");
    let deadline = Instant::now() + Duration::from_secs(15);
    let ready_expr = "(()=>{\
        if(document.readyState!=='complete')return false;\
        const ok=el=>!!(el&&el.offsetParent!==null&&!el.disabled);\
        const btn=document.querySelector('#log\\\\.login')\
                  ||document.querySelector('button[type=submit]');\
        return ok(document.querySelector('#id'))\
               &&ok(document.querySelector('#pw'))&&!!btn;\
    })()";
    loop {
        if client.evaluate_bool(ready_expr).unwrap_or(false) {
            eprintln!(
                "[LOGIN] ✓ 로그인 폼 완전 로딩 확인 (readyState=complete · #id/#pw 입력 가능 · 로그인 버튼 준비)"
            );
            return true;
        }
        if Instant::now() >= deadline {
            eprintln!("[LOGIN] ✗ 로그인 폼 로딩 시간 초과(15초)");
            return false;
        }
        sleep(Duration::from_millis(250));
    }
}

// 한 글자에 대응하는 US 키보드 물리키 정보. 합성 키 이벤트에 실제 브라우저와 동일한
// `code`·`windowsVirtualKeyCode`를 채워, DOM `event.keyCode`가 0이 되지 않게 한다.
struct KeyInfo {
    code: String,
    vk: u32,
    shift: bool,
}

// 문자를 US 키보드 배열의 (code, windowsVirtualKeyCode, Shift 필요 여부)로 매핑한다(순수 함수).
// 알파벳/숫자/비밀번호에 흔한 기호를 덮는다. 미지의 문자는 vk=0·code="" 로 떨어뜨려도
// keyDown의 `text`가 글자 입력을 담당하므로 값 자체는 들어간다(베스트에포트).
fn key_info(ch: char) -> KeyInfo {
    if ch.is_ascii_alphabetic() {
        let upper = ch.to_ascii_uppercase();
        return KeyInfo {
            code: format!("Key{upper}"),
            vk: upper as u32,
            shift: ch.is_ascii_uppercase(),
        };
    }
    if ch.is_ascii_digit() {
        return KeyInfo {
            code: format!("Digit{ch}"),
            vk: ch as u32,
            shift: false,
        };
    }
    // (code, windowsVirtualKeyCode, shift) — Shift+숫자 기호와 OEM 구두점.
    let (code, vk, shift): (&str, u32, bool) = match ch {
        ')' => ("Digit0", 0x30, true),
        '!' => ("Digit1", 0x31, true),
        '@' => ("Digit2", 0x32, true),
        '#' => ("Digit3", 0x33, true),
        '$' => ("Digit4", 0x34, true),
        '%' => ("Digit5", 0x35, true),
        '^' => ("Digit6", 0x36, true),
        '&' => ("Digit7", 0x37, true),
        '*' => ("Digit8", 0x38, true),
        '(' => ("Digit9", 0x39, true),
        ' ' => ("Space", 0x20, false),
        '-' => ("Minus", 0xBD, false),
        '_' => ("Minus", 0xBD, true),
        '=' => ("Equal", 0xBB, false),
        '+' => ("Equal", 0xBB, true),
        '[' => ("BracketLeft", 0xDB, false),
        '{' => ("BracketLeft", 0xDB, true),
        ']' => ("BracketRight", 0xDD, false),
        '}' => ("BracketRight", 0xDD, true),
        '\\' => ("Backslash", 0xDC, false),
        '|' => ("Backslash", 0xDC, true),
        ';' => ("Semicolon", 0xBA, false),
        ':' => ("Semicolon", 0xBA, true),
        '\'' => ("Quote", 0xDE, false),
        '"' => ("Quote", 0xDE, true),
        ',' => ("Comma", 0xBC, false),
        '<' => ("Comma", 0xBC, true),
        '.' => ("Period", 0xBE, false),
        '>' => ("Period", 0xBE, true),
        '/' => ("Slash", 0xBF, false),
        '?' => ("Slash", 0xBF, true),
        '`' => ("Backquote", 0xC0, false),
        '~' => ("Backquote", 0xC0, true),
        _ => ("", 0, false),
    };
    KeyInfo {
        code: code.to_owned(),
        vk,
        shift,
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
            let k = key_info(ch);
            // 실제 브라우저와 동일하게 code·windowsVirtualKeyCode·nativeVirtualKeyCode를 채운다.
            // 이게 없으면 DOM `event.keyCode`가 0이라, 네이버 default_ecc.js의 keydown 암호화
            // 훅/봇탐지가 합성 입력으로 판단 → eccpw가 깨지고 캡차/수동입력을 요구한다(패킷
            // 분석상 캡처된 성공 로그인은 NNB 쿠키만·bvsd 빈 채로도 즉시 성공했다).
            // Shift는 대문자뿐 아니라 Shift로 입력하는 기호(!@#$ 등)에도 일반화한다. Shift 없이
            // 대문자를 보내면 네이버가 "Caps Lock 켜짐"으로 오판해 경고를 띄우는 문제도 함께 막는다.
            let modifiers = if k.shift { 8 } else { 0 };
            client.call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": "keyDown",
                    "text": s,
                    "key": s,
                    "code": k.code,
                    "windowsVirtualKeyCode": k.vk,
                    "nativeVirtualKeyCode": k.vk,
                    "modifiers": modifiers,
                }),
            )?;
            client.call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": "keyUp",
                    "key": s,
                    "code": k.code,
                    "windowsVirtualKeyCode": k.vk,
                    "nativeVirtualKeyCode": k.vk,
                    "modifiers": modifiers,
                }),
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

    // --- key_info: 합성 키 이벤트가 실제 브라우저 keyCode와 일치하는지 ---

    #[test]
    fn key_info_lowercase_letter_has_keycode_without_shift() {
        let k = key_info('a');
        assert_eq!(k.code, "KeyA");
        assert_eq!(k.vk, 0x41); // VK_A == 'A'
        assert!(!k.shift);
    }

    #[test]
    fn key_info_uppercase_letter_sends_shift() {
        let k = key_info('A');
        assert_eq!(k.code, "KeyA");
        assert_eq!(k.vk, 0x41); // 대문자도 물리키는 'A'(=0x41)
        assert!(k.shift);
    }

    #[test]
    fn key_info_digit_has_keycode() {
        let k = key_info('7');
        assert_eq!(k.code, "Digit7");
        assert_eq!(k.vk, 0x37); // '7'
        assert!(!k.shift);
    }

    #[test]
    fn key_info_shifted_symbol_maps_to_base_digit_with_shift() {
        // '!' 는 Shift+1 → 물리키는 Digit1, vk 는 '1'(=0x31), shift=true.
        let bang = key_info('!');
        assert_eq!(bang.code, "Digit1");
        assert_eq!(bang.vk, 0x31);
        assert!(bang.shift);
        // '@' 는 Shift+2.
        let at = key_info('@');
        assert_eq!(at.code, "Digit2");
        assert!(at.shift);
    }

    #[test]
    fn key_info_oem_punctuation_has_nonzero_keycode() {
        for ch in ['-', '_', '.', '/', ';', '\'', '=', '+'] {
            assert_ne!(key_info(ch).vk, 0, "{ch} 의 vk 가 0이면 안 된다");
        }
        assert!(key_info('_').shift);
        assert!(!key_info('-').shift);
    }

    #[test]
    fn key_info_unknown_char_is_best_effort_zero() {
        // 한글 등 매핑 없는 문자는 vk=0·code="" — text 가 입력을 담당한다.
        let k = key_info('가');
        assert_eq!(k.vk, 0);
        assert_eq!(k.code, "");
        assert!(!k.shift);
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
